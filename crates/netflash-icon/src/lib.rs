//! Rasterize tray icons from a painted color and displayed score.
//!
//! Skins consume pixels only. The engine never knows about rasters.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod bars;
mod flower;
mod kawaii;
mod matrix;
mod paint;
mod ring;
mod spark;

use netflash_core::Srgb8;

use crate::paint::{fill_circle, finish, pixmap};

/// Default menu-bar raster size (2× a 22 pt slot).
pub const DEFAULT_SIZE: u32 = 44;

/// RGBA8 pixmap (unpremultiplied for opaque fills).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaIcon {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes, row-major RGBA.
    pub rgba: Vec<u8>,
}

/// Tray appearance. Scoring never branches on this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Skin {
    /// Default filled circle.
    Dot,
    /// Retro 5×5 LED matrix in the painted status color.
    Matrix,
    /// Rounded square in the painted color with an ASCII face (kawaii).
    Kawaii,
    /// Daisy: status-colored petals, cream heart.
    Flower,
    /// Four-point spark in the painted color.
    Spark,
    /// Donut whose stroke thickness follows the painted score.
    Ring,
    /// Four bars; how many are lit follows the painted score.
    Bars,
}

impl Skin {
    /// All skins in menu order.
    pub const ALL: [Skin; 7] = [
        Skin::Dot,
        Skin::Matrix,
        Skin::Kawaii,
        Skin::Flower,
        Skin::Spark,
        Skin::Ring,
        Skin::Bars,
    ];

    /// Config / TOML key (`snake_case`).
    pub const fn key(self) -> &'static str {
        match self {
            Skin::Dot => "dot",
            Skin::Matrix => "matrix",
            Skin::Kawaii => "kawaii",
            Skin::Flower => "flower",
            Skin::Spark => "spark",
            Skin::Ring => "ring",
            Skin::Bars => "bars",
        }
    }

    /// Inverse of [`Skin::key`]. Unknown keys yield `None`.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|s| s.key() == key)
    }

    /// Appearance-menu label (English).
    pub const fn menu_label(self) -> &'static str {
        match self {
            Skin::Dot => "Dot",
            Skin::Matrix => "Matrix",
            Skin::Kawaii => "Kawaii",
            Skin::Flower => "Flower",
            Skin::Spark => "Spark",
            Skin::Ring => "Ring",
            Skin::Bars => "Bars",
        }
    }
}

/// Paints a tray raster from the displayed color/score only. No network.
pub trait IconRenderer {
    /// `score` is the painted value in `[0, 1]`.
    ///
    /// Dot / Matrix / Flower / Spark key off `color`. Kawaii maps score to a
    /// face mood. Ring and Bars also vary stroke / fill count with score.
    fn render(&self, color: Srgb8, score: f64, size: u32) -> RgbaIcon;
}

impl IconRenderer for Skin {
    fn render(&self, color: Srgb8, score: f64, size: u32) -> RgbaIcon {
        match self {
            Skin::Dot => render_dot(color, size),
            Skin::Matrix => matrix::render_matrix(color, score, size),
            Skin::Kawaii => kawaii::render_kawaii(color, score, size),
            Skin::Flower => flower::render_flower(color, score, size),
            Skin::Spark => spark::render_spark(color, score, size),
            Skin::Ring => ring::render_ring(color, score, size),
            Skin::Bars => bars::render_bars(color, score, size),
        }
    }
}

/// Draw a filled circle with optical padding (~14%) so it does not kiss the icon edges.
pub fn render_dot(color: Srgb8, size: u32) -> RgbaIcon {
    let size = size.max(8);
    let mut pm = pixmap(size);
    let dim = size as f32;
    let pad = dim * 0.14;
    let radius = (dim / 2.0) - pad;
    let cx = dim / 2.0;
    let cy = dim / 2.0;
    fill_circle(&mut pm, cx, cy, radius, color, 255);

    // Soft inner highlight — tiny, so the 16–22 px tray still reads as a hue.
    let hi = Srgb8::new(
        color.r.saturating_add(28).min(255),
        color.g.saturating_add(28).min(255),
        color.b.saturating_add(28).min(255),
    );
    fill_circle(
        &mut pm,
        cx - radius * 0.18,
        cy - radius * 0.22,
        radius * 0.42,
        hi,
        70,
    );
    finish(pm)
}

/// Dim grey used while probing is paused so we never imply live WAN truth.
pub fn paused_color() -> Srgb8 {
    Srgb8::new(0x86, 0x86, 0x8F)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_has_expected_buffer() {
        let icon = render_dot(Srgb8::new(0x22, 0xC5, 0x5E), 44);
        assert_eq!(icon.width, 44);
        assert_eq!(icon.height, 44);
        assert_eq!(icon.rgba.len(), 44 * 44 * 4);
        // Center pixel should be the fill (opaque green-ish).
        let i = ((22 * 44) + 22) * 4;
        assert!(icon.rgba[i + 3] > 200, "center must be opaque");
        assert!(icon.rgba[i + 1] > icon.rgba[i], "green channel dominates");
    }

    #[test]
    fn corners_are_transparent() {
        let icon = render_dot(Srgb8::new(0xEF, 0x44, 0x44), 44);
        assert_eq!(icon.rgba[3], 0, "top-left must stay empty padding");
    }

    #[test]
    fn skin_dispatch_matches_helpers() {
        let c = Srgb8::new(0x22, 0xC5, 0x5E);
        assert_eq!(Skin::Dot.render(c, 0.7, 44), render_dot(c, 44));
        let matrix = Skin::Matrix.render(c, 0.7, 44);
        assert_eq!(matrix.width, 44);
        assert_eq!(matrix.rgba.len(), 44 * 44 * 4);
        let kawaii = Skin::Kawaii.render(c, 0.7, 44);
        assert_eq!(kawaii.width, 44);
        for skin in Skin::ALL {
            let icon = skin.render(c, 0.7, 44);
            assert_eq!(icon.rgba.len(), 44 * 44 * 4);
            assert_eq!(Skin::from_key(skin.key()), Some(skin));
        }
    }
}
