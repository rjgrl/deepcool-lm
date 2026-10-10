//! Drawing surface used by the widget renderer. Windows draws with GDI+
//! (anti-aliased shapes, real fonts); `SoftCanvas` is a simple software
//! fallback used for development and tests (text shows as blocks).

pub type Rgb = (u8, u8, u8);

/// Font for a run of text. An empty or unknown family means the default (Segoe UI).
#[derive(Clone, Copy)]
pub struct FontSpec<'a> {
    pub family: &'a str,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    /// Symbol font (arrows, shapes, dingbats) instead of `family`.
    pub symbol: bool,
}

pub trait Canvas {
    fn size(&self) -> (usize, usize);
    fn fill_all(&mut self, c: Rgb);
    /// Copy a full-canvas BGRA picture (same size as the canvas).
    fn blit_bgra(&mut self, bgra: &[u8]);
    fn fill_rounded_rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, radius: f32, c: Rgb);
    fn stroke_rounded_rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, radius: f32, width: f32, c: Rgb);
    fn fill_ellipse(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: Rgb);
    /// Arc along a circle of radius `r` (centre of the stroke), angles in degrees
    /// clockwise from 3 o'clock, stroked `thickness` wide.
    fn arc(&mut self, cx: f32, cy: f32, r: f32, thickness: f32, start: f32, sweep: f32, c: Rgb, round_caps: bool);
    fn polyline(&mut self, pts: &[(f32, f32)], width: f32, c: Rgb);
    fn fill_polygon(&mut self, pts: &[(f32, f32)], c: Rgb);
    /// (ascent, descent) in pixels for a font of `size` pixels.
    fn font_metrics(&mut self, f: &FontSpec) -> (f32, f32);
    fn text_width(&mut self, text: &str, f: &FontSpec) -> f32;
    /// Draw text with its baseline at `baseline`.
    fn draw_text(&mut self, x: f32, baseline: f32, text: &str, f: &FontSpec, c: Rgb);
    /// BGRA pixels and row stride in bytes.
    fn pixels(&mut self) -> (Vec<u8>, usize);
}

// ---------------------------------------------------------------------------

pub struct SoftCanvas {
    w: usize,
    h: usize,
    px: Vec<u8>, // BGRA
}

impl SoftCanvas {
    pub fn new(w: usize, h: usize) -> Self {
        SoftCanvas { w, h, px: vec![0; w * h * 4] }
    }

    fn put(&mut self, x: i64, y: i64, c: Rgb) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            let i = (y as usize * self.w + x as usize) * 4;
            self.px[i] = c.2;
            self.px[i + 1] = c.1;
            self.px[i + 2] = c.0;
            self.px[i + 3] = 255;
        }
    }

    fn fill_where<F: Fn(f32, f32) -> bool>(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: Rgb, inside: F) {
        let (xa, xb) = (x0.floor().max(0.0) as i64, x1.ceil().min(self.w as f32) as i64);
        let (ya, yb) = (y0.floor().max(0.0) as i64, y1.ceil().min(self.h as f32) as i64);
        for y in ya..yb {
            for x in xa..xb {
                if inside(x as f32 + 0.5, y as f32 + 0.5) {
                    self.put(x, y, c);
                }
            }
        }
    }
}

fn in_rounded(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> bool {
    if px < x0 || px > x1 || py < y0 || py > y1 {
        return false;
    }
    let r = r.min((x1 - x0) / 2.0).min((y1 - y0) / 2.0).max(0.0);
    let cx = px.clamp(x0 + r, x1 - r);
    let cy = py.clamp(y0 + r, y1 - r);
    (px - cx).powi(2) + (py - cy).powi(2) <= r * r + 0.25
}

fn dist_to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    ((p.0 - a.0 - t * dx).powi(2) + (p.1 - a.1 - t * dy).powi(2)).sqrt()
}

impl Canvas for SoftCanvas {
    fn size(&self) -> (usize, usize) {
        (self.w, self.h)
    }

    fn fill_all(&mut self, c: Rgb) {
        for p in self.px.chunks_mut(4) {
            p.copy_from_slice(&[c.2, c.1, c.0, 255]);
        }
    }

    fn blit_bgra(&mut self, bgra: &[u8]) {
        if bgra.len() == self.px.len() {
            self.px.copy_from_slice(bgra);
        }
    }

