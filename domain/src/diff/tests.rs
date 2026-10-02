use std::time::Instant;

use proptest::prelude::*;

use super::*;

fn lines(text: &str) -> Vec<&str> {
    text.split('\n').collect()
}

/// Replaces each hunk's left lines with its right lines.
fn apply_hunks(left: &str, right: &str, comparison: &Comparison) -> String {
    let (left, right) = (lines(left), lines(right));
    let mut out: Vec<&str> = Vec::new();
    let mut next = 0;
    for hunk in &comparison.hunks {
        out.extend(&left[next..hunk.left.start]);
        out.extend(&right[hunk.right.clone()]);
        next = hunk.left.end;
    }
    out.extend(&left[next..]);
    out.join("\n")
}

fn rows(alignment: &Alignment) -> Vec<(Option<usize>, Option<usize>)> {
    let line = |slot| match slot {
        RowSlot::Line(line) => Some(line),
        RowSlot::Pad { .. } => None,
    };
    (0..alignment.rows())
        .map(|row| {
            (
                line(alignment.slot(Side::Left, row)),
                line(alignment.slot(Side::Right, row)),
            )
        })
        .collect()
}

fn exact() -> CompareOptions {
    CompareOptions::default()
}

#[test]
fn identical_texts_have_no_hunks_and_align_line_for_line() {
    let text = "fn main() {\n    println!(\"hi\");\n}\n";
    let comparison = compare(text, text, &exact());
    assert!(comparison.is_identical());
    assert_eq!(comparison.stats, CompareStats::default());
    assert_eq!(comparison.alignment.rows(), 4);
    assert_eq!(comparison.alignment.chunks().len(), 1);
    assert!(comparison.alignment.pads(Side::Left).is_empty());
    for line in 0..4 {
        assert_eq!(
            comparison.alignment.counterpart(Side::Left, line),
            Some(line)
        );
    }
    assert!(compare("", "", &exact()).is_identical());
}

#[test]
fn an_added_line_pads_the_left_view() {
    let comparison = compare("a\nb\nc", "a\nx\nb\nc", &exact());
    assert_eq!(
        comparison.hunks,
        vec![Hunk {
            kind: HunkKind::Added,
            left: 1..1,
            right: 1..2,
        }]
    );
    assert_eq!(
        rows(&comparison.alignment),
        vec![
            (Some(0), Some(0)),
            (None, Some(1)),
            (Some(1), Some(2)),
            (Some(2), Some(3)),
        ]
    );
    assert_eq!(
        comparison.alignment.pads(Side::Left),
        vec![Pad { before: 1, rows: 1 }]
    );
    assert_eq!(comparison.line_kind(Side::Right, 1), Some(LineKind::Added));
    assert_eq!(comparison.line_kind(Side::Right, 2), None);
    assert_eq!(comparison.alignment.counterpart(Side::Left, 1), Some(2));
    assert_eq!(comparison.alignment.scroll_line(Side::Right, 1), 1);
    assert_eq!(comparison.stats.added, 1);
}

#[test]
fn a_removed_line_pads_the_right_view() {
    let comparison = compare("a\nb\nc", "a\nc", &exact());
    assert_eq!(
        comparison.hunks,
        vec![Hunk {
            kind: HunkKind::Removed,
            left: 1..2,
            right: 1..1,
        }]
    );
    assert_eq!(
        comparison.alignment.pads(Side::Right),
        vec![Pad { before: 1, rows: 1 }]
    );
    assert_eq!(comparison.line_kind(Side::Left, 1), Some(LineKind::Removed));
    assert_eq!(comparison.stats.removed, 1);
}

