//! Omarchy palette to editor theme: a GtkSourceView 5 style scheme and libadwaita CSS variables.
//! Pure: callers read `colors.toml` (or `omarchy-theme-color --all`) and write the results.

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};

/// Scheme style for words matching the selection (smart highlighting).
pub const SMART_HIGHLIGHT_STYLE: &str = "stet:smart-highlight";
/// Scheme style for the search match the caret is on.
pub const SEARCH_CURRENT_STYLE: &str = "stet:search-current";
/// Scheme styles for the Find Mark Style and the five token styles (M7): the backgrounds the
/// Mark command and Style All Occurrences of Token give text.
pub const MARK_STYLE: &str = "stet:mark";
pub const TOKEN_STYLES: [&str; 5] = [
    "stet:token-1",
    "stet:token-2",
    "stet:token-3",
    "stet:token-4",
    "stet:token-5",
];
/// Scheme style whose foreground is the bookmark in the gutter (M7).
pub const BOOKMARK_STYLE: &str = "stet:bookmark";
/// Scheme styles for a comparison (M7): the backgrounds of added, removed, changed and moved
/// lines, of the characters that differ inside changed lines, and of the padding that faces
/// lines only the other side has.
pub const COMPARE_ADDED_STYLE: &str = "stet:compare-added";
pub const COMPARE_REMOVED_STYLE: &str = "stet:compare-removed";
pub const COMPARE_CHANGED_STYLE: &str = "stet:compare-changed";
pub const COMPARE_MOVED_STYLE: &str = "stet:compare-moved";
pub const COMPARE_INLINE_STYLE: &str = "stet:compare-changed-text";
pub const COMPARE_PAD_STYLE: &str = "stet:compare-padding";

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

/// The hues compare's kinds fall back to: added, removed, changed, moved.
pub const COMPARE_HUES: [f64; 4] = [130.0, 0.0, 215.0, 290.0];
/// The hues the five token styles fall back to: cyan, orange, yellow, magenta, green.
pub const TOKEN_HUES: [f64; 5] = [185.0, 30.0, 50.0, 300.0, 120.0];

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

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Mode {
    #[default]
    Dark,
    Light,
}

impl Mode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
}

const SHORT_NAMES: [(&str, &str); 8] = [
    ("background", "bg"),
    ("dark_background", "dark_bg"),
    ("darker_background", "darker_bg"),
    ("lighter_background", "lighter_bg"),
    ("foreground", "fg"),
    ("dark_foreground", "dark_fg"),
    ("light_foreground", "light_fg"),
    ("bright_foreground", "bright_fg"),
];

const ANSI_NAMES: [(&str, &str); 16] = [
    ("color0", "background"),
    ("color1", "red"),
    ("color2", "green"),
    ("color3", "yellow"),
    ("color4", "blue"),
    ("color5", "magenta"),
    ("color6", "cyan"),
    ("color7", "foreground"),
    ("color8", "muted"),
    ("color9", "bright_red"),
    ("color10", "bright_green"),
    ("color11", "bright_yellow"),
    ("color12", "bright_blue"),
    ("color13", "bright_magenta"),
    ("color14", "bright_cyan"),
    ("color15", "bright_foreground"),
];

const ACCENTS: [&str; 6] = ["red", "yellow", "green", "cyan", "blue", "magenta"];

/// Applies `omarchy-theme-color --all`'s alias and fallback cascade to a raw `colors.toml` map,
/// so the result matches the script's output minus its empty values.
///
/// Two deliberate differences: shades are derived only from `#rrggbb` values (the script
/// mixes garbage into `#333333`), and a `light.mode` file is the caller's job (insert
/// `mode = "light"` when neither `mode` nor `theme_type` is set).
pub fn resolve_colors(raw: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut colors = Cascade(
        raw.iter()
            .filter(|(_, value)| !value.is_empty())
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    );

    for (name, short) in SHORT_NAMES {
        colors.fill(name, colors.first(&[short]));
    }
    colors.fill("background", colors.first(&["color0"]));
    colors.fill("foreground", colors.first(&["color7"]));
    for (ansi, name) in [("color0", "background"), ("color7", "foreground")] {
        if let Some(value) = colors.first(&[name]) {
            colors.set(ansi, Some(value));
        }
    }
    for (ansi, name) in ANSI_NAMES {
        if !matches!(ansi, "color0" | "color7" | "color8" | "color15") {
            colors.fill(name, colors.first(&[ansi]));
        }
    }
    colors.fill("magenta", colors.first(&["purple"]));
    colors.fill("bright_magenta", colors.first(&["bright_purple"]));

    colors.fill("light_foreground", colors.first(&["color7", "foreground"]));
    colors.fill(
        "bright_foreground",
        colors.first(&["color15", "foreground"]),
    );
    colors.set("cursor", colors.first(&["bright_foreground"]));
    colors.fill(
        "lighter_background",
        colors.first(&["color0", "background"]),
    );
    colors.fill("dark_foreground", colors.first(&["color8", "foreground"]));
    colors.fill("muted", colors.first(&["color8", "dark_foreground"]));
    colors.fill(
        "selection",
        colors.first(&["selection_background", "color8", "color0", "background"]),
    );
    colors.fill("selection_background", colors.first(&["selection"]));
    colors.fill("selection_foreground", colors.first(&["bright_foreground"]));
    colors.fill("orange", colors.first(&["yellow"]));
    colors.fill("brown", colors.mixed("orange", Rgb::BLACK, 50));

    colors.fill(
        "dark_background",
        colors.mixed("background", Rgb::BLACK, 25),
    );
    colors.fill(
        "darker_background",
        colors.mixed("background", Rgb::BLACK, 50),
    );
    for name in ACCENTS {
        let bright = format!("bright_{name}");
        colors.fill(&bright, colors.mixed(name, Rgb::WHITE, 20));
    }
    colors.fill("purple", colors.first(&["magenta"]));
    colors.fill("bright_purple", colors.first(&["bright_magenta"]));

    for (ansi, name) in ANSI_NAMES {
        colors.fill(ansi, colors.first(&[name]));
    }
    for (name, short) in SHORT_NAMES {
        if let Some(value) = colors.first(&[name]) {
            colors.set(short, Some(value));
        }
    }

    colors.fill("mode", colors.first(&["theme_type"]));
    if colors.first(&["mode"]).is_none() {
        let light = colors
            .first(&["background"])
            .filter(|value| value.len() == 7)
            .and_then(|value| Rgb::parse(&value))
            .is_some_and(|bg| u16::from(bg.r) + u16::from(bg.g) + u16::from(bg.b) > 382);
        let mode = if light { Mode::Light } else { Mode::Dark };
        colors.set("mode", Some(mode.as_str().to_owned()));
    }
    colors.set("theme_type", colors.first(&["mode"]));
    colors.0
}

