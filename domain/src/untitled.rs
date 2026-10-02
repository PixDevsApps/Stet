//! Names for untitled documents (M8): the tab shows the first line that has text, and Save As
//! proposes a file name made from it.

/// How many characters of the first line a tab shows before an ellipsis.
pub const FIRST_LINE_CHARS: usize = 30;

/// How many lines the name looks through for one that isn't blank.
pub const FIRST_LINE_SCAN: usize = 100;

/// How many characters of each line the name needs at most: enough for
/// [`FIRST_LINE_CHARS`] after white space at the start and between words is folded.
pub const FIRST_LINE_READ: usize = 256;

/// The longest proposed file name, in bytes, below the 255 that Linux file systems allow.
const MAX_FILE_NAME_BYTES: usize = 200;

/// The name of an untitled document whose text starts with `lines`: the first of the first
/// [`FIRST_LINE_SCAN`] lines that isn't blank, with control characters as spaces, runs of
/// white space folded into one and both ends trimmed, cut to [`FIRST_LINE_CHARS`] characters
/// with an ellipsis. `None` when they are all blank: the tab keeps its "Untitled N".
pub fn first_line_name<'a>(lines: impl IntoIterator<Item = &'a str>) -> Option<String> {
    lines
        .into_iter()
        .take(FIRST_LINE_SCAN)
        .find_map(|line| fold(line, FIRST_LINE_CHARS + 1))
        .map(|name| {
            if name.chars().count() > FIRST_LINE_CHARS {
                let cut: String = name.chars().take(FIRST_LINE_CHARS).collect();
                format!("{}…", cut.trim_end())
            } else {
                name
            }
        })
}

/// `line` with control characters as spaces, white space folded and the start trimmed, at
/// most `limit` characters; `None` when blank. It ends in a space only when that space is the
/// last character the limit allows.
fn fold(line: &str, limit: usize) -> Option<String> {
    let mut out = String::new();
    let mut count = 0;
    let mut pending_space = false;
    for c in line.chars() {
        if count >= limit {
            break;
        }
        if c.is_whitespace() || c.is_control() {
            pending_space = count > 0;
            continue;
        }
        if pending_space {
            out.push(' ');
            count += 1;
            pending_space = false;
            if count >= limit {
                break;
            }
        }
        out.push(c);
        count += 1;
    }
    (count > 0).then_some(out)
}

/// The file name Save As proposes for an untitled document called `name`: characters that
/// file managers and other systems reject (`/ \ : * ? " < > |` and control characters) become
/// spaces, white space is folded, a trailing ellipsis and dots or spaces at either end go, and
/// `extension` (without its dot) is added, `txt` when there is none. `None` when nothing is
/// left of the name.
pub fn save_file_name(name: &str, extension: Option<&str>) -> Option<String> {
    let cleaned: String = name
        .trim_end_matches('…')
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                ' '
            } else {
                c
            }
        })
        .collect();
    let folded = fold(&cleaned, usize::MAX)?;
    let stem = folded.trim_matches(|c: char| c == '.' || c == ' ');
    let extension = extension
        .map(|extension| extension.trim_start_matches('.'))
        .filter(|extension| !extension.is_empty())
        .unwrap_or("txt");
    let suffix = format!(".{extension}");
    // A name that already ends with the extension keeps it, in its own case.
    let split = stem
        .len()
        .checked_sub(suffix.len())
        .filter(|&at| at > 0 && stem.is_char_boundary(at))
        .filter(|&at| stem[at..].eq_ignore_ascii_case(&suffix));
    let (base, suffix) = match split {
        Some(at) => (&stem[..at], &stem[at..]),
        None => (stem, suffix.as_str()),
    };
    let base = clip(base, MAX_FILE_NAME_BYTES - suffix.len()).trim_end_matches(['.', ' ']);
    if base.is_empty() {
        return None;
    }
    Some(format!("{base}{suffix}"))
}

/// `text` cut to at most `bytes` bytes at a character boundary.
fn clip(text: &str, bytes: usize) -> &str {
    &text[..text.floor_char_boundary(bytes)]
}