#[test]
fn a_changed_line_carries_the_columns_that_differ() {
    let comparison = compare(
        "fn main() {\n    let x = 1;\n}",
        "fn main() {\n    let x = 2;\n}",
        &exact(),
    );
    assert_eq!(
        comparison.hunks,
        vec![Hunk {
            kind: HunkKind::Changed,
            left: 1..2,
            right: 1..2,
        }]
    );
    assert_eq!(
        comparison.inline,
        vec![InlineDiff {
            left_line: 1,
            right_line: 1,
            left: vec![12..13],
            right: vec![12..13],
        }]
    );
    assert_eq!(
        comparison
            .inline_at(Side::Right, 1)
            .map(|d| d.columns(Side::Left)),
        Some(&[12..13][..])
    );
    assert_eq!(comparison.inline_at(Side::Right, 0), None);
    assert_eq!(comparison.stats.changed, 1);
}

#[test]
fn a_block_pairs_similar_lines_and_shows_the_rest_beside_padding() {
    let left = "start\nfn parse(input: &str) -> Result<Value> {\n    parse_value(input)\nend";
    let right = "start\n// Parses a value.\nfn parse(input: &str) -> Option<Value> {\n    parse_value(input).ok()\n}\nend";
    let comparison = compare(left, right, &exact());
    assert_eq!(
        comparison.hunks,
        vec![Hunk {
            kind: HunkKind::Changed,
            left: 1..3,
            right: 1..5,
        }]
    );
    assert_eq!(
        rows(&comparison.alignment),
        vec![
            (Some(0), Some(0)),
            (None, Some(1)),
            (Some(1), Some(2)),
            (Some(2), Some(3)),
            (None, Some(4)),
            (Some(3), Some(5)),
        ]
    );
    assert_eq!(
        comparison.line_runs(Side::Right),
        &[
            LineRun {
                lines: 1..2,
                kind: LineKind::Added,
            },
            LineRun {
                lines: 2..4,
                kind: LineKind::Changed,
            },
            LineRun {
                lines: 4..5,
                kind: LineKind::Added,
            },
        ]
    );
    assert_eq!(
        comparison.alignment.pads(Side::Left),
        vec![Pad { before: 1, rows: 1 }, Pad { before: 3, rows: 1 }]
    );
    assert_eq!(comparison.hunk_rows(0), 1..5);
}

#[test]
fn unpaired_lines_of_a_block_share_rows() {
    let comparison = compare("a\nold one\nold two\nz", "a\n12345\nz", &exact());
    assert_eq!(comparison.hunks[0].kind, HunkKind::Changed);
    assert!(comparison.inline.is_empty());
    assert_eq!(
        rows(&comparison.alignment),
        vec![
            (Some(0), Some(0)),
            (Some(1), Some(1)),
            (Some(2), None),
            (Some(3), Some(2)),
        ]
    );
    assert_eq!(comparison.line_kind(Side::Left, 2), Some(LineKind::Removed));
    assert_eq!(comparison.line_kind(Side::Right, 1), Some(LineKind::Added));
}

#[test]
fn a_missing_final_newline_is_an_empty_last_line() {
    let comparison = compare("a\n", "a", &exact());
    assert_eq!(
        comparison.hunks,
        vec![Hunk {
            kind: HunkKind::Removed,
            left: 1..2,
            right: 1..1,
        }]
    );
    assert_eq!(comparison.alignment.line_count(Side::Left), 2);
    assert_eq!(comparison.alignment.line_count(Side::Right), 1);
}

#[test]
fn whitespace_and_case_options_hide_differences() {
    let options = |ignore_whitespace, ignore_case| CompareOptions {
        ignore_whitespace,
        ignore_case,
        ..exact()
    };
    let same = |left: &str, right: &str, options: CompareOptions| {
        compare(left, right, &options).is_identical()
    };
    assert!(!same(
        "a  \nb",
        "a\nb",
        options(IgnoreWhitespace::None, false)
    ));
    assert!(same(
        "a  \nb",
        "a\nb",
        options(IgnoreWhitespace::Trailing, false)
    ));
    assert!(!same(" a", "a", options(IgnoreWhitespace::Trailing, false)));
    assert!(same(
        "a  b\t",
        "a\tb",
        options(IgnoreWhitespace::Changes, false)
    ));
    assert!(!same(
        "a b",
        "ab",
        options(IgnoreWhitespace::Changes, false)
    ));
    assert!(same("a b", "ab", options(IgnoreWhitespace::All, false)));
    assert!(!same(
        "Hello",
        "hello",
        options(IgnoreWhitespace::None, false)
    ));
    assert!(same(
        "Hello",
        "hello",
        options(IgnoreWhitespace::None, true)
    ));
    assert!(same(
        "HELLO  World",
        "hello world",
        options(IgnoreWhitespace::Changes, true)
    ));
}

