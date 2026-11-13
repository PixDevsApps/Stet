//! The CSS the theme and the font settings load: libadwaita's colour variables for an
//! [`EditorTheme`], and the editor font.

use std::fmt::Write as _;
use stet_domain::theme::{EditorTheme, Rgb, readable_on};

/// The libadwaita colour variables the generated CSS overrides, in output order.
fn css_colors(theme: &EditorTheme) -> Vec<(&'static str, Rgb)> {
    let p = &theme.palette;
    vec![
        ("--window-bg-color", p.background),
        ("--window-fg-color", p.foreground),
        ("--view-bg-color", p.background),
        ("--view-fg-color", p.foreground),
        ("--headerbar-bg-color", theme.headerbar),
        ("--headerbar-fg-color", p.foreground),
        ("--headerbar-backdrop-color", theme.headerbar),
        ("--headerbar-shade-color", theme.card),
        ("--sidebar-bg-color", theme.headerbar),
        ("--sidebar-fg-color", p.foreground),
        ("--sidebar-backdrop-color", theme.headerbar),
        ("--popover-bg-color", theme.popover),
        ("--popover-fg-color", p.foreground),
        ("--dialog-bg-color", theme.popover),
        ("--dialog-fg-color", p.foreground),
        ("--card-bg-color", theme.card),
        ("--card-fg-color", p.foreground),
        ("--border-color", theme.card),
        ("--accent-bg-color", p.accent),
        ("--accent-fg-color", theme.accent_text),
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
pub fn adwaita_css(theme: &EditorTheme) -> String {
    let mut css = String::from(":root {\n");
    for (name, color) in css_colors(theme) {
        let _ = writeln!(css, "  {name}: {color};");
    }
    css.push_str("  --window-radius: 0px;\n}\n");
    css
}

/// CSS for the editor font (`.stet-editor`) and the monospace chrome (`.stet-mono`).
pub fn font_css(family: &str, editor_size_pt: i32) -> String {
    let family = css_string(family);
    format!(
        ".stet-editor {{ font-family: {family}; font-size: {editor_size_pt}pt; }}\n\
         .stet-mono {{ font-family: {family}; }}\n"
    )
}

/// A CSS string literal, quoted and escaped.
fn css_string(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for character in text.chars() {
        match character {
            '"' | '\\' => {
                quoted.push('\\');
                quoted.push(character);
            }
            '\n' | '\r' => quoted.push(' '),
            _ => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::{CATPPUCCIN, CATPPUCCIN_LATTE, LEGACY, theme};
    use super::*;

    #[test]
    fn css_sets_every_planned_variable() {
        let css = adwaita_css(&theme(CATPPUCCIN_LATTE));
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
    fn font_css_quotes_the_family() {
        let css = font_css("JetBrainsMono Nerd Font", 12);
        assert_eq!(
            css,
            ".stet-editor { font-family: \"JetBrainsMono Nerd Font\"; font-size: 12pt; }\n\
             .stet-mono { font-family: \"JetBrainsMono Nerd Font\"; }\n"
        );
        let hostile = font_css("A\"; } * { color: red; \\", 11);
        assert!(hostile.contains("\"A\\\"; } * { color: red; \\\\\""));
        assert_eq!(hostile.matches('\n').count(), 2);
    }

    #[test]
    fn snapshots() {
        for (name, pairs) in [
            ("catppuccin", CATPPUCCIN),
            ("catppuccin_latte", CATPPUCCIN_LATTE),
            ("legacy_ansi", LEGACY),
        ] {
            let theme = theme(pairs);
            insta::assert_snapshot!(format!("{name}_css"), adwaita_css(&theme));
        }
    }
}
