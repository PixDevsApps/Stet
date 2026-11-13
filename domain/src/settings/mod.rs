//! Stet's two settings files in `$XDG_CONFIG_HOME/stet/` (ADR-017): `config.toml` with the
//! editor's settings ([`Settings`]) and `keys.toml` with changes to the keymap ([`Keymap`]).
//! Both are TOML and are read with positions, so every mistake is reported with its line and
//! column; a file with mistakes is not applied at all. Missing keys keep their defaults.

mod keymap;

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::num::NonZeroUsize;
use std::ops::{Range, RangeInclusive};
use std::time::Duration;

use toml::Spanned;
use toml::de::{DeTable, DeValue};

use crate::document::indent::Indentation;
use crate::text::{Line, LineNumber, Position};
use crate::view::{DEFAULT_FONT_SIZE_PT, HIGHLIGHT_MAX_BYTES, HIGHLIGHT_MAX_LINES, TabWidth};

pub use keymap::{Canonical, Keymap, default_keys_file};

/// The settings file's name in Stet's configuration folder.
pub const CONFIG_FILE: &str = "config.toml";
/// The keymap file's name in Stet's configuration folder.
pub const KEYS_FILE: &str = "keys.toml";

/// A mistake in a settings file, at a line and column (counted in characters) as people count
/// them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileError {
    /// The line the mistake is on.
    pub line: LineNumber,
    /// The column the mistake starts at, in characters.
    pub column: NonZeroUsize,
    /// What is wrong, in a sentence.
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
        let position = Position::new(
            Line::new(text[..line_start].matches('\n').count()),
            text[line_start..byte].chars().count(),
        );
        Self {
            line: position.line.number(),
            column: position.column_number(),
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
    /// The columns a tab takes, if the language sets it.
    pub tab_width: Option<TabWidth>,
    /// Whether Tab inserts spaces, if the language sets it.
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

const fn spaces(width: u8) -> LanguageIndent {
    LanguageIndent {
        tab_width: TabWidth::new(width),
        insert_spaces: Some(true),
    }
}

/// Generates [`Settings`], its `Default`, the reader of each top-level setting and the
/// entries of [`default_config_file`] from one table. A row is the field with its doc
/// comment, type and default; how `config.toml` writes it: `Text(normalise)`, `Flag` or
/// `Number(range)`; and what the default file says about it in braces: the comment lines
/// that open its group (`heading`), what it shows in place of the default (`example`) and
/// the comment after it (`note`).
macro_rules! settings {
    (@read $text:ident, $name:ident, $value:ident, Text($normalise:path)) => {
        text_value($text, $name, $value).map($normalise)
    };
    (@read $text:ident, $name:ident, $value:ident, Flag) => {
        flag_value($text, $name, $value)
    };
    (@read $text:ident, $name:ident, $value:ident, Number($range:expr)) => {
        number_value($text, $name, $value, &$range)
    };
    (@or $fallback:expr, $given:expr) => {
        $given
    };
    (@or $fallback:expr) => {
        $fallback
    };
    ($(
        $(#[$attr:meta])*
        $field:ident: $ty:ty = $default:expr, $kind:ident $(($($arg:tt)*))? {
            $(heading: [$($heading:literal),+ $(,)?],)?
            $(example: $example:literal,)?
            $(note: $note:literal,)?
        }
    )*) => {
        /// The settings in `config.toml`.
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct Settings {
            $($(#[$attr])* pub $field: $ty,)*
            /// The `[language.<id>]` tables, by GtkSourceView language id.
            pub languages: BTreeMap<String, LanguageIndent>,
        }

        impl Default for Settings {
            fn default() -> Self {
                Self {
                    $($field: $default,)*
                    languages: BTreeMap::new(),
                }
            }
        }

        /// Every top-level setting, in the order the default file lists them.
        const NAMES: &[&str] = &[$(stringify!($field)),*];

        impl Settings {
            /// Reads the top-level setting `name` into its field; `None` when there is none.
            fn read(
                &mut self,
                text: &str,
                name: &str,
                value: &Spanned<DeValue<'_>>,
            ) -> Option<Result<(), FileError>> {
                match name {
                    $(stringify!($field) => Some(
                        settings!(@read text, name, value, $kind $(($($arg)*))?)
                            .map(|read| self.$field = read),
                    ),)*
                    _ => None,
                }
            }

            /// A field's value, by setting name, for tests that go through every setting.
            #[cfg(test)]
            fn field(&self, name: &str) -> Option<String> {
                match name {
                    $(stringify!($field) => Some(format!("{:?}", self.$field)),)*
                    _ => None,
                }
            }
        }

        /// The default file's lines for the top-level settings, commented out.
        fn default_entries(out: &mut String) {
            let defaults = Settings::default();
            $(
                $(
                    out.push('\n');
                    $(let _ = writeln!(out, "# {}", $heading);)+
                )?
                default_entry(
                    out,
                    stringify!($field),
                    &settings!(@or defaults.$field.to_string() $(, String::from($example))?),
                    settings!(@or "" $(, $note)?),
                );
            )*
        }
    };
}

/// The tab widths `tab_width` takes, globally and per language.
const TAB_WIDTHS: RangeInclusive<i64> = 1..=16;

settings! {
    /// The editor font family; `None` follows Omarchy's monospace font (ADR-004).
    font: Option<String> = None, Text(font_family) {
        heading: [
            "The editor font: Omarchy's monospace font (fc-match monospace) unless you name one.",
        ],
        example: "\"JetBrainsMono Nerd Font\"",
    }
    /// The editor font size in points, before zoom.
    font_size: u32 = DEFAULT_FONT_SIZE_PT as u32, Number(4..=72) {
        note: "points; zoom (Ctrl++ and Ctrl+-) adds to it",
    }
    /// The columns a tab takes, for new documents and files whose indentation isn't detected.
    tab_width: TabWidth = Indentation::DEFAULT.tab_width, Number(TAB_WIDTHS) {
        heading: ["Indentation of new documents, and of files whose own can't be detected."],
    }
    /// Whether Tab inserts spaces, for new documents and files whose indentation isn't detected.
    insert_spaces: bool = Indentation::DEFAULT.insert_spaces, Flag {
        note: "true: Tab inserts spaces",
    }
    /// Follow the indentation an opened file already uses.
    detect_indentation: bool = true, Flag {
        note: "follow the indentation a file already uses",
    }
    /// Whether long lines wrap at the window's edge.
    word_wrap: bool = false, Flag {
        heading: ["What the View menu starts with."],
    }
    /// Whether spaces, tabs and line ends are drawn as marks.
    show_whitespace: bool = false, Flag {}
    /// Whether the document map shows beside the text; never in large-file mode.
    document_map: bool = false, Flag {
        note: "never in large-file mode",
    }
    /// Whether the other places of a selected word are highlighted.
    smart_highlight: bool = true, Flag {
        note: "highlight the other places of a selected word",
    }
    /// Count words in the status bar.
    word_count: bool = false, Flag {
        note: "count words in the status bar",
    }
    /// Whether unsaved and untitled work is backed up.
    backups: bool = true, Flag {
        heading: [
            "Backups of unsaved and untitled work. With backups off, unsaved changes are never",
            "written anywhere: closing the window asks about them, and logging out loses them.",
        ],
    }
    /// How often backups are written, in seconds.
    backup_interval: u32 = 7, Number(1..=3600) {
        note: "seconds",
    }
    /// The largest file, in bytes, that is highlighted when it opens (ADR-014).
    highlight_max_bytes: usize = HIGHLIGHT_MAX_BYTES, Number(0..=1 << 40) {
        heading: ["Syntax highlighting is off when a file over these sizes opens (ADR-014)."],
    }
    /// The most lines a file may have to be highlighted when it opens (ADR-014).
    highlight_max_lines: usize = HIGHLIGHT_MAX_LINES, Number(0..=1 << 32) {}
}

/// The font a `font` string names: trimmed, and blank follows Omarchy's font.
fn font_family(font: &str) -> Option<String> {
    Some(font.trim().to_owned()).filter(|font| !font.is_empty())
}

fn text_value<'a>(
    text: &str,
    name: &str,
    value: &'a Spanned<DeValue<'_>>,
) -> Result<&'a str, FileError> {
    match value.get_ref() {
        DeValue::String(string) => Ok(string),
        _ => Err(mismatch(text, name, value, "a string in quotes")),
    }
}

fn flag_value(text: &str, name: &str, value: &Spanned<DeValue<'_>>) -> Result<bool, FileError> {
    match value.get_ref() {
        DeValue::Boolean(flag) => Ok(*flag),
        _ => Err(mismatch(text, name, value, "true or false")),
    }
}

/// A whole number in `range`, converted to the field's type; a number the type cannot hold
/// is out of range too.
fn number_value<T: TryFrom<i64>>(
    text: &str,
    name: &str,
    value: &Spanned<DeValue<'_>>,
    range: &RangeInclusive<i64>,
) -> Result<T, FileError> {
    let DeValue::Integer(integer) = value.get_ref() else {
        return Err(mismatch(text, name, value, "a whole number"));
    };
    i64::from_str_radix(integer.as_str(), integer.radix())
        .ok()
        .filter(|number| range.contains(number))
        .and_then(|number| T::try_from(number).ok())
        .ok_or_else(|| {
            FileError::spanned(
                text,
                value.span(),
                format!("{name} must be from {} to {}", range.start(), range.end()),
            )
        })
}

fn mismatch(text: &str, name: &str, value: &Spanned<DeValue<'_>>, takes: &str) -> FileError {
    FileError::spanned(
        text,
        value.span(),
        format!(
            "{name} takes {takes}, not {}",
            article(value.get_ref().type_str())
        ),
    )
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
            match settings.read(text, name, value) {
                Some(Ok(())) => {}
                Some(Err(error)) => errors.push(error),
                None => {
                    let names = NAMES.iter().copied().chain(["language"]);
                    let hint = closest(name, names)
                        .map(|near| format!("; did you mean {near}?"))
                        .unwrap_or_default();
                    errors.push(FileError::spanned(
                        text,
                        key.span(),
                        format!("unknown setting {name}{hint}"),
                    ));
                }
            }
        }
        if errors.is_empty() {
            Ok(settings)
        } else {
            Err(errors)
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
                let read = match name {
                    "tab_width" => number_value(text, name, value, &TAB_WIDTHS)
                        .map(|width| indent.tab_width = Some(width)),
                    "insert_spaces" => {
                        flag_value(text, name, value).map(|on| indent.insert_spaces = Some(on))
                    }
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
                if let Err(error) = read {
                    errors.push(error);
                }
            }
            self.languages.insert(id.get_ref().to_string(), indent);
        }
    }

    /// The indentation of a document in `language` (a GtkSourceView id): the global settings,
    /// then Stet's defaults for the language, then the file's `[language.<id>]`.
    #[must_use]
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
    #[must_use]
    pub fn highlight_by_default(&self, bytes: usize, lines: usize) -> bool {
        bytes <= self.highlight_max_bytes && lines <= self.highlight_max_lines
    }

    /// How often backups are written, or `None` when they are off.
    #[must_use]
    pub fn backup_every(&self) -> Option<Duration> {
        self.backups
            .then(|| Duration::from_secs(u64::from(self.backup_interval)))
    }
}

