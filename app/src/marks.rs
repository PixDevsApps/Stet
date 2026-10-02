//! Bookmarks and marks in a document's buffer (M7).
//!
//! - **Bookmarks** are GtkSourceView source marks of [`BOOKMARK_CATEGORY`] at line starts, so
//!   the buffer moves them with the text, and the views draw them in their gutter. Stet's own
//!   operations (bulk edits, the bookmarked-line operations) set them from
//!   `stet_domain::marks` afterwards, and keep a [`BookmarkHistory`] step, because GTK's Undo
//!   and Redo replay such a step as replacements that collapse the marks inside them (ADR-003
//!   amendment). While GTK replays an undo or a redo, the replacements it makes are added up
//!   into a [`Footprint`]; when it is one of those steps, the bookmarks of its lines come back.
//! - **Marks** (the Find Mark Style and the five token styles) are ranges in a [`Marks`]
//!   model, moved through every edit, and shown with text tags in the scheme's colours around
//!   the lines the views show, as the find bar's own matches are (M4). Stet's own steps keep
//!   them from before and after too, as GTK's replay of such a step would drop them.

use gtk4 as gtk;
use gtk4::glib;
use gtk4::glib::translate::{ToGlibPtr, ToGlibPtrMut};
use gtk4::prelude::*;
use sourceview5::prelude::*;
use std::cell::{Cell, Ref, RefCell};
use std::ops::Range;
use stet_domain::marks::{BookmarkHistory, Bookmarks, Footprint, MarkStyle, Marks, Step};
use stet_domain::text::{LineIndex, TextEdit};
use stet_domain::theme::{MARK_STYLE, TOKEN_STYLES};

/// The source-mark category of bookmarks.
pub const BOOKMARK_CATEGORY: &str = "stet-bookmark";

/// How many lines above and below the visible ones get the marks' tags.
const TAG_MARGIN_LINES: i32 = 100;

/// The text tag of a mark style.
pub fn tag_name(style: MarkStyle) -> &'static str {
    match style {
        MarkStyle::Mark => "stet-mark",
        MarkStyle::Token1 => "stet-token-1",
        MarkStyle::Token2 => "stet-token-2",
        MarkStyle::Token3 => "stet-token-3",
        MarkStyle::Token4 => "stet-token-4",
        MarkStyle::Token5 => "stet-token-5",
    }
}

/// The scheme style a mark style's tag takes its colours from.
pub fn scheme_style(style: MarkStyle) -> &'static str {
    match style {
        MarkStyle::Mark => MARK_STYLE,
        MarkStyle::Token1 => TOKEN_STYLES[0],
        MarkStyle::Token2 => TOKEN_STYLES[1],
        MarkStyle::Token3 => TOKEN_STYLES[2],
        MarkStyle::Token4 => TOKEN_STYLES[3],
        MarkStyle::Token5 => TOKEN_STYLES[4],
    }
}

/// Creates the mark styles' tags in a new buffer.
pub fn create_tags(buffer: &sourceview5::Buffer) {
    for style in MarkStyle::ALL {
        buffer.create_tag(Some(tag_name(style)), &[]);
    }
}

/// The bookmarked lines of `buffer`.
pub fn bookmarks(buffer: &sourceview5::Buffer) -> Bookmarks {
    let mut iter = buffer.start_iter();
    let mut lines = Vec::new();
    if !buffer
        .source_marks_at_iter(&mut iter, Some(BOOKMARK_CATEGORY))
        .is_empty()
    {
        lines.push(0);
    }
    while forward_to_bookmark(buffer, &mut iter) {
        lines.push(iter.line().max(0) as usize);
    }
    lines.into_iter().collect()
}

/// Moves `iter` to the next bookmark after it; false when there is none.
fn forward_to_bookmark(buffer: &sourceview5::Buffer, iter: &mut gtk::TextIter) -> bool {
    let category = std::ffi::CString::new(BOOKMARK_CATEGORY).expect("no NUL in the category");
    // SAFETY: a valid buffer and iterator and a NUL-terminated category.
    unsafe {
        sourceview5::ffi::gtk_source_buffer_forward_iter_to_source_mark(
            buffer.to_glib_none().0,
            iter.to_glib_none_mut().0,
            category.as_ptr(),
        ) != glib::ffi::GFALSE
    }
}

pub fn has_bookmarks(buffer: &sourceview5::Buffer) -> bool {
    let mut iter = buffer.start_iter();
    !buffer
        .source_marks_at_iter(&mut iter, Some(BOOKMARK_CATEGORY))
        .is_empty()
        || forward_to_bookmark(buffer, &mut iter)
}

