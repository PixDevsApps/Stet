//! The column operations against a naive model: every line rendered to a grid of cells, edited
//! cell by cell and turned back into text. Also the S5 prototype's checks and a timing report.

use super::*;
use crate::text::{LineIndex, TextEdit, apply, normalize};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;
use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cell {
    Char(char),
    Tab,
    Fill,
}

const SPACE: Cell = Cell::Char(' ');

/// The cells of `text` when it starts at `column`: a tab is a `Tab` cell followed by `Fill`
/// cells up to the next stop.
fn render(text: &str, column: usize, tab: usize) -> Vec<Cell> {
    let mut cells = Vec::new();
    for ch in text.chars() {
        if ch == '\t' {
            cells.push(Cell::Tab);
            let width = tab - (column + cells.len() - 1) % tab;
            cells.extend(std::iter::repeat_n(Cell::Fill, width - 1));
        } else {
            cells.push(Cell::Char(ch));
        }
    }
    cells
}

fn unrender(cells: &[Cell]) -> String {
    cells
        .iter()
        .filter_map(|cell| match cell {
            Cell::Char(ch) => Some(*ch),
            Cell::Tab => Some('\t'),
            Cell::Fill => None,
        })
        .collect()
}

/// What a line looks like: tabs are blank.
fn display(line: &str, tab: usize) -> String {
    render(line, 0, tab)
        .iter()
        .map(|cell| match cell {
            Cell::Char(ch) => *ch,
            Cell::Tab | Cell::Fill => ' ',
        })
        .collect()
}

/// Turns the tab that `column` falls strictly inside, if any, into spaces.
fn cut(cells: &mut [Cell], column: usize) {
    if cells.get(column) != Some(&Cell::Fill) {
        return;
    }
    let head = (0..column).rfind(|&i| cells[i] == Cell::Tab).unwrap();
    let end = (column..cells.len())
        .find(|&i| cells[i] != Cell::Fill)
        .unwrap_or(cells.len());
    cells[head..end].fill(SPACE);
}

/// Cells `left..right` of `line` replaced by `insert` (padded to `fill` cells when cells
/// follow), tabs cut by either edge turned into spaces first, short lines padded. A line where
/// nothing is removed or inserted stays as it is.
fn model_splice(
    line: &str,
    left: usize,
    right: usize,
    insert: &str,
    fill: usize,
    tab: usize,
) -> String {
    let mut cells = render(line, 0, tab);
    let removes = right > left && left < cells.len();
    let mut edited = cells.clone();
    cut(&mut edited, left);
    cut(&mut edited, right);
    if left < edited.len() {
        edited.drain(left..right.clamp(left, edited.len()));
    }
    let mut new = render(insert, left, tab);
    if edited.len() > left {
        new.resize(new.len().max(fill), SPACE);
    }
    if !removes && new.is_empty() {
        return line.to_owned();
    }
    cells = edited;
    if cells.len() < left {
        cells.resize(left, SPACE);
    }
    cells.splice(left..left, new);
    unrender(&cells)
}

