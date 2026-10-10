//! Notification-area icon, its menu, and the dashboard window (an Edge "app"
//! window this process starts and closes again on Quit).

use super::ffi::*;
use crate::config;
use crate::engine::Engine;
use crate::json::Json;
use crate::util;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

pub const APP_NAME: &str = "DeepCool LM";
const CLASS: &str = "DeepCoolLMTrayWindow";
const MUTEX: &str = "Local\\DeepCoolLMTray";
const WM_TRAY: u32 = WM_APP + 1;
/// Sent by a second copy of the app: open the dashboard.
pub const WM_OPEN_DASHBOARD: u32 = WM_APP + 2;
const TIMER_STATUS: usize = 1;

const ID_OPEN: usize = 1;
const ID_MONITOR: usize = 10;
const ID_MEDIA: usize = 11;
const ID_ROTATION: usize = 20; // + 0..3
const ID_BRIGHTNESS: usize = 30; // + index
const ID_TEST: usize = 40;
const ID_AUTOSTART: usize = 50;
const ID_QUIT: usize = 99;
const LEVELS: [u8; 5] = [10, 25, 50, 75, 100];

struct Tray {
    engine: Arc<Engine>,
    url: String,
    hwnd: Mutex<usize>,
    edge: Mutex<Option<usize>>, // process handle of the dashboard window
    icon: HICON,
    taskbar_created: u32,
    shown_connected: AtomicBool,
}

unsafe impl Send for Tray {}
unsafe impl Sync for Tray {}

static TRAY: OnceLock<Tray> = OnceLock::new();

/// True if no other copy is running. Otherwise asks that copy to open its
/// dashboard (so starting the app twice just shows the window).
pub fn single_instance() -> bool {
    unsafe {
        let m = CreateMutexW(std::ptr::null_mut(), 0, wide(MUTEX).as_ptr());
        if m.is_null() || GetLastError() != ERROR_ALREADY_EXISTS {
            return true; // the handle stays open for the life of the process
        }
        AllowSetForegroundWindow(u32::MAX);
        let hwnd = FindWindowW(wide(CLASS).as_ptr(), std::ptr::null());
        if !hwnd.is_null() {
            PostMessageW(hwnd, WM_OPEN_DASHBOARD, 0, 0);
        }
        false
    }
}

fn state_dir() -> std::path::PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(base).join("deepcool-lm")
}

pub fn log_path() -> std::path::PathBuf {
    state_dir().join("tray.log")
}

// --- start at sign-in ---------------------------------------------------------

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_NAME: &str = "DeepCool LM";
const LEGACY_RUN_NAME: &str = "DeepCool LM Tray"; // older installers, machine-wide

fn exe_command() -> String {
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
    format!("\"{}\"", exe)
}

/// Is the app set to start when you sign in?
pub fn autostart_enabled() -> bool {
    !super::sys::reg_strings(HKEY_CURRENT_USER, RUN_KEY, Some(RUN_NAME)).is_empty()
        || !super::sys::reg_strings(HKEY_LOCAL_MACHINE, RUN_KEY, Some(LEGACY_RUN_NAME)).is_empty()
}

/// Start (or stop starting) this exe, from where it is now, when you sign in.
pub fn set_autostart(on: bool) -> Result<(), String> {
    unsafe {
        let (key, name) = (wide(RUN_KEY), wide(RUN_NAME));
        if on {
            let value = wide(&exe_command());
            let r = RegSetKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), REG_SZ, value.as_ptr() as *const std::ffi::c_void, (value.len() * 2) as u32);
            if r != 0 {
                return Err(format!("couldn't write the registry (error {})", r));
            }
        } else {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr());
            // the machine-wide entry from older installers needs admin rights; try anyway
            RegDeleteKeyValueW(HKEY_LOCAL_MACHINE, key.as_ptr(), wide(LEGACY_RUN_NAME).as_ptr());
            if autostart_enabled() {
                return Err("an older machine-wide entry remains; re-run install.ps1 or uninstall.ps1 as administrator to remove it".into());
            }
        }
    }
    Ok(())
}

/// If the exe was moved, point the sign-in entry at its new location.
fn refresh_autostart_path() {
    let current = super::sys::reg_strings(HKEY_CURRENT_USER, RUN_KEY, Some(RUN_NAME));
    if let Some(cmd) = current.first() {
        if !cmd.eq_ignore_ascii_case(&exe_command()) {
            let _ = set_autostart(true);
        }
    }
}

fn find_edge() -> Option<String> {
    for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        if let Some(p) = super::sys::reg_strings(root, r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\msedge.exe", None).into_iter().next() {
            if std::path::Path::new(&p).is_file() {
                return Some(p);
            }
        }
    }
    for var in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Ok(base) = std::env::var(var) {
            let p = std::path::Path::new(&base).join(r"Microsoft\Edge\Application\msedge.exe");
            if p.is_file() {
                return Some(p.display().to_string());
            }
        }
    }
    None
}

fn focus_dashboard() -> bool {
    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), wide(APP_NAME).as_ptr());
        if hwnd.is_null() {
            return false;
        }
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd);
        true
    }
}

