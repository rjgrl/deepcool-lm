//! The dashboard's local web server (127.0.0.1 only, every request needs the
//! session key). Serves the built-in dashboard.html and its JSON API.

use crate::canvas::encode_png;
use crate::config;
use crate::engine::Engine;
use crate::json::{self, Json};
use crate::media::Clip;
use crate::render;
use crate::util;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

const DASHBOARD: &str = include_str!("dashboard.html");
const MAX_BODY: usize = 60 * 1024 * 1024; // 40 MB of images, base64-encoded

pub struct Server {
    pub engine: Arc<Engine>,
    pub token: String,
    /// Called after settings are saved (the tray refreshes its menu).
    pub on_saved: Option<Box<dyn Fn() + Send + Sync>>,
}

/// Start serving; returns the dashboard URL.
pub fn start(server: Arc<Server>) -> Result<String, String> {
    let port = std::env::var("DEEPCOOL_LM_TRAY_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(0);
    let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let url = format!("http://127.0.0.1:{}/?t={}", port, server.token);
    std::thread::Builder::new()
        .name("http".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            for conn in listener.incoming().flatten() {
                let server = server.clone();
                let _ = std::thread::Builder::new().stack_size(1024 * 1024).spawn(move || {
                    let _ = serve(conn, &server);
                });
            }
        })
        .map_err(|e| e.to_string())?;
    log!("Dashboard at http://127.0.0.1:{}/", port);
    Ok(url)
}

