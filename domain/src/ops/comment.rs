//! Comment and uncomment lines, and toggle block comments (Edit > Comment/Uncomment). The UI
//! passes the tokens from GtkSourceView's language metadata (`line-comment-start`,
//! `block-comment-start`, `block-comment-end`).
//!
//! A line comment goes in at the smallest indentation of the lines, not at each line's own,
//! followed by a space, so the tokens line up (as in VS Code). Uncommenting removes the token
//! and one space after it wherever the line's indentation ends, and matches the token ignoring
//! ASCII case (`REM`). Blank lines are left alone unless all lines are blank. Toggling
//! uncomments when every non-blank line is commented and comments them all otherwise, rather
//! than toggling each line separately. Languages without a line comment, such as HTML and CSS,
//! get each line wrapped in the block tokens instead.

use std::ops::Range;

use super::{
    Change, Doc, Target, char_len, is_blank, is_blank_char, leading_blanks, trailing_blanks,
};
use crate::text::TextEdit;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommentTokens {
    pub line: Option<String>,
    pub block: Option<(String, String)>,
}

impl CommentTokens {
    fn line_token(&self) -> Option<&str> {
        self.line.as_deref().filter(|token| !token.is_empty())
    }

    fn block_tokens(&self) -> Option<(&str, &str)> {
        self.block
            .as_ref()
            .filter(|(open, close)| !open.is_empty() && !close.is_empty())
            .map(|(open, close)| (open.as_str(), close.as_str()))
    }

    fn style(&self) -> Option<Style<'_>> {
        self.line_token().map(Style::Line).or_else(|| {
            self.block_tokens()
                .map(|(open, close)| Style::Wrap(open, close))
        })
    }
}

/// Comments or uncomments the lines of the target (Ctrl+Q).
pub fn toggle_line(text: &str, target: Target, tokens: &CommentTokens) -> Change {
    line_comments(text, target, tokens, Mode::Toggle)
}

/// Comments the lines of the target (Ctrl+K).
pub fn comment_lines(text: &str, target: Target, tokens: &CommentTokens) -> Change {
    line_comments(text, target, tokens, Mode::Comment)
}

/// Uncomments the lines of the target that are commented (Ctrl+Shift+K).
pub fn uncomment_lines(text: &str, target: Target, tokens: &CommentTokens) -> Change {
    line_comments(text, target, tokens, Mode::Uncomment)
}

/// Removes the block comment that the selection is, or that the selection or caret is inside;
/// otherwise wraps the selection, or with a caret the line without its indentation and
/// trailing blanks, in the block tokens (Ctrl+Shift+Q). A wrapped selection is selected with
/// its tokens, so toggling again unwraps it. Does nothing without block tokens.
pub fn toggle_block(text: &str, target: Target, tokens: &CommentTokens) -> Change {
    let Some((open, close)) = tokens.block_tokens() else {
        return Change::default();
    };
    let doc = Doc::new(text);
    let (range, caret) = match &target {
        Target::Selection(selection) => {
            let selection = doc.clamp(selection);
            if selection.is_empty() {
                let line = doc.index.line_of_char(selection.start);
                let content = line_content(&doc, line).unwrap_or(selection.clone());
                (content, Some(selection.start))
            } else {
                (selection, None)
            }
        }
        Target::Lines(_) => {
            let lines = doc.lines(&target);
            let first = lines.clone().find_map(|line| line_content(&doc, line));
            let last = lines.rev().find_map(|line| line_content(&doc, line));
            match first.zip(last) {
                Some((first, last)) => (first.start..last.end, None),
                None => return Change::default(),
            }
        }
        Target::Document => {
            let is_space = |c: char| matches!(c, ' ' | '\t' | '\n');
            let lead = text.len() - text.trim_start_matches(is_space).len();
            let content = text.trim_end_matches(is_space).len();
            if lead >= content {
                return Change::default();
            }
            let start = doc.index.byte_to_char(text, lead);
            (start..doc.index.byte_to_char(text, content), None)
        }
    };
    let byte = |offset: usize| doc.index.char_to_byte(text, offset);
    let bytes = byte(range.start)..byte(range.end);
    let is_space = |c: char| matches!(c, ' ' | '\t' | '\n');
    let slice = &text[bytes.clone()];
    let trimmed = slice.trim_matches(is_space);
    if trimmed.len() >= open.len() + close.len()
        && trimmed.starts_with(open)
        && trimmed.ends_with(close)
    {
        let start = bytes.start + slice.len() - slice.trim_start_matches(is_space).len();
        return unwrap(&doc, start..start + trimmed.len(), open, close);
    }
    let probe = caret.map_or(bytes, |caret| byte(caret)..byte(caret));
    if let Some(comment) = enclosing(text, probe, open, close) {
        return unwrap(&doc, comment, open, close);
    }
    let (open_len, close_len) = (char_len(open), char_len(close));
    let edits = if range.is_empty() {
        vec![TextEdit::insert(range.start, format!("{open}  {close}"))]
    } else {
        vec![
            TextEdit::insert(range.start, format!("{open} ")),
            TextEdit::insert(range.end, format!(" {close}")),
        ]
    };
    let selection = match (caret, &target) {
        (Some(caret), _) => {
            let caret = if caret < range.start {
                caret
            } else if caret <= range.end {
                caret + open_len + 1
            } else {
                caret + open_len + close_len + 2
            };
            Some(caret..caret)
        }
        (None, Target::Selection(_)) => Some(range.start..range.end + open_len + close_len + 2),
        (None, _) => None,
    };
    Change::new(edits, selection)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Toggle,
    Comment,
    Uncomment,
}