fn model_lines(
    text: &str,
    lines: Range<usize>,
    mut edit: impl FnMut(usize, &str) -> String,
) -> String {
    text.split('\n')
        .enumerate()
        .map(|(line, line_text)| {
            if lines.contains(&line) {
                edit(line, line_text)
            } else {
                line_text.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn model_copy(text: &str, rect: Rect, tab: usize) -> Vec<String> {
    let (left, right) = (rect.left(), rect.right());
    text.split('\n')
        .enumerate()
        .filter(|(line, _)| rect.lines().contains(line))
        .map(|(_, line)| {
            let mut cells = render(line, 0, tab);
            cut(&mut cells, left);
            cut(&mut cells, right);
            if left >= cells.len() {
                String::new()
            } else {
                unrender(&cells[left..right.clamp(left, cells.len())])
            }
        })
        .collect()
}

fn model_paste(text: &str, rect: Rect, rows: &[String], tab: usize) -> String {
    let (top, left, right) = (rect.top(), rect.left(), rect.right());
    let width = rows
        .iter()
        .map(|row| render(row, left, tab).len())
        .max()
        .unwrap_or(0);
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    for (line, line_text) in lines.iter_mut().enumerate() {
        let right = if rect.lines().contains(&line) {
            right
        } else {
            left
        };
        match line.checked_sub(top).and_then(|i| rows.get(i)) {
            Some(row) => *line_text = model_splice(line_text, left, right, row, width, tab),
            None if rect.lines().contains(&line) => {
                *line_text = model_splice(line_text, left, right, "", 0, tab);
            }
            None => {}
        }
    }
    let end = if rows.is_empty() { 0 } else { top + rows.len() };
    for line in lines.len()..end {
        let row = line.checked_sub(top).map_or("", |i| rows[i].as_str());
        lines.push(if row.is_empty() {
            String::new()
        } else {
            format!("{}{row}", " ".repeat(left))
        });
    }
    lines.join("\n")
}

enum Around {
    Short(usize),
    Char(usize),
    Cut,
}

/// How many cells Backspace (`before`) or Delete removes at `column` on the rectangle's lines.
fn model_cells(text: &str, rect: Rect, column: usize, before: bool, tab: usize) -> usize {
    let arounds: Vec<Around> = text
        .split('\n')
        .enumerate()
        .filter(|(line, _)| rect.lines().contains(line))
        .map(|(_, line)| {
            let cells = render(line, 0, tab);
            if cells.get(column) == Some(&Cell::Fill) {
                Around::Cut
            } else if before && cells.len() < column {
                Around::Short(cells.len())
            } else if before {
                let head = (0..column).rfind(|&i| cells[i] != Cell::Fill).unwrap();
                Around::Char(column - head)
            } else if column >= cells.len() {
                Around::Short(cells.len())
            } else {
                let fills = cells[column + 1..]
                    .iter()
                    .take_while(|cell| **cell == Cell::Fill)
                    .count();
                Around::Char(fills + 1)
            }
        })
        .collect();
    if arounds.iter().any(|around| matches!(around, Around::Cut)) {
        return 1;
    }
    let widths: Vec<usize> = arounds
        .iter()
        .filter_map(|around| match around {
            Around::Char(width) => Some(*width),
            _ => None,
        })
        .collect();
    let Some(&width) = widths.first() else {
        return 1;
    };
    let fits_short = arounds.iter().all(|around| match around {
        Around::Short(len) => !before || len + width <= column,
        _ => true,
    });
    if widths.iter().all(|&other| other == width) && fits_short {
        width
    } else {
        1
    }
}

fn line_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop::sample::select(vec![
            'a', 'b', ' ', '\t', '\t', '\t', 'é', '日', '😀', '\u{301}',
        ]),
        0..12,
    )
    .prop_map(String::from_iter)
}

fn text_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec(line_strategy(), 1..6).prop_map(|lines| lines.join("\n"))
}

fn rect_strategy() -> impl Strategy<Value = Rect> {
    (0usize..7, 0usize..16, 0usize..7, 0usize..16)
        .prop_map(|(top, left, bottom, right)| Rect::new((top, left), (bottom, right)))
}

fn caret_strategy() -> impl Strategy<Value = Rect> {
    (0usize..7, 0usize..7, 0usize..16)
        .prop_map(|(top, bottom, column)| Rect::new((top, column), (bottom, column)))
}

fn typed_strategy() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(vec!['x', 'é', '\t', '日']), 1..5)
        .prop_map(String::from_iter)
}

fn rows_strategy() -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(
        prop::collection::vec(prop::sample::select(vec!['p', '\t', 'ø', ' ']), 0..5)
            .prop_map(String::from_iter),
        1..6,
    )
}

/// Applies `edit`, checking that its edits are sorted, at most one per line, and equal to
/// their bulk form.
fn applied(text: &str, index: &LineIndex, edit: &ColumnEdit) -> Result<String, TestCaseError> {
    prop_assert_eq!(
        normalize(edit.edits.clone(), index.len_chars()),
        Ok(edit.edits.clone())
    );
    let lines: Vec<usize> = edit
        .edits
        .iter()
        .map(|edit| index.line_of_char(edit.start))
        .collect();
    prop_assert!(lines.windows(2).all(|pair| pair[0] < pair[1]));
    let result = apply(text, edit.edits.clone()).unwrap();
    if let Some(bulk) = edit.to_bulk(text, index) {
        prop_assert_eq!(apply(text, vec![bulk.edit]).unwrap(), result.clone());
        prop_assert_eq!(
            LineIndex::new(&result).line_count(),
            index.line_count() + bulk.appended_lines
        );
    }
    Ok(result)
}

