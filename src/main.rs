//! DeepCool LM: drives the LCD on DeepCool LM-series AIO coolers (LM360) on
//! Windows. One small program: the screen, a notification-area icon, the
//! settings dashboard and the `deepcool-lm` command line.
#![cfg_attr(windows, windows_subsystem = "windows")]
// Some helpers are only used on one platform or in tests.
#![allow(dead_code)]

#[macro_use]
mod log;
mod canvas;
mod config;
mod device;
mod engine;
mod http;
mod icon;
mod ipc;
mod json;
mod media;
mod platform;
mod render;
mod sensors;
mod util;
#[cfg(any(windows, wincheck))]
mod win;

use json::Json;
use std::process::ExitCode;
use std::sync::Arc;

const USAGE: &str = "\
DeepCool LM screen app

Usage:
  deepcool-lm                     Start the app: cooler screen, tray icon and dashboard
  deepcool-lm --open              Start (if needed) and open the dashboard
  deepcool-lm --no-tray           Run the screen and dashboard without a tray icon
  deepcool-lm --boot              Display only, before sign-in (scheduled task)

  deepcool-lm monitor             Back to the live monitor (after image/solid)
  deepcool-lm image FILE          Show a picture
  deepcool-lm solid --color R G B Show a solid color
  deepcool-lm brightness LEVEL    'up', 'down' or 0-100
  deepcool-lm zen                 Toggle the screen on/off
  deepcool-lm test-pattern [SEC]  Show color bars for a few seconds
  deepcool-lm info                USB details (quit the app first)
  deepcool-lm autostart on|off    Start the app when you sign in (or stop)
  deepcool-lm settings [options]  Show or change saved settings:
      --set-rotation 0|90|180|270   --set-orientation landscape|portrait
      --brightness 0-100            --mode monitor|media
      --interval SECONDS

Options:
  --rotation 0|90|180|270   Screen orientation when the settings don't say
                            (clockwise turn; 0 landscape, 90 portrait)

Commands go to the running app; without it they talk to the cooler directly.
Settings: %ProgramData%\\deepcool-lm\\config.json";

fn env_rotation() -> i32 {
    if let Some(r) = std::env::var("DEEPCOOL_LM_ROTATION").ok().and_then(|v| v.trim().parse::<f64>().ok()).and_then(util::normalize_rotation) {
        return r;
    }
    match std::env::var("DEEPCOOL_LM_ORIENTATION").map(|v| v.trim().to_ascii_lowercase()) {
        Ok(v) if v == "portrait" => 90,
        _ => 0,
    }
}

struct Args {
    rotation: Option<i32>,
    flags: Vec<String>,
    command: Option<String>,
    rest: Vec<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args { rotation: None, flags: Vec::new(), command: None, rest: Vec::new() };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        if args.command.is_some() {
            args.rest.push(a);
            continue;
        }
        match a.as_str() {
            "--rotation" => {
                let v = it.next().ok_or("--rotation needs a value")?;
                args.rotation = Some(v.parse::<f64>().ok().and_then(util::normalize_rotation).ok_or("--rotation must be 0, 90, 180 or 270")?);
            }
            "--orientation" => {
                let v = it.next().ok_or("--orientation needs a value")?;
                args.rotation = Some(if v == "portrait" { 90 } else { 0 });
            }
            "--write-icon" => {
                args.command = Some(a);
            }
            f if f.starts_with("--") || f == "-h" => args.flags.push(f.to_string()),
            _ => args.command = Some(a),
        }
    }
    Ok(args)
}

fn opt<'a>(rest: &'a [String], name: &str) -> Option<&'a str> {
    rest.iter().position(|a| a == name).and_then(|i| rest.get(i + 1)).map(|s| s.as_str())
}

fn ok(msg: &str) -> ExitCode {
    println!("✓ {}", msg);
    ExitCode::SUCCESS
}

fn fail(msg: &str) -> ExitCode {
    println!("✗ {}", msg);
    ExitCode::FAILURE
}

/// Send a command to the running app, or run `direct` with the cooler when the app isn't running.
fn command(cmd: Json, success: &str, direct: impl FnOnce(&mut device::Lm360) -> Result<(), String>) -> ExitCode {
    if let Some(reply) = ipc::send(&cmd) {
        return if reply.str_or("status", "") == "ok" { ok(success) } else { fail(&format!("Error: {}", reply.str_or("message", "unknown error"))) };
    }
    match device::Lm360::open() {
        Ok(mut d) => match direct(&mut d) {
            Ok(()) => ok(success),
            Err(e) => fail(&e),
        },
        Err(e) => fail(&format!(
            "Failed to connect to LM360 ({}). Make sure it's plugged in and DeepCool's own software is closed.",
            e
        )),
    }
}

