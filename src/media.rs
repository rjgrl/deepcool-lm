//! Images and GIFs: `Clip` (one fitted, possibly animated picture) and
//! `MediaPlayer` (the multimedia-mode slideshow).

use crate::canvas::Rgb;
use crate::config;
use crate::json::Json;
use crate::platform::{self, ImageSource};
use crate::render;
use crate::util;
use std::time::{Instant, SystemTime};

/// Frames are kept decoded (as RGB565, 2 bytes a pixel) when they fit in this
/// budget; longer animations are decoded frame by frame instead.
const CACHE_BUDGET: usize = 16 * 1024 * 1024;

/// One picture fitted to a size, with an optional dim toward `bg`.
pub struct Clip {
    source: Box<dyn ImageSource>,
    fit: String,
    size: (usize, usize),
    bg: Rgb,
    dim: u32, // brightness 0-100
    count: usize,
    delays: Vec<f64>,
    cache: Vec<Option<Vec<u16>>>,
    cacheable: bool,
}

impl Clip {
    pub fn open(path: &std::path::Path, fit: &str, size: (usize, usize), bg: Rgb, dim: u32, max_frames: usize) -> Result<Clip, String> {
        let source = platform::open_image(path)?;
        let count = source.frame_count().clamp(1, max_frames.max(1));
        let delays: Vec<f64> = (0..count).map(|i| source.delay(i).max(0.02)).collect();
        let cacheable = count * size.0 * size.1 * 2 <= CACHE_BUDGET;
        let mut clip = Clip { source, fit: fit.to_string(), size, bg, dim: dim.min(100), count, delays, cache: vec![None; count], cacheable };
        clip.frame(0)?; // fail early for broken files
        Ok(clip)
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn delay(&self, i: usize) -> f64 {
        self.delays[i % self.count]
    }

    pub fn total(&self) -> f64 {
        self.delays.iter().sum()
    }

    pub fn min_delay(&self) -> Option<f64> {
        if self.count < 2 {
            None
        } else {
            self.delays.iter().copied().reduce(f64::min)
        }
    }

    /// Frame showing at time `now` when the animation loops forever.
    pub fn index_at(&self, now: f64) -> usize {
        let total = self.total();
        if self.count == 1 || total <= 0.0 {
            return 0;
        }
        let mut t = now.rem_euclid(total);
        for (i, d) in self.delays.iter().enumerate() {
            if t < *d {
                return i;
            }
            t -= d;
        }
        self.count - 1
    }

    /// Frame `i` as upright BGRA pixels (stride width * 4).
    pub fn frame(&mut self, i: usize) -> Result<Vec<u8>, String> {
        let i = i % self.count;
        if let Some(Some(px)) = self.cache.get(i) {
            return Ok(rgb565_to_bgra(px));
        }
        let (w, h) = self.size;
        let mut px = self.source.render(i, &self.fit, w, h, self.bg)?;
        if self.dim < 100 {
            // dim toward the background color so widgets stay readable
            let t = self.dim;
            let b = [self.bg.2 as u32, self.bg.1 as u32, self.bg.0 as u32];
            for p in px.chunks_mut(4) {
                for c in 0..3 {
                    p[c] = ((b[c] * (100 - t) + p[c] as u32 * t) / 100) as u8;
                }
            }
        }
        if self.cacheable {
            let packed = bgra_to_rgb565(&px);
            px = rgb565_to_bgra(&packed); // show exactly what later loops will show
            self.cache[i] = Some(packed);
        }
        Ok(px)
    }
}

pub fn bgra_to_rgb565(px: &[u8]) -> Vec<u16> {
    px.chunks(4).map(|p| ((p[2] as u16 >> 3) << 11) | ((p[1] as u16 >> 2) << 5) | (p[0] as u16 >> 3)).collect()
}

pub fn rgb565_to_bgra(px: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(px.len() * 4);
    for &v in px {
        let (r, g, b) = ((v >> 11) as u8, ((v >> 5) & 63) as u8, (v & 31) as u8);
        out.extend_from_slice(&[(b << 3) | (b >> 2), (g << 2) | (g >> 4), (r << 3) | (r >> 2), 255]);
    }
    out
}

// ---------------------------------------------------------------------------

#[derive(PartialEq)]
struct PlayerKey {
    items: Vec<String>,
    fit: String,
    background: String,
    size: (usize, usize),
    rotation: i32,
    stamps: Vec<Option<(SystemTime, u64)>>,
}

/// Plays the selected images/GIFs, moving to the next one every `slide_seconds`.
pub struct MediaPlayer {
    key: Option<PlayerKey>,
    pub items: Vec<String>,
    pub index: usize,
    clip: Option<Clip>,
    frame: usize,
    slide_started: Instant,
    placeholder: Option<Vec<u8>>,
}

fn stamp(path: &std::path::Path) -> Option<(SystemTime, u64)> {
    std::fs::metadata(path).ok().and_then(|m| Some((m.modified().ok()?, m.len())))
}

impl MediaPlayer {
    pub fn new() -> Self {
        MediaPlayer { key: None, items: Vec::new(), index: 0, clip: None, frame: 0, slide_started: Instant::now(), placeholder: None }
    }