struct Cascade(BTreeMap<String, String>);

impl Cascade {
    fn first(&self, keys: &[&str]) -> Option<String> {
        keys.iter()
            .find_map(|key| self.0.get(*key).filter(|value| !value.is_empty()))
            .cloned()
    }

    fn fill(&mut self, key: &str, value: Option<String>) {
        if self.first(&[key]).is_none() {
            self.set(key, value);
        }
    }

    fn set(&mut self, key: &str, value: Option<String>) {
        match value {
            Some(value) => self.0.insert(key.to_owned(), value),
            None => self.0.remove(key),
        };
    }

    fn mixed(&self, key: &str, toward: Rgb, percent: u8) -> Option<String> {
        let value = self.first(&[key])?;
        let hex = value.strip_prefix('#')?;
        if !matches!(hex.len(), 6 | 8) {
            return None;
        }
        Some(Rgb::parse(&value)?.mix(toward, percent).to_string())
    }
}

/// The semantic Omarchy palette with every fallback applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub mode: Mode,
    pub accent: Rgb,
    pub background: Rgb,
    pub dark_background: Rgb,
    pub darker_background: Rgb,
    pub lighter_background: Rgb,
    pub foreground: Rgb,
    pub dark_foreground: Rgb,
    pub light_foreground: Rgb,
    pub bright_foreground: Rgb,
    pub muted: Rgb,
    pub selection_background: Rgb,
    pub selection_foreground: Rgb,
    pub cursor: Rgb,
    pub red: Rgb,
    pub yellow: Rgb,
    pub orange: Rgb,
    pub green: Rgb,
    pub cyan: Rgb,
    pub blue: Rgb,
    pub magenta: Rgb,
    pub brown: Rgb,
    pub bright_red: Rgb,
    pub bright_yellow: Rgb,
    pub bright_green: Rgb,
    pub bright_cyan: Rgb,
    pub bright_blue: Rgb,
    pub bright_magenta: Rgb,
}

impl Palette {
    /// Built-in GNOME-palette colours for running outside Omarchy, and for keys a theme lacks.
    pub const DARK: Self = Self {
        mode: Mode::Dark,
        accent: Rgb::from_hex(0x3584e4),
        background: Rgb::from_hex(0x1d1d20),
        dark_background: Rgb::from_hex(0x161618),
        darker_background: Rgb::from_hex(0x0f0f10),
        lighter_background: Rgb::from_hex(0x2e2e32),
        foreground: Rgb::from_hex(0xdeddda),
        dark_foreground: Rgb::from_hex(0x9a9996),
        light_foreground: Rgb::from_hex(0xdeddda),
        bright_foreground: Rgb::from_hex(0xffffff),
        muted: Rgb::from_hex(0x77767b),
        selection_background: Rgb::from_hex(0x254165),
        selection_foreground: Rgb::from_hex(0xffffff),
        cursor: Rgb::from_hex(0xffffff),
        red: Rgb::from_hex(0xf66151),
        yellow: Rgb::from_hex(0xf8e45c),
        orange: Rgb::from_hex(0xffa348),
        green: Rgb::from_hex(0x57e389),
        cyan: Rgb::from_hex(0x5bc8af),
        blue: Rgb::from_hex(0x62a0ea),
        magenta: Rgb::from_hex(0xdc8add),
        brown: Rgb::from_hex(0x805224),
        bright_red: Rgb::from_hex(0xf88174),
        bright_yellow: Rgb::from_hex(0xf9e97d),
        bright_green: Rgb::from_hex(0x79e9a1),
        bright_cyan: Rgb::from_hex(0x7cd3bf),
        bright_blue: Rgb::from_hex(0x81b3ee),
        bright_magenta: Rgb::from_hex(0xe3a1e4),
    };

    pub const LIGHT: Self = Self {
        mode: Mode::Light,
        accent: Rgb::from_hex(0x3584e4),
        background: Rgb::from_hex(0xffffff),
        dark_background: Rgb::from_hex(0xf5f5f5),
        darker_background: Rgb::from_hex(0xdedede),
        lighter_background: Rgb::from_hex(0xebebed),
        foreground: Rgb::from_hex(0x241f31),
        dark_foreground: Rgb::from_hex(0x77767b),
        light_foreground: Rgb::from_hex(0x3d3846),
        bright_foreground: Rgb::from_hex(0x000000),
        muted: Rgb::from_hex(0x9a9996),
        selection_background: Rgb::from_hex(0xcde0f8),
        selection_foreground: Rgb::from_hex(0x241f31),
        cursor: Rgb::from_hex(0x000000),
        red: Rgb::from_hex(0xc01c28),
        yellow: Rgb::from_hex(0x9c6e03),
        orange: Rgb::from_hex(0xc64600),
        green: Rgb::from_hex(0x26a269),
        cyan: Rgb::from_hex(0x218787),
        blue: Rgb::from_hex(0x1a5fb4),
        magenta: Rgb::from_hex(0x813d9c),
        brown: Rgb::from_hex(0x632300),
        bright_red: Rgb::from_hex(0xcd4953),
        bright_yellow: Rgb::from_hex(0xb08b35),
        bright_green: Rgb::from_hex(0x51b587),
        bright_cyan: Rgb::from_hex(0x4d9f9f),
        bright_blue: Rgb::from_hex(0x487fc3),
        bright_magenta: Rgb::from_hex(0x9a64b0),
    };

    pub const fn builtin(mode: Mode) -> Self {
        match mode {
            Mode::Dark => Self::DARK,
            Mode::Light => Self::LIGHT,
        }
    }

    /// Builds the palette from a raw `colors.toml` map by applying [`resolve_colors`] first.
    pub fn from_colors(raw: &BTreeMap<String, String>) -> Self {
        Self::from_resolved(&resolve_colors(raw))
    }

