//! The app icon (a tiny screen with three bars), drawn in code so the same
//! picture serves the tray and the Start menu shortcut (`--write-icon`).

type Rgba = (u8, u8, u8, u8);

struct Shape {
    rect: (f32, f32, f32, f32), // on a 64x64 grid
    radius: f32,
    fill: Rgba,
    outline: Option<(Rgba, f32)>,
}

const SHAPES: [Shape; 4] = [
    Shape { rect: (4.0, 4.0, 61.0, 61.0), radius: 14.0, fill: (22, 24, 32, 255), outline: Some(((90, 170, 255, 255), 3.0)) },
    Shape { rect: (14.0, 16.0, 51.0, 27.0), radius: 4.0, fill: (90, 170, 255, 255), outline: None },
    Shape { rect: (14.0, 31.0, 41.0, 40.0), radius: 4.0, fill: (110, 230, 140, 255), outline: None },
    Shape { rect: (14.0, 44.0, 47.0, 52.0), radius: 3.0, fill: (255, 190, 70, 255), outline: None },
];

fn inside(px: f32, py: f32, (x0, y0, x1, y1): (f32, f32, f32, f32), r: f32) -> bool {
    if px < x0 || px > x1 || py < y0 || py > y1 {
        return false;
    }
    let r = r.min((x1 - x0) / 2.0).min((y1 - y0) / 2.0).max(0.0);
    let cx = px.clamp(x0 + r, x1 - r);
    let cy = py.clamp(y0 + r, y1 - r);
    (px - cx).powi(2) + (py - cy).powi(2) <= r * r
}

/// BGRA pixels (straight alpha) of the icon at `size` x `size`.
pub fn draw(size: usize) -> Vec<u8> {
    const SS: usize = 4; // 4x4 samples per pixel for smooth edges
    let scale = 64.0 / size as f32;
    let mut out = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let (mut r, mut g, mut b, mut a) = (0f32, 0f32, 0f32, 0f32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let px = (x as f32 + (sx as f32 + 0.5) / SS as f32) * scale;
                    let py = (y as f32 + (sy as f32 + 0.5) / SS as f32) * scale;
                    let mut c: Rgba = (0, 0, 0, 0);
                    for s in &SHAPES {
                        if inside(px, py, s.rect, s.radius) {
                            c = s.fill;
                            if let Some((oc, w)) = s.outline {
                                let (x0, y0, x1, y1) = s.rect;
                                if !inside(px, py, (x0 + w, y0 + w, x1 - w, y1 - w), (s.radius - w).max(0.0)) {
                                    c = oc;
                                }
                            }
                        }
                    }
                    let ca = c.3 as f32 / 255.0;
                    r += c.0 as f32 * ca;
                    g += c.1 as f32 * ca;
                    b += c.2 as f32 * ca;
                    a += ca;
                }
            }
            let i = (y * size + x) * 4;
            if a > 0.0 {
                out[i] = (b / a).round() as u8;
                out[i + 1] = (g / a).round() as u8;
                out[i + 2] = (r / a).round() as u8;
                out[i + 3] = (a / (SS * SS) as f32 * 255.0).round() as u8;
            }
        }
    }
    out
}

/// A Windows .ico file with the icon at several sizes.
pub fn ico() -> Vec<u8> {
    let sizes = [16usize, 20, 24, 32, 40, 48, 64, 256];
    let images: Vec<Vec<u8>> = sizes
        .iter()
        .map(|&s| {
            let px = draw(s);
            let mut d = Vec::new();
            // BITMAPINFOHEADER; height counts the colour rows plus the mask rows
            for v in [40u32, s as u32, (s * 2) as u32] {
                d.extend_from_slice(&v.to_le_bytes());
            }
            d.extend_from_slice(&1u16.to_le_bytes());
            d.extend_from_slice(&32u16.to_le_bytes());
            d.extend_from_slice(&[0u8; 24]);
            for y in (0..s).rev() {
                d.extend_from_slice(&px[y * s * 4..(y + 1) * s * 4]);
            }
            let mask_row = s.div_ceil(32) * 4;
            d.extend(std::iter::repeat(0u8).take(mask_row * s));
            d
        })
        .collect();
    let mut out = vec![0, 0, 1, 0];
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len();
    for (s, img) in sizes.iter().zip(&images) {
        let dim = if *s >= 256 { 0 } else { *s as u8 };
        out.extend_from_slice(&[dim, dim, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(img.len() as u32).to_le_bytes());
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += img.len();
    }
    for img in images {
        out.extend(img);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn icon_shapes() {
        let px = super::draw(64);
        let at = |x: usize, y: usize| &px[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
        assert_eq!(at(0, 0)[3], 0); // transparent corner
        assert_eq!(at(30, 20), &[255, 170, 90, 255]); // blue bar (BGRA)
        assert_eq!(at(55, 32), &[32, 24, 22, 255]); // body
        let ico = super::ico();
        assert_eq!(&ico[0..4], &[0, 0, 1, 0]);
    }
}