struct Request {
    method: String,
    path: String,
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    fn param(&self, name: &str) -> Option<&str> {
        self.query.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

fn read_request(conn: &TcpStream) -> Result<Request, String> {
    let mut r = BufReader::new(conn);
    let mut line = String::new();
    r.read_line(&mut line).map_err(|e| e.to_string())?;
    let mut parts = line.split_whitespace();
    let method = parts.next().ok_or("empty request")?.to_string();
    let target = parts.next().ok_or("no path")?.to_string();
    let mut headers = Vec::new();
    let mut total = 0;
    loop {
        let mut h = String::new();
        let n = r.read_line(&mut h).map_err(|e| e.to_string())?;
        total += n;
        if n == 0 || h == "\r\n" || h == "\n" || total > 64 * 1024 {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let len: usize = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
    if len > MAX_BODY {
        return Err("Upload too large (40 MB max)".into());
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).map_err(|e| e.to_string())?;
    let (path, qs) = target.split_once('?').unwrap_or((&target, ""));
    let query = qs
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (util::url_decode(k), util::url_decode(v))
        })
        .collect();
    Ok(Request { method, path: util::url_decode(path), query, headers, body })
}

fn respond(conn: &mut TcpStream, code: u16, ctype: &str, body: &[u8]) -> std::io::Result<()> {
    let reason = match code {
        200 => "OK",
        204 => "No Content",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        code,
        reason,
        ctype,
        body.len()
    );
    conn.write_all(head.as_bytes())?;
    conn.write_all(body)?;
    conn.flush()
}

fn respond_json(conn: &mut TcpStream, code: u16, v: &Json) -> std::io::Result<()> {
    respond(conn, code, "application/json", v.to_string_compact().as_bytes())
}

fn err_json(e: &str) -> Json {
    json::object([("ok", false.into()), ("error", e.into())])
}

fn serve(mut conn: TcpStream, s: &Server) -> std::io::Result<()> {
    let _ = conn.set_read_timeout(Some(Duration::from_secs(30)));
    let req = match read_request(&conn) {
        Ok(r) => r,
        Err(e) => return respond_json(&mut conn, 200, &err_json(&e)),
    };
    if req.path == "/favicon.ico" {
        return respond(&mut conn, 204, "image/x-icon", b"");
    }
    let header_token = req.header("X-Token").unwrap_or("");
    let authorized = if req.method == "POST" {
        // POSTs must carry the header (a page elsewhere can't set it cross-origin)
        util::secure_eq(header_token, &s.token)
    } else {
        util::secure_eq(if header_token.is_empty() { req.param("t").unwrap_or("") } else { header_token }, &s.token)
    };
    if !authorized {
        return respond_json(&mut conn, 403, &json::object([("error", "forbidden".into())]));
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") | ("GET", "/index.html") => {
            let html = DASHBOARD.replace("__TOKEN__", &s.token);
            respond(&mut conn, 200, "text/html; charset=utf-8", html.as_bytes())
        }
        ("GET", "/api/ping") => respond_json(&mut conn, 200, &json::object([("ok", true.into())])),
        ("GET", "/api/state") => respond_json(&mut conn, 200, &state(s)),
        ("GET", "/api/sensors") => {
            let v = json::object([("sensors", s.engine.sensors_json()), ("service", s.engine.connected().into())]);
            respond_json(&mut conn, 200, &v)
        }
        ("GET", "/api/media/thumb") => match thumb(req.param("name").unwrap_or("")) {
            Ok(png) => respond(&mut conn, 200, "image/png", &png),
            Err(e) => respond_json(&mut conn, 404, &json::object([("error", e.into())])),
        },
        ("POST", path) => {
            let body = match std::str::from_utf8(&req.body).map_err(|e| e.to_string()).and_then(|t| if t.trim().is_empty() { Ok(Json::obj()) } else { json::parse(t) }) {
                Ok(b) => b,
                Err(e) => return respond_json(&mut conn, 200, &err_json(&e)),
            };
            match path {
                "/api/preview" => {
                    let cfg = body.get("config").cloned().unwrap_or_else(Json::obj);
                    let png = preview(s, &cfg, body.bool_or("test_pattern", false));
                    respond(&mut conn, 200, "image/png", &png)
                }
                "/api/config" => {
                    let cfg = body.get("config").cloned().unwrap_or_else(Json::obj);
                    match config::save(&cfg) {
                        Ok(()) => {
                            log!("Settings saved");
                            s.engine.poke();
                            if let Some(f) = &s.on_saved {
                                f();
                            }
                            respond_json(&mut conn, 200, &json::object([("ok", true.into()), ("service", s.engine.connected().into())]))
                        }
                        Err(e) => respond_json(&mut conn, 200, &err_json(&e)),
                    }
                }
                "/api/media" => {
                    let data = util::base64_decode(body.str_or("data", ""));
                    match data.and_then(|d| media_add(body.str_or("name", ""), &d)) {
                        Ok(name) => respond_json(&mut conn, 200, &json::object([("ok", true.into()), ("name", name.into()), ("media", media_list())])),
                        Err(e) => respond_json(&mut conn, 200, &err_json(&e)),
                    }
                }
                "/api/media/delete" => match media_delete(s, body.str_or("name", "")) {
                    Ok(()) => respond_json(&mut conn, 200, &json::object([("ok", true.into()), ("media", media_list())])),
                    Err(e) => respond_json(&mut conn, 200, &err_json(&e)),
                },
                "/api/test_pattern" => {
                    s.engine.show_test_pattern(body.num_or("seconds", 15.0));
                    respond_json(&mut conn, 200, &device_result(s.engine.connected()))
                }
                "/api/brightness" => {
                    let ok = s.engine.set_brightness(body.num_or("level", 100.0).clamp(0.0, 100.0) as u8);
                    respond_json(&mut conn, 200, &device_result(ok))
                }
                _ => respond_json(&mut conn, 404, &json::object([("error", "not found".into())])),
            }
        }
        _ => respond_json(&mut conn, 404, &json::object([("error", "not found".into())])),
    }
}

fn device_result(ok: bool) -> Json {
    json::object([("ok", ok.into()), ("error", if ok { Json::Null } else { "The cooler is not connected.".into() })])
}

/// Settings for the editor, with full layouts for both canvas shapes.
pub fn dashboard_config(engine: &Engine) -> Json {
    let mut cfg = config::load_or_default();
    if cfg.get("rotation").map(|r| r.is_null()).unwrap_or(true) {
        cfg.set("rotation", (engine.current_rotation() as i64).into());
    }
    let r = engine.renderer.lock().unwrap_or_else(|e| e.into_inner());
    let mut layouts = Json::obj();
    if let Some(Json::Obj(stored)) = cfg.get("layouts") {
        for (k, v) in stored {
            layouts.set(k, v.clone()); // keep anything we don't know about
        }
    }
    for shape in ["landscape", "portrait"] {
        layouts.set(shape, config::layout_json(&r.layout(Some(&cfg), shape)));
    }
    drop(r);
    cfg.set("layouts", layouts);
    cfg
}

fn state(s: &Server) -> Json {
    let r = s.engine.renderer.lock().unwrap_or_else(|e| e.into_inner());
    let (defaults, wdefaults) = (r.default_layouts.clone(), r.widget_defaults.clone());
    drop(r);
    let size = |w: i64, h: i64| Json::Arr(vec![w.into(), h.into()]);
    json::object([
        ("config", dashboard_config(&s.engine)),
        ("defaults", defaults),
        ("widget_defaults", wdefaults),
        ("sizes", json::object([("landscape", size(320, 240)), ("portrait", size(240, 320))])),
        ("service", s.engine.connected().into()),
        ("media", media_list()),
        ("fonts", Json::Arr(fonts().iter().map(|f| f.as_str().into()).collect())),
        ("config_path", config::config_path().display().to_string().into()),
        ("media_dir", config::media_dir().display().to_string().into()),
    ])
}

/// Installed font families (looked up once; fonts installed later show after a restart).
fn fonts() -> &'static Vec<String> {
    static FONTS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    FONTS.get_or_init(crate::platform::font_families)
}