    /// Builds the palette from `omarchy-theme-color --all` output or [`resolve_colors`] output.
    /// The cascade is not re-run: like the script, it is not idempotent. Keys still missing or
    /// unparsable come from the built-in palette for the mode; `accent` tries `blue` first.
    pub fn from_resolved(resolved: &BTreeMap<String, String>) -> Self {
        let mode = if resolved
            .get("mode")
            .is_some_and(|mode| mode.trim().eq_ignore_ascii_case("light"))
        {
            Mode::Light
        } else {
            Mode::Dark
        };
        let base = Self::builtin(mode);
        let parsed = |key: &str| resolved.get(key).and_then(|value| Rgb::parse(value));
        let color = |key: &str, fallback: Rgb| parsed(key).unwrap_or(fallback);
        let blue = color("blue", base.blue);
        Self {
            mode,
            accent: parsed("accent")
                .or_else(|| parsed("blue"))
                .unwrap_or(base.accent),
            background: color("background", base.background),
            dark_background: color("dark_background", base.dark_background),
            darker_background: color("darker_background", base.darker_background),
            lighter_background: color("lighter_background", base.lighter_background),
            foreground: color("foreground", base.foreground),
            dark_foreground: color("dark_foreground", base.dark_foreground),
            light_foreground: color("light_foreground", base.light_foreground),
            bright_foreground: color("bright_foreground", base.bright_foreground),
            muted: color("muted", base.muted),
            selection_background: color("selection_background", base.selection_background),
            selection_foreground: color("selection_foreground", base.selection_foreground),
            cursor: color("cursor", base.cursor),
            red: color("red", base.red),
            yellow: color("yellow", base.yellow),
            orange: color("orange", base.orange),
            green: color("green", base.green),
            cyan: color("cyan", base.cyan),
            blue,
            magenta: color("magenta", base.magenta),
            brown: color("brown", base.brown),
            bright_red: color("bright_red", base.bright_red),
            bright_yellow: color("bright_yellow", base.bright_yellow),
            bright_green: color("bright_green", base.bright_green),
            bright_cyan: color("bright_cyan", base.bright_cyan),
            bright_blue: color("bright_blue", base.bright_blue),
            bright_magenta: color("bright_magenta", base.bright_magenta),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Underline {
    Single,
    Low,
    Error,
}

impl Underline {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Low => "low",
            Self::Error => "error",
        }
    }
}

/// One `<style>` of the generated scheme. Unset flags are left out, not written as `false`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemeStyle {
    pub name: &'static str,
    pub foreground: Option<Rgb>,
    pub background: Option<Rgb>,
    pub bold: bool,
    pub italic: bool,
    pub underline: Option<Underline>,
    pub underline_color: Option<Rgb>,
    pub strikethrough: bool,
}

impl SchemeStyle {
    const fn new(name: &'static str) -> Self {
        Self {
            name,
            foreground: None,
            background: None,
            bold: false,
            italic: false,
            underline: None,
            underline_color: None,
            strikethrough: false,
        }
    }

    const fn fg(self, color: Rgb) -> Self {
        Self {
            foreground: Some(color),
            ..self
        }
    }

    const fn bg(self, color: Rgb) -> Self {
        Self {
            background: Some(color),
            ..self
        }
    }

    const fn bold(self) -> Self {
        Self { bold: true, ..self }
    }

    const fn italic(self) -> Self {
        Self {
            italic: true,
            ..self
        }
    }

    const fn underline(self, underline: Underline, color: Option<Rgb>) -> Self {
        Self {
            underline: Some(underline),
            underline_color: color,
            ..self
        }
    }

    const fn strikethrough(self) -> Self {
        Self {
            strikethrough: true,
            ..self
        }
    }
}

/// Editor and chrome colours derived from a palette, following the DESIGN.md role table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorTheme {
    pub palette: Palette,
    /// `lighter_background`, unless it would hide the line or the selection.
    pub current_line: Rgb,
    pub selection_unfocused: Rgb,
    pub search_match: Rgb,
    pub search_current_text: Rgb,
    pub smart_highlight: Rgb,
    pub snippet_focus: Rgb,
    /// The Find Mark Style and the five token styles, from the palette: red; cyan, orange,
    /// yellow, magenta and green (M7).
    pub mark: Rgb,
    pub tokens: [Rgb; 5],
    /// A comparison's added, removed, changed and moved lines, the changed characters, and
    /// the padding (M7).
    pub compare_added: Rgb,
    pub compare_removed: Rgb,
    pub compare_changed: Rgb,
    pub compare_moved: Rgb,
    pub compare_inline: Rgb,
    pub compare_pad: Rgb,
    /// Tab strip, status bar, header bar and sidebar.
    pub headerbar: Rgb,
    /// Popovers, the palette and dialogs.
    pub popover: Rgb,
    /// Cards, hover rows, borders and separators.
    pub card: Rgb,
    pub accent_text: Rgb,
}

impl EditorTheme {
    pub fn new(palette: &Palette) -> Self {
        let p = palette;
        let current_line = if p.lighter_background == p.selection_background
            || p.lighter_background == p.background
        {
            p.background.mix(p.foreground, 8)
        } else {
            p.lighter_background
        };
        let compare = distinct([p.green, p.red, p.blue, p.magenta], COMPARE_HUES);
        Self {
            palette: p.clone(),
            current_line,
            selection_unfocused: p.selection_background.mix(p.background, 40),
            search_match: p.background.mix(p.yellow, 35),
            search_current_text: readable_on(p.yellow, p.background, p.foreground),
            smart_highlight: p.background.mix(p.accent, 30),
            snippet_focus: p.background.mix(p.accent, 20),
            mark: p.background.mix(p.red, 40),
            tokens: distinct([p.cyan, p.orange, p.yellow, p.magenta, p.green], TOKEN_HUES)
                .map(|color| p.background.mix(color, 40)),
            compare_added: p.background.mix(compare[0], 20),
            compare_removed: p.background.mix(compare[1], 20),
            compare_changed: p.background.mix(compare[2], 20),
            compare_moved: p.background.mix(compare[3], 20),
            compare_inline: p.background.mix(compare[2], 45),
            compare_pad: p.background.mix(p.muted, 25),
            headerbar: p.dark_background,
            popover: p.darker_background,
            card: p.lighter_background,
            accent_text: readable_on(p.accent, p.background, p.foreground),
        }
    }

    pub fn is_dark(&self) -> bool {
        self.palette.mode == Mode::Dark
    }

