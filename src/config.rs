//! Settings file shared with the dashboard.

use crate::json::{self, Json};
use crate::util;
use std::path::PathBuf;
use std::time::SystemTime;

pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("DEEPCOOL_LM_DATA") {
        if !d.is_empty() {
            return PathBuf::from(d);
        }
    }
    if cfg!(windows) {
        let base = std::env::var("PROGRAMDATA").unwrap_or_else(|_| r"C:\ProgramData".into());
        PathBuf::from(base).join("deepcool-lm")
    } else {
        PathBuf::from("/etc/deepcool-lm")
    }
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

pub fn media_dir() -> PathBuf {
    data_dir().join("media")
}

pub const MEDIA_EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "gif", "bmp"];

pub fn is_media_name(name: &str) -> bool {
    name.rsplit_once('.').map(|(_, e)| MEDIA_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str())).unwrap_or(false)
}

/// Path of a media item (a file name inside the media folder; never escapes it).
pub fn media_path(item: &str) -> PathBuf {
    let name = item.rsplit(['/', '\\']).next().unwrap_or(item);
    media_dir().join(name)
}

const DEFAULT_CONFIG: &str = r##"{
  "version": 1,
  "rotation": null,
  "brightness": null,
  "mode": "monitor",
  "interval": 1.0,
  "layouts": {},
  "media": {"items": [], "fit": "cover", "slide_seconds": 10, "background": "#000000"}
}"##;

pub const WIDGET_DEFAULTS: &str = r##"{
  "text":  {"w": 120, "h": 32, "text": "{value}", "size": 24, "font": "", "bold": false, "italic": false, "align": "left", "decimals": 0, "color": "#e6e6f0"},
  "bar":   {"w": 200, "h": 16, "min": 0, "max": 100, "color": "auto", "bg": "#1e1e26", "vertical": false},
  "gauge": {"w": 100, "h": 100, "min": 0, "max": 100, "color": "auto", "bg": "#1e1e26", "thickness": 10,
            "show_value": true, "decimals": 0, "suffix": "", "text_color": "#e6e6f0",
            "font": "", "bold": true, "italic": false},
  "graph": {"w": 200, "h": 60, "min": 0, "max": 100, "auto_scale": false, "color": "auto",
            "bg": "#16161c", "seconds": 60, "fill": true},
  "panel": {"w": 150, "h": 100, "fill": "", "outline": "#282832", "radius": 8, "border": 2}
}"##;

