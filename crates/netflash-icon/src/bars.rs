//! Four rounded bars. Lit count follows painted score; hue is status.

use netflash_core::Srgb8;

use crate::paint::{fill, finish, pixmap, rounded_rect};
use crate::RgbaIcon;

/// Cellular-style bars, one color. One bar on at none, four at ultra.
pub fn render_bars(color: Srgb8, score: f64, size: u32) -> RgbaIcon {
    let size = size.max(16);
    let mut pixmap = pixmap(size);
    paint(&mut pixmap, color, score.clamp(0.0, 1.0));
    finish(pixmap)
}

fn paint(pixmap: &mut tiny_skia::Pixmap, color: Srgb8, score: f64) {
    let dim = pixmap.width() as f32;
    let pad = dim * 0.14;
    let inner = dim - 2.0 * pad;
    let gap = inner * 0.10;
    let w = (inner - 3.0 * gap) / 4.0;
    let lit = 1 + (score * 3.0).round() as usize;
    let lit = lit.clamp(1, 4);
    let max_h = inner;
    let radii = w * 0.35;

    for i in 0..4 {
        let h = max_h * (0.40 + 0.20 * i as f32);
        let x = pad + i as f32 * (w + gap);
        let y = pad + (max_h - h);
        let on = i < lit;
        let alpha = if on { 255 } else { 48 };
        if let Some(path) = rounded_rect(x, y, w, h, radii) {
            fill(pixmap, &path, color, alpha);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use netflash_core::color_for_score;

    #[test]
    fn corners_stay_empty() {
        let icon = render_bars(color_for_score(0.55), 1.0, 44);
        assert_eq!(icon.rgba[3], 0);
    }

    #[test]
    fn ultra_lights_more_than_none() {
        let c = color_for_score(0.55);
        let low = opaque_count(&render_bars(c, 0.0, 44).rgba);
        let high = opaque_count(&render_bars(c, 1.0, 44).rgba);
        assert!(high > low);
    }

    fn opaque_count(rgba: &[u8]) -> usize {
        rgba.chunks_exact(4).filter(|p| p[3] > 160).count()
    }
}
