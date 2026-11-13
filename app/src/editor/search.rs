//! The document's search state, and the current search match: highlighted, and remembered with
//! marks so the next search continues from it.

use super::document::CURRENT_MATCH_TAG;
use super::page::EditorPage;
use crate::page_search::PageSearch;
use gtk4 as gtk;
use gtk4::prelude::*;

const MATCH_START_MARK: &str = "stet-match-start";
const MATCH_END_MARK: &str = "stet-match-end";

impl EditorPage {
    /// Marks `start..end` as the current search match: highlighted, and remembered with marks
    /// so the next search continues from it.
    pub fn set_current_match(&self, start: &gtk::TextIter, end: &gtk::TextIter) {
        self.clear_current_match();
        let buffer = self.buffer();
        if let Some(tag) = buffer.tag_table().lookup(CURRENT_MATCH_TAG) {
            buffer.apply_tag(&tag, start, end);
        }
        buffer.create_mark(Some(MATCH_START_MARK), start, true);
        buffer.create_mark(Some(MATCH_END_MARK), end, false);
        self.keep_current_match_on_top();
    }

    /// GtkSourceView raises its own search-match tag to the top priority whenever it scans,
    /// which would hide the current match's style; put ours back above it.
    pub fn keep_current_match_on_top(&self) {
        let buffer = self.buffer();
        if buffer.mark(MATCH_START_MARK).is_none() {
            return;
        }
        let table = buffer.tag_table();
        if let Some(tag) = table.lookup(CURRENT_MATCH_TAG)
            && tag.priority() < table.size() - 1
        {
            tag.set_priority(table.size() - 1);
        }
    }

    pub fn current_match(&self) -> Option<(gtk::TextIter, gtk::TextIter)> {
        let buffer = self.buffer();
        let start = buffer.mark(MATCH_START_MARK)?;
        let end = buffer.mark(MATCH_END_MARK)?;
        Some((buffer.iter_at_mark(&start), buffer.iter_at_mark(&end)))
    }

    pub fn clear_current_match(&self) {
        let buffer = self.buffer();
        buffer.remove_tag_by_name(CURRENT_MATCH_TAG, &buffer.start_iter(), &buffer.end_iter());
        for name in [MATCH_START_MARK, MATCH_END_MARK] {
            if let Some(mark) = buffer.mark(name) {
                buffer.delete_mark(&mark);
            }
        }
    }

    /// The document's search state: its search contexts and our own matcher's matches.
    pub fn search(&self) -> &PageSearch {
        &self.document().search
    }
}