/// Makes the bookmarks exactly `lines` (those past the end of the text are dropped).
///
/// A replacement collapses the marks inside it onto one place, and GtkTextBuffer keeps a
/// line's marks in a list that removing a mark walks from its head: GtkSourceView's
/// `remove_source_marks` took 1.06 s for 10,562 collapsed bookmarks (release build). Removing
/// each place's marks in the order of that list takes 0.14 s.
pub fn set_bookmarks(buffer: &sourceview5::Buffer, lines: &Bookmarks) {
    let mut iter = buffer.start_iter();
    loop {
        let here: Vec<gtk::TextMark> = iter
            .marks()
            .into_iter()
            .filter(|mark| {
                mark.downcast_ref::<sourceview5::Mark>()
                    .is_some_and(|mark| mark.category() == BOOKMARK_CATEGORY)
            })
            .collect();
        for mark in here.iter().rev() {
            buffer.delete_mark(mark);
        }
        if !forward_to_bookmark(buffer, &mut iter) {
            break;
        }
    }
    let count = buffer.line_count().max(1) as usize;
    for &line in lines.lines().iter().take_while(|&&line| line < count) {
        if let Some(iter) = buffer.iter_at_line(line as i32) {
            buffer.create_source_mark(None, BOOKMARK_CATEGORY, &iter);
        }
    }
}

/// Toggle Bookmark on `line`; returns whether it is bookmarked now.
pub fn toggle_bookmark(buffer: &sourceview5::Buffer, line: usize) -> bool {
    let Some(start) = buffer.iter_at_line(line as i32) else {
        return false;
    };
    let mut end = start;
    if !end.ends_line() {
        end.forward_to_line_end();
    }
    let marked = !buffer
        .source_marks_at_line(line as i32, Some(BOOKMARK_CATEGORY))
        .is_empty();
    if marked {
        buffer.remove_source_marks(&start, &end, Some(BOOKMARK_CATEGORY));
    } else {
        buffer.create_source_mark(None, BOOKMARK_CATEGORY, &start);
    }
    !marked
}

/// What one of Stet's own operations found before it changed the buffer
/// ([`DocMarks::begin`]).
pub struct Begun {
    bookmarks: Bookmarks,
    marks: Marks,
    len: usize,
}

/// The bookmarks and marks of one document, and the steps that keep bookmarks across Undo
/// and Redo.
#[derive(Default)]
pub struct DocMarks {
    history: RefCell<BookmarkHistory>,
    /// GTK is replaying an undo (`true`) or a redo, and what it changed so far.
    replay: Cell<Option<(bool, Footprint)>>,
    /// One of Stet's own operations is changing the buffer: it accounts for its edits itself.
    own: Cell<u32>,
    marks: RefCell<Marks>,
    /// The character ranges the marks' tags cover now.
    tagged: RefCell<Vec<Range<i32>>>,
}

