//! The columns that differ between two lines paired as versions of each other.
//!
//! Lines are compared word by word (runs of letters, digits and `_`, runs of whitespace, and
//! single other characters) under the same options as the lines themselves. A word replaced by
//! one word is narrowed to the characters between their common prefix and suffix. When all
//! whitespace is ignored, lines are compared character by character instead, because words can
//! then join or split. Lines longer than [`MAX_WORD_DIFF_BYTES`] get one range each: everything
//! between their common prefix and suffix.

use std::borrow::Cow;
use std::ops::Range;

use imara_diff::{Algorithm, Diff, Interner, NoSliderHeuristic, Token};

use super::key::{fold_case, fold_char};
use super::{CompareOptions, IgnoreWhitespace};

const MAX_WORD_DIFF_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Word,
    Space,
    Other,
}

struct Piece<'a> {
    text: &'a str,
    columns: Range<usize>,
    kind: Kind,
}

/// Finds the differing columns (character offsets within each line) of paired lines, reusing
/// its buffers from pair to pair.
pub(super) struct InlineDiffer<'a> {
    options: CompareOptions,
    interner: Interner<Cow<'a, str>>,
    diff: Diff,
    pieces: [Vec<Piece<'a>>; 2],
    tokens: [Vec<Token>; 2],
}

impl<'a> InlineDiffer<'a> {
    pub(super) fn new(options: CompareOptions) -> Self {
        Self {
            options,
            interner: Interner::new(64),
            diff: Diff::default(),
            pieces: [Vec::new(), Vec::new()],
            tokens: [Vec::new(), Vec::new()],
        }
    }

    pub(super) fn changed_columns(
        &mut self,
        left: &'a str,
        right: &'a str,
    ) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
        let options = self.options;
        if left.len() > MAX_WORD_DIFF_BYTES || right.len() > MAX_WORD_DIFF_BYTES {
            let whole = |line: &'a str| Piece {
                text: line,
                columns: 0..line.chars().count(),
                kind: Kind::Other,
            };
            let (left, right) = narrowed(&whole(left), &whole(right), options.ignore_case);
            let some = |columns: Range<usize>| {
                if columns.is_empty() {
                    Vec::new()
                } else {
                    vec![columns]
                }
            };
            return (some(left), some(right));
        }
        self.interner.clear();
        for (side, line) in [left, right].into_iter().enumerate() {
            compared_pieces(line, &options, &mut self.pieces[side]);
            let interner = &mut self.interner;
            self.tokens[side].clear();
            self.tokens[side].extend(
                self.pieces[side]
                    .iter()
                    .map(|piece| interner.intern(piece_key(piece, &options))),
            );
        }
        let [left_tokens, right_tokens] = &self.tokens;
        self.diff.compute_with(
            Algorithm::Myers,
            left_tokens,
            right_tokens,
            self.interner.num_tokens(),
        );
        self.diff
            .postprocess_with(left_tokens, right_tokens, NoSliderHeuristic);

        let [left, right] = &self.pieces;
        let mut columns = (Vec::new(), Vec::new());
        for hunk in self.diff.hunks() {
            let before = hunk.before.start as usize..hunk.before.end as usize;
            let after = hunk.after.start as usize..hunk.after.end as usize;
            let (left_span, right_span) = if before.len() == 1 && after.len() == 1 {
                narrowed(
                    &left[before.start],
                    &right[after.start],
                    options.ignore_case,
                )
            } else {
                (span(&left[before]), span(&right[after]))
            };
            if !left_span.is_empty() {
                columns.0.push(left_span);
            }
            if !right_span.is_empty() {
                columns.1.push(right_span);
            }
        }
        columns
    }
}

fn compared_pieces<'a>(line: &'a str, options: &CompareOptions, pieces: &mut Vec<Piece<'a>>) {
    let by_char = options.ignore_whitespace == IgnoreWhitespace::All;
    split_pieces(line, by_char, pieces);
    match options.ignore_whitespace {
        IgnoreWhitespace::None => {}
        IgnoreWhitespace::Trailing | IgnoreWhitespace::Changes => {
            if pieces.last().is_some_and(|piece| piece.kind == Kind::Space) {
                pieces.pop();
            }
        }
        IgnoreWhitespace::All => pieces.retain(|piece| piece.kind != Kind::Space),
    }
}

fn split_pieces<'a>(line: &'a str, by_char: bool, pieces: &mut Vec<Piece<'a>>) {
    pieces.clear();
    let mut start_byte = 0;
    for (column, (byte, c)) in line.char_indices().enumerate() {
        let kind = if c.is_whitespace() {
            Kind::Space
        } else if c.is_alphanumeric() || c == '_' {
            Kind::Word
        } else {
            Kind::Other
        };
        let end_byte = byte + c.len_utf8();
        let joins = kind == Kind::Space || (kind == Kind::Word && !by_char);
        if let Some(last) = pieces.last_mut()
            && joins
            && last.kind == kind
        {
            last.text = &line[start_byte..end_byte];
            last.columns.end = column + 1;
            continue;
        }
        start_byte = byte;
        pieces.push(Piece {
            text: &line[byte..end_byte],
            columns: column..column + 1,
            kind,
        });
    }
}

