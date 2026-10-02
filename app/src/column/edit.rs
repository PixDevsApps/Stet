//! Column edits on the buffer, and the buffer hooks that route typing into them.
//!
//! An operation snapshots only the lines it edits, runs the `domain::column` function on the
//! snapshot and applies the result in one user action: edit by edit, or above
//! `BULK_THRESHOLD` edits as one replacement of the line range (ADR-003 amendment), keeping
//! the scroll position and re-creating the source marks the replacement collapses.
//!
//! Typed text reaches the buffer as `insert-text` at the caret, from GtkTextView's input-method
//! commit (fcitx5, compose, dead keys and plain keys alike). The hook stops that emission and
//! types the text on every line of the rectangle instead; any other insertion, deletion or
//! caret move that column mode did not make ends column mode first.

use super::view::{Collapse, StetView};
use gtk4 as gtk;
use gtk4::glib;
use gtk4::glib::translate::{FromGlibPtrNone, ToGlibPtr, ToGlibPtrMut};
use sourceview5::prelude::*;
use std::cell::Cell;
use std::ffi::{CStr, c_char, c_int};
use std::ops::Range;
use std::rc::Rc;
use std::time::{Duration, Instant};
use stet_domain::column::{ColumnEdit, ColumnError, Rect};
use stet_domain::marks::Footprint;
use stet_domain::text::{LineIndex, TextEdit};

/// What the last column operation did, for the self-test and the timings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpStats {
    pub lines: usize,
    pub edits: usize,
    pub bulk: bool,
    /// From the snapshot to the caret placed afterwards.
    pub took: Duration,
    /// Changing the buffer alone.
    pub applying: Duration,
}

/// The text of `lines` (clamped to the buffer), without the last line's break, and the
/// character offset where it starts.
pub(super) fn snapshot(view: &StetView, lines: Range<usize>) -> (usize, String, usize) {
    let buffer = view.buffer();
    let last = view.last_line();
    let first = lines.start.min(last);
    let end_line = lines.end.saturating_sub(1).clamp(first, last);
    let start = buffer
        .iter_at_line(first as i32)
        .unwrap_or_else(|| buffer.start_iter());
    let mut end = buffer
        .iter_at_line(end_line as i32)
        .unwrap_or_else(|| buffer.end_iter());
    if !end.ends_line() {
        end.forward_to_line_end();
    }
    let text = buffer.text(&start, &end, true).to_string();
    (first, text, start.offset().max(0) as usize)
}

/// Runs a column operation on `lines`, applies its edits as one undo step (inside a typing
/// burst's step when one is open) and shows the rectangle it leaves.
pub(super) fn apply(
    view: &StetView,
    rect: Rect,
    lines: Range<usize>,
    op: impl FnOnce(&str, &LineIndex, Rect) -> Result<ColumnEdit, ColumnError>,
) -> Result<Rect, ColumnError> {
    let started = Instant::now();
    let (first, text, base) = snapshot(view, lines);
    let index = LineIndex::new(&text);
    let edit = op(&text, &index, rect.shift_lines(-(first as isize)))?;
    let after = edit.rect.shift_lines(first as isize);
    let buffer = view.source_buffer();
    let applying = Instant::now();
    let bulk = edit.needs_bulk();
    if bulk && let Some(tracker) = view.column_tracker() {
        // Undo and Redo collapse the bookmarks on the replaced lines; the document keeps
        // them (M7).
        let logical: Vec<TextEdit> = edit
            .edits
            .iter()
            .map(|change| TextEdit::new(base + change.start..base + change.end, &*change.insert))
            .collect();
        let mut run = || {
            view.busy(|| {
                buffer.begin_user_action();
                let applied = apply_bulk(view, &buffer, &edit, &text, &index, first, base);
                buffer.end_user_action();
                Footprint::of(&applied)
            })
        };
        tracker(first..first + index.line_count(), &logical, &mut run);
    } else {
        view.busy(|| {
            buffer.begin_user_action();
            if bulk {
                apply_bulk(view, &buffer, &edit, &text, &index, first, base);
            } else {
                for change in edit.edits.iter().rev() {
                    replace(&buffer, base, change);
                }
            }
            buffer.end_user_action();
        });
    }
    let applying = applying.elapsed();
    view.set_rect(after);
    view.record_op(OpStats {
        lines: index.line_count(),
        edits: edit.edits.len(),
        bulk,
        took: started.elapsed(),
        applying,
    });
    Ok(after)
}