pub const DEFAULT_LAYOUTS: &str = r##"{
  "landscape": {"background": "#0e0e12", "widgets": [
    {"type": "panel", "x": 12, "y": 10, "w": 296, "h": 115},
    {"type": "text", "x": 20, "y": 18, "w": 150, "h": 34, "text": "⚙ CPU", "size": 26, "bold": true, "color": "#6e6e82"},
    {"type": "text", "x": 170, "y": 14, "w": 130, "h": 46, "sensor": "cpu.temp", "text": "{value}°", "size": 40, "bold": true, "align": "right", "color": "auto"},
    {"type": "text", "x": 20, "y": 64, "w": 280, "h": 26, "sensor": "cpu.usage", "text": "Usage: {value}% • {cpu.clock|2} GHz", "size": 20, "color": "#828296"},
    {"type": "bar", "x": 23, "y": 93, "w": 274, "h": 18, "sensor": "cpu.usage"},
    {"type": "panel", "x": 12, "y": 135, "w": 296, "h": 95},
    {"type": "text", "x": 20, "y": 143, "w": 150, "h": 34, "text": "▣ GPU", "size": 26, "bold": true, "color": "#6e6e82"},
    {"type": "text", "x": 170, "y": 139, "w": 130, "h": 46, "sensor": "gpu.temp", "text": "{value}°", "size": 40, "bold": true, "align": "right", "color": "auto"},
    {"type": "bar", "x": 23, "y": 200, "w": 274, "h": 18, "sensor": "gpu.temp"}
  ]},
  "portrait": {"background": "#0e0e12", "widgets": [
    {"type": "panel", "x": 10, "y": 8, "w": 220, "h": 148},
    {"type": "text", "x": 18, "y": 16, "w": 120, "h": 34, "text": "⚙ CPU", "size": 26, "bold": true, "color": "#6e6e82"},
    {"type": "text", "x": 110, "y": 12, "w": 112, "h": 46, "sensor": "cpu.temp", "text": "{value}°", "size": 40, "bold": true, "align": "right", "color": "auto"},
    {"type": "text", "x": 20, "y": 64, "w": 100, "h": 26, "text": "Usage", "size": 20, "color": "#828296"},
    {"type": "text", "x": 120, "y": 64, "w": 100, "h": 26, "sensor": "cpu.usage", "text": "{value}%", "size": 20, "align": "right", "color": "#c8c8d7"},
    {"type": "text", "x": 20, "y": 90, "w": 100, "h": 26, "text": "Clock", "size": 20, "color": "#828296"},
    {"type": "text", "x": 100, "y": 90, "w": 120, "h": 26, "sensor": "cpu.clock", "text": "{value} GHz", "size": 20, "align": "right", "decimals": 2, "color": "#c8c8d7"},
    {"type": "bar", "x": 20, "y": 124, "w": 200, "h": 16, "sensor": "cpu.usage"},
    {"type": "panel", "x": 10, "y": 164, "w": 220, "h": 148},
    {"type": "text", "x": 18, "y": 172, "w": 120, "h": 34, "text": "▣ GPU", "size": 26, "bold": true, "color": "#6e6e82"},
    {"type": "text", "x": 110, "y": 168, "w": 112, "h": 46, "sensor": "gpu.temp", "text": "{value}°", "size": 40, "bold": true, "align": "right", "color": "auto"},
    {"type": "text", "x": 20, "y": 220, "w": 100, "h": 26, "text": "Usage", "size": 20, "color": "#828296"},
    {"type": "text", "x": 120, "y": 220, "w": 100, "h": 26, "sensor": "gpu.usage", "text": "{value}%", "size": 20, "align": "right", "color": "#c8c8d7"},
    {"type": "text", "x": 20, "y": 246, "w": 100, "h": 26, "text": "RAM", "size": 20, "color": "#828296"},
    {"type": "text", "x": 120, "y": 246, "w": 100, "h": 26, "sensor": "ram.usage", "text": "{value}%", "size": 20, "align": "right", "color": "#c8c8d7"},
    {"type": "bar", "x": 20, "y": 280, "w": 200, "h": 16, "sensor": "gpu.temp"}
  ]}
}"##;

pub fn widget_defaults() -> Json {
    json::parse(WIDGET_DEFAULTS).expect("built-in widget defaults")
}

pub fn default_layouts() -> Json {
    json::parse(DEFAULT_LAYOUTS).expect("built-in layouts")
}

