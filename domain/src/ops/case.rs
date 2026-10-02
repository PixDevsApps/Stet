//! Case conversion (Edit > Convert Case to). It uses full Unicode mappings (`ß` → `SS`),
//! titlecase for digraphs and ligatures (`ǆ` → `ǅ`), the final sigma, and grapheme clusters,
//! so a combining mark stays with its letter. There are no locale rules (Turkish dotted and
//! dotless i).

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

use super::{BULK_EDIT_THRESHOLD, Change, Doc, Target, char_len};
use crate::text::TextEdit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Case {
    Upper,
    Lower,
    /// The first letter of every word up and the rest down. A letter after an apostrophe
    /// within a word stays down: `don't` becomes `Don't`.
    Proper,
    /// Like `Proper`, but the rest of each word keeps its case.
    ProperBlend,
    /// The first letter of every sentence up and the rest down. A sentence starts at the
    /// start, after `.`, `!` or `?` followed by anything but a letter or digit, and after an
    /// empty line. A lone `i` becomes `I`.
    Sentence,
    /// Like `Sentence`, but the rest keeps its case.
    SentenceBlend,
    /// Lowercase letters up, everything else down.
    Invert,
}

/// Converts the selection, or with a caret the word at the caret ([`word_at`]); `Lines` and
/// `Document` convert whole lines and the whole text. A selection is kept on the converted
/// text, and a caret stays where it was.
pub fn convert(text: &str, target: Target, case: Case) -> Change {
    let doc = Doc::new(text);
    let (range, caret) = match &target {
        Target::Selection(selection) => {
            let selection = doc.clamp(selection);
            if !selection.is_empty() {
                (selection, None)
            } else if let Some(word) = word_in(&doc, selection.start) {
                (word, Some(selection.start))
            } else {
                return Change::default();
            }
        }
        Target::Lines(_) => {
            let lines = doc.lines(&target);
            if lines.is_empty() {
                return Change::default();
            }
            (doc.start(lines.start)..doc.end(lines.end - 1), None)
        }
        Target::Document => (0..doc.len(), None),
    };
    let old = doc.slice(range.clone());
    let new = transform(old, case);
    if new == old {
        return Change::default();
    }
    let end = range.start + char_len(&new);
    let selection = match (caret, &target) {
        (Some(caret), _) => {
            let caret = if caret == range.end {
                end
            } else {
                caret.min(end)
            };
            Some(caret..caret)
        }
        (None, Target::Selection(_)) => Some(range.start..end),
        (None, _) => None,
    };
    Change::new(diff(range.start, old, &new), selection)
}

/// `text` in `case`.
pub fn transform(text: &str, case: Case) -> String {
    match case {
        Case::Upper => text.to_uppercase(),
        Case::Lower => text.to_lowercase(),
        Case::Proper => proper(text, false),
        Case::ProperBlend => proper(text, true),
        Case::Sentence => sentence(text, false),
        Case::SentenceBlend => sentence(text, true),
        Case::Invert => {
            let mut out = String::with_capacity(text.len());
            for c in text.chars() {
                if c.is_lowercase() {
                    out.extend(c.to_uppercase());
                } else {
                    out.extend(c.to_lowercase());
                }
            }
            out
        }
    }
}

/// The word at `offset`: the Unicode word (UAX #29) that contains the character at `offset`
/// or, failing that, ends at `offset`. Words hold at least one letter or digit.
pub fn word_at(text: &str, offset: usize) -> Option<Range<usize>> {
    word_in(&Doc::new(text), offset)
}

fn word_in(doc: &Doc<'_>, offset: usize) -> Option<Range<usize>> {
    let offset = offset.min(doc.len());
    let line = doc.index.line_of_char(offset);
    let content = doc.line(line);
    let caret = content
        .char_indices()
        .nth(offset - doc.start(line))
        .map_or(content.len(), |(byte, _)| byte);
    let mut before = None;
    for (start, word) in content.split_word_bound_indices() {
        let end = start + word.len();
        if start > caret {
            break;
        }
        if !word.chars().any(char::is_alphanumeric) {
            continue;
        }
        if caret < end {
            before = Some(start..end);
            break;
        }
        if end == caret {
            before = Some(start..end);
        }
    }
    before.map(|bytes| doc.offset_in(line, bytes.start)..doc.offset_in(line, bytes.end))
}

