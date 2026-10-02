use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use stet_domain::theme::resolve_colors;

/// Reads `colors.toml` leniently, like `omarchy-theme-color`: `key = "value"` lines, `#`
/// comments and blank lines. Unknown keys are kept; malformed lines are skipped.
pub fn parse_colors(text: &str) -> BTreeMap<String, String> {
    text.trim_start_matches('\u{feff}')
        .lines()
        .filter_map(parse_line)
        .collect()
}

fn parse_line(line: &str) -> Option<(String, String)> {
    let (key, value) = line.split_once('=')?;
    let key: String = key
        .chars()
        .filter(|character| !matches!(character, '"' | '\'') && !character.is_whitespace())
        .collect();
    let valid = !key.is_empty()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
    if !valid {
        return None;
    }
    let value = match value.find(['"', '\'']) {
        Some(open) => {
            let rest = &value[open + 1..];
            rest.find(['"', '\'']).map_or(rest, |close| &rest[..close])
        }
        None => value.trim().split(" #").next().unwrap_or_default(),
    };
    Some((key, value.trim().to_owned()))
}

/// Reads the `key<TAB>value` lines `omarchy-theme-color --all` prints, dropping empty values.
pub fn parse_resolved(output: &str) -> BTreeMap<String, String> {
    output
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .filter(|(key, value)| !key.is_empty() && !value.trim().is_empty())
        .map(|(key, value)| (key.to_owned(), value.trim().to_owned()))
        .collect()
}

/// The fully resolved palette for a `colors.toml`, with every fallback applied.
/// Uses Omarchy's `omarchy-theme-color` when installed, else the built-in resolver.
pub fn resolve_palette(file: &Path) -> io::Result<BTreeMap<String, String>> {
    resolve_palette_with(file, super::theme_color_script().as_deref())
}

/// [`resolve_palette`] with an explicit resolver script; `None` forces the built-in resolver.
/// A failing script falls back to the built-in resolver with a warning. Errors only when
/// `file` cannot be read.
pub fn resolve_palette_with(
    file: &Path,
    script: Option<&Path>,
) -> io::Result<BTreeMap<String, String>> {
    let text = String::from_utf8_lossy(&std::fs::read(file)?).into_owned();
    if let Some(script) = script {
        match run_script(script, file) {
            Ok(colors) => return Ok(colors),
            Err(error) => tracing::warn!(
                script = %script.display(),
                %error,
                "omarchy-theme-color failed; using the built-in resolver"
            ),
        }
    }
    let mut colors = parse_colors(&text);
    let has_mode = ["mode", "theme_type"]
        .iter()
        .any(|key| colors.get(*key).is_some_and(|value| !value.is_empty()));
    if !has_mode && file.with_file_name("light.mode").is_file() {
        colors.insert("mode".to_owned(), "light".to_owned());
    }
    Ok(resolve_colors(&colors))
}

