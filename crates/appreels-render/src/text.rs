//! Font loading and small text drawing helpers.

use std::sync::OnceLock;

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use image::RgbaImage;

static FONT: OnceLock<FontRef<'static>> = OnceLock::new();

/// Load the bundled bold UI font.
pub fn font() -> FontRef<'static> {
    FONT.get_or_init(|| {
        FontRef::try_from_slice(include_bytes!("../assets/DejaVuSans-Bold.ttf"))
            .expect("bundled font should parse")
    })
    .clone()
}

/// Measure rendered text width in pixels.
pub fn text_width(font: &FontRef<'static>, text: &str, px: f32) -> f32 {
    let scaled = font.as_scaled(PxScale::from(px));
    let mut width = 0.0;
    let mut prev = None;
    for c in text.chars() {
        let gid = font.glyph_id(c);
        if let Some(p) = prev {
            width += scaled.kern(p, gid);
        }
        width += scaled.h_advance(gid);
        prev = Some(gid);
    }
    width
}

/// Alpha-composite `color` onto one pixel at `coverage` (0..1).
pub(crate) fn blend_pixel(img: &mut RgbaImage, x: u32, y: u32, color: [u8; 3], coverage: f32) {
    let a = coverage.clamp(0.0, 1.0);
    if a <= 0.0 {
        return;
    }
    let p = img.get_pixel_mut(x, y);
    for (i, channel) in color.iter().enumerate() {
        p.0[i] = (f32::from(*channel) * a + f32::from(p.0[i]) * (1.0 - a)).round() as u8;
    }
    p.0[3] = 255;
}

/// Draw `text` with its top-left at `(x, y)`.
pub fn draw_text(
    img: &mut RgbaImage,
    font: &FontRef<'static>,
    text: &str,
    px: f32,
    x: f32,
    y: f32,
    color: [u8; 3],
) {
    let scaled = font.as_scaled(PxScale::from(px));
    let ascent = scaled.ascent();
    let (iw, ih) = img.dimensions();
    let mut caret = x;
    let mut prev = None;
    for c in text.chars() {
        let gid = font.glyph_id(c);
        if let Some(p) = prev {
            caret += scaled.kern(p, gid);
        }
        let glyph = gid.with_scale_and_position(PxScale::from(px), point(caret, y + ascent));
        if let Some(outline) = font.outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, cov| {
                let px_x = bounds.min.x as i32 + gx as i32;
                let px_y = bounds.min.y as i32 + gy as i32;
                if px_x < 0 || px_y < 0 {
                    return;
                }
                let (px_x, px_y) = (px_x as u32, px_y as u32);
                if px_x < iw && px_y < ih {
                    blend_pixel(img, px_x, px_y, color, cov);
                }
            });
        }
        caret += scaled.h_advance(gid);
        prev = Some(gid);
    }
}

/// Draw `text` horizontally centered, with its top at `y`.
pub fn draw_text_centered(
    img: &mut RgbaImage,
    font: &FontRef<'static>,
    text: &str,
    px: f32,
    y: f32,
    color: [u8; 3],
) {
    let width = text_width(font, text, px);
    let x = (img.width() as f32 - width) / 2.0;
    draw_text(img, font, text, px, x.max(0.0), y, color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn bundled_font_loads() {
        let f = font();
        assert!(text_width(&f, "Hello", 32.0) > 0.0);
    }

    #[test]
    fn drawing_text_changes_pixels() {
        let f = font();
        let mut img = RgbaImage::from_pixel(240, 80, Rgba([0, 0, 0, 255]));
        draw_text(&mut img, &f, "Hello", 32.0, 10.0, 10.0, [255, 255, 255]);
        assert!(img.pixels().any(|p| p.0[0] > 0));
    }

    #[test]
    fn centered_text_changes_center_band() {
        let f = font();
        let mut img = RgbaImage::from_pixel(240, 80, Rgba([0, 0, 0, 255]));
        draw_text_centered(&mut img, &f, "Hi", 32.0, 10.0, [255, 255, 255]);
        assert!(img.pixels().any(|p| p.0[0] > 0));
    }
}