#[test]
fn ignored_empty_lines_are_aligned_against_padding() {
    let options = CompareOptions {
        ignore_empty_lines: true,
        ..exact()
    };
    let comparison = compare("a\n\nb", "a\nb", &options);
    assert!(comparison.is_identical());
    assert_eq!(
        rows(&comparison.alignment),
        vec![(Some(0), Some(0)), (Some(1), None), (Some(2), Some(1))]
    );
    assert_eq!(comparison.alignment.counterpart(Side::Left, 2), Some(1));

    let whitespace_only = CompareOptions {
        ignore_whitespace: IgnoreWhitespace::Trailing,
        ..options
    };
    assert!(compare("a\n   \nb\n", "a\nb", &whitespace_only).is_identical());

    let comparison = compare("a\n\nx\n\nb", "a\nb", &options);
    assert_eq!(
        comparison.hunks,
        vec![Hunk {
            kind: HunkKind::Removed,
            left: 2..3,
            right: 1..1,
        }]
    );
    assert_eq!(comparison.line_kind(Side::Left, 1), None);
    assert_eq!(comparison.line_kind(Side::Left, 3), None);
}

#[test]
fn a_moved_block_is_reported_on_both_sides() {
    let left = "fn alpha() {\n    first_step();\n}\nfn beta() {\n    second_step();\n}\n";
    let right = "fn beta() {\n    second_step();\n}\nfn alpha() {\n    first_step();\n}\n";
    let comparison = compare(left, right, &exact());
    assert_eq!(comparison.moves.len(), 1);
    let moved = &comparison.moves[0];
    assert_eq!(moved.left.len(), 3);
    assert_eq!(moved.right.len(), 3);
    assert_eq!(
        lines(left)[moved.left.clone()],
        lines(right)[moved.right.clone()]
    );
    assert!(
        comparison
            .hunks
            .iter()
            .all(|hunk| hunk.kind == HunkKind::Moved)
    );
    assert_eq!(comparison.stats.moved, 3);
    assert_eq!(
        comparison.line_kind(Side::Left, moved.left.start),
        Some(LineKind::Moved)
    );
    assert_eq!(apply_hunks(left, right, &comparison), right);

    let without = CompareOptions {
        detect_moves: false,
        ..exact()
    };
    let comparison = compare(left, right, &without);
    assert!(comparison.moves.is_empty());
    assert!(
        comparison
            .hunks
            .iter()
            .all(|hunk| hunk.kind != HunkKind::Moved)
    );
}

#[test]
fn short_lines_never_anchor_a_move() {
    let comparison = compare("}\na\nb\nc", "a\nb\nc\n}", &exact());
    assert!(comparison.moves.is_empty());
}

