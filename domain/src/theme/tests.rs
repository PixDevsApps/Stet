use super::palette::SHORT_NAMES;
use super::*;
use proptest::prelude::*;
use std::collections::BTreeMap;

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
}
