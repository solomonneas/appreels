//! appshots-style image framing.

#[derive(Debug, Clone)]
pub struct PresentationStyle {
    pub seed: u64,
    pub palette_name: String,
    pub start: [u8; 3],
    pub end: [u8; 3],
    pub accent: [u8; 3],
    pub padding: u32,
    pub corner_radius: u32,
    pub shadow_blur: f32,
    pub shadow_offset_y: i32,
}

/// A named palette: `(name, start, end, accent)`.
pub type Palette = (&'static str, [u8; 3], [u8; 3], [u8; 3]);

pub const PALETTES: [Palette; 5] = [
    ("dusk-berry", [34, 40, 78], [178, 48, 104], [118, 79, 178]),
    ("aurora-teal", [15, 77, 87], [62, 148, 126], [165, 212, 141]),
    (
        "graphite-rose",
        [38, 42, 49],
        [158, 64, 91],
        [222, 134, 113],
    ),
    (
        "indigo-copper",
        [31, 45, 92],
        [190, 104, 62],
        [240, 167, 92],
    ),
    ("forest-slate", [23, 65, 55], [73, 88, 103], [129, 160, 126]),
];

// Deterministic splitmix64 so the crate needs no rng dependency.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

fn range_u32(state: &mut u64, lo: u32, hi: u32) -> u32 {
    lo + (splitmix64(state) % u64::from(hi - lo + 1)) as u32
}

pub fn style_from_seed(seed: u64) -> PresentationStyle {
    let mut state = seed;
    let idx = (splitmix64(&mut state) % PALETTES.len() as u64) as usize;
    let (name, start, end, accent) = PALETTES[idx];
    PresentationStyle {
        seed,
        palette_name: name.to_string(),
        start,
        end,
        accent,
        padding: range_u32(&mut state, 56, 88),
        corner_radius: range_u32(&mut state, 14, 22),
        shadow_blur: range_u32(&mut state, 18, 30) as f32,
        shadow_offset_y: range_u32(&mut state, 14, 28) as i32,
    }
}

use image::{ImageBuffer, Rgba, RgbaImage, imageops};

const SHADOW_ALPHA: u8 = 95;

/// Frame a single image with the appshots look: gradient backdrop, rounded
/// corners, soft shadow, padding. Pure and deterministic.
pub fn compose_frame(input: &RgbaImage, style: &PresentationStyle) -> RgbaImage {
    let window = rounded_window(input, style.corner_radius);
    let (w, h) = window.dimensions();
    let canvas_w = w + style.padding * 2;
    let canvas_h = h + style.padding * 2 + style.shadow_offset_y as u32;

    let mut canvas = gradient_backdrop(canvas_w, canvas_h, style);
    let shadow = shadow_layer(w, h, canvas_w, canvas_h, style);
    alpha_composite(&mut canvas, &shadow, 0, 0);
    alpha_composite(
        &mut canvas,
        &window,
        style.padding as i32,
        style.padding as i32,
    );
    canvas
}

fn rounded_window(input: &RgbaImage, radius: u32) -> RgbaImage {
    let mut image = input.clone();
    let (width, height) = image.dimensions();
    for y in 0..height {
        for x in 0..width {
            let alpha = rounded_alpha(x, y, width, height, radius);
            if alpha < 255 {
                let p = image.get_pixel_mut(x, y);
                p.0[3] = ((u16::from(p.0[3]) * u16::from(alpha)) / 255) as u8;
            }
        }
    }
    image
}

fn rounded_alpha(x: u32, y: u32, width: u32, height: u32, radius: u32) -> u8 {
    if radius == 0 || width <= radius * 2 || height <= radius * 2 {
        return 255;
    }
    let cx = if x < radius {
        Some(radius as i32)
    } else if x >= width - radius {
        Some((width - radius - 1) as i32)
    } else {
        None
    };
    let cy = if y < radius {
        Some(radius as i32)
    } else if y >= height - radius {
        Some((height - radius - 1) as i32)
    } else {
        None
    };
    let (Some(cx), Some(cy)) = (cx, cy) else {
        return 255;
    };
    let dx = x as i32 - cx;
    let dy = y as i32 - cy;
    let distance = ((dx * dx + dy * dy) as f32).sqrt();
    let edge = radius as f32;
    if distance <= edge - 1.0 {
        255
    } else if distance >= edge {
        0
    } else {
        ((edge - distance) * 255.0).round() as u8
    }
}

fn shadow_layer(
    win_w: u32,
    win_h: u32,
    canvas_w: u32,
    canvas_h: u32,
    style: &PresentationStyle,
) -> RgbaImage {
    let mut mask = RgbaImage::from_pixel(canvas_w, canvas_h, Rgba([0, 0, 0, 0]));
    let sx = style.padding as i32;
    let sy = style.padding as i32 + style.shadow_offset_y;
    for y in 0..win_h {
        for x in 0..win_w {
            let alpha = rounded_alpha(x, y, win_w, win_h, style.corner_radius);
            if alpha == 0 {
                continue;
            }
            let (tx, ty) = (sx + x as i32, sy + y as i32);
            if tx < 0 || ty < 0 {
                continue;
            }
            let (tx, ty) = (tx as u32, ty as u32);
            if tx < canvas_w && ty < canvas_h {
                let sa = ((u16::from(alpha) * u16::from(SHADOW_ALPHA)) / 255) as u8;
                mask.put_pixel(tx, ty, Rgba([0, 0, 0, sa]));
            }
        }
    }
    imageops::blur(&mask, style.shadow_blur)
}

/// Render the appshots gradient backdrop at the given size. Pure, opaque.
pub fn gradient_backdrop(width: u32, height: u32, style: &PresentationStyle) -> RgbaImage {
    ImageBuffer::from_fn(width, height, |x, y| {
        let fx = x as f32 / width.max(1) as f32;
        let fy = y as f32 / height.max(1) as f32;
        let diagonal = fx * 0.62 + fy * 0.38;
        let radial = ((fx - 0.78).powi(2) + (fy - 0.18).powi(2)).sqrt();
        let accent_mix = (1.0 - radial * 1.6).clamp(0.0, 0.45);
        let base = mix_rgb(style.start, style.end, diagonal);
        let mixed = mix_rgb(base, style.accent, accent_mix);
        let vignette = 1.0 - (((fx - 0.5).powi(2) + (fy - 0.5).powi(2)).sqrt() * 0.18);
        Rgba([
            (f32::from(mixed[0]) * vignette).round().clamp(0.0, 255.0) as u8,
            (f32::from(mixed[1]) * vignette).round().clamp(0.0, 255.0) as u8,
            (f32::from(mixed[2]) * vignette).round().clamp(0.0, 255.0) as u8,
            255,
        ])
    })
}

fn mix_rgb(start: [u8; 3], end: [u8; 3], amount: f32) -> [u8; 3] {
    [
        lerp(f32::from(start[0]), f32::from(end[0]), amount) as u8,
        lerp(f32::from(start[1]), f32::from(end[1]), amount) as u8,
        lerp(f32::from(start[2]), f32::from(end[2]), amount) as u8,
    ]
}

fn lerp(start: f32, end: f32, amount: f32) -> f32 {
    start + (end - start) * amount.clamp(0.0, 1.0)
}

fn alpha_composite(base: &mut RgbaImage, overlay: &RgbaImage, offset_x: i32, offset_y: i32) {
    let (bw, bh) = base.dimensions();
    for y in 0..overlay.height() {
        for x in 0..overlay.width() {
            let (tx, ty) = (offset_x + x as i32, offset_y + y as i32);
            if tx < 0 || ty < 0 {
                continue;
            }
            let (tx, ty) = (tx as u32, ty as u32);
            if tx >= bw || ty >= bh {
                continue;
            }
            let src = overlay.get_pixel(x, y);
            let alpha = f32::from(src.0[3]) / 255.0;
            if alpha == 0.0 {
                continue;
            }
            let dst = base.get_pixel(tx, ty);
            let inv = 1.0 - alpha;
            base.put_pixel(
                tx,
                ty,
                Rgba([
                    (f32::from(src.0[0]) * alpha + f32::from(dst.0[0]) * inv).round() as u8,
                    (f32::from(src.0[1]) * alpha + f32::from(dst.0[1]) * inv).round() as u8,
                    (f32::from(src.0[2]) * alpha + f32::from(dst.0[2]) * inv).round() as u8,
                    255,
                ]),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_from_seed_is_deterministic() {
        let a = style_from_seed(12345);
        let b = style_from_seed(12345);
        assert_eq!(a.palette_name, b.palette_name);
        assert_eq!(a.padding, b.padding);
        assert_eq!(a.start, b.start);
    }

    #[test]
    fn style_uses_a_known_palette() {
        let style = style_from_seed(1);
        assert!(PALETTES.iter().any(|p| p.0 == style.palette_name));
    }

    use image::{Rgba, RgbaImage};

    #[test]
    fn compose_frame_pads_by_style() {
        let style = style_from_seed(42);
        let input = RgbaImage::from_pixel(100, 60, Rgba([10, 20, 30, 255]));
        let out = compose_frame(&input, &style);
        assert_eq!(out.width(), 100 + style.padding * 2);
        assert_eq!(
            out.height(),
            60 + style.padding * 2 + style.shadow_offset_y as u32
        );
    }

    #[test]
    fn compose_frame_is_opaque() {
        let style = style_from_seed(7);
        let input = RgbaImage::from_pixel(40, 40, Rgba([255, 255, 255, 255]));
        let out = compose_frame(&input, &style);
        assert_eq!(out.get_pixel(0, 0).0[3], 255); // backdrop corner is opaque
    }

    #[test]
    fn gradient_backdrop_is_sized_and_opaque() {
        let style = style_from_seed(42);
        let bg = gradient_backdrop(120, 80, &style);
        assert_eq!(bg.dimensions(), (120, 80));
        assert_eq!(bg.get_pixel(0, 0).0[3], 255);
        assert_eq!(bg.get_pixel(119, 79).0[3], 255);
    }
}