/// Fill in defaults, validate values and migrate older settings.
pub fn normalize(data: &Json) -> Json {
    let mut cfg = json::parse(DEFAULT_CONFIG).expect("built-in config");
    if let Json::Obj(items) = data {
        for (k, v) in items {
            if k == "media" && v.is_obj() {
                if let (Some(Json::Obj(dst)), Json::Obj(src)) = (cfg.get_mut("media"), v) {
                    for (mk, mv) in src {
                        if let Some(slot) = dst.iter_mut().find(|(k2, _)| k2 == mk) {
                            slot.1 = mv.clone();
                        } else {
                            dst.push((mk.clone(), mv.clone()));
                        }
                    }
                }
            } else {
                cfg.set(k, v.clone());
            }
        }
    }
    let mode = cfg.str_or("mode", "monitor");
    if mode != "monitor" && mode != "media" {
        cfg.set("mode", "monitor".into());
    }
    let interval = cfg.num_or("interval", 1.0).clamp(0.2, 60.0);
    cfg.set("interval", interval.into());

    let mut rotation = cfg.get("rotation").and_then(|v| v.as_f64()).and_then(util::normalize_rotation);
    // Older settings had a separate 'portrait' panel type; its equivalent is a quarter turn
    if cfg.remove("orientation").and_then(|v| v.as_str().map(|s| s == "portrait")).unwrap_or(false) {
        rotation = Some((rotation.unwrap_or(0) + 90) % 360);
    }
    cfg.set("rotation", rotation.map(|r| Json::Num(r as f64)).unwrap_or(Json::Null));

    let brightness = cfg.get("brightness").and_then(|v| v.as_f64()).map(|b| b.clamp(0.0, 100.0).round());
    cfg.set("brightness", brightness.into());
    if !cfg.get("layouts").map(|l| l.is_obj()).unwrap_or(false) {
        cfg.set("layouts", Json::obj());
    }
    cfg
}

pub fn load() -> Result<Option<Json>, String> {
    match std::fs::read_to_string(config_path()) {
        Ok(text) => json::parse(&text).map(|v| Some(normalize(&v))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Settings as the dashboard expects them (never None).
pub fn load_or_default() -> Json {
    load().ok().flatten().unwrap_or_else(|| normalize(&Json::obj()))
}

pub fn save(cfg: &Json) -> Result<(), String> {
    let path = config_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, normalize(cfg).to_string_pretty()).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            format!("No permission to write {}. Re-run install.ps1 as administrator to fix the folder permissions.", path.display())
        } else {
            e.to_string()
        }
    })
}

/// Reloads the settings file when it changes; keeps the last good copy on errors.
pub struct ConfigWatcher {
    stamp: Option<(SystemTime, u64)>,
    pub config: Option<Json>,
}

impl ConfigWatcher {
    pub fn new() -> Self {
        ConfigWatcher { stamp: None, config: None }
    }

    pub fn poll(&mut self) -> Option<&Json> {
        let stamp = std::fs::metadata(config_path()).ok().and_then(|m| Some((m.modified().ok()?, m.len())));
        if stamp != self.stamp {
            self.stamp = stamp;
            if stamp.is_none() {
                self.config = None;
            } else {
                match load() {
                    Ok(cfg) => {
                        crate::log!("Settings loaded from {}", config_path().display());
                        self.config = cfg;
                    }
                    Err(e) => {
                        crate::log!("Settings file unreadable ({}); keeping previous settings", e);
                        self.stamp = None; // half-written file: try again next time
                    }
                }
            }
        }
        self.config.as_ref()
    }
}

// ---------------------------------------------------------------------------
// Layouts

/// A widget with its type's defaults filled in and coordinates as integers.
pub fn normalize_widget(w: &Json, defaults: &Json) -> Json {
    let kind = w.str_or("type", "text");
    let kind = if defaults.get(kind).is_some() { kind } else { "text" };
    let mut out = json::object([("type", kind.into()), ("x", 0i64.into()), ("y", 0i64.into()), ("sensor", "".into())]);
    if let Some(Json::Obj(d)) = defaults.get(kind) {
        for (k, v) in d {
            out.set(k, v.clone());
        }
    }
    if let Json::Obj(items) = w {
        for (k, v) in items {
            if !v.is_null() {
                out.set(k, v.clone());
            }
        }
    }
    out.set("type", kind.into());
    for key in ["x", "y", "w", "h"] {
        let v = out.num_or(key, 0.0).round();
        let v = if key == "w" || key == "h" { v.max(4.0) } else { v };
        out.set(key, v.into());
    }
    out
}

pub struct Layout {
    pub background: String,
    pub background_image: String,
    pub background_fit: String,
    pub background_brightness: f64,
    pub widgets: Vec<Json>,
}