#[derive(Debug, Clone, Copy)]
enum Style<'a> {
    Line(&'a str),
    Wrap(&'a str, &'a str),
}

fn line_comments(text: &str, target: Target, tokens: &CommentTokens, mode: Mode) -> Change {
    let Some(style) = tokens.style() else {
        return Change::default();
    };
    let doc = Doc::new(text);
    let lines = doc.lines(&target);
    let mut chosen: Vec<usize> = lines
        .clone()
        .filter(|&line| !is_blank(doc.line(line)))
        .collect();
    if chosen.is_empty() {
        chosen = lines.collect();
    }
    let uncomment = match mode {
        Mode::Comment => false,
        Mode::Uncomment => true,
        Mode::Toggle => {
            !chosen.is_empty()
                && chosen
                    .iter()
                    .all(|&line| style.is_commented(doc.line(line)))
        }
    };
    let mut edits = Vec::new();
    if uncomment {
        for &line in &chosen {
            style.uncomment(&doc, line, &mut edits);
        }
    } else {
        let indent = chosen
            .iter()
            .map(|&line| leading_blanks(doc.line(line)))
            .min()
            .unwrap_or(0);
        for &line in &chosen {
            style.comment(&doc, line, indent, &mut edits);
        }
    }
    Change::new(edits, None)
}

impl Style<'_> {
    fn is_commented(self, line: &str) -> bool {
        match self {
            Self::Line(token) => starts_with_token(&line[leading_blanks(line)..], token),
            Self::Wrap(open, close) => {
                let content = line.trim_matches(is_blank_char);
                content.len() >= open.len() + close.len()
                    && starts_with_token(content, open)
                    && ends_with_token(content, close)
            }
        }
    }

    fn comment(self, doc: &Doc<'_>, line: usize, indent: usize, edits: &mut Vec<TextEdit>) {
        let at = doc.start(line) + indent;
        match self {
            Self::Line(token) => edits.push(TextEdit::insert(at, format!("{token} "))),
            Self::Wrap(open, close) => {
                let content = doc.line(line);
                let end = content.len() - trailing_blanks(content);
                if end <= indent {
                    edits.push(TextEdit::insert(at, format!("{open}  {close}")));
                } else {
                    edits.push(TextEdit::insert(at, format!("{open} ")));
                    edits.push(TextEdit::insert(
                        doc.offset_in(line, end),
                        format!(" {close}"),
                    ));
                }
            }
        }
    }

    fn uncomment(self, doc: &Doc<'_>, line: usize, edits: &mut Vec<TextEdit>) {
        let content = doc.line(line);
        let lead = leading_blanks(content);
        let start = doc.start(line) + lead;
        match self {
            Self::Line(token) => {
                let rest = &content[lead..];
                if starts_with_token(rest, token) {
                    let space = usize::from(rest[token.len()..].starts_with(' '));
                    edits.push(TextEdit::delete(start..start + char_len(token) + space));
                }
            }
            Self::Wrap(open, close) if self.is_commented(content) => {
                let end = content.len() - trailing_blanks(content);
                let inner = &content[lead + open.len()..end - close.len()];
                let (open_space, close_space) = inner_spaces(inner);
                let open_end = start + char_len(open) + open_space;
                let close_end = doc.offset_in(line, end);
                let close_start = (close_end - char_len(close) - close_space).max(open_end);
                edits.push(TextEdit::delete(start..open_end));
                edits.push(TextEdit::delete(close_start..close_end));
            }
            Self::Wrap(..) => {}
        }
    }
}

