//! Daisy: eight status-colored petals around a cream heart.

use netflash_core::Srgb8;

use crate::paint::{fill_circle, finish, pixmap};
use crate::RgbaIcon;

const HEART: Srgb8 = Srgb8::new(0xFD, 0xE6, 0x8A);

/// Daisy. Petals follow the painted status color; the heart stays cream.
pub fn render_flower(color: Srgb8, _score: f64, size: u32) -> RgbaIcon {
    let size = size.max(16);
    let mut pixmap = pixmap(size);
    paint(&mut pixmap, color);
    finish(pixmap)
}

fn paint(pixmap: &mut tiny_skia::Pixmap, color: Srgb8) {
    let dim = pixmap.width() as f32;
    let pad = dim * 0.10;
    let radius = (dim / 2.0) - pad;
    let cx = dim * 0.5;
    let cy = dim * 0.5;
    let petal_r = radius * 0.38;
    let reach = radius * 0.58;

    for i in 0..8 {
        let a = (i as f32) * std::f32::consts::PI / 4.0;
        let px = cx + reach * a.cos();
        let py = cy + reach * a.sin();
        fill_circle(pixmap, px, py, petal_r, color, 255);
    }

    fill_circle(pixmap, cx, cy, radius * 0.36, HEART, 255);
    fill_circle(
        pixmap,
        cx - radius * 0.08,
        cy - radius * 0.10,
        radius * 0.16,
        Srgb8::new(0xFF, 0xF7, 0xD1),
        140,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use netflash_core::color_for_score;

    #[test]
    fn corners_stay_empty() {
        let icon = render_flower(color_for_score(0.55), 0.55, 44);
        assert_eq!(icon.rgba[3], 0);
    }

    #[test]
    fn petals_follow_status_hue() {
        let green = color_for_score(0.55);
        let icon = render_flower(green, 0.55, 44);
        // A petal sits near the left-middle, not the cream heart.
        let i = ((22 * 44) + 8) * 4;
        assert!(icon.rgba[i + 3] > 100);
        assert!(
            icon.rgba[i + 1] > icon.rgba[i],
            "green petals must dominate the rim"
        );
    }
}
