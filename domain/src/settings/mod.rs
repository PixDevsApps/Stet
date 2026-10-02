//! Stet's two settings files in `$XDG_CONFIG_HOME/stet/` (ADR-017): `config.toml` with the
//! editor's settings ([`Settings`]) and `keys.toml` with changes to the keymap ([`Keymap`]).
//! Both are TOML and are read with positions, so every mistake is reported with its line and
//! column; a file with mistakes is not applied at all. Missing keys keep their defaults.

pub mod keymap;

use std::collections::BTreeMap;
use std::fmt;
use std::ops::Range;
use std::time::Duration;

use toml::Spanned;
use toml::de::{DeTable, DeValue};

use crate::indent::Indentation;
use crate::view::{DEFAULT_FONT_SIZE_PT, HIGHLIGHT_MAX_BYTES, HIGHLIGHT_MAX_LINES};

pub use keymap::Keymap;

pub const CONFIG_FILE: &str = "config.toml";
pub const KEYS_FILE: &str = "keys.toml";

/// A mistake in a settings file, at a 1-based line and column (counted in characters).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileError {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl FileError {
    /// The error at byte `byte` of `text`.
    pub fn at(text: &str, byte: usize, message: impl Into<String>) -> Self {
        let mut byte = byte.min(text.len());
        while !text.is_char_boundary(byte) {
            byte -= 1;
        }
        let line_start = text[..byte].rfind('\n').map_or(0, |at| at + 1);
        Self {
            line: text[..line_start].matches('\n').count() + 1,
            column: text[line_start..byte].chars().count() + 1,
            message: message.into(),
        }
    }

    fn spanned(text: &str, span: Range<usize>, message: impl Into<String>) -> Self {
        Self::at(text, span.start, message)
    }
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "line {}, column {}: {}",
            self.line, self.column, self.message
        )
    }
}

impl std::error::Error for FileError {}

/// Parses TOML with positions; a syntax error becomes a [`FileError`].
fn parse_toml(text: &str) -> Result<Spanned<DeTable<'_>>, FileError> {
    DeTable::parse(text).map_err(|error| {
        let message = error.message().lines().next().unwrap_or("invalid TOML");
        FileError::at(text, error.span().map_or(0, |span| span.start), message)
    })
}

/// A table's entries in the order the file lists them.
fn entries<'a, 'i>(
    table: &'a DeTable<'i>,
) -> Vec<(
    &'a Spanned<std::borrow::Cow<'i, str>>,
    &'a Spanned<DeValue<'i>>,
)> {
    let mut entries: Vec<_> = table.iter().collect();
    entries.sort_by_key(|(key, _)| key.span().start);
    entries
}

/// The candidate closest to `word`, when it is a likely typo of it.
fn closest<'a>(word: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let limit = (word.chars().count() / 3).clamp(1, 3);
    candidates
        .into_iter()
        .map(|candidate| (edit_distance(word, candidate), candidate))
        .filter(|(distance, _)| *distance <= limit)
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, candidate)| candidate)
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (diagonal + usize::from(ca != *cb))
                .min(row[j] + 1)
                .min(above + 1);
            diagonal = above;
        }
    }
    row[b.len()]
}

/// Indentation for one language in `[language.<id>]`; what it leaves out comes from Stet's
/// defaults for the language, then from the global settings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LanguageIndent {
    pub tab_width: Option<u32>,
    pub insert_spaces: Option<bool>,
}

/// Stet's own indentation defaults per GtkSourceView language id: Makefiles need tabs, Go's
/// gofmt uses them, Python and Rust use four spaces, YAML, JSON and XML two. They win over
/// the global `tab_width` and `insert_spaces`, so `insert_spaces = true` never breaks a
/// Makefile; `[language.<id>]` wins over them.
pub const LANGUAGE_INDENTS: &[(&str, LanguageIndent)] = &[
    ("makefile", tabs()),
    ("go", tabs()),
    ("python", spaces(4)),
    ("python3", spaces(4)),
    ("rust", spaces(4)),
    ("yaml", spaces(2)),
    ("json", spaces(2)),
    ("xml", spaces(2)),
];

