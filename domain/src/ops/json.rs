//! JSON tools. Formatting and minifying work on tokens: every string, number and literal is
//! copied verbatim, so escapes, the spelling and precision of numbers, duplicate keys and key
//! order all survive, and only the whitespace between tokens changes (technical critique #9).
//! Empty objects and arrays stay `{}` and `[]`. Validation uses serde_json with `IgnoredAny`,
//! which builds no values and has no nesting limit; `format` and `minify` validate first.

use serde::de::IgnoredAny;

use super::{Breaks, Change, Indent, SyntaxError, Target, reformat_target};

pub type JsonError = SyntaxError;

/// Checks that `text` is exactly one JSON value (RFC 8259), with whitespace around it.
pub fn validate(text: &str) -> Result<(), JsonError> {
    serde_json::from_str::<IgnoredAny>(text)
        .map(|_| ())
        .map_err(|error| to_error(text, &error))
}

/// Pretty-prints `text`: one member or element per line, `": "` after keys.
pub fn format(text: &str, indent: Indent) -> Result<String, JsonError> {
    validate(text)?;
    Ok(write(text, Some(Breaks::new(indent))))
}

/// Removes all whitespace between tokens.
pub fn minify(text: &str) -> Result<String, JsonError> {
    validate(text)?;
    Ok(write(text, None))
}

/// [`format()`] on the selection, or on the whole document for a caret or `Document`. The
/// whitespace around the JSON value stays; error positions are positions in `text`.
pub fn format_target(text: &str, target: Target, indent: Indent) -> Result<Change, JsonError> {
    reformat_target(text, target, |json| format(json, indent))
}

/// [`minify`] on the selection, or on the whole document for a caret or `Document`.
pub fn minify_target(text: &str, target: Target) -> Result<Change, JsonError> {
    reformat_target(text, target, minify)
}

fn to_error(text: &str, error: &serde_json::Error) -> JsonError {
    let full = error.to_string();
    let suffix = format!(" at line {} column {}", error.line(), error.column());
    let message = full.strip_suffix(&suffix).unwrap_or(&full);
    // serde_json counts lines from 1, and columns in bytes up to the byte it stopped at.
    let line_start = match error.line() {
        0 | 1 => 0,
        line => memchr::memchr_iter(b'\n', text.as_bytes())
            .nth(line - 2)
            .map_or(text.len(), |newline| newline + 1),
    };
    let byte = line_start + error.column().saturating_sub(1);
    SyntaxError::at_byte(text, byte, message)
}

/// Writes the tokens of validated JSON with `breaks` between them, or none.
fn write(text: &str, mut breaks: Option<Breaks>) -> String {
    let bytes = text.as_bytes();
    let pretty = breaks.is_some();
    let mut out = String::with_capacity(if pretty {
        text.len() + text.len() / 2
    } else {
        text.len()
    });
    let mut depth = 0usize;
    let mut line_break = |out: &mut String, depth: usize| {
        if let Some(breaks) = &mut breaks {
            breaks.push(out, depth);
        }
    };
    let mut at = skip_space(bytes, 0);
    while at < bytes.len() {
        match bytes[at] {
            open @ (b'{' | b'[') => {
                let close = open + 2;
                let next = skip_space(bytes, at + 1);
                out.push(char::from(open));
                if bytes.get(next) == Some(&close) {
                    out.push(char::from(close));
                    at = skip_space(bytes, next + 1);
                } else {
                    depth += 1;
                    line_break(&mut out, depth);
                    at = next;
                }
            }
            close @ (b'}' | b']') => {
                depth = depth.saturating_sub(1);
                line_break(&mut out, depth);
                out.push(char::from(close));
                at = skip_space(bytes, at + 1);
            }
            b',' => {
                out.push(',');
                line_break(&mut out, depth);
                at = skip_space(bytes, at + 1);
            }
            b':' => {
                out.push(':');
                if pretty {
                    out.push(' ');
                }
                at = skip_space(bytes, at + 1);
            }
            b'"' => {
                let end = string_end(bytes, at + 1);
                out.push_str(&text[at..end]);
                at = skip_space(bytes, end);
            }
            _ => {
                let end = scalar_end(bytes, at);
                out.push_str(&text[at..end]);
                at = skip_space(bytes, end);
            }
        }
    }
    out
}

