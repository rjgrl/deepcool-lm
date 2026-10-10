//! Drawing and image decoding with GDI+ (part of Windows): anti-aliased
//! shapes, ClearType-quality text and PNG/JPG/GIF/BMP decoding.

use super::ffi::*;
use crate::canvas::{Canvas, FontSpec, Rgb};
use crate::platform::{fit_rects, ImageSource};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Once;

static STARTUP: Once = Once::new();

pub fn startup() {
    STARTUP.call_once(|| unsafe {
        let mut token = 0usize;
        let input = GdiplusStartupInput { GdiplusVersion: 1, DebugEventCallback: std::ptr::null_mut(), SuppressBackgroundThread: 0, SuppressExternalCodecs: 0 };
        let status = GdiplusStartup(&mut token, &input, std::ptr::null_mut());
        if status != 0 {
            log!("GDI+ failed to start (status {})", status);
        }
    });
}

fn argb(c: Rgb) -> u32 {
    0xFF00_0000 | (c.0 as u32) << 16 | (c.1 as u32) << 8 | c.2 as u32
}

const FONT_TEXT: &str = "Segoe UI";
/// Has the arrows, shapes and dingbats (⚙ ▣ ▲) that Segoe UI lacks.
const FONT_SYMBOL: &str = "Segoe UI Symbol";

struct FontEntry {
    font: *mut GpFont,
    ascent: f32,
    descent: f32,
}

/// A BGRA bitmap with a GDI+ drawing surface on top.
pub struct GdiCanvas {
    w: usize,
    h: usize,
    px: Vec<u8>,
    bitmap: *mut GpBitmap,
    g: *mut GpGraphics,
    format: *mut GpStringFormat,
    families: HashMap<String, *mut GpFontFamily>, // null = not installed
    fonts: HashMap<(String, u32, i32), FontEntry>,
}

pub fn new_canvas(w: usize, h: usize) -> Box<dyn Canvas> {
    startup();
    let mut c = GdiCanvas {
        w,
        h,
        px: vec![0u8; w * h * 4],
        bitmap: std::ptr::null_mut(),
        g: std::ptr::null_mut(),
        format: std::ptr::null_mut(),
        families: HashMap::new(),
        fonts: HashMap::new(),
    };
    unsafe {
        GdipCreateBitmapFromScan0(w as i32, h as i32, (w * 4) as i32, PIXEL_FORMAT_32BPP_ARGB, c.px.as_mut_ptr(), &mut c.bitmap);
        if !c.bitmap.is_null() {
            GdipGetImageGraphicsContext(c.bitmap, &mut c.g);
        }
        if !c.g.is_null() {
            GdipSetSmoothingMode(c.g, SMOOTHING_ANTIALIAS);
            GdipSetPixelOffsetMode(c.g, PIXEL_OFFSET_HALF);
            GdipSetTextRenderingHint(c.g, TEXT_ANTIALIAS_GRIDFIT);
            GdipSetCompositingQuality(c.g, COMPOSITING_QUALITY_HIGH);
        }
        let mut generic: *mut GpStringFormat = std::ptr::null_mut();
        GdipStringFormatGetGenericTypographic(&mut generic);
        if !generic.is_null() {
            GdipCloneStringFormat(generic, &mut c.format);
            if !c.format.is_null() {
                GdipSetStringFormatFlags(c.format, STRING_FORMAT_MEASURE_TRAILING_SPACES | STRING_FORMAT_NO_WRAP | STRING_FORMAT_NO_CLIP);
            }
        }
    }
    Box::new(c)
}

impl Drop for GdiCanvas {
    fn drop(&mut self) {
        unsafe {
            for f in self.fonts.values() {
                GdipDeleteFont(f.font);
            }
            for fam in self.families.values().filter(|f| !f.is_null()) {
                GdipDeleteFontFamily(*fam);
            }
            if !self.format.is_null() {
                GdipDeleteStringFormat(self.format);
            }
            if !self.g.is_null() {
                GdipDeleteGraphics(self.g);
            }
            if !self.bitmap.is_null() {
                GdipDisposeImage(self.bitmap);
            }
        }
    }
}

