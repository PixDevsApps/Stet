//! Domain edits applied to a GtkSourceBuffer (ADR-003): in one user action, so an operation is
//! one undo step, and above [`BULK_EDIT_THRESHOLD`] edits as one bulk edit that replaces the
//! span from the first edit to the last (ADR-003 amendment). The caret and the selection are
//! mapped through the edits.

use gtk4::prelude::*;
use std::ops::Range;
use std::time::{Duration, Instant};
use stet_domain::marks::Footprint;
use stet_domain::ops::Change;
use stet_domain::text::{Bias, TextEdit, map_offset};
use stet_infrastructure::search::{BULK_EDIT_THRESHOLD, ReplaceAll};

/// How an operation reached the buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Applied {
    /// Edits in the operation.
    pub edits: usize,
    /// Applied as one bulk edit.
    pub bulk: bool,
    /// Time spent changing the buffer, on the GTK thread.
    pub took: Duration,
    /// What reached the buffer, for keeping bookmarks across Undo and Redo (M7).
    pub footprint: Footprint,
}

/// The caret and selection as character offsets.
#[derive(Debug, Clone, Copy)]
struct Marks {
    insert: usize,
    bound: usize,
}

impl Marks {
    fn read(buffer: &sourceview5::Buffer) -> Self {
        let offset = |mark: &gtk4::TextMark| buffer.iter_at_mark(mark).offset() as usize;
        Self {
            insert: offset(&buffer.get_insert()),
            bound: offset(&buffer.selection_bound()),
        }
    }

    /// Maps the marks through `edits` (sorted, in the old text's offsets). A selection grows
    /// over text inserted at its edges, so a replaced selection stays selected; a caret inside
    /// a replaced match moves to its start.
    fn map(self, edits: &[TextEdit]) -> Self {
        if self.insert == self.bound {
            let caret = map_offset(self.insert, edits, Bias::Before);
            return Self {
                insert: caret,
                bound: caret,
            };
        }
        let (start, end) = (self.insert.min(self.bound), self.insert.max(self.bound));
        let start = map_offset(start, edits, Bias::Before);
        let end = map_offset(end, edits, Bias::After);
        if self.insert <= self.bound {
            Self {
                insert: start,
                bound: end.max(start),
            }
        } else {
            Self {
                insert: end.max(start),
                bound: start,
            }
        }
    }

    fn restore(self, buffer: &sourceview5::Buffer) {
        let insert = buffer.iter_at_offset(self.insert as i32);
        let bound = buffer.iter_at_offset(self.bound as i32);
        buffer.select_range(&insert, &bound);
    }
}

/// Applies sorted, non-overlapping `edits` in the buffer's current character offsets, last
/// first, in one user action.
pub fn apply_edits(buffer: &sourceview5::Buffer, edits: &[TextEdit]) -> Applied {
    let started = Instant::now();
    let marks = Marks::read(buffer).map(edits);
    buffer.begin_user_action();
    for edit in edits.iter().rev() {
        replace_range(buffer, edit);
    }
    buffer.end_user_action();
    marks.restore(buffer);
    Applied {
        edits: edits.len(),
        bulk: false,
        took: started.elapsed(),
        footprint: Footprint::of(edits),
    }
}

/// Applies `edits` (sorted, in the buffer's current character offsets) as `bulk`, the one
/// replacement of the span they cover that the caller computed (ADR-003), in one user action.
pub fn apply_coalesced(
    buffer: &sourceview5::Buffer,
    edits: &[TextEdit],
    bulk: &TextEdit,
) -> Applied {
    let started = Instant::now();
    let marks = Marks::read(buffer).map(edits);
    buffer.begin_user_action();
    replace_range(buffer, bulk);
    buffer.end_user_action();
    marks.restore(buffer);
    Applied {
        edits: edits.len(),
        bulk: true,
        took: started.elapsed(),
        footprint: Footprint::of(std::slice::from_ref(bulk)),
    }
}

/// Applies a Replace All computed on `text`, which must still be the buffer's text: edit by
/// edit, or above [`BULK_EDIT_THRESHOLD`] edits as one bulk edit.
pub fn apply_replace_all(buffer: &sourceview5::Buffer, result: &ReplaceAll, text: &str) -> Applied {
    if !result.is_bulk() {
        return apply_edits(buffer, &result.edits);
    }
    let Some(edit) = result.as_single_edit(text) else {
        return Applied {
            edits: 0,
            bulk: false,
            took: Duration::ZERO,
            footprint: Footprint::default(),
        };
    };
    let started = Instant::now();
    let marks = Marks::read(buffer).map(&result.edits);
    buffer.begin_user_action();
    let deleted = replace_range(buffer, &edit);
    buffer.end_user_action();
    marks.restore(buffer);
    debug_assert!(result.edits.len() > BULK_EDIT_THRESHOLD);
    tracing::info!(
        edits = result.edits.len(),
        delete_ms = deleted.as_secs_f64() * 1e3,
        total_ms = started.elapsed().as_secs_f64() * 1e3,
        "bulk edit"
    );
    Applied {
        edits: result.edits.len(),
        bulk: true,
        took: started.elapsed(),
        footprint: Footprint::of(std::slice::from_ref(&edit)),
    }
}

/// The selection in character offsets, ordered, and whether the caret is at its start.
pub fn selection(buffer: &sourceview5::Buffer) -> (Range<usize>, bool) {
    let marks = Marks::read(buffer);
    (
        marks.insert.min(marks.bound)..marks.insert.max(marks.bound),
        marks.insert < marks.bound,
    )
}

/// Selects `range` (character offsets), with the caret at its start when `backward`.
pub fn select(buffer: &sourceview5::Buffer, range: Range<usize>, backward: bool) {
    let (insert, bound) = if backward {
        (range.start, range.end)
    } else {
        (range.end, range.start)
    };
    Marks { insert, bound }.restore(buffer);
}

/// Applies a text tool's `change`, computed on `text` (still the buffer's text), in one user
/// action, as one bulk edit above [`BULK_EDIT_THRESHOLD`] edits (ADR-003), then selects what
/// the operation asks for.
pub fn apply_change(buffer: &sourceview5::Buffer, change: &Change, text: &str) -> Applied {
    let started = Instant::now();
    let (selected, backward) = selection(buffer);
    let after = change.selection_after(selected);
    let bulk = change.is_bulk();
    let mut footprint = Footprint::of(&change.edits);
    buffer.begin_user_action();
    if bulk {
        if let Some(edit) = change.coalesce(text) {
            replace_range(buffer, &edit);
            footprint = Footprint::of(std::slice::from_ref(&edit));
        }
    } else {
        for edit in change.edits.iter().rev() {
            replace_range(buffer, edit);
        }
    }
    buffer.end_user_action();
    select(buffer, after, backward);
    Applied {
        edits: change.edits.len(),
        bulk,
        took: started.elapsed(),
        footprint,
    }
}

/// Replaces one range; returns how long the deletion took.
pub fn replace_range(buffer: &sourceview5::Buffer, edit: &TextEdit) -> Duration {
    let started = Instant::now();
    let mut start = buffer.iter_at_offset(edit.start as i32);
    if edit.end > edit.start {
        let mut end = buffer.iter_at_offset(edit.end as i32);
        buffer.delete(&mut start, &mut end);
    }
    let deleted = started.elapsed();
    if !edit.insert.is_empty() {
        buffer.insert(&mut start, &edit.insert);
    }
    deleted
}