/// Edits that turn `old`, at character `start`, into `new`: one per run of changed
/// characters when both have the same length and there are no more than
/// [`BULK_EDIT_THRESHOLD`] runs, otherwise one for the part that differs.
fn diff(start: usize, old: &str, new: &str) -> Vec<TextEdit> {
    let mut edits = Vec::new();
    if char_len(old) == char_len(new) {
        let mut run: Option<(usize, String)> = None;
        for (index, (a, b)) in old.chars().zip(new.chars()).enumerate() {
            if a != b {
                run.get_or_insert_with(|| (index, String::new())).1.push(b);
            } else if let Some((from, insert)) = run.take() {
                edits.push(TextEdit::new(start + from..start + index, insert));
                if edits.len() > BULK_EDIT_THRESHOLD {
                    break;
                }
            }
        }
        if let Some((from, insert)) = run {
            let to = from + char_len(&insert);
            edits.push(TextEdit::new(start + from..start + to, insert));
        }
        if edits.len() <= BULK_EDIT_THRESHOLD {
            return edits;
        }
        edits.clear();
    }
    let same = |(a, b): &(char, char)| a == b;
    let prefix: usize = old
        .chars()
        .zip(new.chars())
        .take_while(same)
        .map(|(a, _)| a.len_utf8())
        .sum();
    let (old_rest, new_rest) = (&old[prefix..], &new[prefix..]);
    let suffix: usize = old_rest
        .chars()
        .rev()
        .zip(new_rest.chars().rev())
        .take_while(same)
        .map(|(a, _)| a.len_utf8())
        .sum();
    let from = start + char_len(&old[..prefix]);
    let replaced = char_len(&old_rest[..old_rest.len() - suffix]);
    edits.push(TextEdit::new(
        from..from + replaced,
        &new_rest[..new_rest.len() - suffix],
    ));
    edits
}

fn base(cluster: &str) -> char {
    cluster.chars().next().unwrap_or('\0')
}

fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '\u{2018}' | '\u{2019}')
}

fn proper(text: &str, blend: bool) -> String {
    let clusters: Vec<&str> = text.graphemes(true).collect();
    let mut out = String::with_capacity(text.len());
    for (index, cluster) in clusters.iter().enumerate() {
        let before = |back: usize| index.checked_sub(back).map(|at| base(clusters[at]));
        if !base(cluster).is_alphabetic() {
            out.push_str(cluster);
            continue;
        }
        let after_apostrophe =
            before(1).is_some_and(is_apostrophe) && before(2).is_some_and(char::is_alphanumeric);
        if !after_apostrophe && !before(1).is_some_and(char::is_alphanumeric) {
            push_title(cluster, &mut out);
        } else if blend {
            out.push_str(cluster);
        } else {
            push_lower(&clusters, index, &mut out);
        }
    }
    out
}

fn sentence(text: &str, blend: bool) -> String {
    let clusters: Vec<&str> = text.graphemes(true).collect();
    let mut out = String::with_capacity(text.len());
    let mut starts_sentence = true;
    let mut after_newline = false;
    for (index, cluster) in clusters.iter().enumerate() {
        let c = base(cluster);
        if c.is_alphabetic() {
            let at = out.len();
            if starts_sentence {
                push_title(cluster, &mut out);
                starts_sentence = false;
            } else if blend {
                out.push_str(cluster);
            } else {
                push_lower(&clusters, index, &mut out);
            }
            if &out[at..] == "i" && is_lone_i(&clusters, index) {
                out.truncate(at);
                out.push('I');
            }
            after_newline = false;
            continue;
        }
        out.push_str(cluster);
        match c {
            '.' | '!' | '?' => {
                starts_sentence = clusters
                    .get(index + 1)
                    .is_some_and(|next| !base(next).is_alphanumeric());
            }
            '\n' if after_newline => starts_sentence = true,
            '\n' => after_newline = true,
            _ => {}
        }
    }
    out
}