struct Brush(*mut GpBrush);
impl Brush {
    fn new(c: Rgb) -> Brush {
        let mut b = std::ptr::null_mut();
        unsafe { GdipCreateSolidFill(argb(c), &mut b) };
        Brush(b)
    }
}
impl Drop for Brush {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { GdipDeleteBrush(self.0) };
        }
    }
}

struct Pen(*mut GpPen);
impl Pen {
    fn new(c: Rgb, width: f32, round: bool) -> Pen {
        let mut p = std::ptr::null_mut();
        unsafe {
            GdipCreatePen1(argb(c), width, UNIT_PIXEL, &mut p);
            if round && !p.is_null() {
                GdipSetPenStartCap(p, LINE_CAP_ROUND);
                GdipSetPenEndCap(p, LINE_CAP_ROUND);
                GdipSetPenLineJoin(p, LINE_JOIN_ROUND);
            }
        }
        Pen(p)
    }
}
impl Drop for Pen {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { GdipDeletePen(self.0) };
        }
    }
}

struct Path(*mut GpPath);
impl Path {
    fn rounded(x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> Path {
        let mut p = std::ptr::null_mut();
        unsafe {
            GdipCreatePath(0, &mut p);
            let (w, h) = (x1 - x0, y1 - y0);
            let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
            if r < 0.5 {
                GdipAddPathRectangle(p, x0, y0, w, h);
            } else {
                let d = r * 2.0;
                GdipAddPathArc(p, x0, y0, d, d, 180.0, 90.0);
                GdipAddPathArc(p, x1 - d, y0, d, d, 270.0, 90.0);
                GdipAddPathArc(p, x1 - d, y1 - d, d, d, 0.0, 90.0);
                GdipAddPathArc(p, x0, y1 - d, d, d, 90.0, 90.0);
                GdipClosePathFigure(p);
            }
        }
        Path(p)
    }
}
impl Drop for Path {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { GdipDeletePath(self.0) };
        }
    }
}

impl GdiCanvas {
    /// Font family by name, or null when it isn't installed.
    fn family(&mut self, name: &str) -> *mut GpFontFamily {
        if let Some(f) = self.families.get(name) {
            return *f;
        }
        let mut fam = std::ptr::null_mut();
        unsafe { GdipCreateFontFamilyFromName(wide(name).as_ptr(), std::ptr::null_mut(), &mut fam) };
        self.families.insert(name.to_string(), fam);
        fam
    }

    fn font(&mut self, f: &FontSpec) -> Option<&FontEntry> {
        let name = if f.symbol {
            FONT_SYMBOL
        } else if f.family.is_empty() {
            FONT_TEXT
        } else {
            f.family
        };
        // GDI+ styles: 1 bold, 2 italic
        let wanted = if f.symbol { 0 } else { (f.bold as i32) | (f.italic as i32) << 1 };
        let key = (name.to_string(), (f.size * 10.0).round() as u32, wanted);
        if !self.fonts.contains_key(&key) {
            let mut fam = self.family(name);
            if fam.is_null() {
                fam = self.family(FONT_TEXT); // not installed: use the default
            }
            if fam.is_null() {
                return None;
            }
            unsafe {
                // some fonts lack a bold or italic face; take the closest one they have
                let mut font = std::ptr::null_mut();
                let mut style = wanted;
                for s in [wanted, wanted & 1, wanted & 2, 0] {
                    GdipCreateFont(fam, f.size, s, UNIT_PIXEL, &mut font);
                    if !font.is_null() {
                        style = s;
                        break;
                    }
                }
                if font.is_null() {
                    return None;
                }
                let (mut a, mut d, mut em) = (0u16, 0u16, 0u16);
                GdipGetCellAscent(fam, style, &mut a);
                GdipGetCellDescent(fam, style, &mut d);
                GdipGetEmHeight(fam, style, &mut em);
                let em = if em == 0 { 2048.0 } else { em as f32 };
                self.fonts.insert(key.clone(), FontEntry { font, ascent: f.size * a as f32 / em, descent: f.size * d as f32 / em });
            }
        }
        self.fonts.get(&key)
    }
}

