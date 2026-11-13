//! The Omarchy palette: `colors.toml` with `omarchy-theme-color`'s aliases and fallbacks
//! applied, and the built-in palettes for running outside Omarchy.

use super::Rgb;
use std::collections::BTreeMap;

/// Whether a theme is dark or light.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Mode {
    /// Light text on a dark background.
    #[default]
    Dark,
    /// Dark text on a light background.
    Light,
}

impl Mode {
    /// The mode's name as `colors.toml` writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
}

pub(super) const SHORT_NAMES: [(&str, &str); 8] = [
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
#[must_use]
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
    /// Whether the theme is dark or light.
    pub mode: Mode,
    /// The accent, for focus rings and highlights.
    pub accent: Rgb,
    /// The editor's background.
    pub background: Rgb,
    /// A darker background, for the header bar, tab strip and status bar.
    pub dark_background: Rgb,
    /// The darkest background, for popovers and dialogs.
    pub darker_background: Rgb,
    /// A lighter background, for the current line and cards.
    pub lighter_background: Rgb,
    /// The text.
    pub foreground: Rgb,
    /// Dimmer text.
    pub dark_foreground: Rgb,
    /// Lighter text.
    pub light_foreground: Rgb,
    /// The brightest text, ANSI `color15`.
    pub bright_foreground: Rgb,
    /// Muted text such as comments, ANSI `color8`.
    pub muted: Rgb,
    /// The selection's background.
    pub selection_background: Rgb,
    /// The selected text.
    pub selection_foreground: Rgb,
    /// The caret.
    pub cursor: Rgb,
    /// ANSI `color1`.
    pub red: Rgb,
    /// ANSI `color3`.
    pub yellow: Rgb,
    /// Orange, which has no ANSI number.
    pub orange: Rgb,
    /// ANSI `color2`.
    pub green: Rgb,
    /// ANSI `color6`.
    pub cyan: Rgb,
    /// ANSI `color4`.
    pub blue: Rgb,
    /// ANSI `color5`.
    pub magenta: Rgb,
    /// Brown, which has no ANSI number.
    pub brown: Rgb,
    /// ANSI `color9`.
    pub bright_red: Rgb,
    /// ANSI `color11`.
    pub bright_yellow: Rgb,
    /// ANSI `color10`.
    pub bright_green: Rgb,
    /// ANSI `color14`.
    pub bright_cyan: Rgb,
    /// ANSI `color12`.
    pub bright_blue: Rgb,
    /// ANSI `color13`.
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

    /// The built-in light palette, for running outside Omarchy and for keys a light theme lacks.
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

    /// The built-in palette for `mode`.
    #[must_use]
    pub const fn builtin(mode: Mode) -> Self {
        match mode {
            Mode::Dark => Self::DARK,
            Mode::Light => Self::LIGHT,
        }
    }

    /// Builds the palette from a raw `colors.toml` map by applying [`resolve_colors`] first.
    #[cfg(test)]
    #[must_use]
    pub fn from_colors(raw: &BTreeMap<String, String>) -> Self {
        Self::from_resolved(&resolve_colors(raw))
    }

    /// Builds the palette from `omarchy-theme-color --all` output or [`resolve_colors`] output.
    /// The cascade is not re-run: like the script, it is not idempotent. Keys still missing or
    /// unparsable come from the built-in palette for the mode; `accent` tries `blue` first.
    #[must_use]
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