const fn tabs() -> LanguageIndent {
    LanguageIndent {
        tab_width: None,
        insert_spaces: Some(false),
    }
}

const fn spaces(width: u32) -> LanguageIndent {
    LanguageIndent {
        tab_width: Some(width),
        insert_spaces: Some(true),
    }
}

/// The settings in `config.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The editor font family; `None` follows Omarchy's monospace font (ADR-004).
    pub font: Option<String>,
    /// The editor font size in points, before zoom.
    pub font_size: u32,
    pub tab_width: u32,
    pub insert_spaces: bool,
    /// Follow the indentation an opened file already uses.
    pub detect_indentation: bool,
    pub word_wrap: bool,
    pub show_whitespace: bool,
    pub document_map: bool,
    pub smart_highlight: bool,
    /// Count words in the status bar.
    pub word_count: bool,
    /// Backups of unsaved work (M2) and how often they are written, in seconds.
    pub backups: bool,
    pub backup_interval: u32,
    /// ADR-014's limits for highlighting a file when it opens.
    pub highlight_max_bytes: usize,
    pub highlight_max_lines: usize,
    pub languages: BTreeMap<String, LanguageIndent>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            font: None,
            font_size: DEFAULT_FONT_SIZE_PT as u32,
            tab_width: Indentation::DEFAULT.tab_width,
            insert_spaces: Indentation::DEFAULT.insert_spaces,
            detect_indentation: true,
            word_wrap: false,
            show_whitespace: false,
            document_map: false,
            smart_highlight: true,
            word_count: false,
            backups: true,
            backup_interval: 7,
            highlight_max_bytes: HIGHLIGHT_MAX_BYTES,
            highlight_max_lines: HIGHLIGHT_MAX_LINES,
            languages: BTreeMap::new(),
        }
    }
}

/// The value a setting takes.
#[derive(Debug, Clone, Copy)]
enum Kind {
    Text,
    Flag,
    Number { min: i64, max: i64 },
}

/// Every top-level setting with its kind, in the order the default file lists them.
const SETTINGS: &[(&str, Kind)] = &[
    ("font", Kind::Text),
    ("font_size", Kind::Number { min: 4, max: 72 }),
    ("tab_width", Kind::Number { min: 1, max: 16 }),
    ("insert_spaces", Kind::Flag),
    ("detect_indentation", Kind::Flag),
    ("word_wrap", Kind::Flag),
    ("show_whitespace", Kind::Flag),
    ("document_map", Kind::Flag),
    ("smart_highlight", Kind::Flag),
    ("word_count", Kind::Flag),
    ("backups", Kind::Flag),
    ("backup_interval", Kind::Number { min: 1, max: 3600 }),
    (
        "highlight_max_bytes",
        Kind::Number {
            min: 0,
            max: 1 << 40,
        },
    ),
    (
        "highlight_max_lines",
        Kind::Number {
            min: 0,
            max: 1 << 32,
        },
    ),
];

/// A setting's value, checked against its kind.
enum Value {
    Text(String),
    Flag(bool),
    Number(i64),
}

fn value_of(
    text: &str,
    name: &str,
    kind: Kind,
    value: &Spanned<DeValue<'_>>,
) -> Result<Value, FileError> {
    let error = |message: String| FileError::spanned(text, value.span(), message);
    match (kind, value.get_ref()) {
        (Kind::Text, DeValue::String(string)) => Ok(Value::Text(string.to_string())),
        (Kind::Flag, DeValue::Boolean(flag)) => Ok(Value::Flag(*flag)),
        (Kind::Number { min, max }, DeValue::Integer(integer)) => {
            match i64::from_str_radix(integer.as_str(), integer.radix()) {
                Ok(number) if (min..=max).contains(&number) => Ok(Value::Number(number)),
                _ => Err(error(format!("{name} must be from {min} to {max}"))),
            }
        }
        (Kind::Text, other) => Err(error(format!(
            "{name} takes a string in quotes, not {}",
            article(other.type_str())
        ))),
        (Kind::Flag, other) => Err(error(format!(
            "{name} takes true or false, not {}",
            article(other.type_str())
        ))),
        (Kind::Number { .. }, other) => Err(error(format!(
            "{name} takes a whole number, not {}",
            article(other.type_str())
        ))),
    }
}