impl DocMarks {
    pub fn marks(&self) -> Ref<'_, Marks> {
        self.marks.borrow()
    }

    pub fn update_marks<R>(&self, update: impl FnOnce(&mut Marks) -> R) -> R {
        update(&mut self.marks.borrow_mut())
    }

    pub fn history_steps(&self) -> usize {
        self.history.borrow().len()
    }

    /// The document loaded again without undo: nothing can be undone into the old text.
    pub fn forget_history(&self) {
        self.history.borrow_mut().clear();
    }

    /// Runs one of Stet's own operations on `buffer`: `apply` makes `edits` (sorted, in the
    /// offsets of `text`, the buffer's text before) and returns the footprint of what reached
    /// the buffer. Afterwards the bookmarks are `after`, or follow `edits` by the domain's
    /// rules; the marks follow `edits`; and the step is kept for Undo and Redo when bookmarks
    /// or marks were involved.
    pub fn track(
        &self,
        buffer: &sourceview5::Buffer,
        text: &str,
        edits: &[TextEdit],
        after: Option<Bookmarks>,
        apply: impl FnOnce() -> Footprint,
    ) {
        let begun = self.begin(buffer);
        let footprint = apply();
        let bookmarked =
            !begun.bookmarks.is_empty() || after.as_ref().is_some_and(|after| !after.is_empty());
        let index = bookmarked.then(|| LineIndex::new(text));
        self.finish(buffer, begun, index.as_ref(), edits, after, footprint);
    }

    /// Starts one of Stet's own operations, which may change the buffer across main-loop
    /// iterations, for [`Self::finish`].
    pub fn begin(&self, buffer: &sourceview5::Buffer) -> Begun {
        let begun = Begun {
            bookmarks: bookmarks(buffer),
            marks: self.marks.borrow().clone(),
            len: buffer.char_count().max(0) as usize,
        };
        self.own.set(self.own.get() + 1);
        begun
    }

    /// Ends an operation [`Self::begin`] started, as [`Self::track`] does. `index` describes
    /// the text before it; without one the bookmarks stay where GTK left them, and Undo can't
    /// bring them back.
    pub fn finish(
        &self,
        buffer: &sourceview5::Buffer,
        begun: Begun,
        index: Option<&LineIndex>,
        edits: &[TextEdit],
        after: Option<Bookmarks>,
        footprint: Footprint,
    ) {
        self.own.set(self.own.get().saturating_sub(1));
        if !edits.is_empty() {
            self.marks.borrow_mut().remap(edits);
        }
        let marks_after = self.marks.borrow().clone();
        let marks = (!begun.marks.is_empty() || !marks_after.is_empty())
            .then(|| Box::new((begun.marks, marks_after)));
        let bookmarked =
            !begun.bookmarks.is_empty() || after.as_ref().is_some_and(|after| !after.is_empty());
        let step = match index {
            Some(index) if bookmarked => {
                let after = after.unwrap_or_else(|| {
                    let mut moved = begun.bookmarks.clone();
                    moved.remap(edits, index);
                    moved
                });
                set_bookmarks(buffer, &after);
                Step::new(edits, &[], index, &begun.bookmarks, &after)
            }
            // Only the marks to keep: a step over no lines.
            _ if marks.is_some() => Some(Step {
                footprint,
                len_before: begun.len,
                len_after: buffer.char_count().max(0) as usize,
                lines_before: 0..0,
                lines_after: 0..0,
                before: Bookmarks::new(),
                after: Bookmarks::new(),
                marks: None,
            }),
            _ => None,
        };
        match step {
            Some(step) => self.history.borrow_mut().record(Step {
                footprint,
                marks,
                ..step
            }),
            None => self.history.borrow_mut().edited(),
        }
    }

    /// Runs a column edit (M6), which never adds or removes lines: the bookmarks of `lines`
    /// stay, and come back after Undo and Redo collapse them. `logical` are its edits in the
    /// buffer's offsets before it, for the marks.
    pub fn track_lines(
        &self,
        buffer: &sourceview5::Buffer,
        lines: Range<usize>,
        logical: &[TextEdit],
        apply: impl FnOnce() -> Footprint,
    ) {
        let begun = self.begin(buffer);
        let footprint = apply();
        self.own.set(self.own.get().saturating_sub(1));
        if !logical.is_empty() {
            self.marks.borrow_mut().remap(logical);
        }
        let marks_after = self.marks.borrow().clone();
        let marks = (!begun.marks.is_empty() || !marks_after.is_empty())
            .then(|| Box::new((begun.marks, marks_after)));
        let kept: Bookmarks = begun
            .bookmarks
            .in_lines(lines.clone())
            .iter()
            .copied()
            .collect();
        if kept.is_empty() && marks.is_none() {
            self.history.borrow_mut().edited();
            return;
        }
        self.history.borrow_mut().record(Step {
            footprint,
            len_before: begun.len,
            len_after: buffer.char_count().max(0) as usize,
            lines_before: lines.clone(),
            lines_after: lines,
            before: kept.clone(),
            after: kept,
            marks,
        });
    }

    /// `chars` characters went in at `offset`.
    pub fn on_insert(&self, offset: usize, chars: usize) {
        match self.replay.get() {
            Some((undo, mut seen)) => {
                seen.observe(offset, 0, chars);
                self.replay.set(Some((undo, seen)));
            }
            None if self.own.get() == 0 => self.history.borrow_mut().edited(),
            None => {}
        }
        if self.own.get() == 0 && !self.marks.borrow().is_empty() {
            // The domain only counts what an edit inserts.
            self.marks
                .borrow_mut()
                .remap(&[TextEdit::insert(offset, " ".repeat(chars))]);
        }
    }

    /// The characters `start..end` went.
    pub fn on_delete(&self, start: usize, end: usize) {
        match self.replay.get() {
            Some((undo, mut seen)) => {
                seen.observe(start, end - start, 0);
                self.replay.set(Some((undo, seen)));
            }
            None if self.own.get() == 0 => self.history.borrow_mut().edited(),
            None => {}
        }
        if self.own.get() == 0 && !self.marks.borrow().is_empty() {
            self.marks
                .borrow_mut()
                .remap(&[TextEdit::delete(start..end)]);
        }
    }

    /// GTK starts replaying an undo (`undo`) or a redo.
    pub fn on_replay_begin(&self, undo: bool) {
        self.replay.set(Some((undo, Footprint::default())));
    }

    /// GTK finished replaying: when it was one of the steps kept here, its bookmarks and marks
    /// come back.
    pub fn on_replay_end(&self, buffer: &sourceview5::Buffer) {
        let Some((undo, seen)) = self.replay.take() else {
            return;
        };
        if seen == Footprint::default() || self.history.borrow().is_empty() {
            return;
        }
        let len = buffer.char_count().max(0) as usize;
        let current = bookmarks(buffer);
        let mut history = self.history.borrow_mut();
        let restored = if undo {
            history.undone(seen, len, &current)
        } else {
            history.redone(seen, len, &current)
        };
        let Some(restored) = restored else {
            return;
        };
        let marks = if undo {
            history.undone_marks()
        } else {
            history.redone_marks()
        };
        if let Some(marks) = marks {
            self.marks.replace(marks.clone());
        }
        drop(history);
        if restored != current {
            set_bookmarks(buffer, &restored);
        }
    }

    /// Tags the marks around the lines `views` show, and removes the tags elsewhere.
    pub fn retag(&self, buffer: &sourceview5::Buffer, views: &[sourceview5::View]) {
        let table = buffer.tag_table();
        let len = buffer.char_count();
        for range in self.tagged.take() {
            let end = range.end.min(len);
            let start = range.start.min(end);
            for style in MarkStyle::ALL {
                buffer.remove_tag_by_name(
                    tag_name(style),
                    &buffer.iter_at_offset(start),
                    &buffer.iter_at_offset(end),
                );
            }
        }
        let marks = self.marks.borrow();
        if marks.is_empty() {
            return;
        }
        let windows = windows(buffer, views);
        for window in &windows {
            for style in MarkStyle::ALL {
                let Some(tag) = table.lookup(tag_name(style)) else {
                    continue;
                };
                let ranges = marks.ranges(style);
                let from = ranges.partition_point(|range| (range.end as i32) <= window.start);
                for range in ranges[from..]
                    .iter()
                    .take_while(|range| (range.start as i32) < window.end)
                {
                    buffer.apply_tag(
                        &tag,
                        &buffer.iter_at_offset(range.start as i32),
                        &buffer.iter_at_offset(range.end as i32),
                    );
                }
            }
        }
        self.tagged.replace(windows);
    }

    /// The character ranges the tags cover, for the self-test.
    pub fn tagged(&self) -> Vec<Range<i32>> {
        self.tagged.borrow().clone()
    }
}