/// A large column edit (ADR-003 amendment): the lines further than a screen from the view are
/// rewritten as one replacement each above and below it, the lines near the view edit by edit.
///
/// A replacement makes new lines, whose heights GtkTextView knows only once it has measured
/// them again, and collapses the marks inside it to its start; its own mark on the view's top
/// line too, and the view would jump. Lines edited one by one keep their heights and marks, so
/// the view stays; at most a few screens of single edits keep undo and redo fast. The source
/// marks on replaced lines (bookmarks) are made again on the same line numbers, which a column
/// edit keeps; undo and redo collapse them again, and the document's bookmark history puts
/// them back (M7). Returns the replacements, in the buffer's offsets before the edit.
fn apply_bulk(
    view: &StetView,
    buffer: &sourceview5::Buffer,
    edit: &ColumnEdit,
    text: &str,
    index: &LineIndex,
    first: usize,
    base: usize,
) -> Vec<TextEdit> {
    let started = Instant::now();
    let visible = view.visible_rect();
    let line_at = |y: i32| view.line_at_y(y).0.line().max(0) as usize;
    let near = line_at(visible.y() - visible.height()).saturating_sub(first)
        ..line_at(visible.y() + 2 * visible.height()).saturating_sub(first) + 1;
    let line_of = |change: &TextEdit| index.line_of_char(change.start);
    let part = |keep: &dyn Fn(usize) -> bool| ColumnEdit {
        edits: edit
            .edits
            .iter()
            .filter(|change| keep(line_of(change)))
            .cloned()
            .collect(),
        rect: edit.rect,
    };
    let above = part(&|line| line < near.start);
    let below = part(&|line| line >= near.end);
    let single = part(&|line| near.contains(&line));
    let mut applied = Vec::with_capacity(single.edits.len() + 2);
    applied.extend(replace_lines(buffer, &below, text, index, first, base));
    for change in single.edits.iter().rev() {
        replace(buffer, base, change);
    }
    applied.extend(replace_lines(buffer, &above, text, index, first, base));
    applied.extend(single.edits);
    let mut applied: Vec<TextEdit> = applied
        .into_iter()
        .map(|change| TextEdit::new(base + change.start..base + change.end, change.insert))
        .collect();
    applied.sort_by_key(|change| change.start);
    tracing::info!(
        edits = edit.edits.len(),
        single = applied.len().saturating_sub(2),
        ms = started.elapsed().as_secs_f64() * 1e3,
        "column bulk edit"
    );
    applied
}

/// Rewrites the lines `part` edits with one replacement, then makes the source marks on them
/// again on the same lines. Returns the replacement, in the snapshot's offsets.
fn replace_lines(
    buffer: &sourceview5::Buffer,
    part: &ColumnEdit,
    text: &str,
    index: &LineIndex,
    first: usize,
    base: usize,
) -> Option<TextEdit> {
    let bulk = part.to_bulk(text, index)?;
    let lines = bulk.lines.start + first..bulk.lines.end + first;
    let marks = source_marks(buffer, lines);
    let keep: Vec<usize> = bulk.marks_to_restore(marks.iter().map(|(line, _)| line - first));
    replace(buffer, base, &bulk.edit);
    for (line, mark) in marks {
        if !keep.contains(&(line - first)) {
            continue;
        }
        let name = mark.name();
        let category = mark.category();
        buffer.delete_mark(&mark);
        if let Some(iter) = buffer.iter_at_line(line as i32) {
            buffer.create_source_mark(name.as_deref(), &category, &iter);
        }
    }
    Some(bulk.edit)
}

/// The source marks on `lines`, with their line numbers.
fn source_marks(
    buffer: &sourceview5::Buffer,
    lines: Range<usize>,
) -> Vec<(usize, sourceview5::Mark)> {
    let mut found = Vec::new();
    let Some(mut iter) = buffer.iter_at_line(lines.start as i32) else {
        return found;
    };
    loop {
        let line = iter.line().max(0) as usize;
        if line >= lines.end {
            break;
        }
        found.extend(
            buffer
                .source_marks_at_iter(&mut iter, None)
                .into_iter()
                .map(|mark| (line, mark)),
        );
        // SAFETY: a valid buffer and iterator; a NULL category means every category.
        let moved = unsafe {
            sourceview5::ffi::gtk_source_buffer_forward_iter_to_source_mark(
                buffer.to_glib_none().0,
                iter.to_glib_none_mut().0,
                std::ptr::null(),
            )
        };
        if moved == glib::ffi::GFALSE {
            break;
        }
    }
    found
}