fn article(kind: &str) -> String {
    match kind.chars().next() {
        Some('a' | 'e' | 'i' | 'o' | 'u') => format!("an {kind}"),
        _ => format!("a {kind}"),
    }
}

impl Settings {
    /// Reads `config.toml`. Every mistake is listed, in the order of the file.
    pub fn parse(text: &str) -> Result<Self, Vec<FileError>> {
        let root = parse_toml(text).map_err(|error| vec![error])?;
        let mut settings = Self::default();
        let mut errors = Vec::new();
        for (key, value) in entries(root.get_ref()) {
            let name = key.get_ref().as_ref();
            if name == "language" {
                settings.read_languages(text, value, &mut errors);
                continue;
            }
            let Some(&(_, kind)) = SETTINGS.iter().find(|(setting, _)| *setting == name) else {
                let names = SETTINGS.iter().map(|(name, _)| *name).chain(["language"]);
                let hint = closest(name, names)
                    .map(|near| format!("; did you mean {near}?"))
                    .unwrap_or_default();
                errors.push(FileError::spanned(
                    text,
                    key.span(),
                    format!("unknown setting {name}{hint}"),
                ));
                continue;
            };
            match value_of(text, name, kind, value) {
                Ok(value) => settings.set(name, value),
                Err(error) => errors.push(error),
            }
        }
        if errors.is_empty() {
            Ok(settings)
        } else {
            Err(errors)
        }
    }

    fn set(&mut self, name: &str, value: Value) {
        match (name, value) {
            ("font", Value::Text(font)) => {
                self.font = Some(font.trim().to_owned()).filter(|font| !font.is_empty());
            }
            ("font_size", Value::Number(size)) => self.font_size = size as u32,
            ("tab_width", Value::Number(width)) => self.tab_width = width as u32,
            ("insert_spaces", Value::Flag(on)) => self.insert_spaces = on,
            ("detect_indentation", Value::Flag(on)) => self.detect_indentation = on,
            ("word_wrap", Value::Flag(on)) => self.word_wrap = on,
            ("show_whitespace", Value::Flag(on)) => self.show_whitespace = on,
            ("document_map", Value::Flag(on)) => self.document_map = on,
            ("smart_highlight", Value::Flag(on)) => self.smart_highlight = on,
            ("word_count", Value::Flag(on)) => self.word_count = on,
            ("backups", Value::Flag(on)) => self.backups = on,
            ("backup_interval", Value::Number(seconds)) => self.backup_interval = seconds as u32,
            ("highlight_max_bytes", Value::Number(bytes)) => {
                self.highlight_max_bytes = bytes as usize;
            }
            ("highlight_max_lines", Value::Number(lines)) => {
                self.highlight_max_lines = lines as usize;
            }
            _ => unreachable!("{name} is checked against SETTINGS"),
        }
    }

    fn read_languages(
        &mut self,
        text: &str,
        value: &Spanned<DeValue<'_>>,
        errors: &mut Vec<FileError>,
    ) {
        let Some(table) = value.get_ref().as_table() else {
            errors.push(FileError::spanned(
                text,
                value.span(),
                "language takes tables, like [language.python]",
            ));
            return;
        };
        for (id, settings) in entries(table) {
            let Some(inner) = settings.get_ref().as_table() else {
                errors.push(FileError::spanned(
                    text,
                    settings.span(),
                    format!(
                        "language.{} takes a table, like [language.python]",
                        id.get_ref()
                    ),
                ));
                continue;
            };
            let mut indent = LanguageIndent::default();
            for (key, value) in entries(inner) {
                let name = key.get_ref().as_ref();
                let kind = match name {
                    "tab_width" => Kind::Number { min: 1, max: 16 },
                    "insert_spaces" => Kind::Flag,
                    _ => {
                        let hint = closest(name, ["tab_width", "insert_spaces"])
                            .map(|near| format!("; did you mean {near}?"))
                            .unwrap_or_default();
                        errors.push(FileError::spanned(
                            text,
                            key.span(),
                            format!(
                                "unknown language setting {name}{hint} (a language has \
                                 tab_width and insert_spaces)"
                            ),
                        ));
                        continue;
                    }
                };
                match value_of(text, name, kind, value) {
                    Ok(Value::Number(width)) => indent.tab_width = Some(width as u32),
                    Ok(Value::Flag(on)) => indent.insert_spaces = Some(on),
                    Ok(Value::Text(_)) => {}
                    Err(error) => errors.push(error),
                }
            }
            self.languages.insert(id.get_ref().to_string(), indent);
        }
    }