/// The character ranges around the lines each view shows, merged where they overlap.
fn windows(buffer: &sourceview5::Buffer, views: &[sourceview5::View]) -> Vec<Range<i32>> {
    let mut windows: Vec<Range<i32>> = views
        .iter()
        .map(|view| {
            let rect = view.visible_rect();
            let top = view
                .iter_at_location(rect.x(), rect.y())
                .map_or(0, |iter| iter.line());
            let bottom = view
                .iter_at_location(rect.x(), rect.y() + rect.height())
                .map_or_else(|| buffer.line_count(), |iter| iter.line());
            let start = buffer
                .iter_at_line((top - TAG_MARGIN_LINES).max(0))
                .map_or(0, |iter| iter.offset());
            let end = buffer
                .iter_at_line(bottom.saturating_add(TAG_MARGIN_LINES))
                .map_or(buffer.char_count(), |iter| iter.offset());
            start..end
        })
        .collect();
    windows.sort_by_key(|window| window.start);
    let mut merged: Vec<Range<i32>> = Vec::new();
    for window in windows {
        match merged.last_mut() {
            Some(last) if window.start <= last.end => last.end = last.end.max(window.end),
            _ => merged.push(window),
        }
    }
    merged
}

/// The bookmark the gutter draws: a disc in `color`, as RGBA pixels `size` wide.
pub fn bookmark_pixbuf(color: &gtk::gdk::RGBA, size: i32) -> gtk::gdk_pixbuf::Pixbuf {
    let size = size.max(8);
    let center = size as f32 / 2.0;
    let radius = size as f32 * 0.32;
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - center;
            let dy = y as f32 + 0.5 - center;
            let distance = (dx * dx + dy * dy).sqrt();
            let coverage = (radius + 0.5 - distance).clamp(0.0, 1.0);
            pixels.extend([
                channel(color.red()),
                channel(color.green()),
                channel(color.blue()),
                channel(coverage * color.alpha()),
            ]);
        }
    }
    gtk::gdk_pixbuf::Pixbuf::from_bytes(
        &glib::Bytes::from_owned(pixels),
        gtk::gdk_pixbuf::Colorspace::Rgb,
        true,
        8,
        size,
        size,
        size * 4,
    )
}