/// The upright picture, as you'd see it on the mounted cooler.
fn preview(s: &Server, cfg: &Json, test_pattern: bool) -> Vec<u8> {
    let cfg = config::normalize(cfg);
    let rotation = cfg.get("rotation").and_then(|r| r.as_f64()).and_then(util::normalize_rotation).unwrap_or(0);
    let size = util::canvas_size(rotation);
    let mut canvas = if test_pattern {
        render::test_pattern(size, rotation)
    } else if cfg.str_or("mode", "monitor") == "media" {
        let media = cfg.get("media").cloned().unwrap_or_else(Json::obj);
        let bg = render::parse_rgb(media.str_or("background", ""), (0, 0, 0));
        let first = media
            .get("items")
            .and_then(|i| i.as_array())
            .into_iter()
            .flatten()
            .filter_map(|i| i.as_str())
            .find_map(|item| Clip::open(&config::media_path(item), media.str_or("fit", "cover"), size, bg, 100, 1).and_then(|mut c| c.frame(0)).ok());
        match first {
            Some(px) => {
                let mut c = crate::platform::new_canvas(size.0, size.1);
                c.blit_bgra(&px);
                c
            }
            None => render::placeholder(size, "No images selected"),
        }
    } else {
        let mut r = s.engine.renderer.lock().unwrap_or_else(|e| e.into_inner());
        let layout = r.layout(Some(&cfg), util::canvas_shape(rotation));
        s.engine.with_readings(|snap, history| r.render(&layout, snap, Some(history), util::now_secs(), size))
    };
    let (px, stride) = canvas.pixels();
    encode_png(&px, size.0, size.1, stride)
}

// --- media files ---------------------------------------------------------------

pub fn media_list() -> Json {
    let mut names: Vec<(String, u64)> = std::fs::read_dir(config::media_dir())
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let meta = e.metadata().ok()?;
                    (meta.is_file() && config::is_media_name(&name)).then_some((name, meta.len()))
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    Json::Arr(
        names
            .into_iter()
            .map(|(n, size)| {
                let animated = n.to_ascii_lowercase().ends_with(".gif");
                json::object([("name", n.into()), ("size", (size as f64).into()), ("animated", animated.into())])
            })
            .collect(),
    )
}

/// Make an uploaded file name safe: letters, digits and a few marks.
fn safe_stem(stem: &str) -> String {
    let mut out = String::new();
    let mut last_bad = false;
    for ch in stem.chars() {
        if ch.is_alphanumeric() || "_- .()".contains(ch) {
            out.push(ch);
            last_bad = false;
        } else if !last_bad {
            out.push('_');
            last_bad = true;
        }
    }
    let out: String = out.chars().take(80).collect();
    if out.trim().is_empty() {
        "image".into()
    } else {
        out
    }
}

fn media_add(name: &str, data: &[u8]) -> Result<String, String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("").trim();
    let base = if base.is_empty() { "image.png" } else { base };
    let (stem, ext) = match base.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, e.to_ascii_lowercase()),
        _ => return Err(format!("Unsupported file type: {}", "(none)")),
    };
    if !config::MEDIA_EXTENSIONS.contains(&ext.as_str()) {
        return Err(format!("Unsupported file type: .{}", ext));
    }
    let stem = safe_stem(stem);
    let dir = config::media_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut target = dir.join(format!("{}.{}", stem, ext));
    let mut n = 1;
    while target.exists() {
        n += 1;
        target = dir.join(format!("{} ({}).{}", stem, n, ext));
    }
    std::fs::write(&target, data).map_err(|e| e.to_string())?;
    // reject files that aren't really images
    if let Err(e) = crate::platform::open_image(&target) {
        let _ = std::fs::remove_file(&target);
        return Err(format!("Not a usable image: {}", e));
    }
    Ok(target.file_name().unwrap().to_string_lossy().into_owned())
}

fn media_delete(s: &Server, name: &str) -> Result<(), String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("").to_string();
    let path = config::media_path(&base);
    if path.is_file() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    let mut cfg = config::load_or_default();
    let changed = if let Some(Json::Arr(items)) = cfg.get_mut("media").and_then(|m| m.get_mut("items")) {
        let before = items.len();
        items.retain(|i| i.as_str() != Some(base.as_str()));
        before != items.len()
    } else {
        false
    };
    if changed {
        config::save(&cfg)?;
        s.engine.poke();
        if let Some(f) = &s.on_saved {
            f();
        }
    }
    Ok(())
}

fn thumb(name: &str) -> Result<Vec<u8>, String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let px = Clip::open(&config::media_path(base), "cover", (240, 180), (46, 50, 60), 100, 1)?.frame(0)?;
    Ok(encode_png(&px, 240, 180, 240 * 4))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems() {
        assert_eq!(safe_stem("my cat (2)"), "my cat (2)");
        assert_eq!(safe_stem("a/b\\c:*?d"), "a_b_c_d");
        assert_eq!(safe_stem("***"), "_");
        assert_eq!(safe_stem(""), "image");
    }
}