fn run_script(script: &Path, file: &Path) -> io::Result<BTreeMap<String, String>> {
    let output = Command::new(script)
        .arg("--file")
        .arg(file)
        .arg("--all")
        .stdin(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "{}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let colors = parse_resolved(&String::from_utf8_lossy(&output.stdout));
    if colors.is_empty() {
        return Err(io::Error::other("printed no colours"));
    }
    Ok(colors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::spawn_lock;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    fn write(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, contents).unwrap();
        path
    }

    fn script(dir: &Path, body: &str) -> PathBuf {
        let path = write(dir, "resolver", &format!("#!/bin/sh\n{body}\n"));
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn parses_quoted_values_and_skips_noise() {
        let text = "\u{feff}# Theme\n\nmode = \"dark\"\naccent = '#89b4fa' # inline\n\
                    \"background\" = \"#1e1e2e\"\r\nmy_extra-key=\"rgba(26a269ee) 45deg\"\n\
                    [section]\nno equals sign\nbad key = \"#ffffff\"\n# muted = \"#000000\"\n";
        assert_eq!(
            parse_colors(text),
            map(&[
                ("mode", "dark"),
                ("accent", "#89b4fa"),
                ("background", "#1e1e2e"),
                ("my_extra-key", "rgba(26a269ee) 45deg"),
                ("badkey", "#ffffff"),
            ])
        );
    }

    #[test]
    fn parses_unquoted_and_empty_values() {
        let text = "foreground = #cdd6f4 # comment\nmuted =  #585b70  \nselection = \"\"\nred =\n";
        assert_eq!(
            parse_colors(text),
            map(&[
                ("foreground", "#cdd6f4"),
                ("muted", "#585b70"),
                ("selection", ""),
                ("red", ""),
            ])
        );
    }

    #[test]
    fn later_duplicates_win() {
        assert_eq!(
            parse_colors("red = \"#000001\"\nred = \"#000002\"\n"),
            map(&[("red", "#000002")])
        );
    }

    #[test]
    fn parses_script_output() {
        let output = "accent\t#89b4fa\nblue\t\nbroken line\nmode\tdark\n\tlonely\n";
        assert_eq!(
            parse_resolved(output),
            map(&[("accent", "#89b4fa"), ("mode", "dark")])
        );
    }

    #[test]
    fn missing_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let error = resolve_palette_with(&dir.path().join("colors.toml"), None).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn builtin_resolver_applies_the_cascade() {
        let dir = tempfile::tempdir().unwrap();
        let file = write(dir.path(), "colors.toml", "color0 = \"#101010\"\n");
        let colors = resolve_palette_with(&file, None).unwrap();
        assert_eq!(colors["background"], "#101010");
        assert_eq!(colors["dark_background"], "#0c0c0c");
        assert_eq!(colors["mode"], "dark");
    }

    #[test]
    fn light_mode_file_sets_the_mode_unless_the_theme_does() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "light.mode", "");
        let file = write(dir.path(), "colors.toml", "background = \"#101010\"\n");
        assert_eq!(resolve_palette_with(&file, None).unwrap()["mode"], "light");

        write(dir.path(), "colors.toml", "theme_type = \"dark\"\n");
        assert_eq!(resolve_palette_with(&file, None).unwrap()["mode"], "dark");
    }

    #[test]
    fn uses_the_script_output_when_it_succeeds() {
        let _spawning = spawn_lock();
        let dir = tempfile::tempdir().unwrap();
        let file = write(dir.path(), "colors.toml", "accent = \"#000000\"\n");
        let resolver = script(
            dir.path(),
            "[ \"$1\" = --file ] && [ \"$3\" = --all ] && printf 'accent\\t#123456\\nmode\\tlight\\n'",
        );
        assert_eq!(
            resolve_palette_with(&file, Some(&resolver)).unwrap(),
            map(&[("accent", "#123456"), ("mode", "light")])
        );
    }

    #[test]
    fn falls_back_when_the_script_fails_or_prints_nothing() {
        let _spawning = spawn_lock();
        let dir = tempfile::tempdir().unwrap();
        let file = write(dir.path(), "colors.toml", "accent = \"#abcdef\"\n");
        for body in ["exit 3", "true"] {
            let resolver = script(dir.path(), body);
            let colors = resolve_palette_with(&file, Some(&resolver)).unwrap();
            assert_eq!(colors["accent"], "#abcdef", "{body}");
        }
        let missing = dir.path().join("no-such-script");
        let colors = resolve_palette_with(&file, Some(&missing)).unwrap();
        assert_eq!(colors["accent"], "#abcdef");
    }

    /// The built-in resolver must agree with Omarchy's script. Skipped outside Omarchy.
    #[test]
    fn builtin_resolver_matches_omarchy_theme_color() {
        let _spawning = spawn_lock();
        let Some(omarchy) = super::super::theme_color_script() else {
            return;
        };
        let cases = [
            "mode = \"dark\"\naccent = \"#89b4fa\"\nbackground = \"#1e1e2e\"\n\
             foreground = \"#cdd6f4\"\nred = \"#f38ba8\"\nyellow = \"#f9e2af\"\n\
             green = \"#a6e3a1\"\ncyan = \"#94e2d5\"\nblue = \"#89b4fa\"\nmagenta = \"#f5c2e7\"\n",
            "color0 = \"#282828\"\ncolor1 = \"#cc241d\"\ncolor2 = \"#98971a\"\n\
             color3 = \"#d79921\"\ncolor4 = \"#458588\"\ncolor5 = \"#b16286\"\n\
             color6 = \"#689d6a\"\ncolor7 = \"#ebdbb2\"\ncolor8 = \"#928374\"\n\
             color15 = \"#fbf1c7\"\n",
            "bg = \"#eeeeee\"\nfg = \"#111111\"\npurple = \"#8839ef\"\nred = \"#d20f39\"\n\
             yellow = \"#df8e1d\"\ngreen = \"#40a02b\"\ncyan = \"#179299\"\nblue = \"#1e66f5\"\n\
             selection_background = \"#acb0be\"\ncursor = \"#ff0000\"\n",
        ];
        let dir = tempfile::tempdir().unwrap();
        for text in cases {
            let file = write(dir.path(), "colors.toml", text);
            assert_eq!(
                resolve_palette_with(&file, None).unwrap(),
                resolve_palette_with(&file, Some(&omarchy)).unwrap(),
                "{text}"
            );
        }
        write(dir.path(), "light.mode", "");
        let file = write(dir.path(), "colors.toml", "background = \"#101010\"\n");
        assert_eq!(
            resolve_palette_with(&file, None).unwrap()["mode"],
            resolve_palette_with(&file, Some(&omarchy)).unwrap()["mode"]
        );
    }
}
