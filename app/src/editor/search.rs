//! A page's search state: the document's two GtkSourceView search contexts (the find bar's and
//! smart highlighting's), the matches our own matcher found for the queries GtkSourceView
//! cannot run (ADR-005 amendment), and the current search match, highlighted and remembered
//! with marks so the next search continues from it. Each tab has its own search settings, so a
//! query reaches a tab's context only when the tab is shown, and hidden tabs never rescan.

use super::document::CURRENT_MATCH_TAG;
use super::page::EditorPage;
use crate::convert::{BufferPositions, IterPosition};
use crate::runtime::{Debounce, Latest};
use crate::theme::scheme::SMART_HIGHLIGHT_STYLE;
use gtk4 as gtk;
use gtk4::prelude::*;
use sourceview5::prelude::*;
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use stet_domain::search::{Query, Translated};
use stet_domain::text::CharOffset;
use stet_infrastructure::search::{Match, Matcher};

/// The tag that shows our own matcher's matches, styled like GtkSourceView's `search-match`.
pub const OWN_MATCH_TAG: &str = "stet-search-match";

/// How many lines above and below the visible ones get our own matches' highlight.
const TAG_MARGIN_LINES: usize = 100;

const MATCH_START_MARK: &str = "stet-match-start";
const MATCH_END_MARK: &str = "stet-match-end";

/// A query compiled once for every tab.
#[derive(Debug)]
pub struct Compiled {
    /// Changes whenever the query or its options change.
    pub id: u64,
    pub query: Query,
    pub matcher: Arc<Matcher>,
}

impl Compiled {
    /// GtkSourceView cannot count, highlight or navigate this query; our matcher does.
    pub fn own(&self) -> bool {
        self.matcher.needs_own_matcher()
    }

    pub fn translated(&self) -> &Translated {
        self.matcher.translated()
    }
}

/// Our matcher's matches for one query in one revision of a tab's text.
#[derive(Debug)]
pub struct OwnMatches {
    pub query: u64,
    pub revision: u64,
    pub matches: Arc<Vec<Match>>,
    /// More matches exist than were collected.
    pub truncated: bool,
}

#[derive(Debug)]
pub struct PageSearch {
    settings: sourceview5::SearchSettings,
    context: RefCell<Option<sourceview5::SearchContext>>,
    smart_settings: sourceview5::SearchSettings,
    smart: RefCell<Option<sourceview5::SearchContext>>,
    /// The compiled query whose pattern the find context holds.
    applied: Cell<Option<u64>>,
    pub own: RefCell<Option<OwnMatches>>,
    /// A ticket per own-matcher search, so stale results are dropped.
    pub own_searches: Latest,
    /// The query and revision of the search in progress.
    pub own_pending: Cell<Option<(u64, u64)>>,
    pub own_cancel: RefCell<Option<Arc<AtomicBool>>>,
    /// The character range our match tag currently covers.
    tagged: Cell<Option<(usize, usize)>>,
    /// The pending search after an edit.
    pub own_refresh: Debounce,
    /// Smart highlighting's pending update.
    pub smart_update: Debounce,
}

impl Default for PageSearch {
    fn default() -> Self {
        let settings = sourceview5::SearchSettings::new();
        settings.set_regex_enabled(true);
        settings.set_at_word_boundaries(false);
        let smart_settings = sourceview5::SearchSettings::new();
        smart_settings.set_regex_enabled(true);
        smart_settings.set_at_word_boundaries(false);
        smart_settings.set_wrap_around(false);
        Self {
            settings,
            context: RefCell::new(None),
            smart_settings,
            smart: RefCell::new(None),
            applied: Cell::new(None),
            own: RefCell::new(None),
            own_searches: Latest::default(),
            own_pending: Cell::new(None),
            own_cancel: RefCell::new(None),
            tagged: Cell::new(None),
            own_refresh: Debounce::default(),
            smart_update: Debounce::default(),
        }
    }
}