#[test]
fn navigation_steps_between_hunks_by_row() {
    let left = "1\n2\n3\n4\n5\n6\n7\n8";
    let right = "1\nnew\n2\n3\n4\n6\n7\n8 changed";
    let comparison = compare(left, right, &exact());
    let starts: Vec<usize> = comparison
        .hunks
        .iter()
        .map(|hunk| hunk.left.start)
        .collect();
    assert_eq!(starts, vec![1, 4, 7]);
    assert_eq!(comparison.next_hunk(Side::Left, 0), Some(0));
    assert_eq!(comparison.next_hunk(Side::Left, 1), Some(1));
    assert_eq!(comparison.next_hunk(Side::Left, 4), Some(2));
    assert_eq!(comparison.next_hunk(Side::Left, 7), None);
    assert_eq!(comparison.prev_hunk(Side::Left, 7), Some(1));
    assert_eq!(comparison.prev_hunk(Side::Left, 1), Some(0));
    assert_eq!(comparison.prev_hunk(Side::Left, 0), None);
    assert_eq!(comparison.next_hunk(Side::Right, 1), Some(1));
    assert_eq!(comparison.hunk_at(Side::Left, 4), Some(1));
    assert_eq!(comparison.hunk_at(Side::Left, 5), None);
    assert_eq!(comparison.hunk_at(Side::Right, 1), Some(0));
}

#[test]
fn the_summary_names_what_differs() {
    let stats = |added, removed, changed, moved| CompareStats {
        hunks: 1,
        added,
        removed,
        changed,
        moved,
    };
    assert_eq!(
        stats(12, 3, 5, 1).summary(),
        "12 lines added, 3 removed, 5 changed, 1 moved"
    );
    assert_eq!(stats(0, 1, 0, 0).summary(), "1 line removed");
    assert_eq!(stats(0, 0, 2, 4).summary(), "2 lines changed, 4 moved");
    assert_eq!(CompareStats::default().summary(), "No differences");
    let comparison = compare(
        "let total = 1;\nkeep\nkeep too",
        "let total = 2;\nkeep\nkeep too\nnew line",
        &exact(),
    );
    assert_eq!(comparison.stats.summary(), "1 line added, 1 changed");
}

prop_compose! {
    fn texts()(
        left in prop::collection::vec(0usize..8, 0..30),
        right in prop::collection::vec(0usize..8, 0..30),
    ) -> (String, String) {
        const LINES: [&str; 8] = [
            "fn apply(edit: Edit) {",
            "}",
            "",
            "    let total = price * count;",
            "    let total = cost * count;",
            "    return total;",
            "  RETURN   total; ",
            "// done",
        ];
        let join = |picks: Vec<usize>| {
            picks.into_iter().map(|i| LINES[i]).collect::<Vec<_>>().join("\n")
        };
        (join(left), join(right))
    }
}

fn any_options() -> impl Strategy<Value = CompareOptions> {
    (
        prop_oneof![
            Just(IgnoreWhitespace::None),
            Just(IgnoreWhitespace::Trailing),
            Just(IgnoreWhitespace::Changes),
            Just(IgnoreWhitespace::All),
        ],
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(ignore_whitespace, ignore_case, ignore_empty_lines, detect_moves)| CompareOptions {
                ignore_whitespace,
                ignore_case,
                ignore_empty_lines,
                detect_moves,
            },
        )
}

/// The keys of the lines that take part in the comparison.
fn compared_keys(text: &str, options: &CompareOptions) -> Vec<String> {
    lines(text)
        .into_iter()
        .map(|line| key::line_key(line, options).into_owned())
        .filter(|key| !(options.ignore_empty_lines && key.is_empty()))
        .collect()
}

