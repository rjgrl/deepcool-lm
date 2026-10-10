//! The display engine: one loop that reads sensors, draws the layout (or plays
//! media), sends it to the cooler and reconnects when the cooler comes back.
//! The dashboard and the command-line IPC talk to it through `Arc<Engine>`.

use crate::config::{self, ConfigWatcher};
use crate::device::Lm360;
use crate::json::{self, Json};
use crate::media::{Clip, MediaPlayer};
use crate::render::{self, Renderer};
use crate::sensors::{History, SensorHub, Snapshot};
use crate::util;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

const RECONNECT: Duration = Duration::from_secs(10);

struct Readings {
    snap: Snapshot,
    history: History,
    last_collect: Option<Instant>,
}

/// A picture shown instead of the normal screen.
enum Overlay {
    /// Color bars until `expires`, drawn at whatever rotation is current.
    TestPattern { expires: Instant },
    /// `image` / `solid` from the command line, drawn for one rotation.
    Frame { frame: Vec<u8>, rotation: i32, label: &'static str },
}

#[derive(Default)]
struct Wake {
    stop: bool,
    poke: bool,
}

/// State only the display loop touches.
struct LoopState {
    watcher: ConfigWatcher,
    hub: SensorHub,
    player: MediaPlayer,
    applied_brightness: Option<u8>,
    next_connect: Instant,
    waiting_logged: bool,
    failures: u32,
    frames: u64,
    last_state: String,
    last_log: Option<Instant>,
}

pub struct Engine {
    cli_rotation: i32,
    default_interval: f64,
    device: Mutex<Option<Lm360>>,
    connected: AtomicBool,
    rotation: AtomicI32,
    data: Mutex<Readings>,
    pub renderer: Mutex<Renderer>,
    overlay: Mutex<Option<Overlay>>,
    wake: Mutex<Wake>,
    wake_cv: Condvar,
    state: Mutex<LoopState>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // a panic elsewhere shouldn't take the screen down with it
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Engine {
    pub fn new(cli_rotation: i32, interval: f64) -> Engine {
        Engine {
            cli_rotation,
            default_interval: interval,
            device: Mutex::new(None),
            connected: AtomicBool::new(false),
            rotation: AtomicI32::new(-1),
            data: Mutex::new(Readings { snap: Snapshot::new(), history: History::new(600.0), last_collect: None }),
            renderer: Mutex::new(Renderer::new()),
            overlay: Mutex::new(None),
            wake: Mutex::new(Wake::default()),
            wake_cv: Condvar::new(),
            state: Mutex::new(LoopState {
                watcher: ConfigWatcher::new(),
                hub: SensorHub::new(),
                player: MediaPlayer::new(),
                applied_brightness: None,
                next_connect: Instant::now(),
                waiting_logged: false,
                failures: 0,
                frames: 0,
                last_state: String::new(),
                last_log: None,
            }),
        }
    }

    pub fn connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    fn stopped(&self) -> bool {
        lock(&self.wake).stop
    }

    /// Rotation the screen is drawn at right now (settings, else the command line).
    pub fn current_rotation(&self) -> i32 {
        match self.rotation.load(Ordering::SeqCst) {
            r if r >= 0 => r,
            _ => self.cli_rotation,
        }
    }

    /// Wake the loop early (settings were just saved).
    pub fn poke(&self) {
        lock(&self.wake).poke = true;
        self.wake_cv.notify_all();
    }

    // --- device commands (dashboard, tray menu, IPC) --------------------------

    fn with_device<F: FnOnce(&mut Lm360) -> bool>(&self, f: F) -> bool {
        match lock(&self.device).as_mut() {
            Some(d) => f(d),
            None => false,
        }
    }

    pub fn set_brightness(&self, level: u8) -> bool {
        self.with_device(|d| d.set_brightness(level.min(100)))
    }

    pub fn zen_toggle(&self) -> bool {
        self.with_device(|d| d.zen_toggle())
    }

    fn show_overlay(&self, overlay: Overlay) {
        *lock(&self.overlay) = Some(overlay);
        self.poke();
    }

    pub fn show_test_pattern(&self, seconds: f64) {
        self.show_overlay(Overlay::TestPattern { expires: Instant::now() + Duration::from_secs_f64(seconds.clamp(1.0, 300.0)) });
    }

    pub fn show_image(&self, path: &std::path::Path) -> Result<(), String> {
        let rot = self.current_rotation();
        let size = util::canvas_size(rot);
        let px = Clip::open(path, "cover", size, (0, 0, 0), 100, 1)?.frame(0)?;
        self.show_overlay(Overlay::Frame { frame: util::bgra_to_frame(&px, size.0, size.1, size.0 * 4, rot), rotation: rot, label: "[IMAGE] Showing a picture" });
        Ok(())
    }

    pub fn show_solid(&self, c: (u8, u8, u8)) {
        let v = ((c.0 as u16 >> 3) << 11) | ((c.1 as u16 >> 2) << 5) | (c.2 as u16 >> 3);
        let frame = v.to_le_bytes().repeat(util::NATIVE_W * util::NATIVE_H);
        self.show_overlay(Overlay::Frame { frame, rotation: self.current_rotation(), label: "[SOLID] Showing a solid color" });
    }

    pub fn monitor_mode(&self) {
        *lock(&self.overlay) = None;
        self.poke();
    }

    // --- readings for the dashboard ---------------------------------------------

    /// Run `f` with the latest sensor snapshot and history.
    pub fn with_readings<R, F: FnOnce(&Snapshot, &History) -> R>(&self, f: F) -> R {
        let d = lock(&self.data);
        f(&d.snap, &d.history)
    }

    /// Sensor list for the dashboard, "Common" group first.
    pub fn sensors_json(&self) -> Json {
        self.with_readings(|snap, _| {
            let mut rows: Vec<(&String, &crate::sensors::Reading)> = snap.iter().collect();
            rows.sort_by(|a, b| (a.1.group != "Common", &a.1.group, &a.1.name).cmp(&(b.1.group != "Common", &b.1.group, &b.1.name)));
            Json::Arr(
                rows.into_iter()
                    .map(|(id, r)| {
                        json::object([
                            ("id", id.as_str().into()),
                            ("name", r.name.as_str().into()),
                            ("group", r.group.as_str().into()),
                            ("value", r.value.filter(|v| v.is_finite()).into()),
                            ("unit", r.unit.as_str().into()),
                        ])
                    })
                    .collect(),
            )
        })
    }

    // --- the loop ---------------------------------------------------------------

    fn try_connect(&self, st: &mut LoopState) {
        if self.connected() || Instant::now() < st.next_connect {
            return;
        }
        st.next_connect = Instant::now() + RECONNECT;
        let mut dev = lock(&self.device);
        if self.stopped() {
            return;
        }
        match Lm360::open() {
            Ok(mut d) => {
                d.init();
                *dev = Some(d);
                self.connected.store(true, Ordering::SeqCst);
                st.applied_brightness = None;
                st.waiting_logged = false;
                log!("Connected to LM360");
            }
            Err(e) => {
                if !st.waiting_logged {
                    log!("Waiting for LM360 ({}); retrying every {}s", e, RECONNECT.as_secs());
                    st.waiting_logged = true;
                }
            }
        }
    }

    fn lost_device(&self, st: &mut LoopState) {
        log!("Lost connection to LM360");
        *lock(&self.device) = None;
        self.connected.store(false, Ordering::SeqCst);
        st.next_connect = Instant::now() + Duration::from_secs(2);
    }

    fn collect(&self, st: &mut LoopState, interval: f64) {
        let due = {
            let d = lock(&self.data);
            d.last_collect.map(|t| t.elapsed().as_secs_f64() >= interval - 0.05).unwrap_or(true)
        };
        if due {
            let snap = st.hub.collect(); // slow part (HTTP to LibreHardwareMonitor) outside the lock
            let mut d = lock(&self.data);
            d.history.add(&snap, util::now_secs());
            d.snap = snap;
            d.last_collect = Some(Instant::now());
        }
    }

    /// Draw and send one frame. Returns seconds until the next one.
    pub fn step(&self) -> f64 {
        let mut st = lock(&self.state);
        let st = &mut *st;
        let cfg = st.watcher.poll().cloned();
        let wanted = cfg
            .as_ref()
            .and_then(|c| c.get("rotation"))
            .and_then(|r| r.as_f64())
            .and_then(util::normalize_rotation)
            .unwrap_or(self.cli_rotation);
        if wanted != self.rotation.swap(wanted, Ordering::SeqCst) {
            log!("Screen: {} ({}°)", util::rotation_name(wanted), wanted);
        }

        self.try_connect(st);
        let wanted_brightness = cfg.as_ref().and_then(|c| c.get("brightness")).and_then(|b| b.as_f64()).map(|b| b.clamp(0.0, 100.0) as u8);
        if let Some(b) = wanted_brightness {
            if self.connected() && Some(b) != st.applied_brightness && self.set_brightness(b) {
                st.applied_brightness = Some(b);
                log!("Brightness set to {}", b);
            }
        }

        let interval = cfg.as_ref().map(|c| c.num_or("interval", self.default_interval)).unwrap_or(self.default_interval).clamp(0.2, 60.0);
        let mut delay = interval;
        self.collect(st, interval); // sensors keep updating for the dashboard even without a device

        let overlay = {
            let mut o = lock(&self.overlay);
            let done = match o.as_ref() {
                Some(Overlay::TestPattern { expires }) => Instant::now() >= *expires,
                Some(Overlay::Frame { rotation, .. }) => *rotation != wanted, // drawn for the other orientation
                None => false,
            };
            if done {
                *o = None;
            }
            match o.as_ref() {
                Some(Overlay::TestPattern { .. }) => {
                    let size = util::canvas_size(wanted);
                    let (px, stride) = render::test_pattern(size, wanted).pixels();
                    Some((util::bgra_to_frame(&px, size.0, size.1, stride, wanted), "[TEST PATTERN] Showing color bars".to_string()))
                }
                Some(Overlay::Frame { frame, label, .. }) => Some((frame.clone(), label.to_string())),
                None => None,
            }
        };

        let mode = cfg.as_ref().map(|c| c.str_or("mode", "monitor").to_string()).unwrap_or_else(|| "monitor".into());
        let (frame, state) = if let Some(f) = overlay {
            f
        } else if mode == "media" {
            let media = cfg.as_ref().and_then(|c| c.get("media")).cloned().unwrap_or_else(Json::obj);
            let (f, d) = st.player.next(&media, wanted);
            delay = d;
            let n = st.player.items.len();
            (f, format!("[MEDIA] {} item(s), showing #{}", n, if n > 0 { st.player.index + 1 } else { 0 }))
        } else {
            let size = util::canvas_size(wanted);
            let mut r = lock(&self.renderer);
            let layout = r.layout(cfg.as_ref(), util::canvas_shape(wanted));
            let mut canvas = {
                let d = lock(&self.data);
                r.render(&layout, &d.snap, Some(&d.history), util::now_secs(), size)
            };
            if let Some(fd) = r.frame_delay(&layout, size) {
                delay = interval.min(fd.max(1.0 / 15.0)); // animated background, capped at 15 fps
            }
            drop(r);
            let (px, stride) = canvas.pixels();
            st.frames += 1;
            let i = &st.hub.info;
            (
                util::bgra_to_frame(&px, size.0, size.1, stride, wanted),
                format!("[MONITOR] CPU {:.1}°C ({:.1}%) | GPU {:.1}°C | {:.2} GHz", i.cpu_temp, i.cpu_percent, i.gpu_temp, i.cpu_freq),
            )
        };

        if self.connected() {
            let ok = self.with_device(|d| d.send_frame(&frame));
            if ok {
                st.failures = 0;
            } else {
                st.failures += 1;
                if st.failures >= 3 {
                    st.failures = 0;
                    self.lost_device(st);
                }
            }
        }

        let kind = state.split(']').next().unwrap_or("").to_string();
        if kind != st.last_state || st.last_log.map(|t| t.elapsed() >= Duration::from_secs(300)).unwrap_or(true) {
            log!("{}{}", state, if self.connected() { "" } else { " (cooler not connected)" });
            st.last_log = Some(Instant::now());
            st.last_state = kind;
        }
        delay
    }

    /// Run until `shutdown`.
    pub fn run(&self) {
        log!("Settings file: {}", config::config_path().display());
        while !self.stopped() {
            let started = Instant::now();
            let delay = self.step();
            let wait = Duration::from_secs_f64(delay.max(0.0)).saturating_sub(started.elapsed());
            let deadline = Instant::now() + wait;
            let mut w = lock(&self.wake);
            while !w.stop && !w.poke {
                let now = Instant::now();
                if now >= deadline {
                    break;
                }
                w = self.wake_cv.wait_timeout(w, deadline - now).unwrap_or_else(|e| e.into_inner()).0;
            }
            w.poke = false;
        }
    }

    /// Stop the loop, optionally blank the screen, and release the cooler.
    pub fn shutdown(&self, blank: bool) {
        lock(&self.wake).stop = true;
        self.wake_cv.notify_all();
        let mut dev = lock(&self.device);
        if let Some(d) = dev.as_mut() {
            if blank {
                d.send_frame(&vec![0u8; util::FRAME_BYTES]);
            }
        }
        *dev = None;
        self.connected.store(false, Ordering::SeqCst);
    }
}