fn skip_space(bytes: &[u8], mut at: usize) -> usize {
    while at < bytes.len() && matches!(bytes[at], b' ' | b'\t' | b'\n' | b'\r') {
        at += 1;
    }
    at
}

/// The end of a string whose contents start at `at`: just past the closing quote.
fn string_end(bytes: &[u8], mut at: usize) -> usize {
    while let Some(found) = memchr::memchr2(b'"', b'\\', &bytes[at.min(bytes.len())..]) {
        let hit = at + found;
        if bytes[hit] == b'"' {
            return hit + 1;
        }
        at = hit + 2;
    }
    bytes.len()
}

/// The end of a number or literal starting at `at`.
fn scalar_end(bytes: &[u8], at: usize) -> usize {
    bytes[at..]
        .iter()
        .position(|byte| {
            matches!(
                byte,
                b' ' | b'\t' | b'\n' | b'\r' | b',' | b':' | b'[' | b']' | b'{' | b'}' | b'"'
            )
        })
        .map_or(bytes.len(), |length| at + length)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{LineIndex, apply};
    use proptest::prelude::*;

    fn pretty(text: &str) -> String {
        format(text, Indent::Spaces(2)).unwrap()
    }

    #[test]
    fn formats_one_member_per_line_and_keeps_empty_containers_compact() {
        let text = r#"{"a":1,"b":[true,false,null],"c":{ },"d":[
        ],"e":{"f":"g"}}"#;
        let expected = r#"{
  "a": 1,
  "b": [
    true,
    false,
    null
  ],
  "c": {},
  "d": [],
  "e": {
    "f": "g"
  }
}"#;
        assert_eq!(pretty(text), expected);
        assert_eq!(
            minify(expected).unwrap(),
            r#"{"a":1,"b":[true,false,null],"c":{},"d":[],"e":{"f":"g"}}"#
        );
        assert_eq!(
            format("[1,[2]]", Indent::Tab).unwrap(),
            "[\n\t1,\n\t[\n\t\t2\n\t]\n]"
        );
        assert_eq!(format("[1]", Indent::Spaces(0)).unwrap(), "[\n1\n]");
    }

    #[test]
    fn copies_every_token_verbatim() {
        let text = r#"{"n":1.0E+10,"z":-0,"big":123456789012345678901234567890.000,"s":"a\u00e9\n\"\\\/ ok","dup":1,"dup":2,"é":"日本"}"#;
        assert_eq!(minify(text).unwrap(), text);
        let formatted = pretty(text);
        assert!(formatted.contains(r#""n": 1.0E+10,"#));
        assert!(formatted.contains(r#""big": 123456789012345678901234567890.000,"#));
        assert!(formatted.contains(r#""s": "a\u00e9\n\"\\\/ ok","#));
        assert!(formatted.contains("\"dup\": 1,\n  \"dup\": 2,"));
        assert_eq!(minify(&formatted).unwrap(), text);
    }

    #[test]
    fn top_level_scalars_are_values_too() {
        assert_eq!(pretty("  42 \n"), "42");
        assert_eq!(pretty(r#""x""#), r#""x""#);
        assert_eq!(minify(" null ").unwrap(), "null");
        assert_eq!(pretty("[ ]"), "[]");
    }

    #[test]
    fn errors_point_at_the_character() {
        let error = validate("[1, 2 3]").unwrap_err();
        assert_eq!(error.message, "expected `,` or `]`");
        assert_eq!((error.line, error.column, error.offset), (1, 7, 6));
        let error = validate("{\"é\": x}").unwrap_err();
        assert_eq!(error.message, "expected value");
        assert_eq!((error.line, error.column, error.offset), (1, 7, 6));
        let error = validate("{\n  \"a\": 1,\n  }").unwrap_err();
        assert_eq!((error.line, error.column, error.offset), (3, 3, 14));
        assert_eq!(
            error.to_string(),
            format!("{} at line 3, column 3", error.message)
        );
        let error = validate("[1,").unwrap_err();
        assert_eq!(error.message, "EOF while parsing a value");
        assert_eq!((error.line, error.column), (1, 3));
        assert!(validate("").is_err());
        assert!(validate("[1] [2]").is_err());
        assert!(validate("[1,]").is_err());
        assert!(validate("{'a':1}").is_err());
        assert!(validate("[NaN]").is_err());
        assert!(validate("\"\u{1}\"").is_err());
        assert!(format("[1,]", Indent::default()).is_err());
        assert!(minify("{").is_err());
    }

    #[test]
    fn nesting_depth_is_unlimited() {
        let deep = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
        assert_eq!(validate(&deep), Ok(()));
        assert_eq!(minify(&deep).unwrap(), deep);
        let text = format!("{}1{}", "[".repeat(1_000), "]".repeat(1_000));
        let formatted = pretty(&text);
        assert_eq!(formatted.lines().count(), 2_001);
        assert_eq!(minify(&formatted).unwrap(), text);
    }

    #[test]
    fn targets_keep_the_surrounding_whitespace() {
        let text = "\n  {\"a\":[1]}\n";
        let change = format_target(text, Target::Document, Indent::Spaces(2)).unwrap();
        assert_eq!(change.selection, Some(3..3));
        assert_eq!(
            apply(text, change.edits).unwrap(),
            "\n  {\n  \"a\": [\n    1\n  ]\n}\n"
        );
        let text = "x = {\"a\": 1};";
        let change = minify_target(text, Target::Selection(4..12)).unwrap();
        assert_eq!(change.selection, Some(4..11));
        assert_eq!(apply(text, change.edits).unwrap(), "x = {\"a\":1};");
        assert!(minify_target("[1]", Target::Document).unwrap().is_empty());
    }

    #[test]
    fn target_errors_are_positions_in_the_document() {
        let text = "x\n  [1,]\n";
        let error = format_target(text, Target::Selection(2..8), Indent::default()).unwrap_err();
        assert_eq!((error.line, error.column, error.offset), (2, 6, 7));
    }

    fn space() -> impl Strategy<Value = String> {
        "[ \t\n\r]{0,2}"
    }

    fn scalar() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("null".to_owned()),
            Just("true".to_owned()),
            Just("false".to_owned()),
            "-?(0|[1-9][0-9]{0,20})(\\.[0-9]{1,5})?([eE][+-]?[0-9]{1,3})?",
            string(),
        ]
    }

    fn string() -> impl Strategy<Value = String> {
        prop::collection::vec(
            prop_oneof![
                "[a-z0-9 ,:{}\\[\\]é日]",
                Just("\\n".to_owned()),
                Just("\\\"".to_owned()),
                Just("\\\\".to_owned()),
                Just("\\/".to_owned()),
                "\\\\u[0-9a-fA-F]{4}",
            ],
            0..6,
        )
        .prop_map(|parts| format!("\"{}\"", parts.concat()))
    }

    /// Valid JSON with random whitespace between its tokens.
    fn json() -> impl Strategy<Value = String> {
        let value = scalar().prop_recursive(4, 48, 5, |inner| {
            let item = (space(), inner.clone(), space()).prop_map(|(a, v, b)| format!("{a}{v}{b}"));
            let member = (space(), string(), space(), space(), inner, space())
                .prop_map(|(a, k, b, c, v, d)| format!("{a}{k}{b}:{c}{v}{d}"));
            prop_oneof![
                (prop::collection::vec(item, 0..5), space()).prop_map(|(items, empty)| {
                    if items.is_empty() {
                        format!("[{empty}]")
                    } else {
                        format!("[{}]", items.join(","))
                    }
                }),
                (prop::collection::vec(member, 0..5), space()).prop_map(|(members, empty)| {
                    if members.is_empty() {
                        format!("{{{empty}}}")
                    } else {
                        format!("{{{}}}", members.join(","))
                    }
                }),
            ]
        });
        (space(), value, space()).prop_map(|(a, v, b)| format!("{a}{v}{b}"))
    }

    /// The text without the whitespace outside strings.
    fn strip_space(text: &str) -> String {
        let mut out = String::new();
        let (mut quoted, mut escaped) = (false, false);
        for c in text.chars() {
            if quoted {
                out.push(c);
                match c {
                    _ if escaped => escaped = false,
                    '\\' => escaped = true,
                    '"' => quoted = false,
                    _ => {}
                }
            } else if !matches!(c, ' ' | '\t' | '\n' | '\r') {
                quoted = c == '"';
                out.push(c);
            }
        }
        out
    }

    proptest! {
        #[test]
        fn minify_only_removes_whitespace(text in json()) {
            prop_assert_eq!(validate(&text), Ok(()));
            prop_assert_eq!(minify(&text).unwrap(), strip_space(&text));
        }

        #[test]
        fn format_keeps_tokens_and_is_idempotent(text in json(), width in 0usize..5, tab in any::<bool>()) {
            let indent = if tab { Indent::Tab } else { Indent::Spaces(width) };
            let formatted = format(&text, indent).unwrap();
            prop_assert_eq!(minify(&formatted).unwrap(), minify(&text).unwrap());
            prop_assert_eq!(format(&formatted, indent).unwrap(), formatted.clone());
            prop_assert_eq!(
                serde_json::from_str::<serde_json::Value>(&formatted).ok(),
                serde_json::from_str::<serde_json::Value>(&text).ok()
            );
        }

        #[test]
        fn error_positions_are_consistent(text in "[\\[\\]{}:,\" a1\\\\n.e\n-]{0,16}") {
            if let Err(error) = validate(&text) {
                let index = LineIndex::new(&text);
                prop_assert!(error.offset <= index.len_chars());
                let position = index.position(error.offset);
                prop_assert_eq!((error.line, error.column), (position.line + 1, position.column + 1));
                prop_assert!(format(&text, Indent::default()).is_err());
            } else {
                prop_assert!(format(&text, Indent::default()).is_ok());
            }
        }
    }

    fn sample(bytes: usize) -> String {
        let mut text = String::from("[");
        let mut id = 0u64;
        while text.len() < bytes {
            if id > 0 {
                text.push(',');
            }
            text.push_str(&std::format!(
                r#"{{"id":{id},"name":"user {id}","email":"user{id}@example.com","score":{}.{},"active":{},"tags":["a","b\u00e9","c\n"],"nested":{{"x":[1,2,3],"y":null,"z":{{}}}}}}"#,
                id % 1000,
                id % 97,
                id.is_multiple_of(2)
            ));
            id += 1;
        }
        text.push(']');
        text
    }

    #[test]
    #[ignore = "timing; run with --release -- --ignored --nocapture"]
    fn timing_on_20_mb() {
        use std::time::Instant;
        let minified = sample(20 * 1024 * 1024);
        let time = |label: &str, run: &dyn Fn() -> String| {
            let mut best = f64::MAX;
            let mut output = String::new();
            for _ in 0..5 {
                let start = Instant::now();
                output = run();
                best = best.min(start.elapsed().as_secs_f64() * 1000.0);
            }
            println!(
                "{label}: best of 5 {best:.1} ms, output {:.1} MB",
                output.len() as f64 / 1_048_576.0
            );
            output
        };
        println!(
            "input: {:.1} MB minified JSON",
            minified.len() as f64 / 1_048_576.0
        );
        time("validate", &|| {
            validate(&minified).unwrap();
            String::new()
        });
        let formatted = time("format (minified input)", &|| pretty(&minified));
        time("format (formatted input)", &|| pretty(&formatted));
        time("minify (formatted input)", &|| minify(&formatted).unwrap());
        time("minify (minified input)", &|| minify(&minified).unwrap());
        let document = std::format!("{minified}\n");
        time("format_target (Document, with the edit)", &|| {
            let change = format_target(&document, Target::Document, Indent::Spaces(2)).unwrap();
            assert_eq!(change.edits.len(), 1);
            String::new()
        });
    }
}