/// The `config.toml` that Open Settings creates: every setting commented out with its
/// default, so the file changes nothing until a line is uncommented.
#[must_use]
pub fn default_config_file() -> String {
    let mut out = String::from(
        "# Stet settings (config.toml). Uncomment a line and change it; Stet applies the file\n\
         # when you save it. A mistake is shown with its line and column, and the previous\n\
         # settings stay until the file is fixed.\n",
    );
    default_entries(&mut out);
    out.push_str(
        "\n# Indentation per language, by GtkSourceView language id (the status bar's language\n\
         # picker lists the languages). These win over tab_width and insert_spaces; Stet's own\n\
         # defaults are listed.\n",
    );
    for (id, indent) in LANGUAGE_INDENTS {
        let _ = writeln!(out, "# [language.{id}]");
        if let Some(width) = indent.tab_width {
            let _ = writeln!(out, "# tab_width = {width}");
        }
        if let Some(spaces) = indent.insert_spaces {
            let _ = writeln!(out, "# insert_spaces = {spaces}");
        }
    }
    out
}

/// One setting's line in the default file, with its note aligned after it.
fn default_entry(out: &mut String, name: &str, shown: &str, note: &str) {
    let setting = format!("{name} = {shown}");
    if note.is_empty() {
        let _ = writeln!(out, "# {setting}");
    } else {
        let _ = writeln!(out, "# {setting:<34} # {note}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn width(cells: u8) -> TabWidth {
        TabWidth::new(cells).unwrap()
    }

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
            (13, width(2), true)
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
                tab_width: Some(width(2)),
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
        assert_eq!((error.line.get(), error.column.get()), (2, 2));
        let error = FileError::at("ø", 1, "inside a character");
        assert_eq!((error.line.get(), error.column.get()), (1, 1));
    }

    #[test]
    fn indentation_layers() {
        let mut settings = Settings::parse("insert_spaces = true\ntab_width = 3\n").unwrap();
        assert_eq!(
            settings.indentation_for(None),
            Indentation {
                tab_width: width(3),
                insert_spaces: true
            }
        );
        // Stet's language defaults win over the global settings: a Makefile keeps its tabs.
        assert_eq!(
            settings.indentation_for(Some("makefile")),
            Indentation {
                tab_width: width(3),
                insert_spaces: false
            }
        );
        assert_eq!(
            settings.indentation_for(Some("yaml")),
            Indentation {
                tab_width: width(2),
                insert_spaces: true
            }
        );
        settings.languages.insert(
            "yaml".to_owned(),
            LanguageIndent {
                tab_width: Some(width(4)),
                insert_spaces: None,
            },
        );
        assert_eq!(
            settings.indentation_for(Some("yaml")),
            Indentation {
                tab_width: width(4),
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
        assert!(!settings.highlight_by_default(1024, HIGHLIGHT_MAX_LINES + 1));
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
            .flat_map(|line| [line, "\n"])
            .collect();
        let settings = Settings::parse(&uncommented).unwrap();
        assert_eq!(settings.font.as_deref(), Some("JetBrainsMono Nerd Font"));
        assert_eq!(settings.tab_width, Settings::default().tab_width);
        assert_eq!(settings.languages.len(), LANGUAGE_INDENTS.len());
        for (id, indent) in LANGUAGE_INDENTS {
            assert_eq!(settings.languages[*id], *indent, "{id}");
        }
    }

    /// Other values for what the default file shows: the other flag, another string, or a
    /// neighbouring number.
    fn other_values(shown: &str) -> Vec<String> {
        match shown {
            "true" => vec!["false".to_owned()],
            "false" => vec!["true".to_owned()],
            _ if shown.starts_with('"') => vec!["\"Iosevka\"".to_owned()],
            _ => {
                let number: i64 = shown.parse().unwrap();
                vec![(number + 1).to_string(), (number - 1).to_string()]
            }
        }
    }

    #[test]
    fn every_setting_is_in_the_default_file_and_reads_back() {
        let file = default_config_file();
        let defaults = Settings::default();
        for name in NAMES {
            let prefix = format!("# {name} = ");
            let line = file
                .lines()
                .find(|line| line.starts_with(&prefix))
                .unwrap_or_else(|| panic!("{name} is not in the default file"));
            let shown = line[prefix.len()..].split(" #").next().unwrap().trim_end();
            let (value, settings) = other_values(shown)
                .into_iter()
                .find_map(|value| {
                    let settings = Settings::parse(&format!("{name} = {value}\n")).ok()?;
                    Some((value, settings))
                })
                .unwrap_or_else(|| panic!("{name} takes no other value than {shown}"));
            let read = settings.field(name).unwrap();
            assert_ne!(read, defaults.field(name).unwrap(), "{name}");
            assert!(read.contains(value.trim_matches('"')), "{name}: {read}");
            for other in NAMES.iter().filter(|other| *other != name) {
                assert_eq!(settings.field(other), defaults.field(other), "{name}");
            }
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