/// Whether one space follows the opening token and one precedes the closing token.
fn inner_spaces(inner: &str) -> (usize, usize) {
    let open = usize::from(inner.starts_with(' '));
    let close = usize::from(inner.len() > open && inner.ends_with(' '));
    (open, close)
}

/// The characters of `line` without indentation and trailing blanks; `None` if it is blank.
fn line_content(doc: &Doc<'_>, line: usize) -> Option<Range<usize>> {
    let content = doc.line(line);
    if is_blank(content) {
        return None;
    }
    let start = doc.start(line) + leading_blanks(content);
    Some(start..doc.offset_in(line, content.len() - trailing_blanks(content)))
}

/// The bytes of the block comment around `probe`, if `probe` lies inside one and holds no
/// token itself. Tokens in strings are not recognized.
fn enclosing(text: &str, probe: Range<usize>, open: &str, close: &str) -> Option<Range<usize>> {
    if open.contains(close) || close.contains(open) {
        return None;
    }
    let inner = &text[probe.clone()];
    if inner.contains(open) || inner.contains(close) {
        return None;
    }
    let before = &text[..probe.start];
    let start = before.rfind(open)?;
    if before[start + open.len()..].contains(close) {
        return None;
    }
    let end = probe.end + text[probe.end..].find(close)? + close.len();
    Some(start..end)
}

/// Removes the tokens of the block comment at `comment` (bytes), with one space inside each.
fn unwrap(doc: &Doc<'_>, comment: Range<usize>, open: &str, close: &str) -> Change {
    let inner = &doc.text[comment.start + open.len()..comment.end - close.len()];
    let (open_space, close_space) = inner_spaces(inner);
    let open_end = comment.start + open.len() + open_space;
    let close_start = (comment.end - close.len() - close_space).max(open_end);
    let char_at = |byte: usize| doc.index.byte_to_char(doc.text, byte);
    Change::new(
        vec![
            TextEdit::delete(char_at(comment.start)..char_at(open_end)),
            TextEdit::delete(char_at(close_start)..char_at(comment.end)),
        ],
        None,
    )
}

fn starts_with_token(text: &str, token: &str) -> bool {
    text.as_bytes()
        .get(..token.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(token.as_bytes()))
}