impl Canvas for GdiCanvas {
    fn size(&self) -> (usize, usize) {
        (self.w, self.h)
    }

    fn fill_all(&mut self, c: Rgb) {
        unsafe { GdipGraphicsClear(self.g, argb(c)) };
    }

    fn blit_bgra(&mut self, bgra: &[u8]) {
        if bgra.len() == self.px.len() {
            unsafe { GdipFlush(self.g, FLUSH_SYNC) };
            self.px.copy_from_slice(bgra);
            for a in self.px.iter_mut().skip(3).step_by(4) {
                *a = 255;
            }
        }
    }

    fn fill_rounded_rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, radius: f32, c: Rgb) {
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let b = Brush::new(c);
        unsafe {
            if radius < 0.5 {
                GdipFillRectangle(self.g, b.0, x0, y0, x1 - x0, y1 - y0);
            } else {
                let p = Path::rounded(x0, y0, x1, y1, radius);
                GdipFillPath(self.g, b.0, p.0);
            }
        }
    }

    fn stroke_rounded_rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, radius: f32, width: f32, c: Rgb) {
        if x1 <= x0 || y1 <= y0 || width <= 0.0 {
            return;
        }
        let pen = Pen::new(c, width, false);
        let p = Path::rounded(x0, y0, x1, y1, radius);
        unsafe { GdipDrawPath(self.g, pen.0, p.0) };
    }

    fn fill_ellipse(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: Rgb) {
        let b = Brush::new(c);
        unsafe { GdipFillEllipse(self.g, b.0, x0, y0, x1 - x0, y1 - y0) };
    }

    fn arc(&mut self, cx: f32, cy: f32, r: f32, thickness: f32, start: f32, sweep: f32, c: Rgb, round_caps: bool) {
        if r <= 0.0 || sweep <= 0.0 {
            return;
        }
        let pen = Pen::new(c, thickness, round_caps);
        unsafe { GdipDrawArc(self.g, pen.0, cx - r, cy - r, r * 2.0, r * 2.0, start, sweep) };
    }

    fn polyline(&mut self, pts: &[(f32, f32)], width: f32, c: Rgb) {
        if pts.len() < 2 {
            return;
        }
        let pen = Pen::new(c, width, true);
        let p: Vec<PointF> = pts.iter().map(|&(x, y)| PointF { x, y }).collect();
        unsafe { GdipDrawLines(self.g, pen.0, p.as_ptr(), p.len() as i32) };
    }

    fn fill_polygon(&mut self, pts: &[(f32, f32)], c: Rgb) {
        if pts.len() < 3 {
            return;
        }
        let b = Brush::new(c);
        let p: Vec<PointF> = pts.iter().map(|&(x, y)| PointF { x, y }).collect();
        unsafe { GdipFillPolygon(self.g, b.0, p.as_ptr(), p.len() as i32, FILL_MODE_WINDING) };
    }

    fn font_metrics(&mut self, spec: &FontSpec) -> (f32, f32) {
        match self.font(spec) {
            Some(f) => (f.ascent, f.descent),
            None => (spec.size * 0.76, spec.size * 0.24),
        }
    }

    fn text_width(&mut self, text: &str, spec: &FontSpec) -> f32 {
        let (g, format) = (self.g, self.format);
        let Some(f) = self.font(spec) else { return text.chars().count() as f32 * spec.size * 0.55 };
        let w: Vec<u16> = text.encode_utf16().collect();
        let layout = RectF { x: 0.0, y: 0.0, w: 10000.0, h: 10000.0 };
        let mut bounds = RectF::default();
        unsafe {
            GdipMeasureString(g, w.as_ptr(), w.len() as i32, f.font, &layout, format, &mut bounds, std::ptr::null_mut(), std::ptr::null_mut());
        }
        bounds.w
    }

    fn draw_text(&mut self, x: f32, baseline: f32, text: &str, spec: &FontSpec, c: Rgb) {
        let (g, format, size) = (self.g, self.format, spec.size);
        // each font has its own ascent; line every run up on the same baseline
        let Some(f) = self.font(spec) else { return };
        let (font, ascent) = (f.font, f.ascent);
        let w: Vec<u16> = text.encode_utf16().collect();
        let b = Brush::new(c);
        let layout = RectF { x, y: baseline - ascent, w: 10000.0, h: size * 3.0 };
        unsafe { GdipDrawString(g, w.as_ptr(), w.len() as i32, font, &layout, format, b.0) };
    }

    fn pixels(&mut self) -> (Vec<u8>, usize) {
        unsafe { GdipFlush(self.g, FLUSH_SYNC) };
        (self.px.clone(), self.w * 4)
    }
}