    fn load_current(&mut self, media: &Json, size: (usize, usize)) {
        self.clip = None;
        self.frame = 0;
        let fit = media.str_or("fit", "cover").to_string();
        let bg = render::parse_rgb(media.str_or("background", ""), (0, 0, 0));
        while !self.items.is_empty() {
            let idx = self.index % self.items.len();
            let path = config::media_path(&self.items[idx]);
            match Clip::open(&path, &fit, size, bg, 100, 600) {
                Ok(c) => {
                    self.clip = Some(c);
                    return;
                }
                Err(e) => log!("Can't show {}: {}", path.display(), e),
            }
            self.items.remove(idx); // skip broken files
            self.index = 0;
        }
    }

    /// Next panel frame (rotated RGB565) and how long to show it.
    pub fn next(&mut self, media: &Json, rotation: i32) -> (Vec<u8>, f64) {
        let size = util::canvas_size(rotation);
        let items: Vec<String> = media
            .get("items")
            .and_then(|i| i.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let key = PlayerKey {
            stamps: items.iter().map(|i| stamp(&config::media_path(i))).collect(),
            items: items.clone(),
            fit: media.str_or("fit", "cover").to_string(),
            background: media.str_or("background", "").to_string(),
            size,
            rotation,
        };
        let now = Instant::now();
        if self.key.as_ref() != Some(&key) {
            self.key = Some(key);
            self.items = items;
            self.index = 0;
            self.slide_started = now;
            self.load_current(media, size);
        }
        let slide = media.num_or("slide_seconds", 10.0).max(1.0);
        let elapsed = now.duration_since(self.slide_started).as_secs_f64();
        if self.items.len() > 1 && elapsed >= slide && self.frame == 0 {
            self.index = (self.index + 1) % self.items.len();
            self.slide_started = now;
            self.load_current(media, size);
        }
        let (w, h) = size;
        let (px, mut delay, count) = match self.clip.as_mut() {
            Some(clip) => {
                let i = self.frame;
                let count = clip.count();
                let px = clip.frame(i).unwrap_or_else(|_| vec![0; w * h * 4]);
                (px, clip.delay(i), count)
            }
            None => {
                let px = self.placeholder.get_or_insert_with(|| render::placeholder(size, "No images selected").pixels().0).clone();
                (px, 1.0, 1)
            }
        };
        self.frame = (self.frame + 1) % count;
        if count == 1 {
            let remaining = if self.items.len() > 1 { slide - now.duration_since(self.slide_started).as_secs_f64() } else { 1.0 };
            delay = remaining.clamp(0.2, 1.0); // still image: resend about once a second
        }
        (util::bgra_to_frame(&px, w, h, w * 4, rotation), delay)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Write a small 24-bit BMP: left half red, right half blue.
    pub fn write_bmp(path: &std::path::Path, w: usize, h: usize) {
        let stride = (w * 3 + 3) & !3;
        let mut d = b"BM".to_vec();
        let size = 54 + stride * h;
        d.extend_from_slice(&(size as u32).to_le_bytes());
        d.extend_from_slice(&[0, 0, 0, 0]);
        d.extend_from_slice(&54u32.to_le_bytes());
        d.extend_from_slice(&40u32.to_le_bytes());
        d.extend_from_slice(&(w as i32).to_le_bytes());
        d.extend_from_slice(&(h as i32).to_le_bytes());
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&24u16.to_le_bytes());
        d.extend_from_slice(&[0; 24]);
        for _y in 0..h {
            for x in 0..w {
                d.extend_from_slice(if x < w / 2 { &[0, 0, 255] } else { &[255, 0, 0] });
            }
            d.extend(std::iter::repeat(0).take(stride - w * 3));
        }
        std::fs::write(path, d).unwrap();
    }

    #[test]
    fn clip_fits_and_caches() {
        let dir = std::env::temp_dir().join(format!("dlm-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.bmp");
        write_bmp(&p, 40, 20);
        let mut c = Clip::open(&p, "contain", (320, 240), (0, 255, 0), 100, 10).unwrap();
        let px = c.frame(0).unwrap();
        let at = |x: usize, y: usize| (px[(y * 320 + x) * 4 + 2], px[(y * 320 + x) * 4 + 1], px[(y * 320 + x) * 4]);
        assert_eq!(at(10, 120), (255, 0, 0));
        assert_eq!(at(310, 120), (0, 0, 255));
        assert_eq!(at(160, 5), (0, 255, 0)); // letterbox
        assert!(c.cache[0].is_some());
        let mut dimmed = Clip::open(&p, "stretch", (32, 24), (0, 0, 0), 50, 10).unwrap();
        let px = dimmed.frame(0).unwrap();
        assert!((120..=135).contains(&px[2]));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rgb565_round_trip() {
        let px = [255u8, 255, 255, 255, 0, 0, 0, 255, 8, 132, 248, 255];
        let back = rgb565_to_bgra(&bgra_to_rgb565(&px));
        assert_eq!(&back[0..4], &[255, 255, 255, 255]);
        assert_eq!(&back[4..8], &[0, 0, 0, 255]);
    }
}
