//! Linux stand-ins (development and tests only).

use super::{CpuTimes, Memory};
use crate::util::LocalTime;

pub fn cpu_times() -> Option<CpuTimes> {
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    let nums: Vec<u64> = stat.lines().next()?.split_whitespace().skip(1).filter_map(|v| v.parse().ok()).collect();
    if nums.len() < 4 {
        return None;
    }
    let idle = nums[3] + nums.get(4).copied().unwrap_or(0);
    Some(CpuTimes { idle, total: nums.iter().sum() })
}

pub fn cpu_mhz() -> Option<f64> {
    let info = std::fs::read_to_string("/proc/cpuinfo").ok()?;
    info.lines().find(|l| l.starts_with("cpu MHz")).and_then(|l| l.split(':').nth(1)).and_then(|v| v.trim().parse().ok())
}

pub fn memory() -> Option<Memory> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let field = |name: &str| -> Option<u64> {
        info.lines().find(|l| l.starts_with(name))?.split_whitespace().nth(1)?.parse::<u64>().ok().map(|kb| kb * 1024)
    };
    let total = field("MemTotal:")?;
    Some(Memory { total, used: total.saturating_sub(field("MemAvailable:")?) })
}

pub fn system_disk_usage() -> Option<f64> {
    None
}

pub fn net_bytes() -> Option<(u64, u64)> {
    let dev = std::fs::read_to_string("/proc/net/dev").ok()?;
    let (mut rx, mut tx) = (0u64, 0u64);
    for line in dev.lines().skip(2) {
        let (name, rest) = line.split_once(':')?;
        if name.trim() == "lo" {
            continue;
        }
        let v: Vec<u64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        rx += v.first().copied().unwrap_or(0);
        tx += v.get(8).copied().unwrap_or(0);
    }
    Some((rx, tx))
}

pub struct Nvml;
impl Nvml {
    pub fn open() -> Option<Nvml> {
        None
    }
    pub fn temperature(&self) -> Option<f64> {
        None
    }
    pub fn utilization(&self) -> Option<f64> {
        None
    }
}

pub fn local_time() -> LocalTime {
    // UTC is good enough for development builds
    let secs = crate::util::now_secs() as i64;
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    // civil-from-days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    LocalTime {
        hour: (rem / 3600) as u32,
        minute: (rem % 3600 / 60) as u32,
        second: (rem % 60) as u32,
        day,
        month,
        weekday: ((days + 4).rem_euclid(7)) as u32,
    }
}

pub fn random_bytes(n: usize) -> Vec<u8> {
    use std::io::Read;
    let mut buf = vec![0u8; n];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut buf);
    }
    buf
}

/// Development builds decode only uncompressed BMP files (Windows uses GDI+
/// for PNG, JPG, GIF and BMP). Enough to exercise the media code in tests.
pub fn open_image(path: &std::path::Path) -> Result<Box<dyn super::ImageSource>, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    Ok(Box::new(Bmp::decode(&data)?))
}

struct Bmp {
    w: usize,
    h: usize,
    bgra: Vec<u8>,
}

impl Bmp {
    fn decode(d: &[u8]) -> Result<Bmp, String> {
        let u32_at = |o: usize| d.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        let u16_at = |o: usize| d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
        if !d.starts_with(b"BM") {
            return Err("not a BMP image (development builds only read BMP)".into());
        }
        let bad = || "unsupported BMP".to_string();
        let off = u32_at(10).ok_or_else(bad)? as usize;
        let w = u32_at(18).ok_or_else(bad)? as i32;
        let h = u32_at(22).ok_or_else(bad)? as i32;
        let bpp = u16_at(28).ok_or_else(bad)?;
        let comp = u32_at(30).ok_or_else(bad)?;
        if w <= 0 || h == 0 || !(bpp == 24 || bpp == 32) || !(comp == 0 || comp == 3) {
            return Err(bad());
        }
        let (w, top_down, h) = (w as usize, h < 0, h.unsigned_abs() as usize);
        let bytes = bpp as usize / 8;
        let stride = (w * bytes + 3) & !3;
        let mut bgra = vec![0u8; w * h * 4];
        for y in 0..h {
            let sy = if top_down { y } else { h - 1 - y };
            for x in 0..w {
                let o = off + sy * stride + x * bytes;
                let p = d.get(o..o + bytes).ok_or_else(bad)?;
                let i = (y * w + x) * 4;
                bgra[i..i + 3].copy_from_slice(&p[..3]);
                bgra[i + 3] = if bytes == 4 { p[3] } else { 255 };
            }
        }
        Ok(Bmp { w, h, bgra })
    }
}