fn replace(buffer: &sourceview5::Buffer, base: usize, edit: &TextEdit) {
    let mut start = buffer.iter_at_offset((base + edit.start) as i32);
    if edit.end > edit.start {
        let mut end = buffer.iter_at_offset((base + edit.end) as i32);
        buffer.delete(&mut start, &mut end);
    }
    if !edit.insert.is_empty() {
        buffer.insert(&mut start, &edit.insert);
    }
}

/// What the views of one buffer share (M7: a clone shows the same buffer in the other view):
/// the redo workaround's state, and which view is in column mode, since only one may be.
#[derive(Default)]
pub struct BufferShared {
    /// An outermost user action is open on the buffer.
    user_action: Cell<bool>,
    /// A redo left more to redo, and GTK left the redone step open to the next edit.
    redone_open: Cell<bool>,
    column_owner: glib::WeakRef<StetView>,
}

impl BufferShared {
    /// `view` enters column mode: another view of the buffer leaves it.
    pub(super) fn take_column(&self, view: &StetView) {
        if let Some(owner) = self.column_owner.upgrade()
            && owner != *view
        {
            owner.end_column_mode(Collapse::Keep);
        }
        self.column_owner.set(Some(view));
    }

    /// After a redo that left more to redo, closes the redone step GTK left open, before
    /// anything joins it: when the outermost user action begins (it has just joined the step),
    /// by ending it and beginning it again, as GtkTextView does between a deleted selection
    /// and the typed text; before an edit outside any user action, with an empty one. Either
    /// clears the redo history, which the user action or the edit clears anyway.
    fn seal_redone_step(&self, buffer: &gtk::TextBuffer) {
        if !self.redone_open.replace(false) {
            return;
        }
        if self.user_action.get() {
            buffer.end_user_action();
            buffer.begin_user_action();
        } else {
            buffer.begin_user_action();
            buffer.end_user_action();
        }
    }
}

/// Connects the buffer's own hooks, once per buffer and before any view gets it, so that they
/// run before the views' hooks and GtkTextView's and GtkSourceView's handlers.
///
/// GTK 4.22's GtkTextHistory ends a step of several edits with a barrier, which undo moves to
/// the redo queue behind the step, so redo moves the step back without it. The next edit then
/// joins the redone step, and one Undo takes back both (a column burst after an undo and a
/// redo, say; docs/upstream/gtk-text-history-redo-barrier.md). Once nothing is left to redo,
/// an empty user action puts the barrier back at once; it only clears a redo history that
/// holds nothing but the barrier. Otherwise the next user action or edit does, as it begins
/// (`seal_redone_step`), since it clears the redo history anyway.
pub fn connect_history(buffer: &sourceview5::Buffer) -> Rc<BufferShared> {
    let shared = Rc::new(BufferShared::default());
    buffer.connect_insert_text(glib::clone!(
        #[weak]
        shared,
        move |buffer, _, _| shared.seal_redone_step(buffer.upcast_ref())
    ));
    buffer.connect_delete_range(glib::clone!(
        #[weak]
        shared,
        move |buffer, _, _| shared.seal_redone_step(buffer.upcast_ref())
    ));
    for signal in ["undo", "redo"] {
        buffer.connect_local(
            signal,
            false,
            glib::clone!(
                #[weak]
                shared,
                #[upgrade_or]
                None,
                move |_| {
                    shared.redone_open.set(false);
                    None
                }
            ),
        );
    }
    buffer.connect_local(
        "redo",
        true,
        glib::clone!(
            #[weak]
            shared,
            #[upgrade_or]
            None,
            move |values| {
                let buffer = values[0].get::<gtk::TextBuffer>().expect("the buffer");
                if buffer.can_redo() {
                    shared.redone_open.set(true);
                } else {
                    buffer.begin_user_action();
                    buffer.end_user_action();
                }
                None
            }
        ),
    );
    // Only the outermost user action emits these, begin after the history has begun it.
    buffer.connect_begin_user_action(glib::clone!(
        #[weak]
        shared,
        move |buffer| {
            shared.user_action.set(true);
            shared.seal_redone_step(buffer.upcast_ref());
        }
    ));
    buffer.connect_end_user_action(glib::clone!(
        #[weak]
        shared,
        move |_| {
            // The history closes the step the user action joined.
            shared.user_action.set(false);
            shared.redone_open.set(false);
        }
    ));
    shared
}