    /// Every style the generated scheme defines, in output order.
    pub fn scheme_styles(&self) -> Vec<SchemeStyle> {
        let p = &self.palette;
        let style = SchemeStyle::new;
        vec![
            style("text").fg(p.foreground).bg(p.background),
            style("selection")
                .fg(p.selection_foreground)
                .bg(p.selection_background),
            style("selection-unfocused")
                .fg(p.selection_foreground)
                .bg(self.selection_unfocused),
            style("cursor").fg(p.cursor),
            style("secondary-cursor").fg(p.muted),
            style("current-line").bg(self.current_line),
            style("line-numbers").fg(p.muted).bg(p.background),
            style("current-line-number")
                .fg(p.foreground)
                .bg(self.current_line),
            style("bracket-match").fg(p.yellow).bold(),
            style("bracket-mismatch")
                .fg(readable_on(p.red, p.background, p.foreground))
                .bg(p.red),
            style("right-margin")
                .fg(p.lighter_background)
                .bg(p.lighter_background),
            style("draw-spaces").fg(p.muted),
            style("background-pattern").bg(p.lighter_background),
            style("search-match").bg(self.search_match),
            style("snippet-focus").bg(self.snippet_focus),
            style("map-overlay").bg(p.muted),
            style(SEARCH_CURRENT_STYLE)
                .fg(self.search_current_text)
                .bg(p.yellow),
            style(SMART_HIGHLIGHT_STYLE).bg(self.smart_highlight),
            style(MARK_STYLE).bg(self.mark),
            style(TOKEN_STYLES[0]).bg(self.tokens[0]),
            style(TOKEN_STYLES[1]).bg(self.tokens[1]),
            style(TOKEN_STYLES[2]).bg(self.tokens[2]),
            style(TOKEN_STYLES[3]).bg(self.tokens[3]),
            style(TOKEN_STYLES[4]).bg(self.tokens[4]),
            style(BOOKMARK_STYLE).fg(p.accent),
            style(COMPARE_ADDED_STYLE).bg(self.compare_added),
            style(COMPARE_REMOVED_STYLE).bg(self.compare_removed),
            style(COMPARE_CHANGED_STYLE).bg(self.compare_changed),
            style(COMPARE_MOVED_STYLE).bg(self.compare_moved),
            style(COMPARE_INLINE_STYLE).bg(self.compare_inline),
            style(COMPARE_PAD_STYLE).bg(self.compare_pad),
            style("def:comment").fg(p.muted).italic(),
            style("def:doc-comment").fg(p.muted).italic(),
            style("def:shebang").fg(p.muted).bold(),
            style("def:doc-comment-element").fg(p.muted).bold().italic(),
            style("def:constant").fg(p.orange),
            style("def:number").fg(p.orange),
            style("def:floating-point").fg(p.orange),
            style("def:decimal").fg(p.orange),
            style("def:base-n-integer").fg(p.orange),
            style("def:boolean").fg(p.orange),
            style("def:special-constant").fg(p.orange),
            style("def:string").fg(p.green),
            style("def:character").fg(p.green),
            style("def:special-char").fg(p.cyan),
            style("def:keyword").fg(p.magenta),
            style("def:statement").fg(p.magenta),
            style("def:operator").fg(p.cyan),
            style("def:function").fg(p.blue),
            style("def:builtin").fg(p.blue),
            style("def:type").fg(p.yellow),
            style("def:preprocessor").fg(p.magenta),
            style("def:error")
                .fg(p.red)
                .underline(Underline::Error, Some(p.red)),
            style("def:warning").underline(Underline::Error, Some(p.yellow)),
            style("def:note")
                .fg(readable_on(p.yellow, p.background, p.foreground))
                .bg(p.yellow)
                .bold(),
            style("def:net-address")
                .fg(p.blue)
                .underline(Underline::Low, None),
            style("def:underlined").underline(Underline::Single, None),
            style("def:emphasis").italic(),
            style("def:strong-emphasis").bold(),
            style("def:inline-code").fg(p.green),
            style("def:preformatted-section").fg(p.green),
            style("def:insertion").fg(p.green),
            style("def:deletion").fg(p.red).strikethrough(),
            style("def:link-text").fg(p.blue),
            style("def:link-symbol").fg(p.muted),
            style("def:link-destination")
                .fg(p.blue)
                .italic()
                .underline(Underline::Low, None),
            style("def:list-marker").fg(p.cyan).bold(),
            style("def:thematic-break").fg(p.muted),
            style("def:heading").fg(p.red).bold(),
            style("def:heading0").fg(p.red).bold(),
            style("def:heading1").fg(p.red).bold(),
            style("def:heading2").fg(p.yellow).bold(),
            style("def:heading3").fg(p.yellow).bold(),
            style("def:heading4").fg(p.green).bold(),
            style("def:heading5").fg(p.blue).bold(),
            style("def:heading6").fg(p.magenta).bold(),
        ]
    }

    /// A GtkSourceView 5 style scheme. `id` must match `[A-Za-z0-9_-]+`; see [`Self::scheme_id`].
    pub fn style_scheme_xml(&self, id: &str, name: &str) -> String {
        let mut xml = String::with_capacity(6 * 1024);
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        let _ = writeln!(
            xml,
            "<style-scheme id=\"{}\" name=\"{}\" version=\"1.0\">",
            escape(id),
            escape(name)
        );
        xml.push_str("  <author>Stet</author>\n");
        xml.push_str("  <description>Generated from the Omarchy palette.</description>\n");
        let _ = writeln!(
            xml,
            "  <metadata>\n    <property name=\"variant\">{}</property>\n  </metadata>",
            self.palette.mode.as_str()
        );
        for style in self.scheme_styles() {
            let _ = write!(xml, "  <style name=\"{}\"", escape(style.name));
            if let Some(color) = style.foreground {
                let _ = write!(xml, " foreground=\"{color}\"");
            }
            if let Some(color) = style.background {
                let _ = write!(xml, " background=\"{color}\"");
            }
            if style.bold {
                xml.push_str(" bold=\"true\"");
            }
            if style.italic {
                xml.push_str(" italic=\"true\"");
            }
            if let Some(underline) = style.underline {
                let _ = write!(xml, " underline=\"{}\"", underline.as_str());
            }
            if let Some(color) = style.underline_color {
                let _ = write!(xml, " underline-color=\"{color}\"");
            }
            if style.strikethrough {
                xml.push_str(" strikethrough=\"true\"");
            }
            xml.push_str("/>\n");
        }
        xml.push_str("</style-scheme>\n");
        xml
    }

