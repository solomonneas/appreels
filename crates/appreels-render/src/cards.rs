//! Full-canvas title and outro cards.

use image::RgbaImage;
use polish_core::PresentationStyle;

use crate::text::{draw_text_centered, font};

/// Render a full-canvas card: gradient backdrop plus centered bold title text.
pub fn render_card(width: u32, height: u32, text: &str, style: &PresentationStyle) -> RgbaImage {
    let mut canvas = polish_core::gradient_backdrop(width, height, style);
    let f = font();
    let px = (width as f32 * 0.06).clamp(24.0, 96.0);
    let y = height as f32 / 2.0 - px / 2.0;
    draw_text_centered(&mut canvas, &f, text, px, y, [245, 245, 245]);
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_is_canvas_sized_and_has_content() {
        let style = polish_core::style_from_seed(42);
        let card = render_card(400, 300, "Create a project", &style);
        assert_eq!(card.dimensions(), (400, 300));
        assert_eq!(card.get_pixel(0, 0).0[3], 255);
        let mid_y = 150;
        let edge = card.get_pixel(2, mid_y).0;
        let has_text = (0..400).any(|x| card.get_pixel(x, mid_y).0 != edge);
        assert!(has_text, "expected text pixels in the center band");
    }
}