/// The extension that a GtkSourceView language's file-name globs give, such as `rs` for
/// `*.rs`: the first glob that is `*.` followed by letters, digits, `_`, `-` or `+`.
pub fn extension_from_globs<'a>(globs: impl IntoIterator<Item = &'a str>) -> Option<String> {
    globs.into_iter().find_map(|glob| {
        let extension = glob.strip_prefix("*.")?;
        (!extension.is_empty()
            && extension
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+')))
        .then(|| extension.to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn name(text: &str) -> Option<String> {
        first_line_name(text.lines())
    }

    #[test]
    fn the_first_line_with_text_names_the_tab() {
        assert_eq!(
            name("Shopping list\nmilk\n").as_deref(),
            Some("Shopping list")
        );
        assert_eq!(
            name("\n   \n\t\n  fn main() {\n").as_deref(),
            Some("fn main() {")
        );
        assert_eq!(name("a\t\tb   c").as_deref(), Some("a b c"));
        assert_eq!(name("").as_deref(), None);
        assert_eq!(name("\n \n\t\n").as_deref(), None);
        assert_eq!(name("␀ x").as_deref(), Some("␀ x"));
    }

    #[test]
    fn long_lines_are_cut_with_an_ellipsis() {
        let exactly = "x".repeat(FIRST_LINE_CHARS);
        assert_eq!(name(&exactly), Some(exactly.clone()));
        let longer = format!("{exactly}y");
        assert_eq!(name(&longer), Some(format!("{exactly}…")));
        assert_eq!(
            name("The quick brown fox jumps over the lazy dog").as_deref(),
            Some("The quick brown fox jumps over…")
        );
        // A cut right after a space doesn't leave the space before the ellipsis.
        assert_eq!(
            name("abcdefghijklmnopqrstuvwxyz012 tail").as_deref(),
            Some("abcdefghijklmnopqrstuvwxyz012…")
        );
    }

    #[test]
    fn only_the_first_lines_are_looked_at() {
        let text = format!("{}found", "\n".repeat(FIRST_LINE_SCAN));
        assert_eq!(name(&text), None);
        let text = format!("{}found", "\n".repeat(FIRST_LINE_SCAN - 1));
        assert_eq!(name(&text).as_deref(), Some("found"));
    }

    #[test]
    fn save_as_proposes_a_clean_file_name() {
        assert_eq!(
            save_file_name("Shopping list", None).as_deref(),
            Some("Shopping list.txt")
        );
        assert_eq!(
            save_file_name("fn main() {", Some("rs")).as_deref(),
            Some("fn main() {.rs")
        );
        assert_eq!(
            save_file_name("TODO: a/b \\ c?", None).as_deref(),
            Some("TODO a b c.txt")
        );
        assert_eq!(
            save_file_name("The quick brown fox jumps over…", None).as_deref(),
            Some("The quick brown fox jumps over.txt")
        );
        assert_eq!(
            save_file_name("...hidden..", None).as_deref(),
            Some("hidden.txt")
        );
        assert_eq!(
            save_file_name("notes.txt", None).as_deref(),
            Some("notes.txt")
        );
        assert_eq!(
            save_file_name("NOTES.TXT", None).as_deref(),
            Some("NOTES.TXT")
        );
        assert_eq!(
            save_file_name("main.rs", Some(".rs")).as_deref(),
            Some("main.rs")
        );
        assert_eq!(save_file_name("a", Some("")).as_deref(), Some("a.txt"));
        assert_eq!(save_file_name("/// ::", None), None);
        assert_eq!(save_file_name("…", None), None);
        let long = "é".repeat(300);
        let proposed = save_file_name(&long, None).unwrap();
        assert!(proposed.len() <= MAX_FILE_NAME_BYTES, "{}", proposed.len());
        assert!(proposed.ends_with(".txt"));
    }

    #[test]
    fn extensions_come_from_the_first_plain_glob() {
        assert_eq!(
            extension_from_globs(["Makefile", "*.rs"]).as_deref(),
            Some("rs")
        );
        assert_eq!(
            extension_from_globs(["*.[ch]", "*.c++", "*.cc"]).as_deref(),
            Some("c++")
        );
        assert_eq!(extension_from_globs(["*.", "Dockerfile"]), None);
        assert_eq!(extension_from_globs([]), None);
    }

    proptest! {
        #[test]
        fn names_are_short_single_lines(text in "(\\PC|\n|\t|\r){0,200}") {
            if let Some(name) = name(&text) {
                prop_assert!(!name.is_empty());
                prop_assert!(name.chars().count() <= FIRST_LINE_CHARS + 1);
                prop_assert!(!name.chars().any(|c| c.is_control()));
                prop_assert_eq!(name.trim(), name.as_str());
                prop_assert!(!name.contains("  "));
            } else {
                prop_assert!(text.lines().take(FIRST_LINE_SCAN).all(|line| line
                    .chars()
                    .all(|c| c.is_whitespace() || c.is_control())));
            }
        }

        #[test]
        fn proposed_names_are_plain_file_names(name in "\\PC{0,80}", rust in any::<bool>()) {
            let extension = rust.then_some("rs");
            if let Some(file) = save_file_name(&name, extension) {
                prop_assert!(!file.contains('/') && !file.contains('\0'));
                prop_assert!(!file.starts_with('.') && !file.starts_with(' '));
                prop_assert!(file.len() <= MAX_FILE_NAME_BYTES);
                let suffix = if rust { ".rs" } else { ".txt" };
                prop_assert!(file.to_lowercase().ends_with(suffix), "{} lacks {}", file, suffix);
            }
        }
    }
}