/// The pronoun: `i` between a space, `(` or `"` and a space or an apostrophe.
fn is_lone_i(clusters: &[&str], index: usize) -> bool {
    let before = index.checked_sub(1).map(|at| base(clusters[at]));
    let after = clusters.get(index + 1).map(|next| base(next));
    before.is_some_and(|c| c.is_whitespace() || c == '(' || c == '"')
        && after.is_some_and(|c| c.is_whitespace() || is_apostrophe(c))
}

fn push_lower(clusters: &[&str], index: usize, out: &mut String) {
    let cluster = clusters[index];
    let after_letter = index > 0 && base(clusters[index - 1]).is_alphabetic();
    let before_letter = clusters
        .get(index + 1)
        .is_some_and(|next| base(next).is_alphabetic());
    match cluster.strip_prefix('\u{3a3}') {
        Some(marks) if after_letter && !before_letter => {
            out.push('\u{3c2}');
            out.push_str(marks);
        }
        _ => out.extend(cluster.chars().flat_map(char::to_lowercase)),
    }
}

fn push_title(cluster: &str, out: &mut String) {
    let mut chars = cluster.chars();
    if let Some(first) = chars.next() {
        match first {
            '\u{10d0}'..='\u{10fa}' | '\u{10fd}'..='\u{10ff}' => out.push(first),
            '\u{1f80}'..='\u{1faf}' => {
                out.push(char::from_u32(u32::from(first) | 0x8).unwrap_or(first));
            }
            _ => match titlecase(first) {
                Some(title) => out.push_str(title),
                None => out.extend(first.to_uppercase()),
            },
        }
        out.push_str(chars.as_str());
    }
}

