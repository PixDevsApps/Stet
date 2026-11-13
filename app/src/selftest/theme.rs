//! Self-test commands for the appearance: theme swaps in the run's fake Omarchy directory,
//! the colours the schemes and the chrome get, dark mode, zoom and the fonts.

use super::registry::{Command, commands};
use super::script::boolean;
use super::{Harness, StepResult, color, equal, number};
use crate::convert::unit_to_u8;
use crate::runtime::worker;
use crate::theme::ThemeSource;
use gtk4 as gtk;
use gtk4::prelude::*;
use libadwaita as adw;
use sourceview5::prelude::*;
use std::fs;
use std::process::Command as Process;
use stet_domain::theme::Rgb;

pub(super) const COMMANDS: &[Command] = commands![
    "theme-set" (2, 2) => async theme_set,
    "assert-theme" (3, 3) => assert_theme,
    "assert-chrome-fg" (1, 1) => assert_chrome_fg,
    "assert-dark" (1, 1) => assert_dark,
    "assert-theme-source" (1, 1) => assert_theme_source,
    "assert-reloads" (1, 1) => assert_reloads,
    "assert-scheme-files" (1, 1) => assert_scheme_files,
    "assert-zoom" (1, 1) => assert_zoom,
    "assert-font-size" (1, 1) => assert_font_size,
    "assert-font-family" (1, 1) => assert_font_family,
    "assert-font-reloads" (1, 1) => assert_font_reloads,
    "assert-editor-font" (1, 1) => assert_editor_font,
];

impl Harness {
    /// `theme-set <theme folder> <name>`: swaps the theme in the fake Omarchy directory the
    /// way `omarchy-theme-set` does: builds `next-theme`, removes `theme`, renames
    /// `next-theme` to `theme` and writes `theme.name`.
    async fn theme_set(&self, args: &[String]) -> StepResult {
        const SWAP: &str = r#"set -e
current="$1"; source="$2"; name="$3"
mkdir -p "$current"
cd "$current"
rm -rf next-theme
mkdir -p next-theme
cp -r "$source"/. next-theme/
rm -rf theme
mv next-theme theme
echo "$name" > theme.name
"#;
        let current = self.root.join("omarchy/current");
        let source = args[0].clone();
        let name = args[1].clone();
        let output = worker::run(move || {
            Process::new("bash")
                .arg("-c")
                .arg(SWAP)
                .arg("theme-set")
                .arg(current)
                .arg(source)
                .arg(name)
                .output()
        })
        .await
        .ok_or("the worker failed")?
        .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        Ok(())
    }

    /// `assert-theme <style> foreground|background <colour>`: a colour of a style of the
    /// current buffer's scheme.
    fn assert_theme(&self, args: &[String]) -> StepResult {
        let (style, attribute) = (args[0].as_str(), args[1].as_str());
        let wanted = color(&args[2])?;
        let scheme = self
            .page()?
            .buffer()
            .style_scheme()
            .ok_or_else(|| "the buffer has no scheme".to_owned())?;
        let style_object = scheme
            .style(style)
            .ok_or_else(|| format!("the scheme has no style {style}"))?;
        let value = match attribute {
            "foreground" => style_object.foreground(),
            "background" => style_object.background(),
            other => return Err(format!("unknown attribute {other}")),
        }
        .ok_or_else(|| format!("{style} has no {attribute}"))?;
        let actual = color(&value)?;
        equal(
            &format!("{style} {attribute}"),
            actual.to_string(),
            wanted.to_string(),
        )
    }

    /// `assert-chrome-fg <colour>`: the status bar's text colour.
    fn assert_chrome_fg(&self, args: &[String]) -> StepResult {
        let wanted = color(&args[0])?;
        let rgba = self.window.status_color();
        let actual = Rgb {
            r: unit_to_u8(rgba.red()),
            g: unit_to_u8(rgba.green()),
            b: unit_to_u8(rgba.blue()),
        };
        equal("status bar colour", actual.to_string(), wanted.to_string())
    }

    /// `assert-dark true|false`: whether libadwaita is dark.
    fn assert_dark(&self, args: &[String]) -> StepResult {
        equal(
            "dark",
            adw::StyleManager::default().is_dark(),
            boolean(&args[0])?,
        )
    }

    /// `assert-theme-source omarchy:<name>|builtin:<mode>|none`: where the theme comes from.
    fn assert_theme_source(&self, args: &[String]) -> StepResult {
        let source = match self.window.shared().appearance.source() {
            Some(ThemeSource::Omarchy(name)) => format!("omarchy:{name}"),
            Some(ThemeSource::Builtin(mode)) => format!("builtin:{}", mode.as_str()),
            None => "none".to_owned(),
        };
        equal("theme source", source, args[0].clone())
    }

    /// `assert-reloads <n>`: how often the theme was loaded.
    fn assert_reloads(&self, args: &[String]) -> StepResult {
        equal(
            "theme reloads",
            self.window.shared().appearance.reloads() as usize,
            number(&args[0])?,
        )
    }

    /// `assert-scheme-files <n>`: how many style scheme files the styles folder holds.
    fn assert_scheme_files(&self, args: &[String]) -> StepResult {
        let dir = self.window.shared().config.dirs.styles_dir();
        let count = fs::read_dir(&dir)
            .map_err(|error| error.to_string())?
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".xml"))
            .count();
        equal("scheme files", count, number(&args[0])?)
    }

    /// `assert-zoom <points>`: the zoom step, in points from the theme's size.
    fn assert_zoom(&self, args: &[String]) -> StepResult {
        equal(
            "zoom",
            self.window.shared().appearance.zoom().points(),
            args[0].parse::<i32>().map_err(|error| error.to_string())?,
        )
    }

    /// `assert-font-size <points>`: the current editor's font size.
    fn assert_font_size(&self, args: &[String]) -> StepResult {
        let page = self.page()?;
        // GTK turns CSS points into an absolute size in pixels at 96 dpi.
        let size = page
            .view()
            .pango_context()
            .font_description()
            .map(|font| {
                let size = f64::from(font.size()) / f64::from(gtk::pango::SCALE);
                if font.is_size_absolute() {
                    size * 72.0 / 96.0
                } else {
                    size
                }
            })
            .unwrap_or_default();
        equal("font size (pt)", format!("{size:.0}"), args[0].clone())
    }

    /// `assert-font-family <family>`: the theme's font family.
    fn assert_font_family(&self, args: &[String]) -> StepResult {
        equal(
            "font family",
            self.window.shared().appearance.family().unwrap_or_default(),
            args[0].clone(),
        )
    }

    /// `assert-font-reloads <n>`: how often the fonts were loaded.
    fn assert_font_reloads(&self, args: &[String]) -> StepResult {
        equal(
            "font reloads",
            self.window.shared().appearance.font_reloads() as usize,
            number(&args[0])?,
        )
    }

    /// `assert-editor-font <family>`: the editor's font family, from `config.toml` or the
    /// theme.
    fn assert_editor_font(&self, args: &[String]) -> StepResult {
        equal(
            "editor font",
            self.window
                .shared()
                .appearance
                .editor_family()
                .unwrap_or_default(),
            args[0].clone(),
        )
    }
}