// The canvas lives on one thread at a time.
unsafe impl Send for GdiCanvas {}

// ---------------------------------------------------------------------------
// Image decoding

pub struct GdiImage {
    image: *mut GpImage,
    stream: *mut c_void,
    dimension: GUID,
    frames: usize,
    delays: Vec<f64>,
    w: usize,
    h: usize,
}

unsafe impl Send for GdiImage {}

impl Drop for GdiImage {
    fn drop(&mut self) {
        unsafe {
            GdipDisposeImage(self.image);
            com_release(self.stream);
        }
    }
}

/// Open an image file. The file is read into memory, so it isn't kept locked.
pub fn open_image(path: &std::path::Path) -> Result<Box<dyn ImageSource>, String> {
    startup();
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    if data.len() > u32::MAX as usize {
        return Err("file too large".into());
    }
    unsafe {
        let stream = SHCreateMemStream(data.as_ptr(), data.len() as u32);
        drop(data);
        if stream.is_null() {
            return Err("out of memory".into());
        }
        let mut image: *mut GpBitmap = std::ptr::null_mut();
        let status = GdipCreateBitmapFromStream(stream, &mut image);
        if status != 0 || image.is_null() {
            com_release(stream);
            return Err("not a PNG, JPG, GIF or BMP image".into());
        }
        let (mut w, mut h) = (0u32, 0u32);
        GdipGetImageWidth(image, &mut w);
        GdipGetImageHeight(image, &mut h);
        let mut dims = 0u32;
        GdipImageGetFrameDimensionsCount(image, &mut dims);
        let mut dimension = GUID::default();
        let mut frames = 1u32;
        if dims > 0 {
            let mut list = vec![GUID::default(); dims as usize];
            GdipImageGetFrameDimensionsList(image, list.as_mut_ptr(), dims);
            dimension = list[0];
            GdipImageGetFrameCount(image, &dimension, &mut frames);
        }
        let frames = frames.max(1) as usize;
        let mut delays = vec![0.1; frames];
        let mut size = 0u32;
        if frames > 1 && GdipGetPropertyItemSize(image, PROPERTY_TAG_FRAME_DELAY, &mut size) == 0 && size as usize > std::mem::size_of::<PropertyItem>() {
            let mut buf = vec![0u64; size as usize / 8 + 1];
            let item = buf.as_mut_ptr() as *mut PropertyItem;
            if GdipGetPropertyItem(image, PROPERTY_TAG_FRAME_DELAY, size, item) == 0 && !(*item).value.is_null() {
                let n = ((*item).length as usize / 4).min(frames);
                let values = std::slice::from_raw_parts((*item).value as *const u32, n);
                for (d, v) in delays.iter_mut().zip(values) {
                    // hundredths of a second; 0 means "as fast as possible" (browsers use 0.1 s)
                    *d = if *v == 0 { 0.1 } else { (*v as f64 / 100.0).max(0.02) };
                }
            }
        }
        Ok(Box::new(GdiImage { image, stream, dimension, frames, delays, w: w as usize, h: h as usize }))
    }
}

impl ImageSource for GdiImage {
    fn size(&self) -> (usize, usize) {
        (self.w, self.h)
    }

    fn frame_count(&self) -> usize {
        self.frames
    }

    fn delay(&self, i: usize) -> f64 {
        self.delays[i % self.frames]
    }

