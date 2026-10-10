//! Widget renderer: draws a layout (text, bars, gauges, graphs, panels over an
//! optional image/GIF background) onto a `Canvas`. Port of the Python driver's
//! render_layout / _draw_widget / format_template.

use crate::canvas::{Canvas, FontSpec, Rgb};
use crate::config::{self, Layout};
use crate::json::Json;
use crate::media::Clip;
use crate::platform;
use crate::sensors::{History, Reading, Snapshot};
use crate::util::{MONTHS, WEEKDAYS};

pub const DEFAULT_TEXT: Rgb = (230, 230, 240);
pub const DEFAULT_BG: Rgb = (14, 14, 18);

/// "#rrggbb" to RGB, or `default` for anything else.
pub fn parse_rgb(color: &str, default: Rgb) -> Rgb {
    let c = color.trim();
    let b = c.as_bytes();
    if b.len() == 7 && b[0] == b'#' && b[1..].iter().all(|x| x.is_ascii_hexdigit()) {
        let h = |i: usize| u8::from_str_radix(&c[i..i + 2], 16).unwrap_or(0);
        (h(1), h(3), h(5))
    } else {
        default
    }
}

fn color_of(w: &Json, key: &str, default: Rgb) -> Rgb {
    parse_rgb(w.str_or(key, ""), default)
}

pub fn temp_color(t: f64) -> Rgb {
    if t < 40.0 {
        (100, 200, 255)
    } else if t < 60.0 {
        (100, 255, 100)
    } else if t < 75.0 {
        (255, 220, 50)
    } else if t < 85.0 {
        (255, 140, 0)
    } else {
        (255, 50, 50)
    }
}

fn is_symbol(ch: char) -> bool {
    (0x2190..=0x2BFF).contains(&(ch as u32))
}

fn decimals_of(v: &Json) -> usize {
    match v {
        Json::Num(n) if n.is_finite() => n.clamp(0.0, 4.0) as usize,
        Json::Str(s) => s.trim().parse::<f64>().map(|n| n.clamp(0.0, 4.0) as usize).unwrap_or(0),
        _ => 0,
    }
}

fn format_value(v: Option<f64>, decimals: usize) -> String {
    match v {
        Some(x) if x.is_finite() => format!("{:.*}", decimals, x),
        _ => "--".into(),
    }
}

