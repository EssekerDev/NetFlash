//! Shared tiny-skia helpers for every tray skin.

use netflash_core::Srgb8;
use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Transform};

use crate::RgbaIcon;

/// Opaque square pixmap. `size` is already clamped by the caller.
pub(crate) fn pixmap(size: u32) -> Pixmap {
    Pixmap::new(size, size).expect("non-zero pixmap")
}

/// Unpremultiply and wrap a finished pixmap.
pub(crate) fn finish(pixmap: Pixmap) -> RgbaIcon {
    RgbaIcon {
        width: pixmap.width(),
        height: pixmap.height(),
        rgba: unpremultiply(pixmap.data()),
    }
}

pub(crate) fn fill(pixmap: &mut Pixmap, path: &Path, color: Srgb8, alpha: u8) {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color.r, color.g, color.b, alpha);
    paint.anti_alias = true;
    pixmap.fill_path(path, &paint, FillRule::Winding, Transform::identity(), None);
}

pub(crate) fn fill_circle(pixmap: &mut Pixmap, cx: f32, cy: f32, r: f32, color: Srgb8, alpha: u8) {
    if let Some(path) = PathBuilder::from_circle(cx, cy, r) {
        fill(pixmap, &path, color, alpha);
    }
}

pub(crate) fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w * 0.5).min(h * 0.5).max(0.0);
    let k = 0.552_284_75 * r;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

pub(crate) fn unpremultiply(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for px in data.chunks_exact(4) {
        let a = px[3];
        if a == 0 || a == 255 {
            out.extend_from_slice(px);
        } else {
            let scale = 255.0 / f32::from(a);
            out.push((f32::from(px[0]) * scale).round().min(255.0) as u8);
            out.push((f32::from(px[1]) * scale).round().min(255.0) as u8);
            out.push((f32::from(px[2]) * scale).round().min(255.0) as u8);
            out.push(a);
        }
    }
    out
}