impl Tray {
    fn hwnd(&self) -> HWND {
        *self.hwnd.lock().unwrap() as HWND
    }

    fn edge_running(&self) -> bool {
        match *self.edge.lock().unwrap() {
            Some(h) => unsafe { WaitForSingleObject(h as HANDLE, 0) == WAIT_TIMEOUT },
            None => false,
        }
    }

    fn open_dashboard(&self) {
        if self.edge_running() && focus_dashboard() {
            return;
        }
        let Some(edge) = find_edge() else {
            // no Edge: use the default browser
            unsafe { ShellExecuteW(std::ptr::null_mut(), wide("open").as_ptr(), wide(&self.url).as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) };
            return;
        };
        let profile = state_dir().join("dashboard-window");
        let cmd = format!(
            "\"{}\" --app={} \"--user-data-dir={}\" --window-size=1240,840 --no-first-run --no-default-browser-check --disable-extensions --disable-sync --disable-features=msEdgeSidebarV2,EdgeCollections",
            edge,
            self.url,
            profile.display()
        );
        let mut cmdline = wide(&cmd);
        unsafe {
            let mut si: STARTUPINFOW = std::mem::zeroed();
            si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
            let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
            AllowSetForegroundWindow(u32::MAX);
            if CreateProcessW(std::ptr::null(), cmdline.as_mut_ptr(), std::ptr::null_mut(), std::ptr::null_mut(), 0, 0, std::ptr::null_mut(), std::ptr::null(), &mut si, &mut pi) == 0 {
                log!("Edge window failed (error {})", GetLastError());
                return;
            }
            CloseHandle(pi.hThread);
            if let Some(old) = self.edge.lock().unwrap().replace(pi.hProcess as usize) {
                CloseHandle(old as HANDLE);
            }
        }
    }

    fn close_dashboard(&self) {
        if let Some(h) = self.edge.lock().unwrap().take() {
            unsafe {
                if WaitForSingleObject(h as HANDLE, 0) == WAIT_TIMEOUT {
                    TerminateProcess(h as HANDLE, 0);
                }
                CloseHandle(h as HANDLE);
            }
        }
    }

    fn notify_data(&self) -> NOTIFYICONDATAW {
        let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = self.hwnd();
        nid.uID = 1;
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = self.icon;
        let connected = self.engine.connected();
        let tip = format!("{} - cooler {}", APP_NAME, if connected { "connected" } else { "not connected" });
        for (dst, src) in nid.szTip.iter_mut().zip(tip.encode_utf16().take(127)) {
            *dst = src;
        }
        self.shown_connected.store(connected, Ordering::SeqCst);
        nid
    }

    fn add_icon(&self) {
        let mut nid = self.notify_data();
        unsafe { Shell_NotifyIconW(NIM_ADD, &mut nid) };
    }

    fn refresh_tip(&self) {
        if self.engine.connected() != self.shown_connected.load(Ordering::SeqCst) {
            let mut nid = self.notify_data();
            unsafe { Shell_NotifyIconW(NIM_MODIFY, &mut nid) };
        }
    }

    fn remove_icon(&self) {
        let mut nid = self.notify_data();
        unsafe { Shell_NotifyIconW(NIM_DELETE, &mut nid) };
    }

    fn update(&self, key: &str, value: Json) {
        let mut cfg = config::load_or_default();
        cfg.set(key, value);
        match config::save(&cfg) {
            Ok(()) => self.engine.poke(),
            Err(e) => log!("Can't save settings: {}", e),
        }
    }

    fn show_menu(&self) {
        let cfg = config::load_or_default();
        let mode = cfg.str_or("mode", "monitor").to_string();
        let rotation = cfg.get("rotation").and_then(|r| r.as_f64()).and_then(util::normalize_rotation).unwrap_or_else(|| self.engine.current_rotation());
        let brightness = cfg.get("brightness").and_then(|b| b.as_f64()).map(|b| b as u8);
        let checked = |on: bool| if on { MF_CHECKED } else { 0 };
        unsafe {
            let menu = CreatePopupMenu();
            let add = |m: HMENU, flags: u32, id: usize, text: &str| AppendMenuW(m, flags, id, wide(text).as_ptr());
            add(menu, MF_STRING, ID_OPEN, "Open dashboard");
            SetMenuDefaultItem(menu, ID_OPEN as u32, 0);
            add(menu, MF_SEPARATOR, 0, "");
            add(menu, MF_STRING | checked(mode == "monitor"), ID_MONITOR, "Monitor mode");
            add(menu, MF_STRING | checked(mode == "media"), ID_MEDIA, "Multimedia mode");
            let rot_menu = CreatePopupMenu();
            for (i, deg) in util::ROTATIONS.iter().enumerate() {
                add(rot_menu, MF_STRING | checked(rotation == *deg), ID_ROTATION + i, &format!("{} ({}°)", util::rotation_name(*deg), deg));
            }
            add(menu, MF_POPUP, rot_menu as usize, "Screen orientation");
            let level_menu = CreatePopupMenu();
            for (i, level) in LEVELS.iter().enumerate() {
                add(level_menu, MF_STRING | checked(brightness == Some(*level)), ID_BRIGHTNESS + i, &format!("{}%", level));
            }
            add(menu, MF_POPUP, level_menu as usize, "Brightness");
            add(menu, MF_STRING, ID_TEST, "Show test pattern");
            add(menu, MF_STRING | checked(autostart_enabled()), ID_AUTOSTART, "Start when I sign in");
            add(menu, MF_SEPARATOR, 0, "");
            let status = if self.engine.connected() { "Cooler: connected" } else { "Cooler: not connected" };
            add(menu, MF_STRING | MF_GRAYED, 0, status);
            add(menu, MF_STRING, ID_QUIT, "Quit (turns the screen off)");

            let mut pt = POINT::default();
            GetCursorPos(&mut pt);
            let hwnd = self.hwnd();
            SetForegroundWindow(hwnd); // so the menu closes when you click elsewhere
            let cmd = TrackPopupMenu(menu, TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY, pt.x, pt.y, 0, hwnd, std::ptr::null()) as usize;
            PostMessageW(hwnd, WM_NULL, 0, 0);
            DestroyMenu(menu); // also destroys the submenus
            self.command(cmd);
        }
    }