fn check_structure(comparison: &Comparison, left: &str, right: &str) -> Result<(), TestCaseError> {
    let counts = [lines(left).len(), lines(right).len()];
    let alignment = &comparison.alignment;
    prop_assert_eq!(alignment.line_count(Side::Left), counts[0]);
    prop_assert_eq!(alignment.line_count(Side::Right), counts[1]);
    let rows = rows(alignment);
    let listed = |side: Side| -> Vec<usize> {
        rows.iter()
            .filter_map(|&(left, right)| if side == Side::Left { left } else { right })
            .collect()
    };
    prop_assert_eq!(listed(Side::Left), (0..counts[0]).collect::<Vec<_>>());
    prop_assert_eq!(listed(Side::Right), (0..counts[1]).collect::<Vec<_>>());
    for side in [Side::Left, Side::Right] {
        for line in 0..counts[side.index()] {
            let row = alignment.row_of(side, line);
            prop_assert_eq!(alignment.slot(side, row), RowSlot::Line(line));
            if let Some(other) = alignment.counterpart(side, line) {
                prop_assert_eq!(alignment.counterpart(side.other(), other), Some(line));
            }
        }
        let pad_rows: usize = alignment.pads(side).iter().map(|pad| pad.rows).sum();
        prop_assert_eq!(pad_rows + counts[side.index()], alignment.rows());
    }
    for pair in comparison.hunks.windows(2) {
        prop_assert!(
            pair[0].left.end < pair[1].left.start || pair[0].right.end < pair[1].right.start
        );
        prop_assert!(pair[0].left.end <= pair[1].left.start);
        prop_assert!(pair[0].right.end <= pair[1].right.start);
    }
    for (index, hunk) in comparison.hunks.iter().enumerate() {
        prop_assert!(!hunk.left.is_empty() || !hunk.right.is_empty());
        let rows = comparison.hunk_rows(index);
        prop_assert!(rows.start < rows.end && rows.end <= alignment.rows());
    }
    for side in [Side::Left, Side::Right] {
        for run in comparison.line_runs(side) {
            for line in run.lines.clone() {
                let hunk = comparison.hunk_at(side, line);
                prop_assert!(hunk.is_some(), "{side:?} line {line} is outside every hunk");
            }
        }
    }
    let texts = [lines(left), lines(right)];
    for diff in &comparison.inline {
        prop_assert_eq!(
            comparison.line_kind(Side::Left, diff.left_line),
            Some(LineKind::Changed)
        );
        prop_assert_eq!(
            comparison.line_kind(Side::Right, diff.right_line),
            Some(LineKind::Changed)
        );
        for side in [Side::Left, Side::Right] {
            let width = texts[side.index()][diff.line(side)].chars().count();
            let columns = diff.columns(side);
            prop_assert!(columns.iter().all(|c| c.start < c.end && c.end <= width));
            prop_assert!(columns.windows(2).all(|pair| pair[0].end <= pair[1].start));
        }
    }
    Ok(())
}

proptest! {
    #[test]
    fn hunks_turn_the_left_text_into_the_right((left, right) in texts(), detect_moves in any::<bool>()) {
        let options = CompareOptions { detect_moves, ..exact() };
        let comparison = compare(&left, &right, &options);
        prop_assert_eq!(apply_hunks(&left, &right, &comparison), right.clone());
        check_structure(&comparison, &left, &right)?;
        for (row, (l, r)) in rows(&comparison.alignment).into_iter().enumerate() {
            let in_hunk = (0..comparison.hunks.len()).any(|i| comparison.hunk_rows(i).contains(&row));
            if let (Some(l), Some(r), false) = (l, r, in_hunk) {
                prop_assert_eq!(lines(&left)[l], lines(&right)[r]);
            }
        }
    }

    #[test]
    fn identical_texts_give_no_hunks((text, _) in texts(), options in any_options()) {
        let comparison = compare(&text, &text, &options);
        prop_assert!(comparison.is_identical());
        prop_assert!(comparison.moves.is_empty() && comparison.inline.is_empty());
        let rows = comparison.alignment.rows();
        prop_assert_eq!(rows, lines(&text).len());
    }

    #[test]
    fn options_only_hide_differences_they_name((left, right) in texts(), options in any_options()) {
        let comparison = compare(&left, &right, &options);
        let patched = apply_hunks(&left, &right, &comparison);
        prop_assert_eq!(compared_keys(&patched, &options), compared_keys(&right, &options));
        check_structure(&comparison, &left, &right)?;
    }
}

struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n
    }
}

