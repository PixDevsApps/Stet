//! The text a line is compared by once the whitespace and case options apply.

use std::borrow::Cow;

use super::{CompareOptions, IgnoreWhitespace};

pub(super) fn line_key<'a>(line: &'a str, options: &CompareOptions) -> Cow<'a, str> {
    let key = match options.ignore_whitespace {
        IgnoreWhitespace::None => Cow::Borrowed(line),
        IgnoreWhitespace::Trailing => Cow::Borrowed(line.trim_end()),
        IgnoreWhitespace::Changes => collapse_whitespace(line.trim_end()),
        IgnoreWhitespace::All => strip_whitespace(line),
    };
    if options.ignore_case {
        fold_case(key)
    } else {
        key
    }
}

/// Replaces every run of whitespace with one space.
fn collapse_whitespace(line: &str) -> Cow<'_, str> {
    let mut previous_space = false;
    let already = line.chars().all(|c| {
        let space = c.is_whitespace();
        let fine = !space || (c == ' ' && !previous_space);
        previous_space = space;
        fine
    });
    if already {
        return Cow::Borrowed(line);
    }
    let mut out = String::with_capacity(line.len());
    let mut previous_space = false;
    for c in line.chars() {
        if c.is_whitespace() {
            if !previous_space {
                out.push(' ');
            }
            previous_space = true;
        } else {
            out.push(c);
            previous_space = false;
        }
    }
    Cow::Owned(out)
}

fn strip_whitespace(line: &str) -> Cow<'_, str> {
    if line.contains(char::is_whitespace) {
        Cow::Owned(line.chars().filter(|c| !c.is_whitespace()).collect())
    } else {
        Cow::Borrowed(line)
    }
}

pub(super) fn fold_case(text: Cow<'_, str>) -> Cow<'_, str> {
    if text.is_ascii() && !text.bytes().any(|byte| byte.is_ascii_uppercase()) {
        text
    } else {
        Cow::Owned(text.to_lowercase())
    }
}

/// Folds one character for a case-insensitive comparison that keeps character counts.
pub(super) fn fold_char(c: char) -> char {
    if c.is_ascii() {
        c.to_ascii_lowercase()
    } else {
        c.to_lowercase().next().unwrap_or(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(line: &str, ignore_whitespace: IgnoreWhitespace, ignore_case: bool) -> String {
        let options = CompareOptions {
            ignore_whitespace,
            ignore_case,
            ..CompareOptions::default()
        };
        line_key(line, &options).into_owned()
    }

    #[test]
    fn whitespace_modes_follow_git_semantics() {
        let line = "  a \t b  ";
        assert_eq!(key(line, IgnoreWhitespace::None, false), line);
        assert_eq!(key(line, IgnoreWhitespace::Trailing, false), "  a \t b");
        assert_eq!(key(line, IgnoreWhitespace::Changes, false), " a b");
        assert_eq!(key(line, IgnoreWhitespace::All, false), "ab");
        assert_eq!(key("ab", IgnoreWhitespace::Changes, false), "ab");
        assert_eq!(key("a b", IgnoreWhitespace::Changes, false), "a b");
    }

    #[test]
    fn case_folds_unicode() {
        assert_eq!(
            key("Straße ÆØÅ", IgnoreWhitespace::None, true),
            "straße æøå"
        );
        assert!(matches!(
            line_key(
                "plain",
                &CompareOptions {
                    ignore_case: true,
                    ..CompareOptions::default()
                }
            ),
            Cow::Borrowed(_)
        ));
    }
}
