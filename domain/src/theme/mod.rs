//! Omarchy palette to editor theme: the colour model, the palette, and the colour each role of
//! the editor and its chrome takes. Pure: callers read `colors.toml` (or
//! `omarchy-theme-color --all`), and the app writes the theme out as a GtkSourceView style
//! scheme and libadwaita CSS.

mod color;
mod palette;

#[cfg(test)]
mod tests;

use color::distinct;
pub use color::{Rgb, readable_on};
pub use palette::{Mode, Palette, resolve_colors};

/// The hues compare's kinds fall back to: added, removed, changed, moved.
pub const COMPARE_HUES: [f64; 4] = [130.0, 0.0, 215.0, 290.0];
/// The hues the five token styles fall back to: cyan, orange, yellow, magenta, green.
pub const TOKEN_HUES: [f64; 5] = [185.0, 30.0, 50.0, 300.0, 120.0];

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
    /// yellow, magenta and green.
    pub mark: Rgb,
    pub tokens: [Rgb; 5],
    /// A comparison's added, removed, changed and moved lines, the changed characters, and
    /// the padding.
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

    #[cfg(test)]
    pub fn is_dark(&self) -> bool {
        self.palette.mode == Mode::Dark
    }
}