fn ends_with_token(text: &str, token: &str) -> bool {
    text.len() >= token.len()
        && text.as_bytes()[text.len() - token.len()..].eq_ignore_ascii_case(token.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use super::*;
    use proptest::prelude::*;

    fn rust() -> CommentTokens {
        CommentTokens {
            line: Some("//".to_owned()),
            block: Some(("/*".to_owned(), "*/".to_owned())),
        }
    }

    fn html() -> CommentTokens {
        CommentTokens {
            line: None,
            block: Some(("<!--".to_owned(), "-->".to_owned())),
        }
    }

    fn batch() -> CommentTokens {
        CommentTokens {
            line: Some("REM".to_owned()),
            block: None,
        }
    }

    #[test]
    fn toggle_comments_at_the_smallest_indentation_and_back() {
        let text = "fn a() {\n    x();\n\n      y();\n}";
        let commented = applied(text, &toggle_line(text, Target::Lines(1..4), &rust()));
        assert_eq!(commented, "fn a() {\n    // x();\n\n    //   y();\n}");
        let restored = applied(
            &commented,
            &toggle_line(&commented, Target::Lines(1..4), &rust()),
        );
        assert_eq!(restored, text);
    }

    #[test]
    fn toggle_comments_everything_when_some_lines_are_not_commented() {
        let text = "// a\nb";
        assert_eq!(
            applied(text, &toggle_line(text, Target::Document, &rust())),
            "// // a\n// b"
        );
    }

    #[test]
    fn a_selection_ending_at_column_zero_leaves_that_line_alone() {
        let text = "a\nb\nc";
        let change = toggle_line(text, Target::Selection(0..4), &rust());
        assert_eq!(applied(text, &change), "// a\n// b\nc");
        assert_eq!(change.selection_after(0..4), 0..10);
    }

    #[test]
    fn uncomment_removes_the_token_and_one_space() {
        let text = "  //a\n  //  b\nc\n//";
        assert_eq!(
            applied(text, &uncomment_lines(text, Target::Document, &rust())),
            "  a\n   b\nc\n"
        );
        assert!(uncomment_lines("plain", Target::Document, &rust()).is_empty());
    }

    #[test]
    fn line_tokens_match_ignoring_ascii_case() {
        let text = "rem hello\nREMark";
        assert_eq!(
            applied(text, &uncomment_lines(text, Target::Document, &batch())),
            "hello\nark"
        );
        assert_eq!(
            applied("echo", &comment_lines("echo", Target::Document, &batch())),
            "REM echo"
        );
    }

    #[test]
    fn a_blank_line_alone_gets_a_comment() {
        let change = toggle_line("a\n\nb", Target::caret(2), &rust());
        assert_eq!(applied("a\n\nb", &change), "a\n// \nb");
        let change = toggle_line("a\n// \nb", Target::caret(3), &rust());
        assert_eq!(applied("a\n// \nb", &change), "a\n\nb");
    }

    #[test]
    fn languages_without_line_comments_wrap_each_line() {
        let text = "  <p>x</p>  \n\n    <b/>";
        let wrapped = applied(text, &toggle_line(text, Target::Document, &html()));
        assert_eq!(wrapped, "  <!-- <p>x</p> -->  \n\n  <!--   <b/> -->");
        let restored = applied(&wrapped, &toggle_line(&wrapped, Target::Document, &html()));
        assert_eq!(restored, text);
        assert_eq!(
            applied(
                "<!---->",
                &uncomment_lines("<!---->", Target::Document, &html())
            ),
            ""
        );
    }

    #[test]
    fn no_tokens_means_no_change() {
        let none = CommentTokens::default();
        assert!(toggle_line("a", Target::Document, &none).is_empty());
        assert!(toggle_block("a", Target::Document, &none).is_empty());
        assert!(toggle_block("a", Target::Document, &batch()).is_empty());
    }

    #[test]
    fn toggle_block_wraps_a_selection_and_unwraps_it_again() {
        let text = "let x = 1 + 2;";
        let change = toggle_block(text, Target::Selection(8..13), &rust());
        assert_eq!(change.selection, Some(8..19));
        let wrapped = applied(text, &change);
        assert_eq!(wrapped, "let x = /* 1 + 2 */;");
        let change = toggle_block(&wrapped, Target::Selection(8..19), &rust());
        assert_eq!(applied(&wrapped, &change), text);
    }

    #[test]
    fn toggle_block_unwraps_the_comment_around_the_caret_or_selection() {
        let text = "int x; /* note */ y";
        assert_eq!(
            applied(text, &toggle_block(text, Target::caret(11), &rust())),
            "int x; note y"
        );
        assert_eq!(
            applied(
                text,
                &toggle_block(text, Target::Selection(10..12), &rust())
            ),
            "int x; note y"
        );
        let text = "/*\n  a\n  b\n*/";
        assert_eq!(
            applied(text, &toggle_block(text, Target::caret(5), &rust())),
            "\n  a\n  b\n"
        );
    }

    #[test]
    fn toggle_block_with_a_caret_wraps_the_line_content() {
        let text = "    foo();  \nbar";
        let change = toggle_block(text, Target::caret(6), &rust());
        assert_eq!(change.selection, Some(9..9));
        assert_eq!(applied(text, &change), "    /* foo(); */  \nbar");
        let change = toggle_block("a\n\nb", Target::caret(2), &rust());
        assert_eq!(change.selection, Some(5..5));
        assert_eq!(applied("a\n\nb", &change), "a\n/*  */\nb");
    }

    #[test]
    fn toggle_block_on_lines_and_the_document() {
        let text = "\n  a\n  b  \n\n";
        let wrapped = applied(text, &toggle_block(text, Target::Document, &rust()));
        assert_eq!(wrapped, "\n  /* a\n  b */  \n\n");
        assert_eq!(
            applied(&wrapped, &toggle_block(&wrapped, Target::Document, &rust())),
            text
        );
        assert_eq!(
            applied(text, &toggle_block(text, Target::Lines(0..3), &rust())),
            "\n  /* a\n  b */  \n\n"
        );
        assert!(toggle_block("  \n", Target::Document, &rust()).is_empty());
    }

    fn plain_text() -> impl Strategy<Value = (String, Target)> {
        text_and_target().prop_filter("no comment tokens", |(text, _)| {
            !text.contains("//") && !text.contains("<!--") && !text.contains("-->")
        })
    }

    /// A text without block comment tokens and a non-empty selection in it.
    fn selected_text() -> impl Strategy<Value = (String, Range<usize>)> {
        text()
            .prop_filter("text to select", |text| {
                !text.is_empty() && !text.contains("/*") && !text.contains("*/")
            })
            .prop_flat_map(|text| {
                let len = char_len(&text);
                (Just(text), 0..len, 1..=len)
            })
            .prop_map(|(text, a, b)| {
                let range = if a < b { a..b } else { b - 1..a + 1 };
                (text, range)
            })
    }

    proptest! {
        #[test]
        fn toggling_twice_restores_uncommented_lines((text, target) in plain_text()) {
            for tokens in [rust(), html()] {
                let once = applied(&text, &toggle_line(&text, target.clone(), &tokens));
                let lines = naive_lines(&text, &target);
                let twice = applied(&once, &toggle_line(&once, Target::Lines(lines), &tokens));
                prop_assert_eq!(twice, text.clone());
            }
        }

        #[test]
        fn comment_matches_a_naive_model((text, target) in plain_text()) {
            let range = naive_lines(&text, &target);
            let lines: Vec<&str> = text.split('\n').collect();
            let blank = |line: &str| line.chars().all(|c| c == ' ' || c == '\t');
            let chosen: Vec<usize> = range.clone().filter(|&i| !blank(lines[i])).collect();
            let chosen = if chosen.is_empty() { range.clone().collect() } else { chosen };
            let indent = chosen
                .iter()
                .map(|&i| lines[i].len() - lines[i].trim_start_matches([' ', '\t']).len())
                .min()
                .unwrap_or(0);
            let expected: Vec<String> = lines
                .iter()
                .enumerate()
                .map(|(i, line)| {
                    if chosen.contains(&i) {
                        format!("{}// {}", &line[..indent], &line[indent..])
                    } else {
                        (*line).to_owned()
                    }
                })
                .collect();
            prop_assert_eq!(
                applied(&text, &comment_lines(&text, target, &rust())),
                expected.join("\n")
            );
        }

        #[test]
        fn toggle_block_twice_restores_a_selection((text, range) in selected_text()) {
            let change = toggle_block(&text, Target::Selection(range), &rust());
            let wrapped = applied(&text, &change);
            let selection = change.selection.clone().unwrap();
            let back = applied(&wrapped, &toggle_block(&wrapped, Target::Selection(selection), &rust()));
            prop_assert_eq!(back, text);
        }
    }
}
