//! Donut ring: status hue, stroke thickness follows painted score.

use netflash_core::Srgb8;
use tiny_skia::{LineCap, LineJoin, Paint, PathBuilder, Stroke, Transform};

use crate::paint::{finish, pixmap};
use crate::RgbaIcon;

/// Open ring so it does not read as the default filled dot.
pub fn render_ring(color: Srgb8, score: f64, size: u32) -> RgbaIcon {
    let size = size.max(16);
    let mut pixmap = pixmap(size);
    paint(&mut pixmap, color, score.clamp(0.0, 1.0) as f32);
    finish(pixmap)
}

fn paint(pixmap: &mut tiny_skia::Pixmap, color: Srgb8, score: f32) {
    let dim = pixmap.width() as f32;
    let cx = dim * 0.5;
    let cy = dim * 0.5;
    let radius = dim * 0.34;
    // Thin when down, chunky when ultra — center stays empty.
    let width = (dim * (0.06 + 0.16 * score)).max(2.2);
    let Some(path) = PathBuilder::from_circle(cx, cy, radius) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color_rgba8(color.r, color.g, color.b, 255);
    paint.anti_alias = true;
    let mut stroke = Stroke::default();
    stroke.width = width;
    stroke.line_cap = LineCap::Round;
    stroke.line_join = LineJoin::Round;
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use netflash_core::color_for_score;

    #[test]
    fn corners_and_center_stay_empty() {
        let icon = render_ring(color_for_score(0.55), 1.0, 44);
        assert_eq!(icon.rgba[3], 0);
        let i = ((22 * 44) + 22) * 4;
        assert!(icon.rgba[i + 3] < 40, "hole must stay open");
    }

    #[test]
    fn ultra_is_heavier_than_none() {
        let c = color_for_score(0.55);
        let thin = opaque_count(&render_ring(c, 0.0, 44).rgba);
        let thick = opaque_count(&render_ring(c, 1.0, 44).rgba);
        assert!(thick > thin, "higher score must paint a thicker ring");
    }

    fn opaque_count(rgba: &[u8]) -> usize {
        rgba.chunks_exact(4).filter(|p| p[3] > 80).count()
    }
}