fn show_direct(d: &mut device::Lm360, frame: &[u8]) -> Result<(), String> {
    d.init();
    std::thread::sleep(std::time::Duration::from_millis(500));
    if d.send_frame(frame) {
        Ok(())
    } else {
        Err("Writing to the cooler failed".into())
    }
}

fn saved_rotation(cli: Option<i32>) -> i32 {
    config::load()
        .ok()
        .flatten()
        .and_then(|c| c.get("rotation").and_then(|r| r.as_f64()))
        .and_then(util::normalize_rotation)
        .or(cli)
        .unwrap_or_else(env_rotation)
}

fn cmd_settings(rest: &[String]) -> ExitCode {
    let cfg = match config::load() {
        Ok(c) => c,
        Err(e) => return fail(&format!("Can't read {}: {}", config::config_path().display(), e)),
    };
    let exists = cfg.is_some();
    let mut cfg = cfg.unwrap_or_else(|| config::normalize(&Json::obj()));
    let mut changes: Vec<(&str, Json)> = Vec::new();
    let rotation = match (opt(rest, "--set-rotation"), opt(rest, "--set-orientation")) {
        (Some(r), _) => match r.parse::<f64>().ok().and_then(util::normalize_rotation) {
            Some(r) => Some(r),
            None => return fail("--set-rotation must be 0, 90, 180 or 270"),
        },
        (None, Some(o)) => Some(if o == "portrait" { 90 } else { 0 }),
        _ => None,
    };
    if let Some(m) = opt(rest, "--mode") {
        if m != "monitor" && m != "media" {
            return fail("--mode must be monitor or media");
        }
        changes.push(("mode", m.into()));
    }
    if let Some(i) = opt(rest, "--interval") {
        match i.parse::<f64>() {
            Ok(v) if v > 0.0 => changes.push(("interval", v.into())),
            _ => return fail("--interval must be a number of seconds"),
        }
    }
    if let Some(r) = rotation {
        changes.push(("rotation", (r as i64).into()));
    }
    if let Some(b) = opt(rest, "--brightness") {
        match b.parse::<i64>() {
            Ok(v) if (0..=100).contains(&v) => changes.push(("brightness", v.into())),
            _ => return fail("--brightness must be 0-100"),
        }
    }
    if !changes.is_empty() {
        let summary: Vec<String> = changes.iter().map(|(k, v)| format!("{}={}", k, v.to_string_compact())).collect();
        for (k, v) in changes {
            cfg.set(k, v);
        }
        if let Err(e) = config::save(&cfg) {
            return fail(&format!("Can't write {}: {}", config::config_path().display(), e));
        }
        return ok(&format!("Saved {} to {}", summary.join(", "), config::config_path().display()));
    }
    println!("Settings file: {}{}", config::config_path().display(), if exists { "" } else { " (not created yet)" });
    let rot = cfg.get("rotation").and_then(|r| r.as_f64()).and_then(util::normalize_rotation);
    let shown = rot.unwrap_or_else(env_rotation);
    println!("  orientation: {} ({}°){}", util::rotation_name(shown), shown, if rot.is_some() { "" } else { " (default)" });
    match cfg.get("brightness").and_then(|b| b.as_f64()) {
        Some(b) => println!("  brightness:  {}", b),
        None => println!("  brightness:  (not set)"),
    }
    println!("  mode:        {}", cfg.str_or("mode", "monitor"));
    println!("  interval:    {} s", cfg.num_or("interval", 1.0));
    let n = cfg.get("media").and_then(|m| m.get("items")).and_then(|i| i.as_array()).map(|a| a.len()).unwrap_or(0);
    println!("  images:      {} selected", n);
    ExitCode::SUCCESS
}

fn cmd_info() -> ExitCode {
    if ipc::running() {
        return fail("The DeepCool LM app is running and using the cooler. Quit it from the tray icon first.");
    }
    println!("deepcool-lm {} ({} {})", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH);
    let mut d = match device::Lm360::open() {
        Ok(d) => d,
        Err(e) => return fail(&format!("Failed to connect to LM360 ({})", e)),
    };
    println!("{}", d.describe());
    println!("Sending init/query command...");
    println!("  {}", if d.init() { "sent" } else { "write failed" });
    match d.read_reply() {
        Ok(r) => println!("  reply: {}", r.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" ")),
        Err(e) => println!("  {}", e),
    }
    ExitCode::SUCCESS
}

