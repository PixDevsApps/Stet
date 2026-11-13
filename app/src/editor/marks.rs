//! A page's bookmarks, in its gutter, and the document's marks and their tags.

use super::page::EditorPage;
use crate::marks::DocMarks;
use gtk4 as gtk;
use gtk4::subclass::prelude::*;
use stet_domain::marks::{Bookmarks, Footprint};
use stet_domain::text::TextEdit;

impl EditorPage {
    pub fn bookmarks(&self) -> Bookmarks {
        crate::marks::bookmarks(&self.buffer())
    }

    pub fn set_bookmarks(&self, bookmarks: &Bookmarks) {
        crate::marks::set_bookmarks(&self.buffer(), bookmarks);
    }

    pub fn has_bookmarks(&self) -> bool {
        crate::marks::has_bookmarks(&self.buffer())
    }

    /// The picture this view's gutter draws for a bookmark.
    pub fn bookmark_pixbuf(&self) -> Option<gtk::gdk_pixbuf::Pixbuf> {
        Some(self.imp().bookmark.get()?.pixbuf())
    }

    /// Toggle Bookmark on `line`; returns whether it is bookmarked now.
    pub fn toggle_bookmark(&self, line: usize) -> bool {
        crate::marks::toggle_bookmark(&self.buffer(), line)
    }

    /// The document's bookmark history and marks.
    pub fn marks(&self) -> &DocMarks {
        &self.document().marks
    }

    /// The marks changed: their tags follow now.
    pub fn retag_marks(&self) {
        self.document().retag_now();
    }

    /// Runs one of Stet's own edits through the document's bookmark and mark tracking
    /// ([`DocMarks::track`]).
    pub fn track_edit(
        &self,
        text: &str,
        edits: &[TextEdit],
        after: Option<Bookmarks>,
        apply: impl FnOnce() -> Footprint,
    ) {
        let document = self.document();
        document
            .marks
            .track(&document.buffer, text, edits, after, apply);
        document.schedule_retag();
    }
}
