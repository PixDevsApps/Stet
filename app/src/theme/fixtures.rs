//! Palettes for the scheme and CSS tests.

use proptest::prelude::*;
use std::collections::BTreeMap;
use stet_domain::theme::{EditorTheme, Palette, resolve_colors};

fn colors(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

pub(super) const CATPPUCCIN: &[(&str, &str)] = &[
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

pub(super) const CATPPUCCIN_LATTE: &[(&str, &str)] = &[
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
pub(super) const LEGACY: &[(&str, &str)] = &[
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

pub(super) fn theme(pairs: &[(&str, &str)]) -> EditorTheme {
    theme_of(&colors(pairs))
}

/// The theme of a raw `colors.toml` map.
pub(super) fn theme_of(raw: &BTreeMap<String, String>) -> EditorTheme {
    EditorTheme::new(&Palette::from_resolved(&resolve_colors(raw)))
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

pub(super) fn raw_colors() -> impl Strategy<Value = BTreeMap<String, String>> {
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
