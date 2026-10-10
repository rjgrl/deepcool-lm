//! OS services used by the platform-independent code. Windows is the real
//! target; the Unix version exists so the app can be built and tested on Linux.

pub struct CpuTimes {
    pub idle: u64,
    pub total: u64,
}

pub struct Memory {
    pub total: u64,
    pub used: u64,
}

/// A decoded (possibly animated) image. Frames are drawn on demand, so a long
/// GIF doesn't have to sit in memory fully decoded.
pub trait ImageSource: Send {
    fn size(&self) -> (usize, usize);
    fn frame_count(&self) -> usize;
    /// Seconds frame `i` stays on screen.
    fn delay(&self, i: usize) -> f64;
    /// Frame `i` fitted into `w` x `h` (see `fit_rects`), transparency
    /// composited over `bg`. Returns BGRA pixels, stride `w * 4`.
    fn render(&mut self, i: usize, fit: &str, w: usize, h: usize, bg: (u8, u8, u8)) -> Result<Vec<u8>, String>;
}

/// Source and destination rectangles (x, y, w, h) for drawing an `sw` x `sh`
/// image into a `w` x `h` box: "cover" crops the middle to fill the box,
/// "contain" letterboxes, "stretch" ignores the aspect ratio.
pub fn fit_rects(sw: usize, sh: usize, fit: &str, w: usize, h: usize) -> ((f64, f64, f64, f64), (f64, f64, f64, f64)) {
    let (sw, sh, w, h) = (sw.max(1) as f64, sh.max(1) as f64, w as f64, h as f64);
    match fit {
        "stretch" => ((0.0, 0.0, sw, sh), (0.0, 0.0, w, h)),
        "contain" => {
            let s = (w / sw).min(h / sh);
            let (dw, dh) = ((sw * s).round(), (sh * s).round());
            ((0.0, 0.0, sw, sh), (((w - dw) / 2.0).floor(), ((h - dh) / 2.0).floor(), dw, dh))
        }
        _ => {
            let s = (w / sw).max(h / sh);
            let (cw, ch) = (w / s, h / s);
            (((sw - cw) / 2.0, (sh - ch) / 2.0, cw, ch), (0.0, 0.0, w, h))
        }
    }
}

/// Bulk pipes to the cooler (OUT 0x01, IN 0x81).
pub trait UsbLink: Send {
    fn write(&mut self, data: &[u8]) -> Result<usize, String>;
    fn read(&mut self, buf: &mut [u8], timeout_ms: u32) -> Result<usize, String>;
    /// Device details for `deepcool-lm info`.
    fn describe(&self) -> String;
}

#[cfg(any(windows, wincheck))]
pub use crate::win::sys::*;

#[cfg(not(any(windows, wincheck)))]
mod unix;
#[cfg(not(any(windows, wincheck)))]
pub use unix::*;
