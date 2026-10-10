//! Command-line control of the running app: `deepcool-lm brightness 50` etc.
//! connect to 127.0.0.1:36330 (DEEPCOOL_LM_PORT), send one JSON command, close
//! the sending side and read one JSON reply.

use crate::engine::Engine;
use crate::json::{self, Json};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

const MAX_REQUEST: u64 = 40 * 1024 * 1024;

pub fn port() -> u16 {
    std::env::var("DEEPCOOL_LM_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(36330)
}

fn addr() -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, port()))
}

pub fn start(engine: Arc<Engine>) -> Result<(), String> {
    let listener = TcpListener::bind(addr()).map_err(|e| {
        format!("Cannot listen on {} ({}). Another program may be using this port; set DEEPCOOL_LM_PORT to pick another.", addr(), e)
    })?;
    log!("IPC server started on {}", addr());
    std::thread::Builder::new()
        .name("ipc".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            for conn in listener.incoming().flatten() {
                let engine = engine.clone();
                let _ = std::thread::Builder::new().stack_size(256 * 1024).spawn(move || handle(conn, &engine));
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn handle(mut conn: TcpStream, engine: &Engine) {
    let _ = conn.set_read_timeout(Some(Duration::from_secs(10)));
    let mut data = Vec::new();
    if (&mut conn).take(MAX_REQUEST).read_to_end(&mut data).is_err() || data.is_empty() {
        return; // an empty connection is a "is anyone there?" probe
    }
    let reply = match std::str::from_utf8(&data).map_err(|e| e.to_string()).and_then(json::parse) {
        Ok(cmd) => match execute(&cmd, engine) {
            Ok(()) => json::object([("status", "ok".into())]),
            Err(e) => json::object([("status", "error".into()), ("message", e.into())]),
        },
        Err(e) => json::object([("status", "error".into()), ("message", format!("bad request: {}", e).into())]),
    };
    let _ = conn.write_all(reply.to_string_compact().as_bytes());
}

fn need_device(ok: bool) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err("The cooler is not connected.".into())
    }
}

fn execute(cmd: &Json, engine: &Engine) -> Result<(), String> {
    match cmd.str_or("action", "") {
        "brightness_up" => need_device(engine.set_brightness(97)),
        "brightness_down" => need_device(engine.set_brightness(29)),
        "brightness_set" => {
            let level = cmd.get("level").and_then(|l| l.as_f64()).ok_or("missing level")?;
            need_device(engine.set_brightness(level.clamp(0.0, 100.0) as u8))
        }
        "zen_toggle" => need_device(engine.zen_toggle()),
        "test_pattern" => {
            engine.show_test_pattern(cmd.num_or("seconds", 15.0));
            Ok(())
        }
        "monitor" => {
            engine.monitor_mode();
            Ok(())
        }
        "solid" => {
            let c: Vec<u8> = cmd
                .get("color")
                .and_then(|c| c.as_array())
                .map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0).clamp(0.0, 255.0) as u8).collect())
                .unwrap_or_default();
            if c.len() != 3 {
                return Err("color must be [R, G, B]".into());
            }
            engine.show_solid((c[0], c[1], c[2]));
            Ok(())
        }
        "image" => {
            if let Some(path) = cmd.get("path").and_then(|p| p.as_str()) {
                return engine.show_image(std::path::Path::new(path));
            }
            // older clients send the file itself, base64-encoded
            let data = crate::util::base64_decode(cmd.str_or("data", ""))?;
            let tmp = std::env::temp_dir().join(format!("deepcool-lm-image-{}", std::process::id()));
            std::fs::write(&tmp, data).map_err(|e| e.to_string())?;
            let r = engine.show_image(&tmp);
            let _ = std::fs::remove_file(&tmp);
            r
        }
        "heartbeat" | "tray_exit" => Ok(()), // from the old two-process tray; nothing to do now
        other => Err(format!("Unknown action: {}", other)),
    }
}

/// Send a command to the running app. None when it isn't running.
pub fn send(cmd: &Json) -> Option<Json> {
    let mut s = TcpStream::connect_timeout(&addr(), Duration::from_secs(2)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(15)));
    let result = (|| -> Result<Json, String> {
        s.write_all(cmd.to_string_compact().as_bytes()).map_err(|e| e.to_string())?;
        s.shutdown(std::net::Shutdown::Write).map_err(|e| e.to_string())?;
        let mut buf = String::new();
        s.read_to_string(&mut buf).map_err(|e| e.to_string())?;
        json::parse(&buf)
    })();
    Some(result.unwrap_or_else(|e| json::object([("status", "error".into()), ("message", e.into())])))
}

pub fn running() -> bool {
    TcpStream::connect_timeout(&addr(), Duration::from_secs(1)).is_ok()
}
