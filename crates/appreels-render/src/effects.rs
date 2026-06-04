//! Per-frame pixel effects: zoom/pan, cursor ring, caption bar.

use ab_glyph::FontRef;
use image::{RgbaImage, imageops};

use crate::text::{blend_pixel, draw_text};
use crate::timeline::{CaptionPosition, ZoomState};

const RING_RADIUS: f64 = 14.0;
const RING_THICKNESS: f64 = 3.0;

/// Maps a source-frame point into the zoomed image produced by [`apply_zoom`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomTransform {
    pub left: f64,
    pub top: f64,
    pub sx: f64,
    pub sy: f64,
}

impl ZoomTransform {
    pub fn identity() -> Self {
        ZoomTransform {
            left: 0.0,
            top: 0.0,
            sx: 1.0,
            sy: 1.0,
        }
    }

    /// Map `(x, y)` from source-frame pixels to zoomed-image pixels.
    pub fn map(&self, x: f64, y: f64) -> (f64, f64) {
        ((x - self.left) * self.sx, (y - self.top) * self.sy)
    }
}

/// Crop a sub-rect centered on the zoom and scale it back to the frame size.
pub fn apply_zoom(frame: &RgbaImage, zoom: ZoomState) -> (RgbaImage, ZoomTransform) {
    let (w, h) = frame.dimensions();
    if zoom.scale <= 1.0 {
        return (frame.clone(), ZoomTransform::identity());
    }
    let crop_w = (f64::from(w) / zoom.scale).round().clamp(1.0, f64::from(w));
    let crop_h = (f64::from(h) / zoom.scale).round().clamp(1.0, f64::from(h));
    let left = (zoom.cx - crop_w / 2.0)
        .round()
        .clamp(0.0, f64::from(w) - crop_w);
    let top = (zoom.cy - crop_h / 2.0)
        .round()
        .clamp(0.0, f64::from(h) - crop_h);
    let sub =
        imageops::crop_imm(frame, left as u32, top as u32, crop_w as u32, crop_h as u32).to_image();
    let scaled = imageops::resize(&sub, w, h, imageops::FilterType::Triangle);
    (
        scaled,
        ZoomTransform {
            left,
            top,
            sx: f64::from(w) / crop_w,
            sy: f64::from(h) / crop_h,
        },
    )
}

/// Draw a soft accent ring centered at `(cx, cy)` in image space.
pub fn draw_cursor_ring(img: &mut RgbaImage, cx: f64, cy: f64, accent: [u8; 3]) {
    let (w, h) = img.dimensions();
    let reach = RING_RADIUS + RING_THICKNESS;
    let x0 = (cx - reach).floor().max(0.0) as u32;
    let y0 = (cy - reach).floor().max(0.0) as u32;
    let x1 = ((cx + reach).ceil().max(0.0) as u32).min(w);
    let y1 = ((cy + reach).ceil().max(0.0) as u32).min(h);
    for y in y0..y1 {
        for x in x0..x1 {
            let dist = ((f64::from(x) - cx).powi(2) + (f64::from(y) - cy).powi(2)).sqrt();
            let edge = (dist - RING_RADIUS).abs();
            if edge <= RING_THICKNESS {
                let coverage = (1.0 - edge / RING_THICKNESS).clamp(0.0, 1.0);
                blend_pixel(img, x, y, accent, coverage as f32);
            }
        }
    }
}

