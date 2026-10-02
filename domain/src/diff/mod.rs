//! Line-by-line comparison of two texts for Tools › Compare.
//!
//! [`compare`] finds the [`Hunk`]s of added, removed, changed and moved lines, the columns that
//! differ inside each pair of changed lines, and an [`Alignment`] that two views use to pad and
//! scroll in step. Texts are LF-normalized (ADR-008) and split into lines as
//! [`LineIndex`](crate::text::LineIndex) does: a text ending in `\n` has an empty last line.
//!
//! Lines are matched with imara-diff's histogram algorithm and git's indent heuristic. Inside a
//! block of differing lines, lines that are versions of each other (similar character bigrams)
//! are paired as changed; the rest are added or removed, shown side by side.

mod align;
mod inline;
mod key;
mod moves;
mod pair;
#[cfg(test)]
#[allow(
    clippy::single_range_in_vec_init,
    reason = "expected columns are one-element lists of ranges"
)]
mod tests;

use std::borrow::Cow;
use std::ops::Range;

use imara_diff::{Algorithm, Diff, IndentHeuristic, IndentLevel, Interner, Token};

pub use align::{AlignedChunk, Alignment, Pad, RowSlot};

use align::AlignBuilder;
use inline::InlineDiffer;
use moves::Moves;
use pair::Pairer;

/// Tab width for git's indent heuristic, which places ambiguous hunks between blocks.
const TAB_WIDTH: u8 = 8;

/// Which whitespace differences [`compare`] ignores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum IgnoreWhitespace {
    /// Every whitespace character counts.
    #[default]
    None,
    /// Whitespace at the end of a line is ignored.
    Trailing,
    /// Trailing whitespace is ignored, and any run of whitespace equals any other.
    Changes,
    /// All whitespace is ignored.
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CompareOptions {
    pub ignore_whitespace: IgnoreWhitespace,
    pub ignore_case: bool,
    /// Lines that are empty once the whitespace option applies take no part in the comparison.
    pub ignore_empty_lines: bool,
    /// Report blocks of lines removed in one place and added in another as [`Move`]s.
    pub detect_moves: bool,
}

impl Default for CompareOptions {
    fn default() -> Self {
        Self {
            ignore_whitespace: IgnoreWhitespace::None,
            ignore_case: false,
            ignore_empty_lines: false,
            detect_moves: true,
        }
    }
}

/// One of the two compared texts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub const fn other(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Right => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HunkKind {
    /// Lines only on the right.
    Added,
    /// Lines only on the left.
    Removed,
    /// Lines on both sides.
    Changed,
    /// Every line of the hunk belongs to a [`Move`].
    Moved,
}

/// A block of differing lines between runs of equal lines; the unit of next and previous
/// difference. An empty range is an insertion point: the other side's lines go before line
/// `start`. Ignored empty lines inside the block are part of it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Hunk {
    pub kind: HunkKind,
    pub left: Range<usize>,
    pub right: Range<usize>,
}

impl Hunk {
    pub fn lines(&self, side: Side) -> &Range<usize> {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }
}

/// How a line differs, for colouring it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LineKind {
    /// Only on the right.
    Added,
    /// Only on the left.
    Removed,
    /// Paired with a changed version on the other side; see [`InlineDiff`].
    Changed,
    /// Part of a [`Move`].
    Moved,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LineRun {
    pub lines: Range<usize>,
    pub kind: LineKind,
}

/// A left and a right line paired as versions of each other, with the columns (character
/// offsets within each line) that differ.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InlineDiff {
    pub left_line: usize,
    pub right_line: usize,
    pub left: Vec<Range<usize>>,
    pub right: Vec<Range<usize>>,
}

impl InlineDiff {
    pub fn line(&self, side: Side) -> usize {
        match side {
            Side::Left => self.left_line,
            Side::Right => self.right_line,
        }
    }

    pub fn columns(&self, side: Side) -> &[Range<usize>] {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }
}

/// Lines removed at `left` that appear again at `right`. The ranges have the same length unless
/// ignored empty lines sit inside the block.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Move {
    pub left: Range<usize>,
    pub right: Range<usize>,
}

impl Move {
    pub fn lines(&self, side: Side) -> &Range<usize> {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }
}

/// Line counts for the comparison summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CompareStats {
    pub hunks: usize,
    pub added: usize,
    pub removed: usize,
    /// Pairs of changed lines.
    pub changed: usize,
    /// Moved lines, counted on the left.
    pub moved: usize,
}

