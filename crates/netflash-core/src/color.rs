//! Score → sRGB via Oklab mixing so violet→red does not travel through green.
//!
//! Mix lives in Oklab (Cartesian). Naive Oklch hue lerp of violet→red would
//! swing through cyan/green — a false “ok” flash on the worst transition.

/// 8-bit sRGB pixel for the tray rasterizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Srgb8 {
    /// Red 0–255.
    pub r: u8,
    /// Green 0–255.
    pub g: u8,
    /// Blue 0–255.
    pub b: u8,
}

impl Srgb8 {
    /// Construct from 8-bit channels.
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// `#RRGGBB`.
    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

/// Named-band colors, violet → blue. Rendering interpolates
/// by score in Oklab; skins consume the mixed color, not these stops directly.
pub const BAND_STOP_COLORS: [Srgb8; 5] = [
    Srgb8::new(0x7C, 0x3A, 0xED),
    Srgb8::new(0xEF, 0x44, 0x44),
    Srgb8::new(0xF9, 0x73, 0x16),
    Srgb8::new(0x22, 0xC5, 0x5E),
    Srgb8::new(0x3B, 0x82, 0xF6),
];

/// Score positions for the five named bands, plus a duplicate ultra stop at 1.0
/// so `color_for_score(1.0)` stays on blue.
const STOP_SCORES: [f64; 6] = [0.00, 0.08, 0.28, 0.55, 0.82, 1.00];

fn stop_color(i: usize) -> Srgb8 {
    BAND_STOP_COLORS[i.min(BAND_STOP_COLORS.len() - 1)]
}

#[derive(Clone, Copy)]
struct Oklab {
    l: f64,
    a: f64,
    b: f64,
}

fn srgb8_to_oklab(r: u8, g: u8, b: u8) -> Oklab {
    linear_srgb_to_oklab(
        srgb_channel_to_linear(r as f64 / 255.0),
        srgb_channel_to_linear(g as f64 / 255.0),
        srgb_channel_to_linear(b as f64 / 255.0),
    )
}

fn oklab_to_srgb8(lab: Oklab) -> Srgb8 {
    let (r, g, b) = oklab_to_linear_srgb(lab);
    Srgb8::new(
        (linear_to_srgb_channel(r) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8,
        (linear_to_srgb_channel(g) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8,
        (linear_to_srgb_channel(b) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8,
    )
}

fn srgb_channel_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb_channel(c: f64) -> f64 {
    if c <= 0.0031308 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Björn Ottosson’s Oklab (https://bottosson.github.io/posts/oklab/).
fn linear_srgb_to_oklab(r: f64, g: f64, b: f64) -> Oklab {
    let l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
    let m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
    let s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;
    let l_ = l.cbrt();
    let m_ = m.cbrt();
    let s_ = s.cbrt();
    Oklab {
        l: 0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
        a: 1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
        b: 0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
    }
}

fn oklab_to_linear_srgb(lab: Oklab) -> (f64, f64, f64) {
    let l_ = lab.l + 0.3963377774 * lab.a + 0.2158037573 * lab.b;
    let m_ = lab.l - 0.1055613458 * lab.a - 0.0638541728 * lab.b;
    let s_ = lab.l - 0.0894841775 * lab.a - 1.2914855480 * lab.b;
    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;
    (
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    )
}

fn mix_oklab(x: Oklab, y: Oklab, t: f64) -> Oklab {
    let t = t.clamp(0.0, 1.0);
    Oklab {
        l: x.l + (y.l - x.l) * t,
        a: x.a + (y.a - x.a) * t,
        b: x.b + (y.b - x.b) * t,
    }
}

/// Interpolate the five-stop gradient in Oklab. `score` is clamped to `[0, 1]`.
pub fn color_for_score(score: f64) -> Srgb8 {
    let s = score.clamp(0.0, 1.0);
    if s <= STOP_SCORES[0] {
        return stop_color(0);
    }
    for i in 0..STOP_SCORES.len() - 1 {
        let t0 = STOP_SCORES[i];
        let t1 = STOP_SCORES[i + 1];
        if s <= t1 {
            let c0 = stop_color(i);
            let c1 = stop_color(i + 1);
            let span = t1 - t0;
            let t = if span <= f64::EPSILON {
                1.0
            } else {
                (s - t0) / span
            };
            return oklab_to_srgb8(mix_oklab(
                srgb8_to_oklab(c0.r, c0.g, c0.b),
                srgb8_to_oklab(c1.r, c1.g, c1.b),
                t,
            ));
        }
    }
    stop_color(BAND_STOP_COLORS.len() - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stops_are_exact() {
        let violet = color_for_score(0.0);
        assert_eq!((violet.r, violet.g, violet.b), (0x7C, 0x3A, 0xED));
        let blue = color_for_score(1.0);
        assert_eq!((blue.r, blue.g, blue.b), (0x3B, 0x82, 0xF6));
        let red = color_for_score(0.08);
        assert_eq!((red.r, red.g, red.b), (0xEF, 0x44, 0x44));
    }

    #[test]
    fn violet_to_red_does_not_pass_through_green() {
        let mid = color_for_score(0.04);
        assert!(
            mid.r > mid.g,
            "violet→red mix must not go through green, got {mid:?}"
        );
    }

    #[test]
    fn oklab_roundtrip_primary() {
        let c = Srgb8::new(0xEF, 0x44, 0x44);
        let back = oklab_to_srgb8(srgb8_to_oklab(c.r, c.g, c.b));
        assert!((back.r as i16 - c.r as i16).abs() <= 1);
        assert!((back.g as i16 - c.g as i16).abs() <= 1);
        assert!((back.b as i16 - c.b as i16).abs() <= 1);
    }
}
