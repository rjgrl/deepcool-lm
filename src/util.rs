//! Small helpers shared by the platform-independent and Windows code.

/// The LCD's native frame: 320x240 pixels, RGB565 little-endian.
pub const NATIVE_W: usize = 320;
pub const NATIVE_H: usize = 240;
pub const FRAME_BYTES: usize = NATIVE_W * NATIVE_H * 2;

pub const ROTATIONS: [i32; 4] = [0, 90, 180, 270];

pub fn rotation_name(r: i32) -> &'static str {
    match r {
        90 => "Portrait",
        180 => "Landscape, upside down",
        270 => "Portrait, upside down",
        _ => "Landscape",
    }
}

/// Size of the upright picture ("canvas") for a rotation.
pub fn canvas_size(rotation: i32) -> (usize, usize) {
    if rotation == 90 || rotation == 270 {
        (NATIVE_H, NATIVE_W)
    } else {
        (NATIVE_W, NATIVE_H)
    }
}

pub fn canvas_shape(rotation: i32) -> &'static str {
    if rotation == 90 || rotation == 270 {
        "portrait"
    } else {
        "landscape"
    }
}

pub fn normalize_rotation(r: f64) -> Option<i32> {
    if !r.is_finite() {
        return None;
    }
    let r = (r as i64).rem_euclid(360) as i32;
    if ROTATIONS.contains(&r) {
        Some(r)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Device protocol

pub fn crc16_xmodem(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

/// 0xAA, payload length (LE16), payload, CRC-16/XMODEM of the payload (LE16).
pub fn build_packet(payload: &[u8]) -> Vec<u8> {
    let mut p = Vec::with_capacity(payload.len() + 5);
    p.push(0xAA);
    p.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    p.extend_from_slice(payload);
    p.extend_from_slice(&crc16_xmodem(payload).to_le_bytes());
    p
}

pub fn init_packet() -> Vec<u8> {
    build_packet(&[0x09])
}

/// Header sent before each 153,600-byte frame: command, frame length (LE32), USB packet count (LE16).
pub fn frame_header() -> Vec<u8> {
    let len = FRAME_BYTES as u32;
    let mut payload = vec![0x00, 0x01];
    payload.extend_from_slice(&len.to_le_bytes());
    payload.extend_from_slice(&((FRAME_BYTES / 512) as u16).to_le_bytes());
    build_packet(&payload)
}

pub fn brightness_packet(level: u8) -> Vec<u8> {
    build_packet(&[0x06, 0x03, level.min(100), 0x00])
}

pub fn zen_packet() -> Vec<u8> {
    build_packet(&[0x03, 0x00, 0x00, 0x00])
}

/// Convert an upright BGRA picture (as GDI+ stores 32bpp ARGB) to the panel's
/// RGB565 frame, turning it `rotation` degrees clockwise onto the 320x240 panel.
pub fn bgra_to_frame(bgra: &[u8], w: usize, h: usize, stride: usize, rotation: i32) -> Vec<u8> {
    let (nw, nh) = if rotation == 90 || rotation == 270 { (h, w) } else { (w, h) };
    let mut out = vec![0u8; nw * nh * 2];
    for ny in 0..nh {
        for nx in 0..nw {
            let (sx, sy) = match rotation {
                90 => (ny, h - 1 - nx),
                180 => (w - 1 - nx, h - 1 - ny),
                270 => (w - 1 - ny, nx),
                _ => (nx, ny),
            };
            let i = sy * stride + sx * 4;
            let (b, g, r) = (bgra[i] as u16, bgra[i + 1] as u16, bgra[i + 2] as u16);
            let v = ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3);
            let o = (ny * nw + nx) * 2;
            out[o] = (v & 0xFF) as u8;
            out[o + 1] = (v >> 8) as u8;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Encoding helpers

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

pub fn base64_decode(text: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\r' | b'\n' | b' ' => continue,
            _ => return Err("invalid base64".into()),
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

pub fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Constant-time comparison for the dashboard's session key.
pub fn secure_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Hex string from bytes (used for the random session key).
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Seconds since the Unix epoch, as f64.
pub fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Local wall-clock time parts for {time}/{date}/{day} in text widgets.
pub struct LocalTime {
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub day: u32,
    pub month: u32,  // 1-12
    pub weekday: u32, // 0 = Sunday
}

pub const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
pub const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packets_match_known_commands() {
        assert_eq!(hex(&init_packet()), "aa0100092991");
        assert_eq!(hex(&frame_header()), "aa0800000100580200 2c01bc11".replace(' ', ""));
        assert_eq!(hex(&brightness_packet(97)), "aa040006036100d246");
        assert_eq!(hex(&brightness_packet(29)), "aa040006031d00e60b");
        assert_eq!(hex(&zen_packet()), "aa040003000000dc9b");
        assert_eq!(brightness_packet(150)[5], 100);
    }

    #[test]
    fn base64_round_trip() {
        for data in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar", &[0u8, 255, 128, 7]] {
            assert_eq!(base64_decode(&base64_encode(data)).unwrap(), data);
        }
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn rotation_maps_top_edge_clockwise() {
        // canvas with a red top row; check which panel edge it lands on
        for (rot, expect) in [(0, "top"), (90, "right"), (180, "bottom"), (270, "left")] {
            let (w, h) = canvas_size(rot);
            let mut px = vec![0u8; w * h * 4];
            for x in 0..w {
                px[x * 4 + 2] = 255; // red, row 0
            }
            let f = bgra_to_frame(&px, w, h, w * 4, rot);
            assert_eq!(f.len(), FRAME_BYTES);
            let red = |x: usize, y: usize| {
                let v = f[(y * NATIVE_W + x) * 2] as u16 | (f[(y * NATIVE_W + x) * 2 + 1] as u16) << 8;
                v >> 11 == 31
            };
            let side = if red(160, 0) { "top" } else if red(319, 120) { "right" } else if red(160, 239) { "bottom" } else if red(0, 120) { "left" } else { "none" };
            assert_eq!(side, expect, "rotation {}", rot);
        }
    }

    #[test]
    fn url_decoding() {
        assert_eq!(url_decode("a%20b+c%2Fd"), "a b c/d");
        assert_eq!(url_decode("100%"), "100%");
    }
}