fn trimmed_display(text: &str, tab: usize) -> Vec<String> {
    text.split('\n')
        .map(|line| display(line, tab).trim_end().to_owned())
        .collect()
}

proptest! {
    #[test]
    fn insert_matches_the_model(
        text in text_strategy(),
        rect in rect_strategy(),
        inserted in typed_strategy(),
        tab in 1usize..9,
    ) {
        let index = LineIndex::new(&text);
        let edit = insert(&text, &index, rect, &inserted, tab).unwrap();
        let result = applied(&text, &index, &edit)?;
        let left = rect.left();
        prop_assert_eq!(
            &result,
            &model_lines(&text, rect.lines(), |_, line| model_splice(line, left, left, &inserted, 0, tab))
        );
        let width = text_width(&inserted, left, tab);
        prop_assert_eq!((edit.rect.left(), edit.rect.right()), (left + width, rect.right() + width));
        let shown: Vec<char> = display(&format!("{}{inserted}", " ".repeat(left)), tab)
            .chars()
            .skip(left)
            .collect();
        for (line, (before, after)) in text.split('\n').zip(result.split('\n')).enumerate() {
            if rect.lines().contains(&line) {
                let at = locate(after, left, tab);
                prop_assert_eq!((at.inside_tab, at.virtual_space), (None, 0));
                prop_assert!(after[at.byte_offset..].starts_with(inserted.as_str()));
                let old: Vec<char> = display(before, tab).chars().chain(std::iter::repeat(' ')).take(left).collect();
                let new: Vec<char> = display(after, tab).chars().collect();
                prop_assert_eq!(&new[..left], &old[..]);
                prop_assert_eq!(&new[left..left + shown.len()], &shown[..]);
            } else {
                prop_assert_eq!(before, after);
            }
        }
    }

    #[test]
    fn delete_block_matches_the_model(text in text_strategy(), rect in rect_strategy(), tab in 1usize..9) {
        let index = LineIndex::new(&text);
        let edit = delete_block(&text, &index, rect, tab);
        let result = applied(&text, &index, &edit)?;
        let (left, right) = (rect.left(), rect.right());
        prop_assert_eq!(
            result,
            model_lines(&text, rect.lines(), |_, line| model_splice(line, left, right, "", 0, tab))
        );
        prop_assert_eq!(edit.rect, rect.with_column(left));
    }

    #[test]
    fn copy_matches_the_model(text in text_strategy(), rect in rect_strategy(), tab in 1usize..9) {
        let index = LineIndex::new(&text);
        let block = copy(&text, &index, rect, tab);
        prop_assert_eq!(&block.rows, &model_copy(&text, rect, tab));
        let left = rect.left();
        for (row, line) in block.rows.iter().zip(text.split('\n').skip(rect.top())) {
            prop_assert!(!row.contains('\n'));
            let shown: String = display(&format!("{}{row}", " ".repeat(left)), tab).chars().skip(left).collect();
            let line: String = display(line, tab).chars().skip(left).take(rect.width()).collect();
            prop_assert_eq!(shown, line);
        }
        if !block.rows.is_empty() {
            prop_assert_eq!(Block::from_text(&block.to_text()), block);
        }
    }

    #[test]
    fn paste_matches_the_model(
        text in text_strategy(),
        rect in rect_strategy(),
        rows in rows_strategy(),
        tab in 1usize..9,
    ) {
        let index = LineIndex::new(&text);
        let block = Block { rows };
        let edit = paste(&text, &index, rect, &block, tab);
        let result = applied(&text, &index, &edit)?;
        prop_assert_eq!(result, model_paste(&text, rect, &block.rows, tab));
        let caret = rect.left() + block.width(rect.left(), tab);
        prop_assert_eq!(edit.rect, Rect::new((rect.top() + block.rows.len() - 1, caret), (rect.top(), caret)));
    }

    #[test]
    fn copy_delete_paste_restores_what_the_lines_look_like(
        text in text_strategy(),
        rect in rect_strategy(),
        tab in 1usize..9,
    ) {
        let index = LineIndex::new(&text);
        let block = copy(&text, &index, rect, tab);
        let deleted = applied(&text, &index, &delete_block(&text, &index, rect, tab))?;
        let index = LineIndex::new(&deleted);
        let caret = Rect::new((rect.top(), rect.left()), (rect.top(), rect.left()));
        let pasted = applied(&deleted, &index, &paste(&deleted, &index, caret, &block, tab))?;
        let expected: Vec<String> = text.split('\n').map(|line| display(line, tab)).collect();
        let restored: Vec<String> = pasted.split('\n').map(|line| display(line, tab)).collect();
        prop_assert_eq!(restored, expected);
    }

    #[test]
    fn typing_key_by_key_matches_one_model_edit(
        text in text_strategy(),
        rect in rect_strategy(),
        typed in typed_strategy(),
        tab in 1usize..9,
    ) {
        let (left, right) = (rect.left(), rect.right());
        let (mut current, mut caret) = (text.clone(), rect);
        for key in typed.chars() {
            let index = LineIndex::new(&current);
            let edit = type_text(&current, &index, caret, &key.to_string(), tab).unwrap();
            current = applied(&current, &index, &edit)?;
            caret = edit.rect;
        }
        prop_assert_eq!(
            &current,
            &model_lines(&text, rect.lines(), |_, line| model_splice(line, left, right, &typed, 0, tab))
        );
        prop_assert_eq!(caret, rect.with_column(left + text_width(&typed, left, tab)));
        let index = LineIndex::new(&text);
        let at_once = type_text(&text, &index, rect, &typed, tab).unwrap();
        prop_assert_eq!(apply(&text, at_once.edits).unwrap(), current);
    }

    #[test]
    fn backspace_and_delete_match_the_model(
        text in text_strategy(),
        rect in prop_oneof![caret_strategy(), rect_strategy()],
        tab in 1usize..9,
    ) {
        let index = LineIndex::new(&text);
        let (left, right) = (rect.left(), rect.right());
        let back = backspace(&text, &index, rect, tab);
        let (from, to) = if !rect.is_empty() || left == 0 {
            (left, right)
        } else {
            (left - model_cells(&text, rect, left, true, tab), left)
        };
        prop_assert_eq!(
            applied(&text, &index, &back)?,
            model_lines(&text, rect.lines(), |_, line| model_splice(line, from, to, "", 0, tab))
        );
        prop_assert_eq!(back.rect, rect.with_column(from));
        let forward = delete_forward(&text, &index, rect, tab);
        let (from, to) = if rect.is_empty() {
            (left, left + model_cells(&text, rect, left, false, tab))
        } else {
            (left, right)
        };
        prop_assert_eq!(
            applied(&text, &index, &forward)?,
            model_lines(&text, rect.lines(), |_, line| model_splice(line, from, to, "", 0, tab))
        );
        prop_assert_eq!(forward.rect, rect.with_column(left));
    }

    #[test]
    fn backspacing_what_was_typed_restores_what_the_lines_look_like(
        (text, rect) in text_strategy().prop_flat_map(|text| {
            let lines = text.split('\n').count();
            (Just(text), (0..lines, 0usize..7, 0usize..16))
        }).prop_map(|(text, (top, height, column))| (text, Rect::new((top + height, column), (top, column)))),
        typed in typed_strategy(),
        tab in 1usize..9,
    ) {
        let (mut current, mut caret) = (text.clone(), rect);
        for key in typed.chars() {
            let index = LineIndex::new(&current);
            let edit = type_text(&current, &index, caret, &key.to_string(), tab).unwrap();
            current = applied(&current, &index, &edit)?;
            caret = edit.rect;
        }
        for _ in typed.chars() {
            let index = LineIndex::new(&current);
            let edit = backspace(&current, &index, caret, tab);
            current = applied(&current, &index, &edit)?;
            caret = edit.rect;
        }
        prop_assert_eq!(caret, rect);
        prop_assert_eq!(trimmed_display(&current, tab), trimmed_display(&text, tab));
    }

    #[test]
    fn a_snapshot_of_the_edited_lines_gives_the_same_edits(
        text in text_strategy(),
        rect in rect_strategy(),
        typed in typed_strategy(),
        tab in 1usize..9,
    ) {
        let index = LineIndex::new(&text);
        let lines = rect.top().min(index.line_count() - 1)..rect.bottom().min(index.line_count() - 1) + 1;
        let first = index.line_chars(lines.start).start;
        let window = &text[index.line_bytes(lines.start).start..index.line_bytes(lines.end - 1).end];
        let window_index = LineIndex::new(window);
        let shift = lines.start as isize;
        let local = type_text(window, &window_index, rect.shift_lines(-shift), &typed, tab).unwrap();
        let whole = type_text(&text, &index, rect, &typed, tab).unwrap();
        let moved: Vec<_> = local
            .edits
            .into_iter()
            .map(|edit| TextEdit::new(edit.start + first..edit.end + first, edit.insert))
            .collect();
        prop_assert_eq!(moved, whole.edits);
        prop_assert_eq!(local.rect.shift_lines(shift), whole.rect);
    }
}

