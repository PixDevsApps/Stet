//! A page's own caret and selection. GTK keeps one caret per buffer, so while another view of
//! the document is in front, this page keeps its caret and selection in marks of its own.

use super::column::StetView;
use super::page::EditorPage;
use crate::convert::IterPosition;
use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use stet_domain::text::Position;

impl EditorPage {
    /// Keeps the caret and selection the buffer has now as this view's, while another view of
    /// the document is in front.
    pub fn save_selection(&self) {
        let buffer = self.buffer();
        let insert = buffer.iter_at_mark(&buffer.get_insert());
        let bound = buffer.iter_at_mark(&buffer.selection_bound());
        let mut saved = self.imp().selection.borrow_mut();
        match saved.as_ref() {
            Some((insert_mark, bound_mark)) => {
                buffer.move_mark(insert_mark, &insert);
                buffer.move_mark(bound_mark, &bound);
            }
            None => {
                *saved = Some((
                    buffer.create_mark(None, &insert, false),
                    buffer.create_mark(None, &bound, false),
                ));
            }
        }
    }

    /// This view is the one in front: the buffer gets its caret and selection, and keeps those
    /// of the view of the document that had them.
    pub fn take_caret(&self) {
        let document = self.document();
        let owner = document.caret_owner.upgrade();
        if owner.as_ref() == Some(self) {
            return;
        }
        if let Some(owner) = owner {
            owner.save_selection();
        }
        self.restore_selection();
        document.caret_owner.set(Some(self));
    }

    /// Whether the buffer has this view's caret: it is the view in front, or no view is yet.
    fn has_caret(&self) -> bool {
        self.document()
            .caret_owner
            .upgrade()
            .is_none_or(|owner| owner == *self)
    }

    /// This view's caret and selection, in characters: (caret, selection).
    pub fn own_selection(&self) -> (usize, Option<(usize, usize)>) {
        let buffer = self.buffer();
        let saved = self.imp().selection.borrow().clone();
        let (insert, bound) = match saved {
            Some((insert, bound)) if !self.has_caret() => {
                (buffer.iter_at_mark(&insert), buffer.iter_at_mark(&bound))
            }
            _ => (
                buffer.iter_at_mark(&buffer.get_insert()),
                buffer.iter_at_mark(&buffer.selection_bound()),
            ),
        };
        let (insert, bound) = (insert.char_offset(), bound.char_offset());
        (
            insert,
            (insert != bound).then(|| (insert.min(bound), insert.max(bound))),
        )
    }

    /// Puts this view's caret at `caret` with the selection reaching to `anchor` (characters):
    /// in the buffer when it has this view's caret, else in the marks this view keeps.
    pub fn place_selection(&self, anchor: usize, caret: usize) {
        let buffer = self.buffer();
        let at = |chars: usize| buffer.iter_at_offset(i32::try_from(chars).unwrap_or(i32::MAX));
        let document = self.document();
        if document.caret_owner.upgrade().is_none() {
            document.caret_owner.set(Some(self));
        }
        if self.has_caret() {
            buffer.select_range(&at(caret), &at(anchor));
            return;
        }
        let mut saved = self.imp().selection.borrow_mut();
        match saved.as_ref() {
            Some((insert, bound)) => {
                buffer.move_mark(insert, &at(caret));
                buffer.move_mark(bound, &at(anchor));
            }
            None => {
                *saved = Some((
                    buffer.create_mark(None, &at(caret), false),
                    buffer.create_mark(None, &at(anchor), false),
                ));
            }
        }
    }

    /// Puts this view's caret and selection back in the buffer, without ending column mode in
    /// any view of the document.
    pub fn restore_selection(&self) {
        let Some((insert, bound)) = self.imp().selection.borrow().clone() else {
            return;
        };
        let buffer = self.buffer();
        let (insert, bound) = (buffer.iter_at_mark(&insert), buffer.iter_at_mark(&bound));
        let views: Vec<StetView> = self
            .document_pages()
            .iter()
            .map(|page| page.column_view().clone())
            .collect();
        quietly(&views, &|| buffer.select_range(&insert, &bound));
    }

    /// Moves the cursor to `position`, clamped to the text, and scrolls it into view.
    pub fn go_to(&self, position: Position) {
        let buffer = self.buffer();
        let Some(iter) = self.iter_at(position) else {
            return;
        };
        buffer.place_cursor(&iter);
        self.view()
            .scroll_to_mark(&buffer.get_insert(), 0.1, true, 0.0, 0.3);
    }

    /// Moves the cursor to `position`, clamped to the text, without scrolling.
    pub fn place_cursor(&self, position: Position) {
        if let Some(iter) = self.iter_at(position) {
            self.buffer().place_cursor(&iter);
        }
    }

    fn iter_at(&self, position: Position) -> Option<gtk::TextIter> {
        let buffer = self.buffer();
        let last_line = (buffer.line_count() - 1).max(0);
        let line = i32::try_from(position.line)
            .unwrap_or(i32::MAX)
            .min(last_line);
        let mut iter = buffer.iter_at_line(line)?;
        let mut column = position.column;
        while column > 0 && !iter.ends_line() {
            iter.forward_char();
            column -= 1;
        }
        Some(iter)
    }

    /// Selects the character at `offset` and shows it (the unmappable dialog's Go To).
    pub fn select_char(&self, offset: usize) {
        let buffer = self.buffer();
        let start = buffer.iter_at_offset(i32::try_from(offset).unwrap_or(i32::MAX));
        let mut end = start;
        end.forward_char();
        buffer.select_range(&start, &end);
        self.view()
            .scroll_to_mark(&buffer.get_insert(), 0.1, true, 0.0, 0.3);
        self.view().grab_focus();
    }

    /// The cursor as a zero-based line and character column.
    pub fn cursor(&self) -> Position {
        let buffer = self.buffer();
        let iter = buffer.iter_at_mark(&buffer.get_insert());
        Position::new(iter.line_number(), iter.column())
    }

    /// The vertical scroll position, to keep it across a reload.
    pub fn scroll_position(&self) -> f64 {
        self.view()
            .vadjustment()
            .map_or(0.0, |adjustment| adjustment.value())
    }

    pub fn set_scroll_position(&self, value: f64) {
        if let Some(adjustment) = self.view().vadjustment() {
            adjustment.set_value(value);
        }
    }
}

/// Runs `f` with every view in `views` letting caret moves through.
fn quietly(views: &[StetView], f: &dyn Fn()) {
    match views.split_first() {
        Some((view, rest)) => view.quietly(|| quietly(rest, f)),
        None => f(),
    }
}