    /// The indentation of a document in `language` (a GtkSourceView id): the global settings,
    /// then Stet's defaults for the language, then the file's `[language.<id>]`.
    pub fn indentation_for(&self, language: Option<&str>) -> Indentation {
        let mut indentation = Indentation {
            tab_width: self.tab_width,
            insert_spaces: self.insert_spaces,
        };
        let Some(language) = language else {
            return indentation;
        };
        let builtin = LANGUAGE_INDENTS
            .iter()
            .find(|(id, _)| *id == language)
            .map(|(_, indent)| *indent);
        for indent in [builtin, self.languages.get(language).copied()]
            .into_iter()
            .flatten()
        {
            if let Some(width) = indent.tab_width {
                indentation.tab_width = width;
            }
            if let Some(spaces) = indent.insert_spaces {
                indentation.insert_spaces = spaces;
            }
        }
        indentation
    }

    /// Whether a file of `bytes` bytes and `lines` lines is highlighted when it opens.
    pub fn highlight_by_default(&self, bytes: usize, lines: usize) -> bool {
        bytes <= self.highlight_max_bytes && lines <= self.highlight_max_lines
    }

    /// How often M2 writes backups, or `None` when they are off.
    pub fn backup_every(&self) -> Option<Duration> {
        self.backups
            .then(|| Duration::from_secs(u64::from(self.backup_interval)))
    }
}