const LINES: usize = 10_000;

/// The S5 test buffer's seven line shapes.
fn s5_lines(count: usize) -> Vec<String> {
    (0..count)
        .map(|i| match i % 7 {
            0 => String::new(),
            1 => format!("\tfn item_{i}() {{}}"),
            2 => "short".to_owned(),
            3 => format!("abcdefgh\t= {i};"),
            4 => format!("héllo wörld — ünïcode {i}"),
            5 => format!("{i:>6} {}", "x".repeat(100)),
            _ => format!("    let value_{i} = {i};"),
        })
        .collect()
}

fn every_line(lines: &[String], edit: impl Fn(&str) -> String) -> String {
    lines
        .iter()
        .map(|line| edit(line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn full_caret(column: usize) -> Rect {
    Rect::new((LINES - 1, column), (0, column))
}

#[test]
fn s5_column_insert_lands_on_the_column_on_every_line() {
    let lines = s5_lines(LINES);
    let text = lines.join("\n");
    let index = LineIndex::new(&text);
    for column in [0, 10] {
        let edit = insert(&text, &index, full_caret(column), "// ", 4).unwrap();
        assert_eq!(edit.edits.len(), LINES);
        assert!(edit.needs_bulk());
        let result = apply(&text, edit.edits.clone()).unwrap();
        assert_eq!(
            result,
            every_line(&lines, |line| model_splice(
                line, column, column, "// ", 0, 4
            ))
        );
        let bulk = edit.to_bulk(&text, &index).unwrap();
        assert_eq!((bulk.lines.clone(), bulk.appended_lines), (0..LINES, 0));
        assert_eq!(apply(&text, vec![bulk.edit]).unwrap(), result);
        for line in result.split('\n') {
            let at = locate(line, column, 4);
            assert_eq!((at.inside_tab, at.virtual_space), (None, 0), "{line:?}");
            assert!(line[at.byte_offset..].starts_with("// "), "{line:?}");
        }
    }
    let tab_line = "abcdefgh\t= 3;";
    let index = LineIndex::new(tab_line);
    let edit = insert(tab_line, &index, Rect::new((0, 10), (0, 10)), "// ", 4).unwrap();
    assert_eq!(apply(tab_line, edit.edits).unwrap(), "abcdefgh  //   = 3;");
}

#[test]
fn s5_typing_and_backspace_replicate_on_every_line() {
    let lines = s5_lines(LINES);
    let (mut text, mut rect) = (lines.join("\n"), full_caret(10));
    for key in ["a", "b", "c", "d", "e", "f"] {
        let index = LineIndex::new(&text);
        let edit = type_text(&text, &index, rect, key, 4).unwrap();
        text = apply(&text, edit.edits).unwrap();
        rect = edit.rect;
    }
    assert_eq!(rect, full_caret(16));
    for _ in 0..2 {
        let index = LineIndex::new(&text);
        let edit = backspace(&text, &index, rect, 4);
        text = apply(&text, edit.edits).unwrap();
        rect = edit.rect;
    }
    assert_eq!(rect, full_caret(14));
    assert_eq!(
        text,
        every_line(&lines, |line| model_splice(line, 10, 10, "abcd", 0, 4))
    );
}

#[test]
fn s5_block_delete_and_clipboard_restore_every_line() {
    let lines = s5_lines(LINES);
    let text = lines.join("\n");
    let index = LineIndex::new(&text);
    let inserted = apply(
        &text,
        insert(&text, &index, full_caret(10), "abcde", 4)
            .unwrap()
            .edits,
    )
    .unwrap();
    let index = LineIndex::new(&inserted);
    let block_rect = Rect::new((0, 10), (LINES - 1, 15));
    let block = copy(&inserted, &index, block_rect, 4);
    assert!(block.rows.iter().all(|row| row == "abcde"));
    let deleted = apply(
        &inserted,
        delete_block(&inserted, &index, block_rect, 4).edits,
    )
    .unwrap();
    assert_eq!(trimmed_display(&deleted, 4), trimmed_display(&text, 4));
    let pasted = paste(&inserted, &index, Rect::new((0, 40), (0, 40)), &block, 4);
    assert_eq!(pasted.edits.len(), LINES);
    assert_eq!(
        apply(&inserted, pasted.edits).unwrap(),
        every_line(
            &inserted.split('\n').map(str::to_owned).collect::<Vec<_>>(),
            |line| model_splice(line, 40, 40, "abcde", 5, 4)
        )
    );
}

/// Run with `cargo test -p stet-domain --release -- --ignored --nocapture column_timings`.
#[test]
#[ignore = "timing report"]
fn column_timings() {
    use std::time::{Duration, Instant};

    fn median(mut run: impl FnMut() -> Duration) -> f64 {
        let mut samples: Vec<Duration> = (0..15).map(|_| run()).collect();
        samples.sort_unstable();
        samples[samples.len() / 2].as_secs_f64() * 1000.0
    }

    fn time<T>(work: impl FnOnce() -> T) -> Duration {
        let started = Instant::now();
        std::hint::black_box(work());
        started.elapsed()
    }

    let text = s5_lines(LINES).join("\n");
    let index = LineIndex::new(&text);
    let block = copy(&text, &index, Rect::new((0, 10), (LINES - 1, 15)), 4);
    let insert_0 = insert(&text, &index, full_caret(0), "// ", 4).unwrap();
    let report = [
        (
            "LineIndex::new (10k lines)",
            median(|| time(|| LineIndex::new(&text))),
        ),
        (
            "insert \"// \" at column 0",
            median(|| time(|| insert(&text, &index, full_caret(0), "// ", 4))),
        ),
        (
            "insert \"// \" at column 10",
            median(|| time(|| insert(&text, &index, full_caret(10), "// ", 4))),
        ),
        (
            "type one key at column 10",
            median(|| time(|| type_text(&text, &index, full_caret(10), "x", 4))),
        ),
        (
            "backspace at column 10",
            median(|| time(|| backspace(&text, &index, full_caret(10), 4))),
        ),
        (
            "delete block 10..15",
            median(|| time(|| delete_block(&text, &index, Rect::new((0, 10), (LINES - 1, 15)), 4))),
        ),
        (
            "copy block 10..15",
            median(|| time(|| copy(&text, &index, Rect::new((0, 10), (LINES - 1, 15)), 4))),
        ),
        (
            "paste 10k rows at column 40",
            median(|| time(|| paste(&text, &index, Rect::new((0, 40), (0, 40)), &block, 4))),
        ),
        (
            "to_bulk of the column-0 insert",
            median(|| time(|| insert_0.to_bulk(&text, &index))),
        ),
        (
            "apply the column-0 edits (text::apply)",
            median(|| time(|| apply(&text, insert_0.edits.clone()))),
        ),
    ];
    for (label, ms) in report {
        println!("column timing: {label}: {ms:.3} ms");
    }
}