fn piece_key<'a>(piece: &Piece<'a>, options: &CompareOptions) -> Cow<'a, str> {
    if piece.kind == Kind::Space && options.ignore_whitespace == IgnoreWhitespace::Changes {
        return Cow::Borrowed(" ");
    }
    if options.ignore_case {
        fold_case(Cow::Borrowed(piece.text))
    } else {
        Cow::Borrowed(piece.text)
    }
}

fn span(pieces: &[Piece<'_>]) -> Range<usize> {
    match (pieces.first(), pieces.last()) {
        (Some(first), Some(last)) => first.columns.start..last.columns.end,
        _ => 0..0,
    }
}

/// The columns of two differing pieces without their common prefix and suffix.
fn narrowed(
    left: &Piece<'_>,
    right: &Piece<'_>,
    ignore_case: bool,
) -> (Range<usize>, Range<usize>) {
    let same = |&(a, b): &(char, char)| {
        if ignore_case {
            fold_char(a) == fold_char(b)
        } else {
            a == b
        }
    };
    let (a, b) = (left.text, right.text);
    let prefix = a.chars().zip(b.chars()).take_while(same).count();
    let suffix = a
        .chars()
        .rev()
        .zip(b.chars().rev())
        .take_while(same)
        .count()
        .min(left.columns.len() - prefix)
        .min(right.columns.len() - prefix);
    (
        left.columns.start + prefix..left.columns.end - suffix,
        right.columns.start + prefix..right.columns.end - suffix,
    )
}

#[cfg(test)]
#[allow(
    clippy::single_range_in_vec_init,
    reason = "expected columns are one-element lists of ranges"
)]
mod tests {
    use super::*;

    fn columns(
        left: &str,
        right: &str,
        options: CompareOptions,
    ) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
        InlineDiffer::new(options).changed_columns(left, right)
    }

    #[test]
    fn reuses_its_buffers_across_pairs() {
        let mut differ = InlineDiffer::new(CompareOptions::default());
        assert_eq!(
            differ.changed_columns("a b", "a c"),
            (vec![2..3], vec![2..3])
        );
        assert_eq!(
            differ.changed_columns("x = 1", "x = 1;"),
            (vec![], vec![5..6])
        );
        assert_eq!(
            differ.changed_columns("a b", "a c"),
            (vec![2..3], vec![2..3])
        );
    }

    #[test]
    fn highlights_changed_words() {
        let (left, right) = columns(
            "let total = price * count;",
            "let total = cost * count + 1;",
            CompareOptions::default(),
        );
        assert_eq!(left, vec![12..17]);
        assert_eq!(right, vec![12..16, 24..28]);
    }

    #[test]
    fn narrows_a_replaced_word_to_the_characters_that_differ() {
        let (left, right) = columns("colour: red", "color: red", CompareOptions::default());
        assert_eq!(left, vec![4..5]);
        assert!(right.is_empty());
        let (left, right) = columns("value_1", "value_2", CompareOptions::default());
        assert_eq!((left, right), (vec![6..7], vec![6..7]));
    }

    #[test]
    fn long_lines_get_one_range_between_their_common_ends() {
        let line = |value: char| format!("{{\"a\":{}{value}}}", "é".repeat(20_000));
        let (left, right) = columns(&line('1'), &line('2'), CompareOptions::default());
        assert_eq!((left, right), (vec![20_005..20_006], vec![20_005..20_006]));
        let (left, right) = columns(&line('1'), &line('1'), CompareOptions::default());
        assert!(left.is_empty() && right.is_empty());
    }

    #[test]
    fn counts_columns_in_characters() {
        let (left, right) = columns("æøå = 1", "æøå = 2", CompareOptions::default());
        assert_eq!((left, right), (vec![6..7], vec![6..7]));
    }

    #[test]
    fn whitespace_and_case_options_apply_inside_lines() {
        let changes = CompareOptions {
            ignore_whitespace: IgnoreWhitespace::Changes,
            ..CompareOptions::default()
        };
        let (left, right) = columns("a  =  b;  ", "a = c;", changes);
        assert_eq!((left, right), (vec![6..7], vec![4..5]));

        let all = CompareOptions {
            ignore_whitespace: IgnoreWhitespace::All,
            ..CompareOptions::default()
        };
        let (left, right) = columns("fo o(x)", "foo(y)", all);
        assert_eq!((left, right), (vec![5..6], vec![4..5]));

        let case = CompareOptions {
            ignore_case: true,
            ..CompareOptions::default()
        };
        let (left, right) = columns("Hello World", "HELLO there", case);
        assert_eq!((left, right), (vec![6..11], vec![6..11]));

        let (left, right) = columns("x = 1   ", "x = 1", CompareOptions::default());
        assert_eq!((left, right), (vec![5..8], vec![]));
    }
}