/// Expand {value}, {unit}, {name}, {time}, {seconds}, {date}, {day},
/// {sensor.id} and {sensor.id|decimals}.
pub fn format_template(template: &str, widget: &Json, snap: &Snapshot) -> String {
    let entry = snap.get(widget.str_or("sensor", ""));
    let decimals = widget.get("decimals").map(decimals_of).unwrap_or(0);
    let t = platform::local_time();
    let token = |name: &str| -> Option<String> {
        Some(match name {
            "value" => entry.map(|e| format_value(e.value, decimals)).unwrap_or_else(|| "--".into()),
            "unit" => entry.map(|e| e.unit.clone()).unwrap_or_default(),
            "name" => entry.map(|e| e.name.clone()).unwrap_or_default(),
            "time" => format!("{:02}:{:02}", t.hour, t.minute),
            "seconds" => format!("{:02}:{:02}:{:02}", t.hour, t.minute, t.second),
            "date" => format!("{} {:02}", MONTHS[(t.month.clamp(1, 12) - 1) as usize], t.day),
            "day" => WEEKDAYS[(t.weekday % 7) as usize].to_string(),
            _ => return None,
        })
    };

    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        // A placeholder is {name} or {name|d}: name has no braces or pipes, d is one digit
        let parsed = after.find(['{', '}']).filter(|&i| after.as_bytes()[i] == b'}').and_then(|close| {
            let inner = &after[..close];
            let (name, dec) = match inner.split_once('|') {
                Some((n, d)) if d.len() == 1 && d.as_bytes()[0].is_ascii_digit() => (n, Some(d)),
                Some(_) => return None,
                None => (inner, None),
            };
            if name.is_empty() {
                return None;
            }
            Some((close, name, dec))
        });
        match parsed {
            Some((close, name, dec)) => {
                let replacement = if dec.is_none() { token(name) } else { None }.or_else(|| {
                    snap.get(name).map(|e| {
                        let d = dec.map(|d| (d.as_bytes()[0] - b'0').min(4) as usize).unwrap_or(decimals);
                        format_value(e.value, d)
                    })
                });
                match replacement {
                    Some(r) => out.push_str(&r),
                    None => out.push_str(&rest[open..open + close + 2]),
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn fraction(lo: f64, hi: f64, value: Option<f64>) -> f64 {
    match value {
        Some(v) if v.is_finite() && hi != lo => ((v - lo) / (hi - lo)).clamp(0.0, 1.0),
        _ => 0.0,
    }
}

fn widget_fraction(w: &Json, value: Option<f64>) -> f64 {
    fraction(w.num_or("min", 0.0), w.num_or("max", 100.0), value)
}

fn auto_color(entry: Option<&Reading>, frac: f64) -> Rgb {
    if let Some(e) = entry {
        if e.unit == "°C" {
            if let Some(v) = e.value.filter(|v| v.is_finite()) {
                return temp_color(v);
            }
        }
    }
    if frac < 0.75 {
        (100, 180, 255)
    } else {
        (255, 150, 50)
    }
}

fn widget_color(w: &Json, key: &str, entry: Option<&Reading>, frac: f64) -> Rgb {
    if w.str_or(key, "") == "auto" {
        auto_color(entry, frac)
    } else {
        color_of(w, key, DEFAULT_TEXT)
    }
}

fn blend(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let m = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t) as u8;
    (m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

/// Box and style for a run of text.
pub struct TextBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub font: String,
    pub align: String,
}

impl TextBox {
    fn from_widget(w: &Json) -> TextBox {
        TextBox {
            x: w.num_or("x", 0.0) as f32,
            y: w.num_or("y", 0.0) as f32,
            w: w.num_or("w", 0.0) as f32,
            h: w.num_or("h", 0.0) as f32,
            size: w.num_or("size", 24.0).max(6.0).floor() as f32,
            bold: w.bool_or("bold", false),
            italic: w.bool_or("italic", false),
            font: w.str_or("font", "").trim().to_string(),
            align: w.str_or("align", "left").to_string(),
        }
    }
}

/// Draw text vertically centred in its box, aligned left/center/right, with
/// symbols (arrows, shapes, dingbats) in the symbol font. Text too wide for the
/// box is shrunk to fit.
pub fn draw_text_runs(c: &mut dyn Canvas, b: &TextBox, text: &str, color: Rgb) {
    draw_text_inner(c, b, b.size, text, color, false)
}

fn draw_text_inner(c: &mut dyn Canvas, b: &TextBox, size: f32, text: &str, color: Rgb, fitted: bool) {
    let mut runs: Vec<(bool, String)> = Vec::new();
    for ch in text.chars() {
        let sym = is_symbol(ch);
        match runs.last_mut() {
            Some((s, t)) if *s == sym => t.push(ch),
            _ => runs.push((sym, ch.to_string())),
        }
    }
    let main = FontSpec { family: &b.font, size, bold: b.bold, italic: b.italic, symbol: false };
    let spec = |sym: bool| if sym { FontSpec { bold: true, italic: false, symbol: true, ..main } } else { main };
    let (ascent, descent) = c.font_metrics(&main);
    let widths: Vec<f32> = runs.iter().map(|(sym, t)| c.text_width(t, &spec(*sym))).collect();
    let total: f32 = widths.iter().sum();
    if total > b.w && b.w > 0.0 && size > 6.0 && !fitted {
        let smaller = (size * b.w / total).floor().max(6.0);
        return draw_text_inner(c, b, smaller, text, color, true);
    }
    let mut x = b.x;
    match b.align.as_str() {
        "center" => x += (b.w - total) / 2.0,
        "right" => x += b.w - total,
        _ => {}
    }
    let baseline = b.y + (b.h + ascent - descent) / 2.0;
    for ((sym, t), tw) in runs.iter().zip(widths) {
        c.draw_text(x, baseline, t, &spec(*sym), color);
        x += tw;
    }
}

// ---------------------------------------------------------------------------

#[derive(PartialEq, Clone)]
struct BgKey {
    path: std::path::PathBuf,
    mtime: Option<std::time::SystemTime>,
    len: u64,
    fit: String,
    size: (usize, usize),
    level: u32,
    background: String,
}

/// Renders layouts; keeps the last couple of background images open.
pub struct Renderer {
    pub widget_defaults: Json,
    pub default_layouts: Json,
    cache: Vec<(BgKey, Option<Clip>)>,
}

impl Renderer {
    pub fn new() -> Self {
        Renderer { widget_defaults: config::widget_defaults(), default_layouts: config::default_layouts(), cache: Vec::new() }
    }

    pub fn layout(&self, cfg: Option<&Json>, shape: &str) -> Layout {
        config::get_layout(cfg, shape, &self.default_layouts, &self.widget_defaults)
    }

    fn background(&mut self, layout: &Layout, size: (usize, usize)) -> Option<&mut Clip> {
        if layout.background_image.is_empty() {
            return None;
        }
        let path = config::media_path(&layout.background_image);
        let meta = std::fs::metadata(&path).ok()?;
        let key = BgKey {
            path: path.clone(),
            mtime: meta.modified().ok(),
            len: meta.len(),
            fit: layout.background_fit.clone(),
            size,
            level: layout.background_brightness.round().clamp(0.0, 100.0) as u32,
            background: layout.background.clone(),
        };
        let pos = match self.cache.iter().position(|(k, _)| *k == key) {
            Some(p) => p,
            None => {
                if self.cache.len() >= 2 {
                    self.cache.remove(0);
                }
                let base = parse_rgb(&layout.background, DEFAULT_BG);
                let clip = Clip::open(&path, &layout.background_fit, size, base, key.level, 300)
                    .map_err(|e| log!("Can't use background {}: {}", path.display(), e))
                    .ok();
                self.cache.push((key, clip));
                self.cache.len() - 1
            }
        };
        self.cache[pos].1.as_mut()
    }

    /// Shortest frame time of an animated background, or None for a still one.
    pub fn frame_delay(&mut self, layout: &Layout, size: (usize, usize)) -> Option<f64> {
        self.background(layout, size).and_then(|b| b.min_delay())
    }

    /// Draw a layout at `size` (the upright canvas size).
    pub fn render(&mut self, layout: &Layout, snap: &Snapshot, history: Option<&History>, now: f64, size: (usize, usize)) -> Box<dyn Canvas> {
        let mut c = platform::new_canvas(size.0, size.1);
        let mut drew_bg = false;
        if let Some(clip) = self.background(layout, size) {
            let i = clip.index_at(now);
            if let Ok(px) = clip.frame(i) {
                c.blit_bgra(&px);
                drew_bg = true;
            }
        }
        if !drew_bg {
            c.fill_all(parse_rgb(&layout.background, DEFAULT_BG));
        }
        for w in &layout.widgets {
            draw_widget(c.as_mut(), w, snap, history, now);
        }
        c
    }
}

fn draw_widget(c: &mut dyn Canvas, w: &Json, snap: &Snapshot, history: Option<&History>, now: f64) {
    let kind = w.str_or("type", "text");
    let (x0, y0) = (w.num_or("x", 0.0) as f32, w.num_or("y", 0.0) as f32);
    let (ww, wh) = (w.num_or("w", 4.0) as f32, w.num_or("h", 4.0) as f32);
    let (x1, y1) = (x0 + ww, y0 + wh);
    let entry = snap.get(w.str_or("sensor", ""));
    let value = entry.and_then(|e| e.value);
    let track_default: Rgb = (30, 30, 38);

    match kind {
        "panel" => {
            let radius = w.num_or("radius", 8.0).max(0.0) as f32;
            let fill = w.str_or("fill", "");
            if !fill.is_empty() {
                let f = parse_rgb(fill, (0, 0, 0));
                if parse_rgb(fill, (1, 2, 3)) == f {
                    c.fill_rounded_rect(x0, y0, x1, y1, radius, f);
                }
            }
            let outline = w.str_or("outline", "");
            let border = w.num_or("border", 2.0).max(0.0) as f32;
            if !outline.is_empty() && border > 0.0 {
                let o = parse_rgb(outline, (0, 0, 0));
                if parse_rgb(outline, (1, 2, 3)) == o {
                    // like PIL, the outline sits inside the box
                    let hw = border / 2.0;
                    c.stroke_rounded_rect(x0 + hw, y0 + hw, x1 - hw, y1 - hw, (radius - hw).max(0.0), border, o);
                }
            }
        }
        "text" => {
            let text = format_template(w.str_or("text", "{value}"), w, snap);
            let frac = widget_fraction(w, value);
            let color = widget_color(w, "color", entry, frac);
            draw_text_runs(c, &TextBox::from_widget(w), &text, color);
        }
        "bar" => {
            let frac = widget_fraction(w, value) as f32;
            let color = widget_color(w, "color", entry, frac as f64);
            let radius = (ww.min(wh) / 2.0).floor();
            c.fill_rounded_rect(x0, y0, x1, y1, radius, color_of(w, "bg", track_default));
            if w.bool_or("vertical", false) {
                let fill_h = (wh * frac).floor();
                if fill_h >= radius * 2.0 && fill_h > 0.0 {
                    c.fill_rounded_rect(x0, y1 - fill_h, x1, y1, radius, color);
                }
            } else {
                let fill_w = (ww * frac).floor();
                if fill_w >= radius * 2.0 && fill_w > 0.0 {
                    c.fill_rounded_rect(x0, y0, x0 + fill_w, y1, radius, color);
                }
            }
        }
        "gauge" => {
            let frac = widget_fraction(w, value);
            let color = widget_color(w, "color", entry, frac);
            let d = ww.min(wh);
            let thick = (w.num_or("thickness", 10.0).floor() as f32).min((d / 2.0).floor()).max(2.0);
            let (cx, cy) = (x0 + ww / 2.0, y0 + wh / 2.0);
            let r = d / 2.0 - thick / 2.0;
            let (start, sweep) = (135.0f32, 270.0f32);
            c.arc(cx, cy, r, thick, start, sweep, color_of(w, "bg", track_default), true);
            if frac > 0.0 {
                c.arc(cx, cy, r, thick, start, sweep * frac as f32, color, true);
            }
            if w.bool_or("show_value", true) {
                let decimals = w.get("decimals").map(decimals_of).unwrap_or(0);
                let suffix = match w.get("suffix") {
                    Some(Json::Str(s)) => s.clone(),
                    Some(Json::Num(n)) => format!("{}", n),
                    _ => String::new(),
                };
                let text = format_value(value, decimals) + &suffix;
                let b = TextBox {
                    x: x0,
                    y: (cy - d * 0.2).floor(),
                    w: ww,
                    h: (d * 0.4).floor(),
                    size: (d * 0.26).floor().max(8.0),
                    bold: w.bool_or("bold", true),
                    italic: w.bool_or("italic", false),
                    font: w.str_or("font", "").trim().to_string(),
                    align: "center".into(),
                };
                draw_text_runs(c, &b, &text, color_of(w, "text_color", DEFAULT_TEXT));
            }
        }
        "graph" => {
            let bg = color_of(w, "bg", (22, 22, 28));
            c.fill_rounded_rect(x0, y0, x1, y1, 4.0, bg);
            let seconds = w.num_or("seconds", 60.0).floor().max(5.0);
            let pts = history.map(|h| h.series(w.str_or("sensor", ""), seconds, now)).unwrap_or_default();
            if pts.len() >= 2 {
                let (lo, hi) = if w.bool_or("auto_scale", false) {
                    let lo = pts.iter().map(|p| p.1).fold(f64::MAX, f64::min);
                    let hi = pts.iter().map(|p| p.1).fold(f64::MIN, f64::max);
                    let pad = if hi > lo { (hi - lo) * 0.1 } else { 1.0 };
                    (lo - pad, hi + pad)
                } else {
                    (w.num_or("min", 0.0), w.num_or("max", 100.0))
                };
                let span = if hi != lo { hi - lo } else { 1.0 };
                let inner_h = (wh - 4.0) as f64;
                let coords: Vec<(f32, f32)> = pts
                    .iter()
                    .map(|&(t, v)| {
                        let px = x0 as f64 + (t - (now - seconds)) / seconds * ww as f64;
                        let py = y1 as f64 - 2.0 - ((v - lo) / span).clamp(0.0, 1.0) * inner_h;
                        (px.clamp(x0 as f64, x1 as f64) as f32, py as f32)
                    })
                    .collect();
                let frac = fraction(lo, hi, Some(pts[pts.len() - 1].1));
                let color = widget_color(w, "color", entry, frac);
                if w.bool_or("fill", true) {
                    let mut poly = coords.clone();
                    poly.push((coords[coords.len() - 1].0, y1 - 1.0));
                    poly.push((coords[0].0, y1 - 1.0));
                    c.fill_polygon(&poly, blend(bg, color, 0.3));
                }
                c.polyline(&coords, 2.0, color);
            }
        }
        _ => {}
    }
}

/// Color bars, a gray ramp and a TOP marker, drawn upright.
pub fn test_pattern(size: (usize, usize), rotation: i32) -> Box<dyn Canvas> {
    let (w, h) = size;
    let mut c = platform::new_canvas(w, h);
    c.fill_all((0, 0, 0));
    let bars: [Rgb; 8] = [(255, 255, 255), (255, 255, 0), (0, 255, 255), (0, 255, 0), (255, 0, 255), (255, 0, 0), (0, 0, 255), (0, 0, 0)];
    let bar_h = h * 45 / 100;
    for (i, col) in bars.iter().enumerate() {
        c.fill_rounded_rect((i * w / 8) as f32, 0.0, ((i + 1) * w / 8) as f32, (bar_h + 1) as f32, 0.0, *col);
    }
    let (ramp_top, ramp_h) = (bar_h + 1, h * 20 / 100);
    for x in 0..w {
        let v = (x as f64 * 255.0 / (w - 1) as f64).round() as u8;
        c.fill_rounded_rect(x as f32, ramp_top as f32, (x + 1) as f32, (ramp_top + ramp_h + 1) as f32, 0.0, (v, v, v));
    }
    let y = ramp_top + ramp_h + 6;
    let third = w / 3;
    for (i, (name, col)) in [("Red", (255, 0, 0)), ("Green", (0, 255, 0)), ("Blue", (0, 0, 255))].iter().enumerate() {
        c.fill_rounded_rect((i * third + 4) as f32, y as f32, ((i + 1) * third - 4 + 1) as f32, (h - 34 + 1) as f32, 0.0, *col);
        let b = TextBox { x: (i * third) as f32, y: y as f32, w: third as f32, h: (h - 34 - y) as f32, size: 18.0, bold: true, italic: false, font: String::new(), align: "center".into() };
        draw_text_runs(c.as_mut(), &b, name, if *name == "Green" { (0, 0, 0) } else { (255, 255, 255) });
    }
    let b = TextBox { x: 0.0, y: (h - 32) as f32, w: w as f32, h: 30.0, size: 18.0, bold: true, italic: false, font: String::new(), align: "center".into() };
    draw_text_runs(c.as_mut(), &b, &format!("▲ TOP   {} × {}   {}°", w, h, rotation), DEFAULT_TEXT);
    c
}

pub fn placeholder(size: (usize, usize), message: &str) -> Box<dyn Canvas> {
    let mut c = platform::new_canvas(size.0, size.1);
    c.fill_all(DEFAULT_BG);
    let b = TextBox { x: 0.0, y: 0.0, w: size.0 as f32, h: size.1 as f32, size: 18.0, bold: false, italic: false, font: String::new(), align: "center".into() };
    draw_text_runs(c.as_mut(), &b, message, (130, 130, 150));
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> Snapshot {
        let mut s = Snapshot::new();
        s.insert("cpu.temp".into(), Reading { name: "CPU".into(), group: "CPU".into(), value: Some(55.44), unit: "°C".into() });
        s.insert("cpu.clock".into(), Reading { name: "Clock".into(), group: "CPU".into(), value: Some(4.6789), unit: "GHz".into() });
        s
    }

    #[test]
    fn templates() {
        let s = snap();
        let w = crate::json::parse(r#"{"sensor": "cpu.temp", "decimals": 1}"#).unwrap();
        assert_eq!(format_template("{value}{unit} {name}", &w, &s), "55.4°C CPU");
        assert_eq!(format_template("{cpu.clock|2} GHz", &w, &s), "4.68 GHz");
        assert_eq!(format_template("{cpu.clock}", &w, &s), "4.7");
        assert_eq!(format_template("{nope} {value|2} {} {a|bc} {", &w, &s), "{nope} {value|2} {} {a|bc} {");
        assert_eq!(format_template("{{value}}", &w, &s), "{55.4}");
        let none = crate::json::parse(r#"{"sensor": "missing"}"#).unwrap();
        assert_eq!(format_template("{value}°", &none, &s), "--°");
        assert_eq!(format_template("{time}", &none, &s).len(), 5);
    }

    #[test]
    fn colors_and_fractions() {
        assert_eq!(parse_rgb("#22A7CF", (0, 0, 0)), (0x22, 0xA7, 0xCF));
        assert_eq!(parse_rgb("red", (1, 2, 3)), (1, 2, 3));
        assert_eq!(fraction(0.0, 100.0, Some(150.0)), 1.0);
        assert_eq!(fraction(0.0, 0.0, Some(5.0)), 0.0);
        assert_eq!(auto_color(snap().get("cpu.temp"), 0.0), (100, 255, 100));
    }

    #[test]
    fn default_layouts_render() {
        let mut r = Renderer::new();
        for (shape, size) in [("landscape", (320, 240)), ("portrait", (240, 320))] {
            let layout = r.layout(None, shape);
            assert!(!layout.widgets.is_empty());
            let mut c = r.render(&layout, &snap(), None, 0.0, size);
            assert_eq!(c.size(), size);
            let (px, stride) = c.pixels();
            assert_eq!(px.len(), stride * size.1);
        }
        let mut t = test_pattern((320, 240), 0);
        let (px, _) = t.pixels();
        assert_eq!(&px[0..3], &[255, 255, 255]); // white bar top-left
    }
}
