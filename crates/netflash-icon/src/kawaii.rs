//! Kawaii status tile: a rounded square in the painted color plus an ASCII face.
//!
//! Mood follows the named band so the mouth stays readable at 16–22 pt:
//! smile (ok/ultra), `–_–` (medium), frown (bad), `x_x` (none).

use netflash_core::{Band, Srgb8};
use tiny_skia::{LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

use crate::paint::{fill, fill_circle, finish, pixmap, rounded_rect};
use crate::RgbaIcon;

/// Off-white “print” so eyes/mouth stay readable on every status hue.
const FACE: Srgb8 = Srgb8::new(0xFF, 0xF8, 0xF0);

/// Colored rounded square + face from `score`’s named band.
pub fn render_kawaii(color: Srgb8, score: f64, size: u32) -> RgbaIcon {
    let size = size.max(16);
    let mut pixmap = pixmap(size);
    paint(&mut pixmap, color, Band::from_score(score));
    finish(pixmap)
}

fn paint(pixmap: &mut Pixmap, color: Srgb8, mood: Band) {
    let dim = pixmap.width() as f32;
    let pad = dim * 0.11;
    let side = dim - 2.0 * pad;
    let cx = dim * 0.5;
    let cy = dim * 0.5;
    let radius = side * 0.22;

    if let Some(body) = rounded_rect(cx - side * 0.5, cy - side * 0.5, side, side, radius) {
        fill(pixmap, &body, color, 255);
    }

    // Candy highlight — keeps the tile from looking like a flat OS badge.
    let hi_side = side * 0.62;
    let hi_r = radius * 0.7;
    if let Some(hi) = rounded_rect(
        cx - hi_side * 0.5,
        cy - side * 0.42,
        hi_side,
        side * 0.38,
        hi_r,
    ) {
        fill(
            pixmap,
            &hi,
            Srgb8::new(
                color.r.saturating_add(36).min(255),
                color.g.saturating_add(36).min(255),
                color.b.saturating_add(36).min(255),
            ),
            55,
        );
    }

    let stroke_w = (dim * 0.068).max(2.2);
    let eye_y = cy - side * 0.06;
    let eye_dx = side * 0.18;
    let eye_r = side * 0.085;
    let mouth_y = cy + side * 0.18;
    let mouth_w = side * 0.20;

    match mood {
        Band::None => {
            draw_x(pixmap, cx - eye_dx, eye_y, eye_r * 1.15, stroke_w);
            draw_x(pixmap, cx + eye_dx, eye_y, eye_r * 1.15, stroke_w);
            draw_line(
                pixmap,
                cx - mouth_w * 0.55,
                mouth_y,
                cx + mouth_w * 0.55,
                mouth_y,
                stroke_w * 0.9,
            );
        }
        Band::Bad => {
            draw_dot(pixmap, cx - eye_dx, eye_y, eye_r);
            draw_dot(pixmap, cx + eye_dx, eye_y, eye_r);
            draw_mouth_arc(pixmap, cx, mouth_y, mouth_w, stroke_w, false);
        }
        Band::Medium => {
            draw_line(
                pixmap,
                cx - eye_dx - eye_r,
                eye_y,
                cx - eye_dx + eye_r,
                eye_y,
                stroke_w,
            );
            draw_line(
                pixmap,
                cx + eye_dx - eye_r,
                eye_y,
                cx + eye_dx + eye_r,
                eye_y,
                stroke_w,
            );
            draw_line(
                pixmap,
                cx - mouth_w * 0.7,
                mouth_y,
                cx + mouth_w * 0.7,
                mouth_y,
                stroke_w,
            );
        }
        Band::Ok | Band::Ultra => {
            draw_dot(pixmap, cx - eye_dx, eye_y, eye_r);
            draw_dot(pixmap, cx + eye_dx, eye_y, eye_r);
            draw_mouth_arc(pixmap, cx, mouth_y, mouth_w, stroke_w, true);
            if mood == Band::Ultra {
                draw_blush(
                    pixmap,
                    cx - eye_dx * 1.35,
                    mouth_y - side * 0.04,
                    eye_r * 0.7,
                );
                draw_blush(
                    pixmap,
                    cx + eye_dx * 1.35,
                    mouth_y - side * 0.04,
                    eye_r * 0.7,
                );
            }
        }
    }
}

fn draw_dot(pixmap: &mut Pixmap, cx: f32, cy: f32, r: f32) {
    fill_circle(pixmap, cx, cy, r, FACE, 255);
}

fn draw_blush(pixmap: &mut Pixmap, cx: f32, cy: f32, r: f32) {
    fill_circle(pixmap, cx, cy, r, Srgb8::new(0xFF, 0x9E, 0xC8), 150);
}

fn draw_x(pixmap: &mut Pixmap, cx: f32, cy: f32, arm: f32, width: f32) {
    draw_line(pixmap, cx - arm, cy - arm, cx + arm, cy + arm, width);
    draw_line(pixmap, cx + arm, cy - arm, cx - arm, cy + arm, width);
}

fn draw_line(pixmap: &mut Pixmap, x0: f32, y0: f32, x1: f32, y1: f32, width: f32) {
    let mut pb = PathBuilder::new();
    pb.move_to(x0, y0);
    pb.line_to(x1, y1);
    if let Some(path) = pb.finish() {
        stroke(pixmap, &path, width);
    }
}

fn draw_mouth_arc(pixmap: &mut Pixmap, cx: f32, cy: f32, half_w: f32, width: f32, smile: bool) {
    let mut pb = PathBuilder::new();
    let lift = half_w * 0.85;
    let ctrl_y = if smile { cy + lift } else { cy - lift };
    pb.move_to(cx - half_w, cy);
    pb.quad_to(cx, ctrl_y, cx + half_w, cy);
    if let Some(path) = pb.finish() {
        stroke(pixmap, &path, width);
    }
}

fn stroke(pixmap: &mut Pixmap, path: &Path, width: f32) {
    let mut paint = Paint::default();
    paint.set_color_rgba8(FACE.r, FACE.g, FACE.b, 255);
    paint.anti_alias = true;
    let mut stroke = Stroke::default();
    stroke.width = width;
    stroke.line_cap = LineCap::Round;
    stroke.line_join = LineJoin::Round;
    pixmap.stroke_path(path, &paint, &stroke, Transform::identity(), None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use netflash_core::color_for_score;

    #[test]
    fn corners_stay_empty() {
        let icon = render_kawaii(color_for_score(0.55), 0.55, 44);
        assert_eq!(icon.rgba[3], 0);
        let n = icon.rgba.len();
        assert_eq!(icon.rgba[n - 1], 0);
    }

    #[test]
    fn none_differs_from_smile() {
        let c = color_for_score(0.55);
        let dead = render_kawaii(color_for_score(0.0), 0.0, 44);
        let smile = render_kawaii(c, 0.55, 44);
        assert_ne!(dead.rgba, smile.rgba);
    }

    #[test]
    fn body_keeps_status_hue() {
        let green = color_for_score(0.55);
        let icon = render_kawaii(green, 0.55, 44);
        // A few pixels in from the left edge of the square, below the highlight.
        let i = ((22 * 44) + 8) * 4;
        assert!(icon.rgba[i + 3] > 180);
        assert!(
            icon.rgba[i + 1] > icon.rgba[i],
            "green body must still read"
        );
    }
}