    fn render(&mut self, i: usize, fit: &str, w: usize, h: usize, bg: (u8, u8, u8)) -> Result<Vec<u8>, String> {
        unsafe {
            if self.frames > 1 {
                GdipImageSelectActiveFrame(self.image, &self.dimension, (i % self.frames) as u32);
            }
            let mut px = vec![0u8; w * h * 4];
            let mut bitmap: *mut GpBitmap = std::ptr::null_mut();
            GdipCreateBitmapFromScan0(w as i32, h as i32, (w * 4) as i32, PIXEL_FORMAT_32BPP_ARGB, px.as_mut_ptr(), &mut bitmap);
            if bitmap.is_null() {
                return Err("out of memory".into());
            }
            let mut g: *mut GpGraphics = std::ptr::null_mut();
            GdipGetImageGraphicsContext(bitmap, &mut g);
            if g.is_null() {
                GdipDisposeImage(bitmap);
                return Err("can't draw image".into());
            }
            GdipSetInterpolationMode(g, INTERPOLATION_HQ_BICUBIC);
            GdipSetPixelOffsetMode(g, PIXEL_OFFSET_HALF);
            GdipSetCompositingQuality(g, COMPOSITING_QUALITY_HIGH);
            GdipGraphicsClear(g, argb(bg)); // transparent pixels show the background color
            let ((sx, sy, sw, sh), (dx, dy, dw, dh)) = fit_rects(self.w, self.h, fit, w, h);
            let status = GdipDrawImageRectRect(
                g,
                self.image,
                dx as f32,
                dy as f32,
                dw as f32,
                dh as f32,
                sx as f32,
                sy as f32,
                sw as f32,
                sh as f32,
                UNIT_PIXEL,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            GdipFlush(g, FLUSH_SYNC);
            GdipDeleteGraphics(g);
            GdipDisposeImage(bitmap);
            if status != 0 {
                return Err(format!("can't draw image (GDI+ status {})", status));
            }
            Ok(px)
        }
    }
}

/// HICON from BGRA pixels with alpha (for the tray icon).
pub fn icon_from_bgra(px: &mut [u8], size: usize) -> HICON {
    startup();
    unsafe {
        let mut bitmap: *mut GpBitmap = std::ptr::null_mut();
        GdipCreateBitmapFromScan0(size as i32, size as i32, (size * 4) as i32, PIXEL_FORMAT_32BPP_ARGB, px.as_mut_ptr(), &mut bitmap);
        let mut icon: HICON = std::ptr::null_mut();
        if !bitmap.is_null() {
            GdipCreateHICONFromBitmap(bitmap, &mut icon);
            GdipDisposeImage(bitmap);
        }
        icon
    }
}

/// Names of the installed font families, sorted.
pub fn font_families() -> Vec<String> {
    startup();
    let mut out = Vec::new();
    unsafe {
        let mut coll: *mut c_void = std::ptr::null_mut();
        if GdipNewInstalledFontCollection(&mut coll) != 0 || coll.is_null() {
            return out;
        }
        let mut n = 0i32;
        GdipGetFontCollectionFamilyCount(coll, &mut n);
        if n <= 0 {
            return out;
        }
        let mut list: Vec<*mut GpFontFamily> = vec![std::ptr::null_mut(); n as usize];
        let mut found = 0i32;
        GdipGetFontCollectionFamilyList(coll, n, list.as_mut_ptr(), &mut found);
        for fam in list.iter().take(found.max(0) as usize).filter(|f| !f.is_null()) {
            let mut name = [0u16; 32]; // LF_FACESIZE
            if GdipGetFamilyName(*fam, name.as_mut_ptr(), 0) == 0 {
                let n = from_wide(&name);
                // skip vertical-writing variants like "@MS Gothic"
                if !n.is_empty() && !n.starts_with('@') {
                    out.push(n);
                }
            }
        }
        // the installed collection and its families belong to GDI+; nothing to free
    }
    out.sort_by_key(|n| n.to_lowercase());
    out.dedup();
    out
}