/// Connects a view's hooks to `buffer` before the view gets it, so they run before
/// GtkTextView's and GtkSourceView's own handlers.
pub(super) fn connect_buffer(view: &StetView, buffer: &sourceview5::Buffer) {
    let weak = view.downgrade();
    // SAFETY: the trampoline matches `insert-text`'s C signature, and the boxed weak reference
    // is freed by `connect_raw`'s destroy notify when the handler goes.
    unsafe {
        let trampoline: unsafe extern "C" fn(
            *mut gtk::ffi::GtkTextBuffer,
            *mut gtk::ffi::GtkTextIter,
            *const c_char,
            c_int,
            glib::ffi::gpointer,
        ) = insert_text_trampoline;
        glib::signal::connect_raw(
            buffer.as_ptr().cast(),
            c"insert-text".as_ptr(),
            Some(std::mem::transmute::<*const (), unsafe extern "C" fn()>(
                trampoline as *const (),
            )),
            Box::into_raw(Box::new(weak)),
        );
    }
    buffer.connect_delete_range(glib::clone!(
        #[weak]
        view,
        move |buffer, start, end| {
            if view.swallows_delete(start, end) {
                buffer.stop_signal_emission_by_name("delete-range");
            }
        }
    ));
    buffer.connect_mark_set(glib::clone!(
        #[weak]
        view,
        move |buffer, _, mark| {
            if !view.is_busy()
                && view.rect().is_some()
                && (*mark == buffer.get_insert() || *mark == buffer.selection_bound())
            {
                view.end_column_mode(Collapse::Keep);
            }
        }
    ));
    for signal in ["undo", "redo"] {
        buffer.connect_local(
            signal,
            false,
            glib::clone!(
                #[weak]
                view,
                #[upgrade_or]
                None,
                move |_| {
                    view.end_column_mode(Collapse::Keep);
                    None
                }
            ),
        );
    }
}

/// `insert-text`, before the default handler. Typed text at the caret becomes column typing:
/// the emission stops, the text goes in on every line, and the caller's iterator is moved to
/// the caret after it, as the default handler would.
unsafe extern "C" fn insert_text_trampoline(
    buffer: *mut gtk::ffi::GtkTextBuffer,
    location: *mut gtk::ffi::GtkTextIter,
    text: *const c_char,
    len: c_int,
    data: glib::ffi::gpointer,
) {
    // SAFETY: GTK passes a valid buffer, iterator and text of `len` bytes (or NUL-terminated
    // when `len` is negative); `data` is the weak reference boxed in `connect_buffer`.
    unsafe {
        let weak = &*(data as *const glib::WeakRef<StetView>);
        let Some(view) = weak.upgrade() else {
            return;
        };
        let bytes = if len < 0 {
            CStr::from_ptr(text).to_bytes()
        } else {
            std::slice::from_raw_parts(text.cast::<u8>(), len as usize)
        };
        let Ok(text) = std::str::from_utf8(bytes) else {
            return;
        };
        let iter = gtk::TextIter::from_glib_none(location as *const gtk::ffi::GtkTextIter);
        if !view.takes_insert(&iter, text) {
            return;
        }
        glib::gobject_ffi::g_signal_stop_emission_by_name(buffer.cast(), c"insert-text".as_ptr());
        let caret = view.column_type(text);
        *location = *caret.to_glib_none().0;
    }
}

impl StetView {
    /// Whether an insertion is typing at the column caret, which column mode carries out.
    /// Anything else ends column mode and goes in as GTK puts it.
    fn takes_insert(&self, location: &gtk::TextIter, text: &str) -> bool {
        if self.is_busy() {
            return false;
        }
        let Some(rect) = self.rect() else {
            return false;
        };
        let buffer = self.buffer();
        let caret = buffer.iter_at_mark(&buffer.get_insert());
        if location.offset() == caret.offset()
            && location.line().max(0) as usize == rect.cursor.line
            && !text.is_empty()
            && !text.contains(['\n', '\r'])
        {
            return true;
        }
        self.end_column_mode(Collapse::Keep);
        false
    }

    /// Whether to drop a deletion: in overwrite mode GtkTextView deletes the character after
    /// the caret before it inserts typed text; column typing replaces the cells itself.
    /// Any other deletion ends column mode.
    fn swallows_delete(&self, start: &gtk::TextIter, end: &gtk::TextIter) -> bool {
        if self.is_busy() {
            return false;
        }
        let Some(rect) = self.rect() else {
            return false;
        };
        let buffer = self.buffer();
        let caret = buffer.iter_at_mark(&buffer.get_insert());
        let (from, to) = if start.offset() <= end.offset() {
            (start, end)
        } else {
            (end, start)
        };
        let mut next = *from;
        next.forward_cursor_position();
        if self.overwrites()
            && *from == caret
            && next == *to
            && from.line().max(0) as usize == rect.cursor.line
        {
            return true;
        }
        self.end_column_mode(Collapse::Keep);
        false
    }
}