    fn fill_rounded_rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, c: Rgb) {
        self.fill_where(x0, y0, x1, y1, c, |px, py| in_rounded(px, py, x0, y0, x1, y1, r));
    }

    fn stroke_rounded_rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, width: f32, c: Rgb) {
        let hw = width / 2.0;
        self.fill_where(x0 - hw, y0 - hw, x1 + hw, y1 + hw, c, |px, py| {
            in_rounded(px, py, x0 - hw, y0 - hw, x1 + hw, y1 + hw, r + hw)
                && !in_rounded(px, py, x0 + hw, y0 + hw, x1 - hw, y1 - hw, (r - hw).max(0.0))
        });
    }

    fn fill_ellipse(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: Rgb) {
        let (cx, cy, rx, ry) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0, (x1 - x0) / 2.0, (y1 - y0) / 2.0);
        if rx <= 0.0 || ry <= 0.0 {
            return;
        }
        self.fill_where(x0, y0, x1, y1, c, |px, py| ((px - cx) / rx).powi(2) + ((py - cy) / ry).powi(2) <= 1.0);
    }

    fn arc(&mut self, cx: f32, cy: f32, r: f32, thickness: f32, start: f32, sweep: f32, c: Rgb, round_caps: bool) {
        let ht = thickness / 2.0;
        let (s, e) = (start, start + sweep);
        self.fill_where(cx - r - ht, cy - r - ht, cx + r + ht, cy + r + ht, c, |px, py| {
            let d = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
            if (d - r).abs() > ht {
                return false;
            }
            let mut a = (py - cy).atan2(px - cx).to_degrees();
            while a < s {
                a += 360.0;
            }
            a <= e
        });
        if round_caps {
            for ang in [s, e] {
                let (x, y) = (cx + r * ang.to_radians().cos(), cy + r * ang.to_radians().sin());
                self.fill_ellipse(x - ht, y - ht, x + ht, y + ht, c);
            }
        }
    }

    fn polyline(&mut self, pts: &[(f32, f32)], width: f32, c: Rgb) {
        let hw = (width / 2.0).max(0.5);
        for seg in pts.windows(2) {
            let (a, b) = (seg[0], seg[1]);
            self.fill_where(a.0.min(b.0) - hw, a.1.min(b.1) - hw, a.0.max(b.0) + hw, a.1.max(b.1) + hw, c, |px, py| {
                dist_to_segment((px, py), a, b) <= hw
            });
        }
    }

    fn fill_polygon(&mut self, pts: &[(f32, f32)], c: Rgb) {
        if pts.len() < 3 {
            return;
        }
        let ymin = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min).max(0.0) as i64;
        let ymax = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max).min(self.h as f32) as i64;
        for y in ymin..=ymax {
            let fy = y as f32 + 0.5;
            let mut xs = Vec::new();
            for i in 0..pts.len() {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                if (a.1 <= fy && b.1 > fy) || (b.1 <= fy && a.1 > fy) {
                    xs.push(a.0 + (fy - a.1) / (b.1 - a.1) * (b.0 - a.0));
                }
            }
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            for pair in xs.chunks(2) {
                if let [x0, x1] = pair {
                    for x in (x0.round() as i64)..(x1.round() as i64) {
                        self.put(x, y, c);
                    }
                }
            }
        }
    }

    fn font_metrics(&mut self, f: &FontSpec) -> (f32, f32) {
        (f.size * 0.76, f.size * 0.24)
    }

    fn text_width(&mut self, text: &str, f: &FontSpec) -> f32 {
        text.chars().count() as f32 * f.size * 0.55
    }

    fn draw_text(&mut self, x: f32, baseline: f32, text: &str, f: &FontSpec, c: Rgb) {
        let size = f.size;
        // Placeholder glyphs: one block per character (real text needs GDI+ on Windows)
        let cw = size * 0.55;
        for (i, ch) in text.chars().enumerate() {
            if ch == ' ' {
                continue;
            }
            let gx = x + i as f32 * cw;
            self.fill_rounded_rect(gx + cw * 0.12, baseline - size * 0.62, gx + cw * 0.88, baseline, 1.0, c);
        }
    }

    fn pixels(&mut self) -> (Vec<u8>, usize) {
        (self.px.clone(), self.w * 4)
    }
}

// ---------------------------------------------------------------------------
// PNG encoding (uncompressed deflate: fine for small, local images)

fn crc32(data: &[u8], mut crc: u32) -> u32 {
    crc = !crc;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB88320 } else { crc >> 1 };
        }
    }
    !crc
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..], 0);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// Encode BGRA pixels as an RGB PNG.
pub fn encode_png(bgra: &[u8], w: usize, h: usize, stride: usize) -> Vec<u8> {
    let mut raw = Vec::with_capacity((w * 3 + 1) * h);
    for y in 0..h {
        raw.push(0); // filter: none
        for x in 0..w {
            let i = y * stride + x * 4;
            raw.extend_from_slice(&[bgra[i + 2], bgra[i + 1], bgra[i]]);
        }
    }
    let mut z = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, b) in blocks.iter().enumerate() {
        z.push(if i + 1 == blocks.len() { 1 } else { 0 });
        z.extend_from_slice(&(b.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(b.len() as u16)).to_le_bytes());
        z.extend_from_slice(b);
    }
    let (mut a, mut bsum) = (1u32, 0u32);
    for &v in &raw {
        a = (a + v as u32) % 65521;
        bsum = (bsum + a) % 65521;
    }
    z.extend_from_slice(&((bsum << 16) | a).to_be_bytes());

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit RGB
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_land_where_expected() {
        let mut c = SoftCanvas::new(40, 30);
        c.fill_all((0, 0, 0));
        c.fill_rounded_rect(5.0, 5.0, 25.0, 15.0, 4.0, (255, 0, 0));
        let (px, stride) = c.pixels();
        let at = |x: usize, y: usize| (px[y * stride + x * 4 + 2], px[y * stride + x * 4 + 1], px[y * stride + x * 4]);
        assert_eq!(at(15, 10), (255, 0, 0));
        assert_eq!(at(5, 5), (0, 0, 0)); // rounded corner stays empty
        assert_eq!(at(30, 10), (0, 0, 0));
        assert_eq!(crc32(b"123456789", 0), 0xCBF43926);
        let png = encode_png(&px, 40, 30, stride);
        assert!(png.starts_with(b"\x89PNG"));
    }
}