    fn command(&self, id: usize) {
        match id {
            ID_OPEN => self.open_dashboard(),
            ID_MONITOR => self.update("mode", "monitor".into()),
            ID_MEDIA => self.update("mode", "media".into()),
            i if (ID_ROTATION..ID_ROTATION + 4).contains(&i) => self.update("rotation", (util::ROTATIONS[i - ID_ROTATION] as i64).into()),
            i if (ID_BRIGHTNESS..ID_BRIGHTNESS + LEVELS.len()).contains(&i) => {
                let level = LEVELS[i - ID_BRIGHTNESS];
                self.engine.set_brightness(level);
                self.update("brightness", (level as i64).into());
            }
            ID_TEST => self.engine.show_test_pattern(15.0),
            ID_AUTOSTART => {
                if let Err(e) = set_autostart(!autostart_enabled()) {
                    log!("Start at sign-in: {}", e);
                }
            }
            ID_QUIT => self.quit(),
            _ => {}
        }
    }

    /// Quit like an app: screen off, dashboard closed, everything stopped.
    fn quit(&self) {
        log!("Quitting");
        self.engine.shutdown(true);
        self.close_dashboard();
        self.remove_icon();
        unsafe { DestroyWindow(self.hwnd()) };
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let Some(t) = TRAY.get() else { return DefWindowProcW(hwnd, msg, w, l) };
    match msg {
        WM_TRAY => {
            match (l & 0xFFFF) as u32 {
                WM_LBUTTONUP | WM_LBUTTONDBLCLK => t.open_dashboard(),
                WM_RBUTTONUP | WM_CONTEXTMENU => t.show_menu(),
                _ => {}
            }
            0
        }
        WM_OPEN_DASHBOARD => {
            t.open_dashboard();
            0
        }
        WM_TIMER => {
            t.refresh_tip();
            0
        }
        WM_QUERYENDSESSION => 1,
        WM_ENDSESSION => {
            if w != 0 {
                // signing out or shutting down: turn the screen off like Quit
                t.engine.shutdown(true);
                t.close_dashboard();
            }
            0
        }
        WM_CLOSE => {
            t.quit();
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        m if m == t.taskbar_created && m != 0 => {
            t.add_icon(); // Explorer restarted
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

/// Run the tray until Quit. The engine and web server are already running.
pub fn run(engine: Arc<Engine>, url: String, open: bool) {
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let class = wide(CLASS);
        let mut px = crate::icon::draw(32);
        let icon = super::gdiplus::icon_from_bgra(&mut px, 32);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: wndproc,
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: icon,
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
            hIconSm: icon,
        };
        RegisterClassExW(&wc);
        let tray = Tray {
            engine,
            url,
            hwnd: Mutex::new(0),
            edge: Mutex::new(None),
            icon,
            taskbar_created: RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
            shown_connected: AtomicBool::new(false),
        };
        if TRAY.set(tray).is_err() {
            return;
        }
        let t = TRAY.get().unwrap();
        // a normal (never shown) window, so it gets the TaskbarCreated broadcast
        let hwnd = CreateWindowExW(0, class.as_ptr(), wide("DeepCool LM tray").as_ptr(), 0, 0, 0, 0, 0, std::ptr::null_mut(), std::ptr::null_mut(), instance, std::ptr::null_mut());
        if hwnd.is_null() {
            log!("Can't create the tray window (error {})", GetLastError());
            return;
        }
        *t.hwnd.lock().unwrap() = hwnd as usize;
        t.add_icon();
        refresh_autostart_path();
        SetTimer(hwnd, TIMER_STATUS, 2000, std::ptr::null_mut());
        if open {
            t.open_dashboard();
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        // also covers ending through other paths
        t.engine.shutdown(true);
        t.close_dashboard();
        t.remove_icon();
    }
}
