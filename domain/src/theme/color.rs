//! Colours: parsing the forms themes write them in, mixing, contrast and hue.

use std::fmt;

/// An sRGB colour, 8 bits per channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const BLACK: Self = Self::from_hex(0x000000);
    pub const WHITE: Self = Self::from_hex(0xffffff);

    pub const fn from_hex(hex: u32) -> Self {
        Self {
            r: (hex >> 16) as u8,
            g: (hex >> 8) as u8,
            b: hex as u8,
        }
    }

    /// Accepts `#rgb`, `#rrggbb`, `#rrggbbaa`, and Hyprland's `rgb(rrggbb)`, `rgba(rrggbbaa)`
    /// and `rgb(r, g, b)`. Alpha is dropped.
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if let Some(hex) = value.strip_prefix('#') {
            return Self::parse_hex(hex);
        }
        let lower = value.to_ascii_lowercase();
        let inner = lower
            .strip_prefix("rgba(")
            .or_else(|| lower.strip_prefix("rgb("))?
            .strip_suffix(')')?;
        if !inner.contains(',') {
            return Self::parse_hex(inner.trim());
        }
        let channels: Vec<&str> = inner.split(',').map(str::trim).collect();
        match channels.as_slice() {
            [r, g, b] | [r, g, b, _] => Some(Self {
                r: r.parse().ok()?,
                g: g.parse().ok()?,
                b: b.parse().ok()?,
            }),
            _ => None,
        }
    }

    fn parse_hex(hex: &str) -> Option<Self> {
        if !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        match hex.len() {
            3 => {
                let value = u32::from_str_radix(hex, 16).ok()?;
                let nibble = |shift: u32| ((value >> shift) & 0xf) as u8 * 17;
                Some(Self {
                    r: nibble(8),
                    g: nibble(4),
                    b: nibble(0),
                })
            }
            6 | 8 => Some(Self::from_hex(u32::from_str_radix(&hex[..6], 16).ok()?)),
            _ => None,
        }
    }

    /// Blends toward `other` by `percent`, rounding exactly like `omarchy-theme-color`'s `mix_color`.
    #[must_use]
    pub fn mix(self, other: Self, percent: u8) -> Self {
        let amount = f64::from(percent.min(100)) / 100.0;
        let channel = |start: u8, end: u8| {
            (f64::from(start) * (1.0 - amount) + f64::from(end) * amount + 0.5) as u8
        };
        Self {
            r: channel(self.r, other.r),
            g: channel(self.g, other.g),
            b: channel(self.b, other.b),
        }
    }

    /// WCAG 2 relative luminance.
    pub fn relative_luminance(self) -> f64 {
        let linear = |channel: u8| {
            let value = f64::from(channel) / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }

    /// WCAG 2 contrast ratio, from 1 to 21.
    pub fn contrast(self, other: Self) -> f64 {
        let (a, b) = (self.relative_luminance(), other.relative_luminance());
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// Hue in degrees, then saturation and lightness from 0 to 1.
    pub fn to_hsl(self) -> (f64, f64, f64) {
        let [r, g, b] = [self.r, self.g, self.b].map(|channel| f64::from(channel) / 255.0);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let lightness = (max + min) / 2.0;
        let delta = max - min;
        if delta == 0.0 {
            return (0.0, 0.0, lightness);
        }
        let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs());
        let hue = if max == r {
            60.0 * ((g - b) / delta).rem_euclid(6.0)
        } else if max == g {
            60.0 * ((b - r) / delta + 2.0)
        } else {
            60.0 * ((r - g) / delta + 4.0)
        };
        (hue, saturation, lightness)
    }

    pub fn from_hsl(hue: f64, saturation: f64, lightness: f64) -> Self {
        let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
        let sector = hue.rem_euclid(360.0) / 60.0;
        let x = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
        let (r, g, b) = match sector as u8 {
            0 => (chroma, x, 0.0),
            1 => (x, chroma, 0.0),
            2 => (0.0, chroma, x),
            3 => (0.0, x, chroma),
            4 => (x, 0.0, chroma),
            _ => (chroma, 0.0, x),
        };
        let m = lightness - chroma / 2.0;
        let channel = |value: f64| ((value + m) * 255.0).round().clamp(0.0, 255.0) as u8;
        Self {
            r: channel(r),
            g: channel(g),
            b: channel(b),
        }
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

/// `colors` as the theme has them when they can be told apart; otherwise each turned to its
/// entry in `hues`, keeping roughly its lightness. Some themes give every colour of their
/// palette one hue (Aether's single-hue themes) or none at all, and compare's added, removed
/// and changed lines, or the five token styles, would look the same. Apart means the hues
/// spread over at least a quarter of the colour wheel and none is nearly grey.
pub fn distinct<const N: usize>(colors: [Rgb; N], hues: [f64; N]) -> [Rgb; N] {
    let hsl = colors.map(Rgb::to_hsl);
    let grey = hsl.iter().any(|&(_, saturation, _)| saturation < 0.15);
    let mut angles: Vec<f64> = hsl.iter().map(|&(hue, _, _)| hue).collect();
    angles.sort_by(f64::total_cmp);
    let largest_gap = angles
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .chain(
            angles
                .first()
                .zip(angles.last())
                .map(|(first, last)| first + 360.0 - last),
        )
        .fold(0.0, f64::max);
    if !grey && 360.0 - largest_gap >= 90.0 {
        return colors;
    }
    std::array::from_fn(|index| {
        let (_, saturation, lightness) = hsl[index];
        Rgb::from_hsl(
            hues[index],
            saturation.max(0.45),
            lightness.clamp(0.4, 0.65),
        )
    })
}

/// Whichever of `a` and `b` contrasts more with `background`.
pub fn readable_on(background: Rgb, a: Rgb, b: Rgb) -> Rgb {
    if background.contrast(a) >= background.contrast(b) {
        a
    } else {
        b
    }
}
