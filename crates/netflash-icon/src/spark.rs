//! Four-point spark in the painted status color.

use netflash_core::Srgb8;
use tiny_skia::{Path, PathBuilder};

use crate::paint::{fill, fill_circle, finish, pixmap};
use crate::RgbaIcon;

/// Compact 4-point star. Hue only — score is unused.
pub fn render_spark(color: Srgb8, _score: f64, size: u32) -> RgbaIcon {
    let size = size.max(16);
    let mut pixmap = pixmap(size);
    paint(&mut pixmap, color);
    finish(pixmap)
}

fn paint(pixmap: &mut tiny_skia::Pixmap, color: Srgb8) {
    let dim = pixmap.width() as f32;
    let cx = dim * 0.5;
    let cy = dim * 0.5;
    let outer = dim * 0.42;
    let inner = dim * 0.14;
    if let Some(path) = star_4(cx, cy, outer, inner) {
        fill(pixmap, &path, color, 255);
    }
    let hi = Srgb8::new(
        color.r.saturating_add(40).min(255),
        color.g.saturating_add(40).min(255),
        color.b.saturating_add(40).min(255),
    );
    fill_circle(
        pixmap,
        cx - outer * 0.08,
        cy - outer * 0.10,
        inner * 0.85,
        hi,
        90,
    );
}

fn star_4(cx: f32, cy: f32, outer: f32, inner: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    for i in 0..8 {
        let r = if i % 2 == 0 { outer } else { inner };
        let a = (i as f32) * std::f32::consts::PI / 4.0 - std::f32::consts::FRAC_PI_2;
        let x = cx + r * a.cos();
        let y = cy + r * a.sin();
        if i == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    pb.close();
    pb.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use netflash_core::color_for_score;

    #[test]
    fn corners_stay_empty() {
        let icon = render_spark(color_for_score(0.55), 0.55, 44);
        assert_eq!(icon.rgba[3], 0);
    }

    #[test]
    fn center_follows_status_hue() {
        let green = color_for_score(0.55);
        let icon = render_spark(green, 0.0, 44);
        let i = ((22 * 44) + 22) * 4;
        assert!(icon.rgba[i + 3] > 180);
        assert!(icon.rgba[i + 1] > icon.rgba[i]);
    }
}