fn cmd_autostart(arg: Option<&str>) -> ExitCode {
    #[cfg(any(windows, wincheck))]
    {
        use win::tray::{autostart_enabled, set_autostart};
        return match arg {
            Some("on") => match set_autostart(true) {
                Ok(()) => ok("DeepCool LM will start when you sign in"),
                Err(e) => fail(&e),
            },
            Some("off") => match set_autostart(false) {
                Ok(()) => ok("DeepCool LM won't start when you sign in"),
                Err(e) => fail(&e),
            },
            _ => {
                println!("Start when you sign in: {}", if autostart_enabled() { "on" } else { "off" });
                ExitCode::SUCCESS
            }
        };
    }
    #[allow(unreachable_code)]
    {
        let _ = arg;
        fail("Start at sign-in is set up by Windows only")
    }
}

/// The app itself: screen engine, IPC, dashboard server and (on Windows) the tray.
fn run_app(cli_rotation: Option<i32>, open: bool, tray: bool) -> ExitCode {
    #[cfg(any(windows, wincheck))]
    let _boot_session = {
        if tray && !win::tray::single_instance() {
            return ExitCode::SUCCESS; // the running copy opens its dashboard
        }
        if tray {
            log::init(&win::tray::log_path());
        }
        match win::boot::user_session() {
            Ok(session) => session,
            Err(e) => {
                log!("Cannot take over the boot display: {}", e);
                return fail(&e);
            }
        }
    };
    log!("DeepCool LM {} starting", env!("CARGO_PKG_VERSION"));
    let engine = Arc::new(engine::Engine::new(cli_rotation.unwrap_or_else(env_rotation), 1.0));
    if let Err(e) = ipc::start(engine.clone()) {
        log!("IPC server not started: {}", e);
    }
    let token = util::hex(&platform::random_bytes(24));
    let server = Arc::new(http::Server { engine: engine.clone(), token, on_saved: None });
    let url = match http::start(server) {
        Ok(u) => u,
        Err(e) => {
            log!("Dashboard server failed: {}", e);
            String::new()
        }
    };
    let e2 = engine.clone();
    let display = std::thread::Builder::new().name("display".into()).spawn(move || e2.run()).expect("display thread");

    #[cfg(any(windows, wincheck))]
    if tray {
        win::tray::run(engine.clone(), url, open);
        engine.shutdown(true);
        let _ = display.join();
        return ExitCode::SUCCESS;
    }
    let _ = (open, tray);
    println!("Dashboard: {}", url);
    println!("Press Ctrl+C to stop.");
    let _ = display.join();
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            #[cfg(any(windows, wincheck))]
            win::attach_console();
            eprintln!("{}\n\n{}", e, USAGE);
            return ExitCode::from(2);
        }
    };
    let gui = args.command.is_none() && !args.flags.iter().any(|f| f == "--help" || f == "-h" || f == "--no-tray" || f == "--boot");
    #[cfg(any(windows, wincheck))]
    if !gui {
        win::attach_console();
    }
    if args.flags.iter().any(|f| f == "--help" || f == "-h") {
        println!("{}", USAGE);
        return ExitCode::SUCCESS;
    }
    if args.flags.iter().any(|f| f == "--boot") {
        if args.command.is_some() || args.flags.iter().any(|f| f != "--boot") {
            return fail("--boot cannot be combined with commands or other startup modes");
        }
        #[cfg(any(windows, wincheck))]
        return match win::boot::run(args.rotation.unwrap_or_else(env_rotation)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => { log!("Boot display failed: {}", e); fail(&e) },
        };
        #[cfg(not(any(windows, wincheck)))]
        return fail("Boot startup is supported on Windows only");
    }
    let rest = &args.rest;
    match args.command.as_deref() {
        None => {
            let open = args.flags.iter().any(|f| f == "--open");
            let tray = cfg!(any(windows, wincheck)) && gui;
            run_app(args.rotation, open, tray)
        }
        Some("--write-icon") => match rest.first() {
            Some(path) => match std::fs::write(path, icon::ico()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => fail(&e.to_string()),
            },
            None => fail("--write-icon needs a file name"),
        },
        Some("monitor") => {
            if let Some(reply) = ipc::send(&json::object([("action", "monitor".into())])) {
                return if reply.str_or("status", "") == "ok" { ok("Switched to monitoring mode") } else { fail(reply.str_or("message", "error")) };
            }
            // not running: drive the screen from this terminal
            let interval = opt(rest, "--interval").and_then(|v| v.parse::<f64>().ok()).unwrap_or(1.0);
            println!("Displaying system monitor (Ctrl+C to stop)...");
            #[cfg(any(windows, wincheck))]
            let _boot_session = match win::boot::user_session() {
                Ok(session) => session,
                Err(e) => return fail(&e),
            };
            let engine = Arc::new(engine::Engine::new(args.rotation.unwrap_or_else(env_rotation), interval));
            if let Err(e) = ipc::start(engine.clone()) {
                log!("IPC server not started: {}", e);
            }
            engine.run();
            ExitCode::SUCCESS
        }
        Some("image") => {
            let Some(path) = rest.first() else { return fail("image needs a file name") };
            let abs = std::fs::canonicalize(path).unwrap_or_else(|_| std::path::PathBuf::from(path));
            let cmd = json::object([("action", "image".into()), ("path", abs.display().to_string().into())]);
            let rotation = saved_rotation(args.rotation);
            command(cmd, &format!("Displayed image: {}", path), |d| {
                let size = util::canvas_size(rotation);
                let px = media::Clip::open(&abs, "cover", size, (0, 0, 0), 100, 1)?.frame(0)?;
                show_direct(d, &util::bgra_to_frame(&px, size.0, size.1, size.0 * 4, rotation))
            })
        }
        Some("solid") => {
            let i = rest.iter().position(|a| a == "--color");
            let c: Vec<u8> = i.map(|i| rest[i + 1..].iter().take(3).filter_map(|v| v.parse().ok()).collect()).unwrap_or_else(|| vec![0, 0, 0]);
            if c.len() != 3 {
                return fail("--color needs three numbers 0-255");
            }
            let cmd = json::object([("action", "solid".into()), ("color", Json::Arr(c.iter().map(|&v| (v as i64).into()).collect()))]);
            command(cmd, &format!("Displayed solid color: ({}, {}, {})", c[0], c[1], c[2]), |d| {
                let v = ((c[0] as u16 >> 3) << 11) | ((c[1] as u16 >> 2) << 5) | (c[2] as u16 >> 3);
                show_direct(d, &v.to_le_bytes().repeat(util::NATIVE_W * util::NATIVE_H))
            })
        }
        Some("brightness") => {
            let (level, cmd, msg) = match rest.first().map(|s| s.as_str()) {
                Some("up") => (97, json::object([("action", "brightness_up".into())]), "Brightness increased".to_string()),
                Some("down") => (29, json::object([("action", "brightness_down".into())]), "Brightness decreased".to_string()),
                Some(n) => match n.parse::<u8>() {
                    Ok(l) if l <= 100 => (l, json::object([("action", "brightness_set".into()), ("level", (l as i64).into())]), format!("Brightness set to {}", l)),
                    _ => return fail("brightness must be 'up', 'down' or 0-100"),
                },
                None => return fail("brightness needs 'up', 'down' or 0-100"),
            };
            command(cmd, &msg, |d| if d.set_brightness(level) { Ok(()) } else { Err("Writing to the cooler failed".into()) })
        }
        Some("zen") => command(json::object([("action", "zen_toggle".into())]), "Toggled screen on/off (Zen mode)", |d| {
            if d.zen_toggle() {
                Ok(())
            } else {
                Err("Writing to the cooler failed".into())
            }
        }),
        Some("test-pattern") => {
            let secs = rest.first().and_then(|s| s.parse::<f64>().ok()).unwrap_or(15.0);
            match ipc::send(&json::object([("action", "test_pattern".into()), ("seconds", secs.into())])) {
                Some(r) if r.str_or("status", "") == "ok" => ok("Showing the test pattern"),
                Some(r) => fail(r.str_or("message", "error")),
                None => {
                    let rotation = saved_rotation(args.rotation);
                    let size = util::canvas_size(rotation);
                    command(Json::Null, "Showing the test pattern", |d| {
                        let (px, stride) = render::test_pattern(size, rotation).pixels();
                        show_direct(d, &util::bgra_to_frame(&px, size.0, size.1, stride, rotation))
                    })
                }
            }
        }
        Some("info") => cmd_info(),
        Some("autostart") => cmd_autostart(rest.first().map(|s| s.as_str())),
        Some("settings") => cmd_settings(rest),
        Some(other) => {
            eprintln!("Unknown command: {}\n\n{}", other, USAGE);
            ExitCode::from(2)
        }
    }
}