pub fn get_layout(cfg: Option<&Json>, shape: &str, defaults: &Json, widget_defaults: &Json) -> Layout {
    let stored = cfg.and_then(|c| c.get("layouts")).and_then(|l| l.get(shape));
    let layout = match stored {
        Some(l) if l.get("widgets").and_then(|w| w.as_array()).is_some() => l,
        _ => defaults.get(shape).expect("default layout"),
    };
    let fit = layout.str_or("background_fit", "cover");
    Layout {
        background: layout.str_or("background", "#0e0e12").to_string(),
        background_image: layout.str_or("background_image", "").to_string(),
        background_fit: if ["cover", "contain", "stretch"].contains(&fit) { fit.to_string() } else { "cover".into() },
        background_brightness: layout.num_or("background_brightness", 100.0).clamp(0.0, 100.0),
        widgets: layout
            .get("widgets")
            .and_then(|w| w.as_array())
            .map(|ws| ws.iter().filter(|w| w.is_obj()).map(|w| normalize_widget(w, widget_defaults)).collect())
            .unwrap_or_default(),
    }
}

/// Layout as JSON (for the dashboard, which always gets both shapes filled in).
pub fn layout_json(l: &Layout) -> Json {
    json::object([
        ("background", l.background.clone().into()),
        ("background_image", l.background_image.clone().into()),
        ("background_fit", l.background_fit.clone().into()),
        ("background_brightness", l.background_brightness.into()),
        ("widgets", Json::Arr(l.widgets.clone())),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_and_defaults() {
        let cases = [
            (r#"{"orientation":"portrait"}"#, Some(90.0)),
            (r#"{"orientation":"portrait","rotation":90}"#, Some(180.0)),
            (r#"{"orientation":"landscape","rotation":90}"#, Some(90.0)),
            (r#"{"rotation":-90}"#, Some(270.0)),
            (r#"{"rotation":45}"#, None),
            (r#"{}"#, None),
        ];
        for (src, want) in cases {
            let c = normalize(&json::parse(src).unwrap());
            assert_eq!(c.get("rotation").unwrap().as_f64(), want, "{}", src);
            assert!(c.get("orientation").is_none());
        }
        let c = normalize(&json::parse(r#"{"mode":"bogus","interval":0.01,"brightness":150,"media":{"fit":"contain"}}"#).unwrap());
        assert_eq!(c.str_or("mode", ""), "monitor");
        assert_eq!(c.num_or("interval", 0.0), 0.2);
        assert_eq!(c.num_or("brightness", 0.0), 100.0);
        assert_eq!(c.get("media").unwrap().str_or("fit", ""), "contain");
        assert_eq!(c.get("media").unwrap().num_or("slide_seconds", 0.0), 10.0);
    }

    #[test]
    fn layouts_fall_back_to_defaults() {
        let d = default_layouts();
        let wd = widget_defaults();
        let l = get_layout(None, "portrait", &d, &wd);
        assert_eq!(l.widgets.len(), 16);
        assert_eq!(l.widgets[4].num_or("h", 0.0), 26.0);
        let bar = &l.widgets[7];
        assert_eq!(bar.str_or("color", ""), "auto"); // filled from widget defaults
        let cfg = json::parse(r#"{"layouts":{"landscape":{"widgets":[{"type":"bogus","x":3.6}],"background_fit":"weird"}}}"#).unwrap();
        let l = get_layout(Some(&cfg), "landscape", &d, &wd);
        assert_eq!(l.widgets[0].str_or("type", ""), "text");
        assert_eq!(l.widgets[0].num_or("x", 0.0), 4.0);
        assert_eq!(l.background_fit, "cover");
    }

    #[test]
    fn media_paths_stay_inside() {
        assert!(media_path("..\\..\\windows\\evil.png").ends_with("evil.png"));
        assert!(media_path("../x.gif").starts_with(media_dir()));
        assert!(is_media_name("a.GIF") && !is_media_name("a.exe"));
    }
}
