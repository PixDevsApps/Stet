//! Smart highlighting: when the selection is one whole word on one line, its other occurrences
//! are highlighted in the scheme's `stet:smart-highlight` style, through a second GtkSourceView
//! search context. Updates are debounced, and large documents skip it.

use super::Window;
use crate::editor::EditorPage;
use gtk4::glib;
use gtk4::prelude::*;
use std::time::Duration;
use stet_domain::search::{SmartHighlight, translate};

/// How long the selection must rest before its occurrences are highlighted.
const SMART_DELAY: Duration = Duration::from_millis(120);
/// Longer selections are not highlighted.
const SMART_MAX_CHARS: i32 = 1_000;

impl Window {
    /// The selection of `page` changed: smart highlighting follows it shortly after.
    pub(super) fn on_selection_changed(&self, page: &EditorPage) {
        let state = page.search();
        state.cancel_smart_timeout();
        let window = self.clone();
        let target = page.clone();
        let source = glib::timeout_add_local_once(SMART_DELAY, move || {
            target.search().smart_timeout.replace(None);
            window.update_smart_highlight(&target);
        });
        state.smart_timeout.replace(Some(source));
    }

    pub(super) fn update_smart_highlight(&self, page: &EditorPage) {
        let buffer = page.buffer();
        let query = buffer
            .selection_bounds()
            .filter(|(start, end)| {
                !page.large_file_mode()
                    && end.offset() - start.offset() <= SMART_MAX_CHARS
                    && self.settings().smart_highlight
            })
            .and_then(|(start, end)| {
                let selected = buffer.text(&start, &end, false);
                let mut before = start;
                let before = before.backward_char().then(|| before.char());
                let after = (!end.is_end()).then(|| end.char());
                SmartHighlight::default().query(&selected, before, after)
            });
        let translated = query.and_then(|query| translate(&query).ok());
        let scheme = self.inner.shared.appearance.scheme();
        page.search()
            .set_smart(&buffer, translated.as_ref(), scheme.as_ref());
    }
}