    /// The libadwaita colour variables the generated CSS overrides, in output order.
    pub fn css_colors(&self) -> Vec<(&'static str, Rgb)> {
        let p = &self.palette;
        vec![
            ("--window-bg-color", p.background),
            ("--window-fg-color", p.foreground),
            ("--view-bg-color", p.background),
            ("--view-fg-color", p.foreground),
            ("--headerbar-bg-color", self.headerbar),
            ("--headerbar-fg-color", p.foreground),
            ("--headerbar-backdrop-color", self.headerbar),
            ("--headerbar-shade-color", self.card),
            ("--sidebar-bg-color", self.headerbar),
            ("--sidebar-fg-color", p.foreground),
            ("--sidebar-backdrop-color", self.headerbar),
            ("--popover-bg-color", self.popover),
            ("--popover-fg-color", p.foreground),
            ("--dialog-bg-color", self.popover),
            ("--dialog-fg-color", p.foreground),
            ("--card-bg-color", self.card),
            ("--card-fg-color", p.foreground),
            ("--border-color", self.card),
            ("--accent-bg-color", p.accent),
            ("--accent-fg-color", self.accent_text),
            ("--accent-color", p.accent),
            ("--error-bg-color", p.red),
            (
                "--error-fg-color",
                readable_on(p.red, p.background, p.foreground),
            ),
            ("--error-color", p.red),
            ("--warning-bg-color", p.yellow),
            (
                "--warning-fg-color",
                readable_on(p.yellow, p.background, p.foreground),
            ),
            ("--warning-color", p.yellow),
            ("--success-bg-color", p.green),
            (
                "--success-fg-color",
                readable_on(p.green, p.background, p.foreground),
            ),
            ("--success-color", p.green),
        ]
    }

    /// CSS for an application-priority provider: libadwaita colour variables and square windows.
    pub fn adwaita_css(&self) -> String {
        let mut css = String::from(":root {\n");
        for (name, color) in self.css_colors() {
            let _ = writeln!(css, "  {name}: {color};");
        }
        css.push_str("  --window-radius: 0px;\n}\n");
        css
    }

    /// FNV-1a over the generated scheme and CSS: equal output, equal fingerprint, across runs.
    pub fn fingerprint(&self) -> u64 {
        let xml = self.style_scheme_xml("", "");
        let css = self.adwaita_css();
        xml.bytes()
            .chain(css.bytes())
            .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
            })
    }

    /// A scheme id that changes whenever the output does, for cache file names.
    pub fn scheme_id(&self) -> String {
        format!("stet-omarchy-{:016x}", self.fingerprint())
    }
}

/// Whichever of `a` and `b` contrasts more with `background`.
pub fn readable_on(background: Rgb, a: Rgb, b: Rgb) -> Rgb {
    if background.contrast(a) >= background.contrast(b) {
        a
    } else {
        b
    }
}

fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn colors(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    const CATPPUCCIN: &[(&str, &str)] = &[
        ("mode", "dark"),
        ("accent", "#89b4fa"),
        ("selection", "#45475a"),
        ("muted", "#585b70"),
        ("background", "#1e1e2e"),
        ("dark_background", "#161622"),
        ("darker_background", "#101019"),
        ("lighter_background", "#313244"),
        ("foreground", "#cdd6f4"),
        ("dark_foreground", "#6c7086"),
        ("light_foreground", "#bac2de"),
        ("bright_foreground", "#cdd6f4"),
        ("red", "#f38ba8"),
        ("yellow", "#f9e2af"),
        ("orange", "#f6b6ab"),
        ("green", "#a6e3a1"),
        ("cyan", "#94e2d5"),
        ("blue", "#89b4fa"),
        ("magenta", "#f5c2e7"),
        ("brown", "#7b5b55"),
        ("bright_red", "#f38ba8"),
        ("bright_yellow", "#f9e2af"),
        ("bright_green", "#a6e3a1"),
        ("bright_cyan", "#94e2d5"),
        ("bright_blue", "#89b4fa"),
        ("bright_magenta", "#f5c2e7"),
    ];

    const CATPPUCCIN_LATTE: &[(&str, &str)] = &[
        ("mode", "light"),
        ("accent", "#1e66f5"),
        ("selection", "#ccd0da"),
        ("muted", "#acb0be"),
        ("background", "#eff1f5"),
        ("dark_background", "#e3e4e8"),
        ("darker_background", "#d7d8dc"),
        ("lighter_background", "#dce0e8"),
        ("foreground", "#4c4f69"),
        ("dark_foreground", "#9ca0b0"),
        ("light_foreground", "#5c5f77"),
        ("bright_foreground", "#4c4f69"),
        ("red", "#d20f39"),
        ("yellow", "#df8e1d"),
        ("orange", "#d84e2b"),
        ("green", "#40a02b"),
        ("cyan", "#179299"),
        ("blue", "#1e66f5"),
        ("magenta", "#ea76cb"),
        ("brown", "#6c2715"),
        ("bright_red", "#d20f39"),
        ("bright_yellow", "#df8e1d"),
        ("bright_green", "#40a02b"),
        ("bright_cyan", "#179299"),
        ("bright_blue", "#1e66f5"),
        ("bright_magenta", "#ea76cb"),
    ];

    /// A pre-semantic theme: ANSI names only, no accent, no mode.
    const LEGACY: &[(&str, &str)] = &[
        ("color0", "#282828"),
        ("color1", "#cc241d"),
        ("color2", "#98971a"),
        ("color3", "#d79921"),
        ("color4", "#458588"),
        ("color5", "#b16286"),
        ("color6", "#689d6a"),
        ("color7", "#ebdbb2"),
        ("color8", "#928374"),
        ("color15", "#fbf1c7"),
    ];

    fn theme(pairs: &[(&str, &str)]) -> EditorTheme {
        EditorTheme::new(&Palette::from_colors(&colors(pairs)))
    }

    #[test]
    fn rgb_parses_supported_forms() {
        let expected = Rgb::from_hex(0x1e66f5);
        for value in [
            "#1e66f5",
            "#1E66F5",
            " #1e66f5 ",
            "#1e66f5ee",
            "rgb(1e66f5)",
            "rgba(1e66f5ee)",
            "rgb(30, 102, 245)",
            "rgba(30,102,245,0.5)",
        ] {
            assert_eq!(Rgb::parse(value), Some(expected), "{value}");
        }
        assert_eq!(Rgb::parse("#fa0"), Some(Rgb::from_hex(0xffaa00)));
    }

    #[test]
    fn rgb_rejects_garbage() {
        for value in [
            "",
            "#",
            "#12345",
            "#1234567",
            "#ggggggg",
            "blue",
            "rgb(1,2)",
            "rgb(256,0,0)",
            "rgb(",
        ] {
            assert_eq!(Rgb::parse(value), None, "{value}");
        }
    }

    #[test]
    fn rgb_displays_lowercase_hex() {
        assert_eq!(Rgb::from_hex(0x1E66F5).to_string(), "#1e66f5");
        assert_eq!(Rgb::BLACK.to_string(), "#000000");
    }

    #[test]
    fn mix_rounds_like_omarchy_theme_color() {
        let dark = Rgb::from_hex(0x101010);
        assert_eq!(dark.mix(Rgb::BLACK, 25), Rgb::from_hex(0x0c0c0c));
        assert_eq!(dark.mix(Rgb::BLACK, 50), Rgb::from_hex(0x080808));
        assert_eq!(
            Rgb::from_hex(0xff0000).mix(Rgb::WHITE, 20),
            Rgb::from_hex(0xff3333)
        );
        assert_eq!(dark.mix(Rgb::WHITE, 0), dark);
        assert_eq!(dark.mix(Rgb::WHITE, 100), Rgb::WHITE);
    }

    #[test]
    fn contrast_spans_one_to_twenty_one() {
        assert!((Rgb::BLACK.contrast(Rgb::WHITE) - 21.0).abs() < 1e-9);
        assert!((Rgb::WHITE.contrast(Rgb::WHITE) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn resolves_an_ansi_only_theme_like_the_script() {
        let resolved = resolve_colors(&colors(&[("color0", "#101010"), ("color1", "#ff0000")]));
        let expected = colors(&[
            ("background", "#101010"),
            ("bg", "#101010"),
            ("bright_red", "#ff3333"),
            ("color0", "#101010"),
            ("color1", "#ff0000"),
            ("color9", "#ff3333"),
            ("dark_background", "#0c0c0c"),
            ("dark_bg", "#0c0c0c"),
            ("darker_background", "#080808"),
            ("darker_bg", "#080808"),
            ("lighter_background", "#101010"),
            ("lighter_bg", "#101010"),
            ("mode", "dark"),
            ("red", "#ff0000"),
            ("selection", "#101010"),
            ("selection_background", "#101010"),
            ("theme_type", "dark"),
        ]);
        assert_eq!(resolved, expected);
    }

    #[test]
    fn canonical_names_win_over_short_names() {
        let resolved = resolve_colors(&colors(&[("bg", "#111111"), ("background", "#222222")]));
        assert_eq!(resolved["background"], "#222222");
        assert_eq!(resolved["bg"], "#222222");
        assert_eq!(resolved["color0"], "#222222");

        let resolved = resolve_colors(&colors(&[("fg", "#dddddd")]));
        assert_eq!(resolved["foreground"], "#dddddd");
        assert_eq!(resolved["color7"], "#dddddd");
    }

    #[test]
    fn empty_values_count_as_missing() {
        let resolved = resolve_colors(&colors(&[("background", ""), ("color0", "#123456")]));
        assert_eq!(resolved["background"], "#123456");
        assert!(!resolve_colors(&colors(&[("accent", "")])).contains_key("accent"));
    }

    #[test]
    fn cursor_always_follows_bright_foreground() {
        let resolved = resolve_colors(&colors(&[
            ("cursor", "#123456"),
            ("bright_foreground", "#abcdef"),
        ]));
        assert_eq!(resolved["cursor"], "#abcdef");
        assert!(!resolve_colors(&colors(&[("cursor", "#123456")])).contains_key("cursor"));
    }

    #[test]
    fn purple_aliases_magenta_both_ways() {
        let resolved = resolve_colors(&colors(&[("purple", "#aa00aa")]));
        assert_eq!(resolved["magenta"], "#aa00aa");
        assert_eq!(resolved["color5"], "#aa00aa");
        assert_eq!(resolved["bright_magenta"], "#bb33bb");
        assert_eq!(resolved["bright_purple"], "#bb33bb");

        let resolved = resolve_colors(&colors(&[("magenta", "#aa00aa")]));
        assert_eq!(resolved["purple"], "#aa00aa");
    }

    #[test]
    fn mode_comes_from_mode_then_theme_type_then_luminance() {
        let mode = |pairs: &[(&str, &str)]| resolve_colors(&colors(pairs))["mode"].clone();
        assert_eq!(
            mode(&[("mode", "light"), ("background", "#000000")]),
            "light"
        );
        assert_eq!(mode(&[("theme_type", "light")]), "light");
        assert_eq!(mode(&[("background", "#808080")]), "light");
        assert_eq!(mode(&[("background", "#7f7f7f")]), "dark");
        assert_eq!(mode(&[("background", "#ffffffff")]), "dark");
        assert_eq!(mode(&[]), "dark");
        let resolved = resolve_colors(&colors(&[("theme_type", "light")]));
        assert_eq!(resolved["theme_type"], "light");
    }

    #[test]
    fn shades_are_derived_only_from_hex_values() {
        let resolved = resolve_colors(&colors(&[("background", "rgb(10, 10, 10)")]));
        assert!(!resolved.contains_key("dark_background"));
        let resolved = resolve_colors(&colors(&[("orange", "#ffa348")]));
        assert_eq!(resolved["brown"], "#805224");
    }

    #[test]
    fn empty_input_gives_the_builtin_dark_palette() {
        assert_eq!(Palette::from_colors(&BTreeMap::new()), Palette::DARK);
        assert_eq!(
            Palette::from_colors(&colors(&[("mode", "light")])),
            Palette::LIGHT
        );
    }

    #[test]
    fn accent_falls_back_to_blue_then_the_builtin_accent() {
        let palette = Palette::from_colors(&colors(&[("blue", "#0000ff")]));
        assert_eq!(palette.accent, Rgb::from_hex(0x0000ff));
        let palette = Palette::from_colors(&colors(&[("color4", "#0000aa")]));
        assert_eq!(palette.accent, Rgb::from_hex(0x0000aa));
        let palette = Palette::from_colors(&colors(&[("background", "#101010")]));
        assert_eq!(palette.accent, Palette::DARK.accent);
    }

    #[test]
    fn legacy_theme_fills_every_key() {
        let palette = Palette::from_colors(&colors(LEGACY));
        assert_eq!(palette.mode, Mode::Dark);
        assert_eq!(palette.background, Rgb::from_hex(0x282828));
        assert_eq!(palette.magenta, Rgb::from_hex(0xb16286));
        assert_eq!(palette.orange, palette.yellow);
        assert_eq!(palette.accent, Rgb::from_hex(0x458588));
        assert_eq!(palette.cursor, Rgb::from_hex(0xfbf1c7));
        assert_eq!(palette.muted, Rgb::from_hex(0x928374));
    }

    #[test]
    fn unparsable_values_fall_back_per_key() {
        let palette = Palette::from_colors(&colors(&[("red", "tomato"), ("mode", "Light")]));
        assert_eq!(palette.mode, Mode::Light);
        assert_eq!(palette.red, Palette::LIGHT.red);
    }

    #[test]
    fn theme_mode_follows_the_palette() {
        assert!(theme(CATPPUCCIN).is_dark());
        assert!(!theme(CATPPUCCIN_LATTE).is_dark());
    }

    #[test]
    fn current_line_never_matches_the_selection() {
        let theme = theme(&[
            ("background", "#ffffff"),
            ("foreground", "#000000"),
            ("lighter_background", "#c0c0c0"),
            ("selection", "#c0c0c0"),
        ]);
        assert_ne!(theme.current_line, theme.palette.selection_background);
        assert_ne!(theme.current_line, theme.palette.background);
    }

    #[test]
    fn chrome_follows_the_design_roles() {
        let theme = theme(CATPPUCCIN);
        let p = &theme.palette;
        assert_eq!(theme.headerbar, p.dark_background);
        assert_eq!(theme.popover, p.darker_background);
        assert_eq!(theme.card, p.lighter_background);
        assert_eq!(theme.current_line, p.lighter_background);
        assert_eq!(theme.search_current_text, p.background);
    }

    #[test]
    fn accent_text_is_the_more_readable_end() {
        let dark = theme(CATPPUCCIN);
        assert_eq!(dark.accent_text, dark.palette.background);
        let light = theme(CATPPUCCIN_LATTE);
        assert_eq!(light.accent_text, light.palette.background);
        let pale_accent = theme(&[
            ("background", "#ffffff"),
            ("foreground", "#000000"),
            ("accent", "#ffff80"),
        ]);
        assert_eq!(pale_accent.accent_text, Rgb::BLACK);
    }

    #[test]
    fn scheme_defines_the_required_styles_once() {
        let styles = theme(CATPPUCCIN).scheme_styles();
        let names: Vec<&str> = styles.iter().map(|style| style.name).collect();
        for required in [
            "text",
            "selection",
            "current-line",
            "line-numbers",
            "cursor",
            "bracket-match",
            "search-match",
            "draw-spaces",
            "def:keyword",
            "def:statement",
            "def:function",
            "def:type",
            "def:number",
            "def:constant",
            "def:string",
            "def:operator",
            "def:special-char",
            "def:comment",
            "def:error",
            SMART_HIGHLIGHT_STYLE,
            SEARCH_CURRENT_STYLE,
        ] {
            assert_eq!(
                names.iter().filter(|name| **name == required).count(),
                1,
                "{required}"
            );
        }
    }

    #[test]
    fn syntax_follows_the_plan_mapping() {
        let theme = theme(CATPPUCCIN);
        let p = &theme.palette;
        let styles = theme.scheme_styles();
        let style = |name: &str| *styles.iter().find(|style| style.name == name).unwrap();
        assert_eq!(style("def:keyword").foreground, Some(p.magenta));
        assert_eq!(style("def:function").foreground, Some(p.blue));
        assert_eq!(style("def:type").foreground, Some(p.yellow));
        assert_eq!(style("def:number").foreground, Some(p.orange));
        assert_eq!(style("def:string").foreground, Some(p.green));
        assert_eq!(style("def:operator").foreground, Some(p.cyan));
        assert!(style("def:comment").italic);
        assert_eq!(style("def:comment").foreground, Some(p.muted));
        assert_eq!(style("def:error").foreground, Some(p.red));
        assert_eq!(style("def:preprocessor").foreground, Some(p.magenta));
        assert_eq!(style("bracket-match").foreground, Some(p.yellow));
        assert_eq!(style(SEARCH_CURRENT_STYLE).background, Some(p.yellow));
        assert_eq!(
            style(SMART_HIGHLIGHT_STYLE).background,
            Some(p.background.mix(p.accent, 30))
        );
    }

    #[test]
    fn marks_tokens_and_comparisons_follow_the_palette() {
        let theme = theme(CATPPUCCIN_LATTE);
        let p = &theme.palette;
        let styles = theme.scheme_styles();
        let style = |name: &str| *styles.iter().find(|style| style.name == name).unwrap();
        assert_eq!(
            style(MARK_STYLE).background,
            Some(p.background.mix(p.red, 40))
        );
        for (name, color) in TOKEN_STYLES
            .iter()
            .zip([p.cyan, p.orange, p.yellow, p.magenta, p.green])
        {
            assert_eq!(
                style(name).background,
                Some(p.background.mix(color, 40)),
                "{name}"
            );
        }
        assert_eq!(style(BOOKMARK_STYLE).foreground, Some(p.accent));
        for (name, color) in [
            (COMPARE_ADDED_STYLE, p.green),
            (COMPARE_REMOVED_STYLE, p.red),
            (COMPARE_CHANGED_STYLE, p.blue),
            (COMPARE_MOVED_STYLE, p.magenta),
        ] {
            assert_eq!(
                style(name).background,
                Some(p.background.mix(color, 20)),
                "{name}"
            );
        }
        // Every background differs from the text's, so every mark and line shows.
        for name in TOKEN_STYLES.iter().chain(&[
            MARK_STYLE,
            COMPARE_ADDED_STYLE,
            COMPARE_REMOVED_STYLE,
            COMPARE_CHANGED_STYLE,
            COMPARE_MOVED_STYLE,
            COMPARE_INLINE_STYLE,
            COMPARE_PAD_STYLE,
        ]) {
            assert_ne!(style(name).background, Some(p.background), "{name}");
        }
    }

    #[test]
    fn hsl_round_trips() {
        for hex in [
            0x926d67, 0x7aa2f7, 0x9ece6a, 0xffffff, 0x000000, 0x808080, 0xde6f69,
        ] {
            let color = Rgb::from_hex(hex);
            let (h, s, l) = color.to_hsl();
            let back = Rgb::from_hsl(h, s, l);
            for (a, b) in [(color.r, back.r), (color.g, back.g), (color.b, back.b)] {
                assert!(a.abs_diff(b) <= 1, "{color} -> {back}");
            }
        }
    }

    #[test]
    fn single_hue_themes_get_distinct_role_colours() {
        // Aether's dark-music2: its green, red, blue and magenta are all reddish browns.
        let mono = [0x926d67, 0xb06e65, 0xa86660, 0xdf9892].map(Rgb::from_hex);
        let turned = distinct(mono, COMPARE_HUES);
        let hues = turned.map(|color| color.to_hsl().0);
        for (index, hue) in hues.iter().enumerate() {
            let gap = (hue - COMPARE_HUES[index]).rem_euclid(360.0);
            assert!(gap.min(360.0 - gap) < 2.0, "{index}: {hue}");
        }
        // A palette with its own hues keeps them, near neighbours (peach and yellow) included.
        let tokyo = [0x9ece6a, 0xf7768e, 0x7aa2f7, 0xbb9af7].map(Rgb::from_hex);
        assert_eq!(distinct(tokyo, COMPARE_HUES), tokyo);
        let catppuccin = [0x94e2d5, 0xfab387, 0xf9e2af, 0xf5c2e7, 0xa6e3a1].map(Rgb::from_hex);
        assert_eq!(distinct(catppuccin, TOKEN_HUES), catppuccin);
        // Greys get hues too.
        let greys = [0x777777, 0x888888, 0x999999, 0xaaaaaa].map(Rgb::from_hex);
        assert_ne!(distinct(greys, COMPARE_HUES), greys);
    }

    #[test]
    fn xml_escapes_the_name_and_carries_the_id() {
        let xml = theme(CATPPUCCIN).style_scheme_xml("stet-test", "Tom & \"Jerry\" <3");
        assert!(xml.contains("id=\"stet-test\""));
        assert!(xml.contains("name=\"Tom &amp; &quot;Jerry&quot; &lt;3\""));
        assert!(xml.contains("<property name=\"variant\">dark</property>"));
        assert!(xml.ends_with("</style-scheme>\n"));
    }

    #[test]
    fn css_sets_every_planned_variable() {
        let css = theme(CATPPUCCIN_LATTE).adwaita_css();
        for variable in [
            "--window-bg-color: #eff1f5;",
            "--view-bg-color: #eff1f5;",
            "--headerbar-bg-color: #e3e4e8;",
            "--popover-bg-color: #d7d8dc;",
            "--dialog-bg-color: #d7d8dc;",
            "--card-bg-color: #dce0e8;",
            "--sidebar-bg-color: #e3e4e8;",
            "--border-color: #dce0e8;",
            "--error-bg-color: #d20f39;",
            "--warning-bg-color: #df8e1d;",
            "--success-bg-color: #40a02b;",
            "--accent-bg-color: #1e66f5;",
            "--accent-color: #1e66f5;",
            "--window-fg-color: #4c4f69;",
            "--view-fg-color: #4c4f69;",
            "--window-radius: 0px;",
        ] {
            assert!(css.contains(variable), "{variable}\n{css}");
        }
    }

    #[test]
    fn fingerprint_tracks_the_output() {
        let dark = theme(CATPPUCCIN);
        assert_eq!(dark.fingerprint(), theme(CATPPUCCIN).fingerprint());
        assert_ne!(dark.fingerprint(), theme(CATPPUCCIN_LATTE).fingerprint());
        let id = dark.scheme_id();
        assert!(id.starts_with("stet-omarchy-"));
        assert!(
            id.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        );
    }

    #[test]
    fn snapshots() {
        for (name, pairs) in [
            ("catppuccin", CATPPUCCIN),
            ("catppuccin_latte", CATPPUCCIN_LATTE),
            ("legacy_ansi", LEGACY),
        ] {
            let theme = theme(pairs);
            insta::assert_snapshot!(
                format!("{name}_scheme"),
                theme.style_scheme_xml("stet-snapshot", name)
            );
            insta::assert_snapshot!(format!("{name}_css"), theme.adwaita_css());
        }
    }

    const KEYS: &[&str] = &[
        "mode",
        "theme_type",
        "accent",
        "background",
        "bg",
        "foreground",
        "fg",
        "muted",
        "selection",
        "selection_background",
        "lighter_background",
        "bright_foreground",
        "orange",
        "yellow",
        "purple",
        "magenta",
        "blue",
        "color0",
        "color4",
        "color7",
        "color8",
        "color15",
    ];

    fn raw_colors() -> impl Strategy<Value = BTreeMap<String, String>> {
        let value = prop_oneof![
            "#[0-9a-fA-F]{6}",
            "#[0-9a-f]{8}",
            "(light|dark|Light)",
            "[ -~]{0,10}",
        ];
        proptest::collection::btree_map(
            proptest::sample::select(KEYS).prop_map(str::to_owned),
            value,
            0..KEYS.len(),
        )
    }

    proptest! {
        #[test]
        fn resolving_keeps_every_value_it_does_not_rewrite(raw in raw_colors()) {
            let resolved = resolve_colors(&raw);
            let rewritten = |key: &str| {
                matches!(key, "cursor" | "theme_type" | "color0" | "color7")
                    || SHORT_NAMES.iter().any(|(_, short)| *short == key)
            };
            for (key, value) in raw.iter().filter(|(key, value)| !value.is_empty() && !rewritten(key)) {
                prop_assert_eq!(resolved.get(key), Some(value), "{}", key);
            }
            prop_assert!(resolved.values().all(|value| !value.is_empty()));
            prop_assert!(resolved.contains_key("mode"));
        }

        #[test]
        fn any_input_yields_a_complete_theme(raw in raw_colors()) {
            let theme = EditorTheme::new(&Palette::from_colors(&raw));
            let xml = theme.style_scheme_xml("stet-prop", "prop");
            prop_assert_eq!(xml.matches("<style name=").count(), theme.scheme_styles().len());
            prop_assert!(theme.adwaita_css().contains("--accent-bg-color: #"));
        }
    }
}