impl CompareStats {
    /// "12 lines added, 3 removed, 5 changed, 1 moved", leaving out what is zero; changed
    /// counts pairs of lines. Identical texts are "No differences".
    pub fn summary(&self) -> String {
        let parts: Vec<(usize, &str)> = [
            (self.added, "added"),
            (self.removed, "removed"),
            (self.changed, "changed"),
            (self.moved, "moved"),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .collect();
        if parts.is_empty() {
            return "No differences".to_owned();
        }
        parts
            .iter()
            .enumerate()
            .map(|(index, (count, what))| match (index, count) {
                (0, 1) => format!("1 line {what}"),
                (0, count) => format!("{count} lines {what}"),
                (_, count) => format!("{count} {what}"),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comparison {
    /// In order on both sides.
    pub hunks: Vec<Hunk>,
    /// Sorted by line on both sides.
    pub inline: Vec<InlineDiff>,
    /// Sorted by left line.
    pub moves: Vec<Move>,
    pub stats: CompareStats,
    pub alignment: Alignment,
    runs: [Vec<LineRun>; 2],
    hunk_rows: Vec<Range<usize>>,
}

impl Comparison {
    pub fn is_identical(&self) -> bool {
        self.hunks.is_empty()
    }

    /// The differing lines of `side` with their kind, in order. Lines not listed are equal.
    pub fn line_runs(&self, side: Side) -> &[LineRun] {
        &self.runs[side.index()]
    }

    pub fn line_kind(&self, side: Side, line: usize) -> Option<LineKind> {
        let runs = self.line_runs(side);
        let index = runs.partition_point(|run| run.lines.end <= line);
        runs.get(index)
            .filter(|run| run.lines.start <= line)
            .map(|run| run.kind)
    }

    pub fn inline_at(&self, side: Side, line: usize) -> Option<&InlineDiff> {
        let index = self.inline.partition_point(|diff| diff.line(side) < line);
        self.inline
            .get(index)
            .filter(|diff| diff.line(side) == line)
    }

    /// The rows of the [`Alignment`] that hunk `index` covers.
    pub fn hunk_rows(&self, index: usize) -> Range<usize> {
        self.hunk_rows[index].clone()
    }

    /// The hunk whose lines on `side` include `line`.
    pub fn hunk_at(&self, side: Side, line: usize) -> Option<usize> {
        let index = self
            .hunks
            .partition_point(|hunk| hunk.lines(side).end <= line);
        self.hunks
            .get(index)
            .filter(|hunk| hunk.lines(side).start <= line)
            .map(|_| index)
    }

    /// The first hunk that starts below the row of `line` on `side`, for Next Difference.
    /// Wrapping around is left to the caller.
    pub fn next_hunk(&self, side: Side, line: usize) -> Option<usize> {
        let row = self.alignment.row_of(side, line);
        let index = self.hunk_rows.partition_point(|rows| rows.start <= row);
        (index < self.hunks.len()).then_some(index)
    }

    /// The last hunk that starts above the row of `line` on `side`, for Previous Difference.
    pub fn prev_hunk(&self, side: Side, line: usize) -> Option<usize> {
        let row = self.alignment.row_of(side, line);
        self.hunk_rows
            .partition_point(|rows| rows.start < row)
            .checked_sub(1)
    }
}

/// Compares `left` with `right` line by line.
pub fn compare(left: &str, right: &str, options: &CompareOptions) -> Comparison {
    let left_lines: Vec<&str> = left.split('\n').collect();
    let right_lines: Vec<&str> = right.split('\n').collect();
    let mut interner = Interner::new(left_lines.len() + right_lines.len());
    let mut samples = Vec::new();
    let left = Sequence::new(left_lines, options, &mut interner, &mut samples);
    let right = Sequence::new(right_lines, options, &mut interner, &mut samples);

    let mut diff = Diff::default();
    diff.compute_with(
        Algorithm::Histogram,
        &left.tokens,
        &right.tokens,
        interner.num_tokens(),
    );
    let indents: Vec<IndentLevel> = samples
        .iter()
        .map(|line| IndentLevel::for_line(line.chars(), TAB_WIDTH))
        .collect();
    diff.postprocess_with(
        &left.tokens,
        &right.tokens,
        IndentHeuristic::new(|token: Token| indents[token.0 as usize]),
    );
    let hunks: Vec<imara_diff::Hunk> = diff.hunks().collect();

    let moves = if options.detect_moves {
        Moves::detect(
            &left.tokens,
            &right.tokens,
            &hunks,
            interner.num_tokens() as usize,
            |token| moves::is_significant(&interner[token]),
        )
    } else {
        Moves::none(left.tokens.len(), right.tokens.len())
    };

    let mut walk = Walk {
        sides: [&left, &right],
        interner: &interner,
        pairer: Pairer::new(),
        differ: InlineDiffer::new(*options),
        moved: [&moves.left, &moves.right],
        align: AlignBuilder::new(),
        hunks: Vec::new(),
        inline: Vec::new(),
        kinds: [Vec::new(), Vec::new()],
    };
    let (mut next_left, mut next_right) = (0, 0);
    for hunk in &hunks {
        let before = hunk.before.start as usize..hunk.before.end as usize;
        let after = hunk.after.start as usize..hunk.after.end as usize;
        while next_left < before.start {
            walk.equal(next_left, next_right);
            next_left += 1;
            next_right += 1;
        }
        (next_left, next_right) = (before.end, after.end);
        walk.hunk(before, after);
    }
    while next_left < left.tokens.len() {
        walk.equal(next_left, next_right);
        next_left += 1;
        next_right += 1;
    }
    walk.finish(&moves)
}

/// The lines of one text and the ones that take part in the comparison (all of them unless
/// empty lines are ignored), as interned keys.
struct Sequence<'a> {
    lines: Vec<&'a str>,
    kept: Vec<usize>,
    tokens: Vec<Token>,
}

impl<'a> Sequence<'a> {
    fn new(
        lines: Vec<&'a str>,
        options: &CompareOptions,
        interner: &mut Interner<Cow<'a, str>>,
        samples: &mut Vec<&'a str>,
    ) -> Self {
        let mut kept = Vec::with_capacity(lines.len());
        let mut tokens = Vec::with_capacity(lines.len());
        for (index, &line) in lines.iter().enumerate() {
            let key = key::line_key(line, options);
            if options.ignore_empty_lines && key.is_empty() {
                continue;
            }
            let token = interner.intern(key);
            if token.0 as usize == samples.len() {
                samples.push(line);
            }
            kept.push(index);
            tokens.push(token);
        }
        Self {
            lines,
            kept,
            tokens,
        }
    }

    /// The line of the kept line `kept`, or the line count past the last one.
    fn line_at(&self, kept: usize) -> usize {
        self.kept.get(kept).copied().unwrap_or(self.lines.len())
    }

    fn lines_of(&self, kept: &Range<usize>) -> Range<usize> {
        self.kept[kept.start]..self.kept[kept.end - 1] + 1
    }
}

/// Emits rows, line kinds, hunks and inline diffs in order while walking the line diff.
struct Walk<'w, 'a> {
    sides: [&'w Sequence<'a>; 2],
    interner: &'w Interner<Cow<'a, str>>,
    pairer: Pairer,
    differ: InlineDiffer<'a>,
    moved: [&'w [bool]; 2],
    align: AlignBuilder,
    hunks: Vec<Hunk>,
    inline: Vec<InlineDiff>,
    kinds: [Vec<(usize, LineKind)>; 2],
}

impl<'w, 'a> Walk<'w, 'a> {
    fn key(&self, side: Side, kept: usize) -> &'w str {
        let interner: &'w Interner<_> = self.interner;
        &interner[self.sides[side.index()].tokens[kept]]
    }

    /// Emits the ignored lines of `side` before `line`.
    fn flush(&mut self, side: Side, line: usize) {
        while self.align.next(side) < line {
            self.align.one(side);
        }
    }

    fn equal(&mut self, left: usize, right: usize) {
        self.flush(Side::Left, self.sides[0].kept[left]);
        self.flush(Side::Right, self.sides[1].kept[right]);
        self.align.both();
    }

    fn single(&mut self, side: Side, kept: usize) {
        let line = self.sides[side.index()].kept[kept];
        self.flush(side, line);
        self.align.one(side);
        let kind = match side {
            _ if self.moved[side.index()][kept] => LineKind::Moved,
            Side::Left => LineKind::Removed,
            Side::Right => LineKind::Added,
        };
        self.kinds[side.index()].push((line, kind));
    }

    fn pair(&mut self, left: usize, right: usize) {
        let left_line = self.sides[0].kept[left];
        let right_line = self.sides[1].kept[right];
        self.flush(Side::Left, left_line);
        self.flush(Side::Right, right_line);
        self.align.both();
        self.kinds[0].push((left_line, LineKind::Changed));
        self.kinds[1].push((right_line, LineKind::Changed));
        let (left_columns, right_columns) = self.differ.changed_columns(
            self.sides[0].lines[left_line],
            self.sides[1].lines[right_line],
        );
        self.inline.push(InlineDiff {
            left_line,
            right_line,
            left: left_columns,
            right: right_columns,
        });
    }

    fn hunk(&mut self, left: Range<usize>, right: Range<usize>) {
        self.flush(Side::Left, self.sides[0].line_at(left.start));
        self.flush(Side::Right, self.sides[1].line_at(right.start));
        self.align.seal();
        let start = self.align.next_lines();

        let unmoved = |side: Side, range: &Range<usize>| -> Vec<usize> {
            range
                .clone()
                .filter(|&kept| !self.moved[side.index()][kept])
                .collect()
        };
        let left_free = unmoved(Side::Left, &left);
        let right_free = unmoved(Side::Right, &right);
        let keys = |side: Side, free: &[usize]| -> Vec<&'w str> {
            free.iter().map(|&kept| self.key(side, kept)).collect()
        };
        let (left_keys, right_keys) =
            (keys(Side::Left, &left_free), keys(Side::Right, &right_free));
        let pairs = self.pairer.pair(&left_keys, &right_keys);

        let (mut next_left, mut next_right) = (left.start, right.start);
        for (paired_left, paired_right) in pairs {
            let (paired_left, paired_right) = (left_free[paired_left], right_free[paired_right]);
            for kept in next_left..paired_left {
                self.single(Side::Left, kept);
            }
            for kept in next_right..paired_right {
                self.single(Side::Right, kept);
            }
            self.pair(paired_left, paired_right);
            (next_left, next_right) = (paired_left + 1, paired_right + 1);
        }
        for kept in next_left..left.end {
            self.single(Side::Left, kept);
        }
        for kept in next_right..right.end {
            self.single(Side::Right, kept);
        }

        let end = self.align.next_lines();
        self.align.seal();
        let moved = self.moved[0][left.clone()].iter().filter(|&&m| m).count()
            + self.moved[1][right.clone()].iter().filter(|&&m| m).count();
        let kind = if moved == left.len() + right.len() {
            HunkKind::Moved
        } else if left.is_empty() {
            HunkKind::Added
        } else if right.is_empty() {
            HunkKind::Removed
        } else {
            HunkKind::Changed
        };
        self.hunks.push(Hunk {
            kind,
            left: start[0]..end[0],
            right: start[1]..end[1],
        });
    }

    fn finish(mut self, moves: &Moves) -> Comparison {
        self.flush(Side::Left, self.sides[0].lines.len());
        self.flush(Side::Right, self.sides[1].lines.len());
        let alignment = self.align.finish();
        let hunk_rows = self
            .hunks
            .iter()
            .map(|hunk| alignment.span_rows(&hunk.left, &hunk.right))
            .collect();
        let moves = moves
            .blocks
            .iter()
            .map(|(left, right)| Move {
                left: self.sides[0].lines_of(left),
                right: self.sides[1].lines_of(right),
            })
            .collect();
        let [left_kinds, right_kinds] = self.kinds;
        let count = |kinds: &[(usize, LineKind)], kind: LineKind| {
            kinds.iter().filter(|(_, k)| *k == kind).count()
        };
        let stats = CompareStats {
            hunks: self.hunks.len(),
            added: count(&right_kinds, LineKind::Added),
            removed: count(&left_kinds, LineKind::Removed),
            changed: self.inline.len(),
            moved: count(&left_kinds, LineKind::Moved),
        };
        Comparison {
            hunks: self.hunks,
            inline: self.inline,
            moves,
            stats,
            alignment,
            runs: [runs(left_kinds), runs(right_kinds)],
            hunk_rows,
        }
    }
}

fn runs(kinds: Vec<(usize, LineKind)>) -> Vec<LineRun> {
    let mut runs: Vec<LineRun> = Vec::new();
    for (line, kind) in kinds {
        match runs.last_mut() {
            Some(run) if run.kind == kind && run.lines.end == line => run.lines.end += 1,
            _ => runs.push(LineRun {
                lines: line..line + 1,
                kind,
            }),
        }
    }
    runs
}