/// Draw a caption bar: scrim, accent keyline, bold text.
pub fn draw_caption(
    img: &mut RgbaImage,
    font: &FontRef<'static>,
    text: &str,
    accent: [u8; 3],
    position: CaptionPosition,
) {
    let (w, h) = img.dimensions();
    let bar_h = match position {
        CaptionPosition::Top => ((h as f32) * 0.10) as u32,
        CaptionPosition::Bottom => ((h as f32) * 0.14) as u32,
    };
    let margin = ((h as f32) * 0.06) as u32;
    let bar_top = match position {
        CaptionPosition::Top => 0,
        CaptionPosition::Bottom => h.saturating_sub(bar_h + margin),
    };
    let bar_bottom = (bar_top + bar_h).min(h);

    for y in bar_top..bar_bottom {
        let fy = (y - bar_top) as f32 / bar_h.max(1) as f32;
        let alpha = (0.80 * (1.0 - (fy - 0.5).abs() * 0.5)).clamp(0.0, 0.85);
        for x in 0..w {
            blend_pixel(img, x, y, [12, 14, 20], alpha);
        }
    }

    let key_x = ((w as f32) * 0.08) as u32;
    for y in bar_top..bar_bottom {
        for x in key_x..(key_x + 5).min(w) {
            blend_pixel(img, x, y, accent, 1.0);
        }
    }

    let px = ((bar_h as f32) * 0.42).clamp(18.0, 60.0);
    let text_y = bar_top as f32 + (bar_h as f32 - px) / 2.0;
    draw_text(
        img,
        font,
        text,
        px,
        (key_x + 16) as f32,
        text_y,
        [240, 240, 240],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn identity_zoom_is_a_noop() {
        let img = RgbaImage::from_pixel(20, 16, Rgba([1, 2, 3, 255]));
        let (out, xf) = apply_zoom(
            &img,
            ZoomState {
                cx: 10.0,
                cy: 8.0,
                scale: 1.0,
            },
        );
        assert_eq!(out.dimensions(), (20, 16));
        assert_eq!((xf.sx, xf.sy), (1.0, 1.0));
        assert_eq!(xf.map(5.0, 4.0), (5.0, 4.0));
    }

    #[test]
    fn zoom_preserves_size_and_magnifies() {
        let mut img = RgbaImage::from_pixel(40, 40, Rgba([255, 0, 0, 255]));
        for y in 0..40 {
            for x in 20..40 {
                img.put_pixel(x, y, Rgba([0, 0, 255, 255]));
            }
        }
        let (out, xf) = apply_zoom(
            &img,
            ZoomState {
                cx: 30.0,
                cy: 20.0,
                scale: 2.0,
            },
        );
        assert_eq!(out.dimensions(), (40, 40));
        assert!(xf.sx > 1.0);
        assert!(out.get_pixel(20, 20).0[2] > 200);
    }

    #[test]
    fn cursor_ring_draws_an_accent_circle() {
        let mut img = RgbaImage::from_pixel(80, 80, Rgba([0, 0, 0, 255]));
        draw_cursor_ring(&mut img, 40.0, 40.0, [240, 167, 92]);
        assert_eq!(img.get_pixel(40, 40).0[0], 0);
        let on_ring = img.get_pixel(54, 40).0;
        assert!(
            on_ring[0] > 100 && on_ring[1] > 60,
            "expected accent on the ring"
        );
    }

    #[test]
    fn caption_bar_darkens_lower_third_and_draws_keyline() {
        let f = crate::text::font();
        let mut img = RgbaImage::from_pixel(320, 240, Rgba([255, 255, 255, 255]));
        draw_caption(
            &mut img,
            &f,
            "Open the menu",
            [240, 167, 92],
            CaptionPosition::Bottom,
        );
        assert_eq!(img.get_pixel(10, 10).0, [255, 255, 255, 255]);
        let darkened = (160..230).any(|y| img.get_pixel(160, y).0[0] < 200);
        assert!(darkened, "expected a darkened caption band");
    }

    #[test]
    fn top_caption_does_not_cover_lower_third() {
        let f = crate::text::font();
        let mut img = RgbaImage::from_pixel(320, 240, Rgba([255, 255, 255, 255]));
        draw_caption(
            &mut img,
            &f,
            "Open the menu",
            [240, 167, 92],
            CaptionPosition::Top,
        );
        let top_darkened = (2..22).any(|y| img.get_pixel(160, y).0[0] < 200);
        assert!(top_darkened, "expected a darkened top caption band");
        assert_eq!(img.get_pixel(160, 220).0, [255, 255, 255, 255]);
    }
}