/// Titlecase mappings that differ from the uppercase ones, from UnicodeData.txt and
/// SpecialCasing.txt (Unicode 16). Georgian Mkhedruli letters, which are their own titlecase,
/// and the Greek letters with ypogegrammeni in U+1F80–U+1FAF are handled in [`push_title`].
fn titlecase(c: char) -> Option<&'static str> {
    Some(match c {
        '\u{1c4}' | '\u{1c5}' | '\u{1c6}' => "\u{1c5}",
        '\u{1c7}' | '\u{1c8}' | '\u{1c9}' => "\u{1c8}",
        '\u{1ca}' | '\u{1cb}' | '\u{1cc}' => "\u{1cb}",
        '\u{1f1}' | '\u{1f2}' | '\u{1f3}' => "\u{1f2}",
        '\u{df}' => "Ss",
        '\u{587}' => "\u{535}\u{582}",
        '\u{1fb2}' => "\u{1fba}\u{345}",
        '\u{1fb3}' | '\u{1fbc}' => "\u{1fbc}",
        '\u{1fb4}' => "\u{386}\u{345}",
        '\u{1fb7}' => "\u{391}\u{342}\u{345}",
        '\u{1fc2}' => "\u{1fca}\u{345}",
        '\u{1fc3}' | '\u{1fcc}' => "\u{1fcc}",
        '\u{1fc4}' => "\u{389}\u{345}",
        '\u{1fc7}' => "\u{397}\u{342}\u{345}",
        '\u{1ff2}' => "\u{1ffa}\u{345}",
        '\u{1ff3}' | '\u{1ffc}' => "\u{1ffc}",
        '\u{1ff4}' => "\u{38f}\u{345}",
        '\u{1ff7}' => "\u{3a9}\u{342}\u{345}",
        '\u{fb00}' => "Ff",
        '\u{fb01}' => "Fi",
        '\u{fb02}' => "Fl",
        '\u{fb03}' => "Ffi",
        '\u{fb04}' => "Ffl",
        '\u{fb05}' | '\u{fb06}' => "St",
        '\u{fb13}' => "\u{544}\u{576}",
        '\u{fb14}' => "\u{544}\u{565}",
        '\u{fb15}' => "\u{544}\u{56b}",
        '\u{fb16}' => "\u{54e}\u{576}",
        '\u{fb17}' => "\u{544}\u{56d}",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use super::*;
    use proptest::prelude::*;

    const CASES: [Case; 7] = [
        Case::Upper,
        Case::Lower,
        Case::Proper,
        Case::ProperBlend,
        Case::Sentence,
        Case::SentenceBlend,
        Case::Invert,
    ];

    #[test]
    fn upper_and_lower_use_full_mappings() {
        assert_eq!(
            transform("stra\u{df}e \u{1c6} \u{fb01}x", Case::Upper),
            "STRASSE \u{1c4} FIX"
        );
        assert_eq!(
            transform("ΟΔΟΣ ΚΑΙ ΣΑΣ", Case::Lower),
            "οδο\u{3c2} και σα\u{3c2}"
        );
    }

    #[test]
    fn proper_case_capitalizes_words() {
        let text = "hello wORLD, it's o'neil's 3rd-party x2y";
        assert_eq!(
            transform(text, Case::Proper),
            "Hello World, It's O'neil's 3rd-Party X2y"
        );
        assert_eq!(
            transform(text, Case::ProperBlend),
            "Hello WORLD, It's O'neil's 3rd-Party X2y"
        );
        assert_eq!(
            transform("don\u{2019}t STOP", Case::Proper),
            "Don\u{2019}t Stop"
        );
    }

    #[test]
    fn proper_case_uses_titlecase_and_grapheme_clusters() {
        assert_eq!(
            transform("\u{1c6}ungla \u{1f3}", Case::Proper),
            "\u{1c5}ungla \u{1f2}"
        );
        assert_eq!(transform("\u{df}ig \u{fb02}at", Case::Proper), "Ssig Flat");
        let georgian =
            "\u{10e5}\u{10d0}\u{10e0}\u{10d7}\u{10e3}\u{10da}\u{10d8} \u{10d4}\u{10dc}\u{10d0}";
        assert_eq!(transform(georgian, Case::Proper), georgian);
        assert_eq!(
            transform(georgian, Case::Upper).chars().next(),
            Some('\u{1ca5}')
        );
        assert_eq!(
            transform("\u{1fb3}\u{3b4}\u{3b5} \u{1f80}", Case::Proper),
            "\u{1fbc}\u{3b4}\u{3b5} \u{1f88}"
        );
        assert_eq!(transform("e\u{301}COLE", Case::Proper), "E\u{301}cole");
        assert_eq!(transform("ΟΔΟΣ ΑΣ", Case::Proper), "Οδο\u{3c2} Α\u{3c2}");
    }

    #[test]
    fn sentence_case_rules() {
        assert_eq!(
            transform(
                "HELLO WORLD. BYE! i think i'm here (i know) i.",
                Case::Sentence
            ),
            "Hello world. Bye! I think I'm here (I know) i."
        );
        assert_eq!(
            transform("version 1.5 is out? yes!no", Case::Sentence),
            "Version 1.5 is out? Yes!no"
        );
        assert_eq!(
            transform("one\ntwo\n\nthree", Case::Sentence),
            "One\ntwo\n\nThree"
        );
        assert_eq!(
            transform("keep MIXED. case", Case::SentenceBlend),
            "Keep MIXED. Case"
        );
    }

    #[test]
    fn invert_swaps_case() {
        assert_eq!(
            transform("Hello \u{df} \u{1c5} 1", Case::Invert),
            "hELLO SS \u{1c6} 1"
        );
    }

    #[test]
    fn a_caret_converts_the_word_at_it_and_stays() {
        let text = "say hello world";
        for caret in [4, 6, 9] {
            let change = convert(text, Target::caret(caret), Case::Upper);
            assert_eq!(change.selection, Some(caret..caret));
            assert_eq!(applied(text, &change), "say HELLO world");
        }
        assert_eq!(
            applied(text, &convert(text, Target::caret(3), Case::Upper)),
            "SAY hello world"
        );
        assert!(convert("a  b", Target::caret(2), Case::Upper).is_empty());
        assert!(convert("", Target::caret(0), Case::Upper).is_empty());
        let change = convert("maß x", Target::caret(3), Case::Upper);
        assert_eq!(change.selection, Some(4..4));
        assert_eq!(applied("maß x", &change), "MASS x");
    }

    #[test]
    fn word_at_finds_unicode_words() {
        assert_eq!(word_at("foo bar", 1), Some(0..3));
        assert_eq!(word_at("foo bar", 3), Some(0..3));
        assert_eq!(word_at("foo bar", 4), Some(4..7));
        assert_eq!(word_at("don't stop", 2), Some(0..5));
        assert_eq!(word_at("a\nsnake_case x", 4), Some(2..12));
        assert_eq!(word_at("x = y", 2), None);
    }

    #[test]
    fn a_selection_stays_on_the_converted_text() {
        let change = convert("a straße b", Target::Selection(2..8), Case::Upper);
        assert_eq!(change.selection, Some(2..9));
        assert_eq!(applied("a straße b", &change), "a STRASSE b");
        assert!(convert("ABC", Target::Selection(0..3), Case::Upper).is_empty());
    }

    #[test]
    fn equal_length_conversions_edit_only_changed_runs() {
        let change = convert("Hello World", Target::Document, Case::Upper);
        assert_eq!(
            change.edits,
            vec![TextEdit::new(1..5, "ELLO"), TextEdit::new(7..11, "ORLD")]
        );
        assert_eq!(change.selection, None);
    }

    #[test]
    fn more_changed_runs_than_the_bulk_threshold_make_one_edit() {
        let text = "a ".repeat(BULK_EDIT_THRESHOLD + 1);
        let change = convert(&text, Target::Document, Case::Upper);
        assert_eq!(change.edits.len(), 1);
        assert_eq!(change.edits[0].range(), 0..text.len() - 1);
        assert_eq!(
            applied(&text, &change),
            "A ".repeat(BULK_EDIT_THRESHOLD + 1)
        );
        let text = "a ".repeat(BULK_EDIT_THRESHOLD);
        let change = convert(&text, Target::Document, Case::Upper);
        assert_eq!(change.edits.len(), BULK_EDIT_THRESHOLD);
    }

    fn ascii() -> impl Strategy<Value = String> {
        "[a-zA-Z0-9 .!?'(\"\n-]{0,40}"
    }

    proptest! {
        #[test]
        fn ascii_keeps_its_length(text in ascii()) {
            for case in CASES {
                prop_assert_eq!(transform(&text, case).len(), text.len(), "{:?}", case);
            }
        }

        #[test]
        fn conversions_are_idempotent(text in "[a-zA-Z \u{df}\u{3a3}\u{3c3}\u{1c6}\u{1c5}.'\n]{0,24}") {
            for case in [Case::Upper, Case::Lower, Case::Proper, Case::Sentence] {
                let once = transform(&text, case);
                prop_assert_eq!(transform(&once, case), once.clone(), "{:?}", case);
            }
        }

        #[test]
        fn invert_is_an_involution_on_ascii(text in ascii()) {
            prop_assert_eq!(transform(&transform(&text, Case::Invert), Case::Invert), text);
        }

        #[test]
        fn convert_replaces_the_target_with_its_transform(
            (text, target) in text_and_target(),
            case_index in 0usize..7,
        ) {
            let case = CASES[case_index];
            let chars: Vec<char> = text.chars().collect();
            let range = match &target {
                Target::Selection(range) => {
                    let (a, b) = (range.start.min(chars.len()), range.end.min(chars.len()));
                    if a == b {
                        word_at(&text, a)
                    } else {
                        Some(a.min(b)..a.max(b))
                    }
                }
                Target::Lines(_) => {
                    let lines = naive_lines(&text, &target);
                    (!lines.is_empty()).then(|| {
                        let doc = Doc::new(&text);
                        doc.start(lines.start)..doc.end(lines.end - 1)
                    })
                }
                Target::Document => Some(0..chars.len()),
            };
            let expected = match range {
                Some(range) => {
                    let old: String = chars[range.clone()].iter().collect();
                    let head: String = chars[..range.start].iter().collect();
                    let tail: String = chars[range.end..].iter().collect();
                    format!("{head}{}{tail}", transform(&old, case))
                }
                None => text.clone(),
            };
            prop_assert_eq!(applied(&text, &convert(&text, target, case)), expected);
        }
    }
}