impl PageSearch {
    /// The find bar's context over `buffer`, created on first use.
    pub fn context(&self, buffer: &sourceview5::Buffer) -> (sourceview5::SearchContext, bool) {
        if let Some(context) = self.context.borrow().as_ref() {
            return (context.clone(), false);
        }
        let context = sourceview5::SearchContext::new(buffer, Some(&self.settings));
        context.set_highlight(false);
        self.context.replace(Some(context.clone()));
        (context, true)
    }

    pub fn existing_context(&self) -> Option<sourceview5::SearchContext> {
        self.context.borrow().clone()
    }

    /// Gives the find context `compiled`'s pattern, or no pattern when there is no query or our
    /// own matcher runs it. Only changed settings are written, so the context rescans only
    /// when the query really changed.
    pub fn apply(&self, compiled: Option<&Compiled>, wrap: bool) {
        if self.settings.wraps_around() != wrap {
            self.settings.set_wrap_around(wrap);
        }
        let id = compiled.map(|compiled| compiled.id);
        if self.applied.get() == id && id.is_some() {
            return;
        }
        self.applied.set(id);
        let widget = compiled.filter(|compiled| !compiled.own());
        let Some(compiled) = widget else {
            if self.settings.search_text().is_some() {
                self.settings.set_search_text(None);
            }
            return;
        };
        let translated = compiled.translated();
        if self.settings.search_text().is_some() {
            self.settings.set_search_text(None);
        }
        self.settings.set_case_sensitive(translated.case_sensitive);
        self.settings
            .set_at_word_boundaries(translated.at_word_boundaries);
        self.settings.set_regex_enabled(true);
        self.settings.set_search_text(Some(&translated.pattern));
    }

    /// Smart highlighting of `pattern` (a translated whole-word query), or none.
    pub fn set_smart(
        &self,
        buffer: &sourceview5::Buffer,
        translated: Option<&Translated>,
        scheme: Option<&sourceview5::StyleScheme>,
    ) {
        let Some(translated) = translated else {
            if self.smart_settings.search_text().is_some() {
                self.smart_settings.set_search_text(None);
            }
            return;
        };
        if self.smart.borrow().is_none() {
            let context = sourceview5::SearchContext::new(buffer, Some(&self.smart_settings));
            let style = scheme.and_then(|scheme| scheme.style(SMART_HIGHLIGHT_STYLE));
            set_match_style(&context, style.as_ref());
            self.smart.replace(Some(context));
        }
        let same = self.smart_settings.search_text().as_deref()
            == Some(translated.pattern.as_str())
            && self.smart_settings.is_case_sensitive() == translated.case_sensitive;
        if same {
            return;
        }
        self.smart_settings.set_search_text(None);
        self.smart_settings
            .set_case_sensitive(translated.case_sensitive);
        self.smart_settings
            .set_search_text(Some(&translated.pattern));
    }

    /// The pattern smart highlighting looks for.
    #[cfg(feature = "selftest")]
    pub fn smart_pattern(&self) -> Option<String> {
        self.smart_settings
            .search_text()
            .map(|text| text.to_string())
    }

    /// The smart-highlight context while it highlights something.
    #[cfg(feature = "selftest")]
    pub fn smart_context(&self) -> Option<sourceview5::SearchContext> {
        self.smart_settings.search_text()?;
        self.smart.borrow().clone()
    }

    pub fn set_scheme(&self, scheme: &sourceview5::StyleScheme) {
        if let Some(context) = self.smart.borrow().as_ref() {
            set_match_style(context, scheme.style(SMART_HIGHLIGHT_STYLE).as_ref());
        }
    }

    /// Our matcher's matches for query `id` at `revision`.
    pub fn own_matches(&self, id: u64, revision: u64) -> Option<(Arc<Vec<Match>>, bool)> {
        self.own
            .borrow()
            .as_ref()
            .filter(|own| own.query == id && own.revision == revision)
            .map(|own| (Arc::clone(&own.matches), own.truncated))
    }

