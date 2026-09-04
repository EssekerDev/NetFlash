//! Retro 5×5 LED matrix. Every LED uses the painted status color.

use netflash_core::Srgb8;

use crate::paint::{fill_circle, finish, pixmap};
use crate::RgbaIcon;

const ROWS: usize = 5;
const COLS: usize = 5;

/// 5×5 LED panel, one hue — the current displayed status color.
pub fn render_matrix(color: Srgb8, _score: f64, size: u32) -> RgbaIcon {
    let size = size.max(16);
    let mut pixmap = pixmap(size);
    paint_matrix(&mut pixmap, color);
    finish(pixmap)
}

fn paint_matrix(pixmap: &mut tiny_skia::Pixmap, color: Srgb8) {
    let dim = pixmap.width() as f32;
    let pad = dim * 0.09;
    let cell = (dim - 2.0 * pad) / ROWS as f32;
    let radius = cell * 0.36;

    for row in 0..ROWS {
        for col in 0..COLS {
            let cx = pad + (col as f32 + 0.5) * cell;
            let cy = pad + (row as f32 + 0.5) * cell;
            draw_led(pixmap, cx, cy, radius, color);
        }
    }
}

fn draw_led(pixmap: &mut tiny_skia::Pixmap, cx: f32, cy: f32, radius: f32, color: Srgb8) {
    fill_circle(pixmap, cx, cy, radius * 1.55, color, 48);
    let body = Srgb8::new(
        color.r.saturating_add(18),
        color.g.saturating_add(18),
        color.b.saturating_add(18),
    );
    fill_circle(pixmap, cx, cy, radius, body, 255);
    fill_circle(
        pixmap,
        cx - radius * 0.22,
        cy - radius * 0.26,
        radius * 0.38,
        Srgb8::new(255, 255, 255),
        55,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use netflash_core::color_for_score;

    #[test]
    fn whole_matrix_follows_status_hue() {
        let green = color_for_score(0.55);
        let icon = render_matrix(green, 0.0, 44);
        let (r, g, b, n) = rgb_sum(&icon.rgba);
        assert!(n > 0);
        assert!(g > r && g > b, "ok/green status must dominate every LED");
    }

    #[test]
    fn low_score_stays_violet_not_rainbow() {
        let violet = color_for_score(0.0);
        let icon = render_matrix(violet, 0.0, 44);
        let (r, g, b, _) = rgb_sum(&icon.rgba);
        assert!(
            b > g && r > g,
            "none must read as violet, not a 5-color stack"
        );
    }

    #[test]
    fn corners_stay_empty() {
        let icon = render_matrix(color_for_score(1.0), 1.0, 44);
        assert_eq!(icon.rgba[3], 0);
        let n = icon.rgba.len();
        assert_eq!(icon.rgba[n - 1], 0);
    }

    fn rgb_sum(rgba: &[u8]) -> (u32, u32, u32, u32) {
        let mut r = 0;
        let mut g = 0;
        let mut b = 0;
        let mut n = 0;
        for px in rgba.chunks_exact(4) {
            if px[3] < 180 {
                continue;
            }
            r += u32::from(px[0]);
            g += u32::from(px[1]);
            b += u32::from(px[2]);
            n += 1;
        }
        (r, g, b, n)
    }
}