impl super::ImageSource for Bmp {
    fn size(&self) -> (usize, usize) {
        (self.w, self.h)
    }
    fn frame_count(&self) -> usize {
        1
    }
    fn delay(&self, _i: usize) -> f64 {
        1.0
    }
    fn render(&mut self, _i: usize, fit: &str, w: usize, h: usize, bg: (u8, u8, u8)) -> Result<Vec<u8>, String> {
        let ((sx, sy, sw, sh), (dx, dy, dw, dh)) = super::fit_rects(self.w, self.h, fit, w, h);
        let mut out = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let o = (y * w + x) * 4;
                let (fx, fy) = (x as f64 + 0.5 - dx, y as f64 + 0.5 - dy);
                let mut px = [bg.2, bg.1, bg.0, 255];
                if fx >= 0.0 && fy >= 0.0 && fx < dw && fy < dh {
                    // nearest neighbour; good enough for development builds
                    let ix = ((sx + fx / dw * sw) as usize).min(self.w - 1);
                    let iy = ((sy + fy / dh * sh) as usize).min(self.h - 1);
                    let s = &self.bgra[(iy * self.w + ix) * 4..][..4];
                    let a = s[3] as u32;
                    for c in 0..3 {
                        px[c] = ((s[c] as u32 * a + px[c] as u32 * (255 - a)) / 255) as u8;
                    }
                }
                out[o..o + 4].copy_from_slice(&px);
            }
        }
        Ok(out)
    }
}

pub fn new_canvas(w: usize, h: usize) -> Box<dyn crate::canvas::Canvas> {
    Box::new(crate::canvas::SoftCanvas::new(w, h))
}

// ---------------------------------------------------------------------------
// USB. Development builds have no real device access; set
// DEEPCOOL_LM_FAKE_DEVICE=<file> to get a fake cooler that saves the last
// frame to <file> and logs other commands to <file>.log. Creating
// <file>.unplugged makes it act unplugged.

pub fn open_usb() -> Result<Box<dyn super::UsbLink>, String> {
    let path = std::env::var("DEEPCOOL_LM_FAKE_DEVICE").map_err(|_| "device not found".to_string())?;
    let fake = FakeUsb { path: std::path::PathBuf::from(path) };
    if fake.unplugged() {
        return Err("device not found".into());
    }
    Ok(Box::new(fake))
}

struct FakeUsb {
    path: std::path::PathBuf,
}

impl FakeUsb {
    fn unplugged(&self) -> bool {
        let mut p = self.path.clone().into_os_string();
        p.push(".unplugged");
        std::path::Path::new(&p).exists()
    }
}

impl super::UsbLink for FakeUsb {
    fn write(&mut self, data: &[u8]) -> Result<usize, String> {
        use std::io::Write;
        if self.unplugged() {
            return Err("device gone".into());
        }
        if data.len() == crate::util::FRAME_BYTES {
            let tmp = self.path.with_extension("tmp");
            std::fs::write(&tmp, data).map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())?;
        } else {
            let mut p = self.path.clone().into_os_string();
            p.push(".log");
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p).map_err(|e| e.to_string())?;
            let _ = writeln!(f, "{}", crate::util::hex(data));
        }
        Ok(data.len())
    }

    fn read(&mut self, buf: &mut [u8], _timeout_ms: u32) -> Result<usize, String> {
        let reply = [0x55, 0x02, 0x00, 0x09, 0x00, 0x98, 0xba];
        let n = reply.len().min(buf.len());
        buf[..n].copy_from_slice(&reply[..n]);
        Ok(n)
    }

    fn describe(&self) -> String {
        format!("fake device ({})", self.path.display())
    }
}

/// Development builds can't list fonts; a few common names for the dashboard.
pub fn font_families() -> Vec<String> {
    ["Arial", "Bahnschrift", "Consolas", "Segoe UI", "Verdana"].iter().map(|s| s.to_string()).collect()
}