    /// Drops our matcher's matches and their highlight, and cancels a search in progress.
    pub fn clear_own(&self, buffer: &sourceview5::Buffer) {
        if let Some(cancel) = self.own_cancel.take() {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.own_searches.invalidate();
        self.own_pending.set(None);
        self.own.replace(None);
        self.untag(buffer);
    }

    fn untag(&self, buffer: &sourceview5::Buffer) {
        if let Some((start, end)) = self.tagged.take() {
            let end = end.min(buffer.total_chars());
            buffer.remove_tag_by_name(
                OWN_MATCH_TAG,
                &buffer.iter_at_char_offset(start.min(end)),
                &buffer.iter_at_char_offset(end),
            );
        }
    }

    /// Highlights our matcher's non-empty matches around the visible lines of `view`, if they
    /// were found in `revision` of the text; nothing when `show` is false.
    pub fn tag_own(&self, view: &sourceview5::View, revision: u64, show: bool) {
        let buffer: sourceview5::Buffer = view.buffer().downcast().expect("a GtkSourceBuffer");
        self.untag(&buffer);
        if !show {
            return;
        }
        let own = self.own.borrow();
        let Some(own) = own.as_ref().filter(|own| own.revision == revision) else {
            return;
        };
        let rect = view.visible_rect();
        let top = view
            .iter_at_location(rect.x(), rect.y())
            .map_or(0, |iter| iter.line_number());
        let bottom = view
            .iter_at_location(rect.x(), rect.y() + rect.height())
            .map_or_else(|| buffer.total_lines(), |iter| iter.line_number());
        let first_line = top.saturating_sub(TAG_MARGIN_LINES);
        let last_line = bottom.saturating_add(TAG_MARGIN_LINES);
        let start = buffer
            .iter_at_line_number(first_line)
            .map_or(0, |iter| iter.char_offset());
        let end = buffer
            .iter_at_line_number(last_line)
            .map_or_else(|| buffer.total_chars(), |iter| iter.char_offset());
        let matches = &own.matches;
        let (first, last) = (CharOffset::new(start), CharOffset::new(end));
        let from = matches.partition_point(|found| found.end < first);
        let Some(tag) = buffer.tag_table().lookup(OWN_MATCH_TAG) else {
            return;
        };
        for found in matches[from..]
            .iter()
            .take_while(|found| found.start <= last)
        {
            if found.end > found.start {
                buffer.apply_tag(
                    &tag,
                    &buffer.iter_at_char_offset(found.start.get()),
                    &buffer.iter_at_char_offset(found.end.get()),
                );
            }
        }
        self.tagged.set(Some((start, end)));
    }

    /// 1-based position of the match `start..end` among our matcher's matches, if it is one.
    pub fn own_position(&self, start: usize, end: usize) -> Option<usize> {
        let own = self.own.borrow();
        let matches = &own.as_ref()?.matches;
        let (start, end) = (CharOffset::new(start), CharOffset::new(end));
        let index = matches.partition_point(|found| found.start < start);
        matches
            .get(index)
            .filter(|found| found.start == start && found.end == end)
            .map(|_| index + 1)
    }
}

/// Gives `context`'s highlighting `style` (the scheme's `search-match` when `None`), and shows
/// it at once: GtkSourceView applies a match style only when the buffer's scheme or the
/// highlighting changes (5.20), so the highlighting is turned off and on again.
fn set_match_style(context: &sourceview5::SearchContext, style: Option<&sourceview5::Style>) {
    context.set_match_style(style);
    context.set_highlight(false);
    context.set_highlight(true);
}

/// Applies a scheme style to a tag, or clears the tag's colours when the scheme has no such
/// style.
pub fn style_tag(tag: &gtk::TextTag, scheme: &sourceview5::StyleScheme, style: &str) {
    match scheme.style(style) {
        Some(style) => style.apply(tag),
        None => {
            tag.set_background_rgba(None);
            tag.set_foreground_rgba(None);
        }
    }
}

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