/// The `config.toml` that Open Settings creates: every setting commented out with its
/// default, so the file changes nothing until a line is uncommented.
pub fn default_config_file() -> String {
    let defaults = Settings::default();
    let mut out = String::from(
        "# Stet settings (config.toml). Uncomment a line and change it; Stet applies the file\n\
         # when you save it. A mistake is shown with its line and column, and the previous\n\
         # settings stay until the file is fixed.\n\
         \n\
         # The editor font: Omarchy's monospace font (fc-match monospace) unless you name one.\n\
         # font = \"JetBrainsMono Nerd Font\"\n",
    );
    let line = |out: &mut String, setting: String, note: &str| {
        if note.is_empty() {
            out.push_str(&format!("# {setting}\n"));
        } else {
            out.push_str(&format!("# {setting:<34} # {note}\n"));
        }
    };
    line(
        &mut out,
        format!("font_size = {}", defaults.font_size),
        "points; zoom (Ctrl++ and Ctrl+-) adds to it",
    );
    out.push_str("\n# Indentation of new documents, and of files whose own can't be detected.\n");
    line(&mut out, format!("tab_width = {}", defaults.tab_width), "");
    line(
        &mut out,
        format!("insert_spaces = {}", defaults.insert_spaces),
        "true: Tab inserts spaces",
    );
    line(
        &mut out,
        format!("detect_indentation = {}", defaults.detect_indentation),
        "follow the indentation a file already uses",
    );
    out.push_str("\n# What the View menu starts with.\n");
    line(&mut out, format!("word_wrap = {}", defaults.word_wrap), "");
    line(
        &mut out,
        format!("show_whitespace = {}", defaults.show_whitespace),
        "",
    );
    line(
        &mut out,
        format!("document_map = {}", defaults.document_map),
        "never in large-file mode",
    );
    line(
        &mut out,
        format!("smart_highlight = {}", defaults.smart_highlight),
        "highlight the other places of a selected word",
    );
    line(
        &mut out,
        format!("word_count = {}", defaults.word_count),
        "count words in the status bar",
    );
    out.push_str(
        "\n# Backups of unsaved and untitled work. With backups off, unsaved changes are never\n\
         # written anywhere: closing the window asks about them, and logging out loses them.\n",
    );
    line(&mut out, format!("backups = {}", defaults.backups), "");
    line(
        &mut out,
        format!("backup_interval = {}", defaults.backup_interval),
        "seconds",
    );
    out.push_str("\n# Syntax highlighting is off when a file over these sizes opens (ADR-014).\n");
    line(
        &mut out,
        format!("highlight_max_bytes = {}", defaults.highlight_max_bytes),
        "",
    );
    line(
        &mut out,
        format!("highlight_max_lines = {}", defaults.highlight_max_lines),
        "",
    );
    out.push_str(
        "\n# Indentation per language, by GtkSourceView language id (the status bar's language\n\
         # picker lists the languages). These win over tab_width and insert_spaces; Stet's own\n\
         # defaults are listed.\n",
    );
    for (id, indent) in LANGUAGE_INDENTS {
        out.push_str(&format!("# [language.{id}]\n"));
        if let Some(width) = indent.tab_width {
            out.push_str(&format!("# tab_width = {width}\n"));
        }
        if let Some(spaces) = indent.insert_spaces {
            out.push_str(&format!("# insert_spaces = {spaces}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn errors(text: &str) -> Vec<String> {
        Settings::parse(text)
            .unwrap_err()
            .into_iter()
            .map(|error| error.to_string())
            .collect()
    }

    #[test]
    fn an_empty_file_is_the_defaults() {
        assert_eq!(Settings::parse(""), Ok(Settings::default()));
        assert_eq!(
            Settings::parse("# only comments\n"),
            Ok(Settings::default())
        );
    }

    #[test]
    fn reads_every_setting() {
        let text = "font = \"Iosevka\"\nfont_size = 13\ntab_width = 2\ninsert_spaces = true\n\
                    detect_indentation = false\nword_wrap = true\nshow_whitespace = true\n\
                    document_map = true\nsmart_highlight = false\nword_count = true\n\
                    backups = false\nbackup_interval = 30\nhighlight_max_bytes = 1_000\n\
                    highlight_max_lines = 0x10\n\n[language.python]\ntab_width = 2\n\n\
                    [language.c]\ninsert_spaces = true\n";
        let settings = Settings::parse(text).unwrap();
        assert_eq!(settings.font.as_deref(), Some("Iosevka"));
        assert_eq!(
            (
                settings.font_size,
                settings.tab_width,
                settings.insert_spaces
            ),
            (13, 2, true)
        );
        assert!(!settings.detect_indentation && settings.word_wrap && settings.show_whitespace);
        assert!(settings.document_map && !settings.smart_highlight && settings.word_count);
        assert_eq!(settings.backup_every(), None);
        assert_eq!(settings.backup_interval, 30);
        assert_eq!(
            (settings.highlight_max_bytes, settings.highlight_max_lines),
            (1000, 16)
        );
        assert_eq!(
            settings.languages["python"],
            LanguageIndent {
                tab_width: Some(2),
                insert_spaces: None
            }
        );
        assert_eq!(settings.languages["c"].insert_spaces, Some(true));
    }

    #[test]
    fn an_empty_font_follows_omarchy() {
        let settings = Settings::parse("font = \"  \"\n").unwrap();
        assert_eq!(settings.font, None);
    }

    #[test]
    fn mistakes_say_where_they_are() {
        assert_eq!(
            errors("tab_width = 4\ntab_widht = 2\n"),
            ["line 2, column 1: unknown setting tab_widht; did you mean tab_width?"]
        );
        assert_eq!(
            errors("tab_width = \"4\"\n"),
            ["line 1, column 13: tab_width takes a whole number, not a string"]
        );
        assert_eq!(
            errors("word_wrap = 1\n"),
            ["line 1, column 13: word_wrap takes true or false, not an integer"]
        );
        assert_eq!(
            errors("tab_width = 0\n"),
            ["line 1, column 13: tab_width must be from 1 to 16"]
        );
        assert_eq!(
            errors("[language.python]\ntabs = 2\n"),
            [
                "line 2, column 1: unknown language setting tabs (a language has tab_width and \
                 insert_spaces)"
            ]
        );
        assert_eq!(
            errors("language = 3\n"),
            ["line 1, column 12: language takes tables, like [language.python]"]
        );
    }

    #[test]
    fn every_mistake_is_listed_in_file_order() {
        let found = errors("zzz = 1\nfont_size = 900\nword_wrap = \"yes\"\n");
        assert_eq!(found.len(), 3);
        assert!(found[0].starts_with("line 1, column 1: unknown setting zzz"));
        assert!(found[1].starts_with("line 2,"));
        assert!(found[2].starts_with("line 3,"));
    }

    #[test]
    fn syntax_errors_have_positions() {
        let found = errors("tab_width = 4\nword_wrap = \n");
        assert_eq!(found.len(), 1);
        assert!(found[0].starts_with("line 2, column 13:"), "{found:?}");
        let found = errors("font = \"unterminated\n");
        assert!(found[0].starts_with("line 1,"), "{found:?}");
        let found = errors("tab_width = 4\ntab_width = 8\n");
        assert!(found[0].starts_with("line 2,"), "{found:?}");
    }

    #[test]
    fn positions_count_characters() {
        let error = FileError::at("ø = 1\nab", 8, "x");
        assert_eq!((error.line, error.column), (2, 2));
        let error = FileError::at("ø", 1, "inside a character");
        assert_eq!((error.line, error.column), (1, 1));
    }

    #[test]
    fn indentation_layers() {
        let mut settings = Settings::parse("insert_spaces = true\ntab_width = 3\n").unwrap();
        assert_eq!(
            settings.indentation_for(None),
            Indentation {
                tab_width: 3,
                insert_spaces: true
            }
        );
        // Stet's language defaults win over the global settings: a Makefile keeps its tabs.
        assert_eq!(
            settings.indentation_for(Some("makefile")),
            Indentation {
                tab_width: 3,
                insert_spaces: false
            }
        );
        assert_eq!(
            settings.indentation_for(Some("yaml")),
            Indentation {
                tab_width: 2,
                insert_spaces: true
            }
        );
        settings.languages.insert(
            "yaml".to_owned(),
            LanguageIndent {
                tab_width: Some(4),
                insert_spaces: None,
            },
        );
        assert_eq!(
            settings.indentation_for(Some("yaml")),
            Indentation {
                tab_width: 4,
                insert_spaces: true
            }
        );
        assert_eq!(
            Settings::default().indentation_for(Some("c")),
            Indentation::DEFAULT
        );
    }

    #[test]
    fn highlight_limits_and_backups() {
        let settings = Settings::default();
        assert!(settings.highlight_by_default(HIGHLIGHT_MAX_BYTES, HIGHLIGHT_MAX_LINES));
        assert!(!settings.highlight_by_default(HIGHLIGHT_MAX_BYTES + 1, 1));
        assert_eq!(settings.backup_every(), Some(Duration::from_secs(7)));
    }

    #[test]
    fn the_default_file_changes_nothing_and_every_line_is_valid() {
        let file = default_config_file();
        assert_eq!(Settings::parse(&file), Ok(Settings::default()));
        let uncommented: String = file
            .lines()
            .filter_map(|line| line.strip_prefix("# "))
            .filter(|line| {
                line.starts_with('[')
                    || line
                        .split_once(" = ")
                        .is_some_and(|(key, _)| !key.contains(' '))
            })
            .map(|line| format!("{line}\n"))
            .collect();
        let settings = Settings::parse(&uncommented).unwrap();
        assert_eq!(settings.font.as_deref(), Some("JetBrainsMono Nerd Font"));
        assert_eq!(settings.tab_width, Settings::default().tab_width);
        assert_eq!(settings.languages.len(), LANGUAGE_INDENTS.len());
        for (id, indent) in LANGUAGE_INDENTS {
            assert_eq!(settings.languages[*id], *indent, "{id}");
        }
    }

    #[test]
    fn typos_get_suggestions() {
        assert_eq!(
            closest("wordwrap", ["word_wrap", "font"]),
            Some("word_wrap")
        );
        assert_eq!(closest("zzz", ["word_wrap", "font"]), None);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }
}
