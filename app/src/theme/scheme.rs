//! The generated GtkSourceView 5 style scheme: the styles each role of an [`EditorTheme`]
//! sets, Stet's own `stet:*` styles among them, and the scheme's XML.

use super::css::adwaita_css;
use std::fmt::Write as _;
use stet_domain::theme::{EditorTheme, Palette, Rgb, readable_on};
use stet_infrastructure::style_cache::SCHEME_PREFIX;

/// Scheme style for words matching the selection (smart highlighting).
pub const SMART_HIGHLIGHT_STYLE: &str = "stet:smart-highlight";
/// Scheme style for the search match the caret is on.
pub const SEARCH_CURRENT_STYLE: &str = "stet:search-current";
/// Scheme styles for the Find Mark Style and the five token styles: the backgrounds the
/// Mark command and Style All Occurrences of Token give text.
pub const MARK_STYLE: &str = "stet:mark";
pub const TOKEN_STYLES: [&str; 5] = [
    "stet:token-1",
    "stet:token-2",
    "stet:token-3",
    "stet:token-4",
    "stet:token-5",
];
/// Scheme style whose foreground is the bookmark in the gutter.
pub const BOOKMARK_STYLE: &str = "stet:bookmark";
/// Scheme styles for a comparison: the backgrounds of added, removed, changed and moved
/// lines, of the characters that differ inside changed lines, and of the padding that faces
/// lines only the other side has.
pub const COMPARE_ADDED_STYLE: &str = "stet:compare-added";
pub const COMPARE_REMOVED_STYLE: &str = "stet:compare-removed";
pub const COMPARE_CHANGED_STYLE: &str = "stet:compare-changed";
pub const COMPARE_MOVED_STYLE: &str = "stet:compare-moved";
pub const COMPARE_INLINE_STYLE: &str = "stet:compare-changed-text";
pub const COMPARE_PAD_STYLE: &str = "stet:compare-padding";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Underline {
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
struct SchemeStyle {
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

/// Every style the generated scheme defines, in output order.
fn scheme_styles(theme: &EditorTheme) -> Vec<SchemeStyle> {
    let mut styles = view_styles(theme);
    styles.extend(stet_styles(theme));
    styles.extend(syntax_styles(&theme.palette));
    styles
}

/// GtkSourceView's own styles: the text, the selection, the caret, the gutter and so on.
fn view_styles(theme: &EditorTheme) -> Vec<SchemeStyle> {
    let p = &theme.palette;
    let style = SchemeStyle::new;
    vec![
        style("text").fg(p.foreground).bg(p.background),
        style("selection")
            .fg(p.selection_foreground)
            .bg(p.selection_background),
        style("selection-unfocused")
            .fg(p.selection_foreground)
            .bg(theme.selection_unfocused),
        style("cursor").fg(p.cursor),
        style("secondary-cursor").fg(p.muted),
        style("current-line").bg(theme.current_line),
        style("line-numbers").fg(p.muted).bg(p.background),
        style("current-line-number")
            .fg(p.foreground)
            .bg(theme.current_line),
        style("bracket-match").fg(p.yellow).bold(),
        style("bracket-mismatch")
            .fg(readable_on(p.red, p.background, p.foreground))
            .bg(p.red),
        style("right-margin")
            .fg(p.lighter_background)
            .bg(p.lighter_background),
        style("draw-spaces").fg(p.muted),
        style("background-pattern").bg(p.lighter_background),
        style("search-match").bg(theme.search_match),
        style("snippet-focus").bg(theme.snippet_focus),
        style("map-overlay").bg(p.muted),
    ]
}

/// The styles of Stet's own tags: matches, marks, bookmarks and comparisons.
fn stet_styles(theme: &EditorTheme) -> Vec<SchemeStyle> {
    let p = &theme.palette;
    let style = SchemeStyle::new;
    vec![
        style(SEARCH_CURRENT_STYLE)
            .fg(theme.search_current_text)
            .bg(p.yellow),
        style(SMART_HIGHLIGHT_STYLE).bg(theme.smart_highlight),
        style(MARK_STYLE).bg(theme.mark),
        style(TOKEN_STYLES[0]).bg(theme.tokens[0]),
        style(TOKEN_STYLES[1]).bg(theme.tokens[1]),
        style(TOKEN_STYLES[2]).bg(theme.tokens[2]),
        style(TOKEN_STYLES[3]).bg(theme.tokens[3]),
        style(TOKEN_STYLES[4]).bg(theme.tokens[4]),
        style(BOOKMARK_STYLE).fg(p.accent),
        style(COMPARE_ADDED_STYLE).bg(theme.compare_added),
        style(COMPARE_REMOVED_STYLE).bg(theme.compare_removed),
        style(COMPARE_CHANGED_STYLE).bg(theme.compare_changed),
        style(COMPARE_MOVED_STYLE).bg(theme.compare_moved),
        style(COMPARE_INLINE_STYLE).bg(theme.compare_inline),
        style(COMPARE_PAD_STYLE).bg(theme.compare_pad),
    ]
}

/// The language definitions' `def:` styles.
fn syntax_styles(p: &Palette) -> Vec<SchemeStyle> {
    let style = SchemeStyle::new;
    vec![
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

/// A GtkSourceView 5 style scheme. `id` must match `[A-Za-z0-9_-]+`; see [`scheme_id`].
pub fn style_scheme_xml(theme: &EditorTheme, id: &str, name: &str) -> String {
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
        theme.palette.mode.as_str()
    );
    for style in scheme_styles(theme) {
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

/// FNV-1a over the generated scheme and CSS: equal output, equal fingerprint, across runs.
fn fingerprint(theme: &EditorTheme) -> u64 {
    let xml = style_scheme_xml(theme, "", "");
    let css = adwaita_css(theme);
    xml.bytes()
        .chain(css.bytes())
        .fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
}

/// A scheme id that changes whenever the output does, for cache file names.
pub fn scheme_id(theme: &EditorTheme) -> String {
    format!("{SCHEME_PREFIX}{:016x}", fingerprint(theme))
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
    use super::super::fixtures::{
        CATPPUCCIN, CATPPUCCIN_LATTE, LEGACY, raw_colors, theme, theme_of,
    };
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn scheme_defines_the_required_styles_once() {
        let styles = scheme_styles(&theme(CATPPUCCIN));
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
        let styles = scheme_styles(&theme);
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
        let styles = scheme_styles(&theme);
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
    fn xml_escapes_the_name_and_carries_the_id() {
        let xml = style_scheme_xml(&theme(CATPPUCCIN), "stet-test", "Tom & \"Jerry\" <3");
        assert!(xml.contains("id=\"stet-test\""));
        assert!(xml.contains("name=\"Tom &amp; &quot;Jerry&quot; &lt;3\""));
        assert!(xml.contains("<property name=\"variant\">dark</property>"));
        assert!(xml.ends_with("</style-scheme>\n"));
    }

    #[test]
    fn fingerprint_tracks_the_output() {
        let dark = theme(CATPPUCCIN);
        assert_eq!(fingerprint(&dark), fingerprint(&theme(CATPPUCCIN)));
        assert_ne!(fingerprint(&dark), fingerprint(&theme(CATPPUCCIN_LATTE)));
        let id = scheme_id(&dark);
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
                style_scheme_xml(&theme, "stet-snapshot", name)
            );
        }
    }

    proptest! {
        #[test]
        fn any_input_yields_a_complete_theme(raw in raw_colors()) {
            let theme = theme_of(&raw);
            let xml = style_scheme_xml(&theme, "stet-prop", "prop");
            prop_assert_eq!(xml.matches("<style name=").count(), scheme_styles(&theme).len());
            prop_assert!(adwaita_css(&theme).contains("--accent-bg-color: #"));
        }
    }
}