fn source_line(rng: &mut Lcg) -> String {
    const WORDS: [&str; 16] = [
        "let", "value", "match", "self", "config", "index", "buffer", "line", "offset", "range",
        "error", "result", "text", "edit", "count", "total",
    ];
    let indent = "    ".repeat(rng.below(4) as usize);
    let words: Vec<&str> = (0..2 + rng.below(6))
        .map(|_| WORDS[rng.below(16) as usize])
        .collect();
    format!("{indent}{} = {};", words.join(" "), rng.below(1000))
}

/// A source-like text of `count` lines and a copy with about 5% of its lines changed, added,
/// removed or moved.
fn five_percent_apart(count: usize) -> (String, String) {
    let mut rng = Lcg(7);
    let left: Vec<String> = (0..count).map(|_| source_line(&mut rng)).collect();
    let mut right = Vec::with_capacity(count);
    let mut moved = Vec::new();
    for line in &left {
        match rng.below(100) {
            0 => right.push(source_line(&mut rng)),
            1 => right.push(format!("{line} // changed")),
            2 => {}
            3 => {
                right.push(line.clone());
                right.push(source_line(&mut rng));
            }
            4 => moved.push(line.clone()),
            _ => right.push(line.clone()),
        }
        if moved.len() == 8 {
            let at = rng.below(right.len() as u64 + 1) as usize;
            right.splice(at..at, moved.drain(..));
        }
    }
    (left.join("\n"), right.join("\n"))
}

fn time_compare(count: usize) {
    let (left, right) = five_percent_apart(count);
    let started = Instant::now();
    let comparison = compare(&left, &right, &exact());
    let elapsed = started.elapsed();
    println!(
        "compare {count} lines: {elapsed:?}, {} hunks, stats {:?}",
        comparison.hunks.len(),
        comparison.stats
    );
    assert_eq!(apply_hunks(&left, &right, &comparison), right);
    assert!(elapsed.as_secs_f64() < 1.0, "{elapsed:?}");
}

#[test]
#[ignore = "timing; run with --release -- --ignored --nocapture"]
fn times_two_10k_line_files() {
    time_compare(10_000);
}

#[test]
#[ignore = "timing; run with --release -- --ignored --nocapture"]
fn times_two_100k_line_files() {
    time_compare(100_000);
}

#[test]
#[ignore = "timing; run with --release -- --ignored --nocapture"]
fn times_many_rewritten_blocks() {
    let mut rng = Lcg(5);
    let (mut left, mut right) = (Vec::new(), Vec::new());
    for block in 0..400 {
        let anchor = format!("// block {block}");
        left.push(anchor.clone());
        right.push(anchor);
        for _ in 0..250 {
            let line = source_line(&mut rng);
            right.push(format!("{line} // edited"));
            left.push(line);
        }
    }
    let (left, right) = (left.join("\n"), right.join("\n"));
    let started = Instant::now();
    let comparison = compare(&left, &right, &exact());
    let elapsed = started.elapsed();
    println!(
        "compare 400 rewritten blocks of 250 lines: {elapsed:?}, stats {:?}",
        comparison.stats
    );
    assert!(elapsed.as_secs_f64() < 1.0, "{elapsed:?}");
}

#[test]
#[ignore = "timing; run with --release -- --ignored --nocapture"]
fn times_a_file_with_every_line_changed() {
    for count in [10_000, 100_000] {
        let mut rng = Lcg(11);
        let left: Vec<String> = (0..count).map(|_| source_line(&mut rng)).collect();
        let right: Vec<String> = left.iter().map(|line| format!("\t{line} ")).collect();
        let (left, right) = (left.join("\n"), right.join("\n"));
        let started = Instant::now();
        let comparison = compare(&left, &right, &exact());
        let elapsed = started.elapsed();
        println!(
            "compare {count} lines, all changed: {elapsed:?}, stats {:?}",
            comparison.stats
        );
        assert_eq!(apply_hunks(&left, &right, &comparison), right);
        assert!(elapsed.as_secs_f64() < 1.0, "{elapsed:?}");
    }
}
