//! The find bar's commands (Ctrl+F, F3, Shift+F3): the window opens and closes the bar, puts
//! its query into the visible tab, steps through matches and counts them.
//!
//! Every query is translated once ([`stet_domain::search::translate`]) and compiled by our own
//! matcher, which also reports pattern errors at the typed character. GtkSourceView's
//! `SearchContext` counts, highlights and finds the queries it can run; our matcher does it
//! for the rest (empty matches, `\K`, `\G`), on a snapshot on a worker, and highlights them
//! with a tag in the scheme's search-match colours. Nothing here replaces text through
//! GtkSourceView (ADR-005).

use super::SearchCommand;
use super::bar::{BarCommand, BarMode, Field, FieldKey, Tone};
use super::marking::MarkCommand;
use crate::convert::{BufferPositions, IterPosition};
use crate::editor::EditorPage;
use crate::editor::search::{Compiled, OwnMatches};
use crate::runtime::worker;
use crate::window::Window;
use crate::window::snapshot::PageSnapshot;
use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use stet_domain::search::{Direction, SearchHistory, Wrap};
use stet_domain::text::CharOffset;

/// Matches our own matcher collects for counting and highlighting in one document.
const OWN_MATCH_LIMIT: usize = 1_000_000;
/// How long after the last edit our own matcher searches again.
const OWN_REFRESH_DELAY: Duration = Duration::from_millis(150);
/// The same in large-file mode, where taking the snapshot itself is noticeable.
const OWN_REFRESH_DELAY_LARGE: Duration = Duration::from_millis(1_000);

/// Hooks the find bar's keys, query, buttons and folder chooser to the window.
pub(in crate::window) fn connect(window: &Window) {
    let find = &window.inner.find;
    find.connect_key(glib::clone!(
        #[weak]
        window,
        move |key| window.on_bar_key(key)
    ));
    find.connect_query_changed(glib::clone!(
        #[weak]
        window,
        move |incremental| window.on_query_changed(incremental)
    ));
    find.connect_wrap_toggled(glib::clone!(
        #[weak]
        window,
        move || window.sync_search_highlight()
    ));
    find.connect_step(glib::clone!(
        #[weak]
        window,
        move |forward| window.find_step(forward)
    ));
    find.connect_close(glib::clone!(
        #[weak]
        window,
        move || window.hide_find()
    ));
    find.connect_command(glib::clone!(
        #[weak]
        window,
        move |command| {
            match command {
                BarCommand::Count => window.run_search(SearchCommand::Count),
                BarCommand::FindAllInDocument => {
                    window.run_search(SearchCommand::FindAllInDocument);
                }
                BarCommand::FindAllInOpenDocuments => {
                    window.run_search(SearchCommand::FindAllInOpenDocuments);
                }
                BarCommand::ReplaceNext => window.run_search(SearchCommand::ReplaceNext),
                BarCommand::ReplaceAll => window.run_search(SearchCommand::ReplaceAll),
                BarCommand::ReplaceAllInOpenDocuments => {
                    window.run_search(SearchCommand::ReplaceAllInOpenDocuments);
                }
                BarCommand::FindInFiles => window.start_find_in_files(),
                BarCommand::ReplaceInFiles => window.run_search(SearchCommand::ReplaceInFiles),
                BarCommand::MarkAll => window.run_mark(MarkCommand::MarkAll),
                BarCommand::ClearMarks => window.run_mark(MarkCommand::ClearMarks),
                BarCommand::CopyMarkedText => window.run_mark(MarkCommand::CopyMarkedText),
            }
        }
    ));
    find.connect_browse(glib::clone!(
        #[weak]
        window,
        move || {
            let clone = window.clone();
            window.spawn(async move { clone.choose_search_folder().await });
        }
    ));
}

impl Window {
    /// Enter in a field runs its command; Escape closes the bar.
    fn on_bar_key(&self, key: FieldKey) {
        let mode = self.inner.find.bar_mode();
        match key {
            FieldKey::Enter { field, shift } => match field {
                Field::Directory | Field::Filters => self.start_find_in_files(),
                Field::Find | Field::Replace if mode == BarMode::Files => {
                    self.start_find_in_files();
                }
                Field::Find if mode == BarMode::Mark => self.mark_all(),
                Field::Find => self.find_step(!shift),
                Field::Replace => self.replace_next(!shift),
            },
            FieldKey::Escape => self.hide_find(),
        }
    }

    /// The find field or an option that changes the query changed: compile it again, show it
    /// in the visible tab, and with `incremental` move to the first match from the caret.
    fn on_query_changed(&self, incremental: bool) {
        let find = &self.inner.find;
        find.set_message("", Tone::Info);
        let compiled = find.compile();
        self.sync_search_highlight();
        // The field's change arrives after a short delay; once the bar is closed, it must not
        // move the selection any more.
        if incremental && compiled.is_some() && find.is_open() {
            self.find_from_selection_start();
        }
        self.update_match_count();
    }

    /// Opens the find bar (Ctrl+F), prefilled with a single-line selection.
    pub fn show_find(&self) {
        self.show_find_mode(BarMode::Find);
    }

    /// Opens the bar with the Find, Replace, Find in Files or Mark rows.
    pub fn show_find_mode(&self, mode: BarMode) {
        let find = &self.inner.find;
        let opening = !find.is_open();
        let entering = opening || find.bar_mode() != mode;
        if let Some(page) = self.current_page() {
            let buffer = page.buffer();
            match buffer.selection_bounds() {
                Some((start, end)) => {
                    let selected = buffer.text(&start, &end, false);
                    if selected.contains('\n') {
                        Self::remember_search_scope(&page);
                    } else {
                        super::replace::forget_search_scope(&page);
                        find.set_query_text(&selected);
                    }
                }
                None if opening => super::replace::forget_search_scope(&page),
                None => {}
            }
        }
        if opening {
            self.note_jump();
        }
        if mode == BarMode::Files && (entering || find.files_request().folder.is_empty()) {
            self.default_search_folder();
        }
        find.show(mode);
        if find.compiled().is_none() && find.pattern_error().is_none() {
            find.compile();
        }
        self.sync_search_highlight();
        self.update_match_count();
    }

    /// Closes the bar. The current match becomes the selection, so typing or copying acts on it.
    pub fn hide_find(&self) {
        self.inner.find.hide();
        if let Some(page) = self.current_page() {
            if let Some((start, end)) = page.current_match() {
                page.buffer().select_range(&end, &start);
            }
            super::replace::forget_search_scope(&page);
            page.view().grab_focus();
        }
        self.sync_search_highlight();
    }

    /// Hooks a newly created find context to the match count.
    fn context_for(&self, page: &EditorPage) -> sourceview5::SearchContext {
        let (context, created) = page.search().context(&page.buffer());
        if created {
            context.connect_occurrences_count_notify(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_| window.update_match_count()
            ));
            context.connect_regex_error_notify(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |context| {
                    if let Some(error) = context.regex_error() {
                        window
                            .inner
                            .find
                            .set_message(&error.to_string(), Tone::Error);
                    }
                }
            ));
        }
        context
    }

    /// Whether the bar shows every match in `page`: while it is open, except in the Mark row,
    /// where the marks show what Mark All found, and never in large-file mode.
    pub(in crate::window) fn shows_matches(&self, page: &EditorPage) -> bool {
        let find = &self.inner.find;
        find.is_open() && find.bar_mode() != BarMode::Mark && !page.large_file_mode()
    }

    /// Puts the query into the visible tab while the bar is open: GtkSourceView's highlight,
    /// or our matcher's matches. Other tabs show nothing and keep their settings, so they
    /// never rescan in the background.
    pub(in crate::window) fn sync_search_highlight(&self) {
        let find = &self.inner.find;
        let open = find.is_open();
        let current = self.current_page();
        let compiled = find.compiled();
        let wrap = find.options().wrap;
        // Per document: one in both views is searched once, in the current view.
        for page in self.documents() {
            let visible = open && Some(&page) == current.as_ref();
            let state = page.search();
            let buffer = page.buffer();
            if !visible {
                if let Some(context) = state.existing_context() {
                    context.set_highlight(false);
                }
                state.clear_own(&buffer);
                page.clear_current_match();
                continue;
            }
            match &compiled {
                Some(compiled) if !compiled.own() => {
                    state.clear_own(&buffer);
                    let context = self.context_for(&page);
                    state.apply(Some(compiled), wrap);
                    context.set_highlight(self.shows_matches(&page));
                }
                Some(compiled) => {
                    state.apply(Some(compiled), wrap);
                    if let Some(context) = state.existing_context() {
                        context.set_highlight(false);
                    }
                    self.refresh_own_matches(&page, compiled);
                }
                None => {
                    state.apply(None, wrap);
                    state.clear_own(&buffer);
                }
            }
        }
    }

    /// Searches the visible tab with our own matcher, on a snapshot on a worker, unless its
    /// matches for this query and revision are known.
    fn refresh_own_matches(&self, page: &EditorPage, compiled: &Rc<Compiled>) {
        let state = page.search();
        let revision = page.revision();
        if state.own_matches(compiled.id, revision).is_some() {
            state.tag_own(page.view(), revision, self.shows_matches(page));
            return;
        }
        if state.own_pending.get() == Some((compiled.id, revision)) {
            return;
        }
        if let Some(cancel) = state.own_cancel.take() {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let ticket = state.own_searches.next();
        state.own_pending.set(Some((compiled.id, revision)));
        let cancel = Arc::new(AtomicBool::new(false));
        state.own_cancel.replace(Some(Arc::clone(&cancel)));
        let text = page.text();
        let matcher = Arc::clone(&compiled.matcher);
        let id = compiled.id;
        let window = self.clone();
        let page = page.clone();
        self.spawn(async move {
            let found =
                worker::run(move || matcher.find_all(&text, None, OWN_MATCH_LIMIT, &cancel)).await;
            let state = page.search();
            if !ticket.is_current() {
                return;
            }
            state.own_pending.set(None);
            state.own_cancel.replace(None);
            if page.revision() != revision {
                return;
            }
            match found {
                Some(Ok(found)) => {
                    state.own.replace(Some(OwnMatches {
                        query: id,
                        revision,
                        matches: Arc::new(found.matches),
                        truncated: found.truncated,
                    }));
                    state.tag_own(page.view(), revision, window.shows_matches(&page));
                }
                Some(Err(error)) => window
                    .inner
                    .find
                    .set_message(&error.to_string(), Tone::Error),
                None => {}
            }
            window.update_match_count();
        });
    }

    /// A new tab's search signals: edits refresh our matcher's matches, caret moves end a
    /// current match, selection changes update smart highlighting, and scrolling moves our
    /// matcher's highlight along.
    pub(in crate::window) fn connect_page_search(&self, page: &EditorPage) {
        let buffer = page.buffer();
        buffer.connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            #[weak]
            page,
            move |_| window.on_buffer_changed(&page)
        ));
        buffer.connect_mark_set(glib::clone!(
            #[weak(rename_to = window)]
            self,
            #[weak]
            page,
            move |_, _, mark| {
                match mark.name().as_deref() {
                    Some("insert") => {
                        window.on_caret_moved(&page);
                        window.on_selection_changed(&page);
                    }
                    Some("selection_bound") => window.on_selection_changed(&page),
                    _ => {}
                }
            }
        ));
        if let Some(adjustment) = page.view().vadjustment() {
            adjustment.connect_value_changed(glib::clone!(
                #[weak(rename_to = window)]
                self,
                #[weak]
                page,
                move |_| window.on_editor_scrolled(&page)
            ));
        }
    }

    /// The buffer of `page` changed: our matcher searches again shortly after.
    pub(in crate::window) fn on_buffer_changed(&self, page: &EditorPage) {
        let find = &self.inner.find;
        if !find.is_open() || self.current_page().as_ref() != Some(page) {
            return;
        }
        let Some(compiled) = find.compiled().filter(|compiled| compiled.own()) else {
            return;
        };
        // A large document's snapshot costs the GTK thread up to about 0.1 s; wait longer.
        let delay = if page.large_file_mode() {
            OWN_REFRESH_DELAY_LARGE
        } else {
            OWN_REFRESH_DELAY
        };
        let window = self.clone();
        let page_ref = page.clone();
        page.search().own_refresh.schedule(delay, move || {
            let current = window.inner.find.compiled();
            if current
                .as_ref()
                .is_some_and(|current| current.id == compiled.id)
            {
                window.refresh_own_matches(&page_ref, &compiled);
            }
        });
    }

    /// The visible tab scrolled: our matcher's highlight follows the visible lines.
    pub(in crate::window) fn on_editor_scrolled(&self, page: &EditorPage) {
        if self.inner.find.is_open()
            && self.current_page().as_ref() == Some(page)
            && page.search().own.borrow().is_some()
        {
            page.search()
                .tag_own(page.view(), page.revision(), self.shows_matches(page));
        }
    }

    /// The caret moved: a current match it left no longer anchors the next search.
    pub(in crate::window) fn on_caret_moved(&self, page: &EditorPage) {
        let Some((start, end)) = page.current_match() else {
            return;
        };
        let buffer = page.buffer();
        let caret = buffer.iter_at_mark(&buffer.get_insert());
        if caret != start && caret != end {
            page.clear_current_match();
            self.update_match_count();
        }
    }

    /// The current match while the bar is open, else the selection, else the cursor.
    pub(in crate::window) fn search_anchor(
        &self,
        page: &EditorPage,
    ) -> (gtk::TextIter, gtk::TextIter) {
        let buffer = page.buffer();
        self.inner
            .find
            .is_open()
            .then(|| page.current_match())
            .flatten()
            .or_else(|| buffer.selection_bounds())
            .unwrap_or_else(|| {
                let cursor = buffer.iter_at_mark(&buffer.get_insert());
                (cursor, cursor)
            })
    }

    /// Find next (`forward`) or previous; Backward direction swaps them.
    pub fn find_step(&self, forward: bool) {
        let find = &self.inner.find;
        if find.compiled().is_none() {
            find.compile();
        }
        let Some(compiled) = find.compiled() else {
            self.show_find();
            return;
        };
        let Some(page) = self.current_page() else {
            return;
        };
        let forward = forward != find.options().backward;
        find.remember(Field::Find, &compiled.query.pattern);
        self.note_jump();
        let (start, end) = self.search_anchor(&page);
        let from = if forward { end } else { start };
        self.search_from(page, from.char_offset(), forward);
    }

    /// Incremental search while typing: the first match at or after the current one.
    fn find_from_selection_start(&self) {
        let Some(page) = self.current_page() else {
            return;
        };
        let (start, _) = self.search_anchor(&page);
        self.search_from(page, start.char_offset(), true);
    }

    /// Searches from character `from` in `page`, with GtkSourceView or our matcher.
    pub(in crate::window) fn search_from(&self, page: EditorPage, from: CharOffset, forward: bool) {
        let find = &self.inner.find;
        let Some(compiled) = find.compiled() else {
            return;
        };
        let wrap = find.options().wrap;
        let state = page.search();
        state.apply(Some(&compiled), wrap);
        let window = self.clone();
        if compiled.own() {
            let (snapshot, text) = PageSnapshot::take(&page);
            let matcher = Arc::clone(&compiled.matcher);
            let direction = if forward {
                Direction::Forward
            } else {
                Direction::Backward
            };
            let wrap_mode = if wrap { Wrap::Around } else { Wrap::Stop };
            self.spawn(async move {
                let found =
                    worker::run(move || matcher.find_next(&text, from, direction, wrap_mode)).await;
                if snapshot.edited() {
                    return;
                }
                match found {
                    Some(Ok(Some(next))) => window.show_match(
                        &page,
                        next.found.start,
                        next.found.end,
                        forward,
                        next.wrapped,
                    ),
                    Some(Ok(None)) => window.not_found(forward, wrap),
                    Some(Err(error)) => window
                        .inner
                        .find
                        .set_message(&error.to_string(), Tone::Error),
                    None => {}
                }
                window.update_match_count();
            });
            return;
        }
        let context = self.context_for(&page);
        context.set_highlight(self.shows_matches(&page));
        let from = page.buffer().iter_at_char(from);
        self.spawn(async move {
            match crate::sys::search::find(&context, &from, forward).await {
                Some((start, end, wrapped)) => {
                    window.show_match(
                        &page,
                        start.char_offset(),
                        end.char_offset(),
                        forward,
                        wrapped,
                    );
                }
                None => window.not_found(forward, wrap),
            }
            window.update_match_count();
        });
    }

    /// While the bar has focus the match is highlighted with the theme's current-match style
    /// (a selection would be drawn in the faint unfocused colour); otherwise it is selected.
    /// The caret goes to the far end of the match in the search direction.
    pub(in crate::window) fn show_match(
        &self,
        page: &EditorPage,
        start: CharOffset,
        end: CharOffset,
        forward: bool,
        wrapped: bool,
    ) {
        let buffer = page.buffer();
        let start = buffer.iter_at_char(start);
        let end = buffer.iter_at_char(end);
        let caret = if forward { &end } else { &start };
        if self.inner.find.is_open() {
            page.set_current_match(&start, &end);
            buffer.place_cursor(caret);
        } else if forward {
            buffer.select_range(&end, &start);
        } else {
            buffer.select_range(&start, &end);
        }
        page.view()
            .scroll_to_mark(&buffer.get_insert(), 0.1, false, 0.0, 0.0);
        let message = match (wrapped, forward) {
            (false, _) => "",
            (true, true) => "Passed the end of the document; continued from the start",
            (true, false) => "Passed the start of the document; continued from the end",
        };
        self.inner.find.set_message(message, Tone::Info);
    }

    fn not_found(&self, forward: bool, wrap: bool) {
        let message = match (wrap, forward) {
            (true, _) => "No match",
            (false, true) => "No match from the caret to the end",
            (false, false) => "No match from the start to the caret",
        };
        self.inner.find.set_message(message, Tone::Info);
    }

    pub(in crate::window) fn update_match_count(&self) {
        let find = &self.inner.find;
        let pattern = find.query_text();
        let label = match (self.current_page(), find.compiled()) {
            _ if pattern.is_empty() => String::new(),
            (_, None) => "Invalid pattern".to_owned(),
            (None, _) => String::new(),
            (Some(page), Some(compiled)) if compiled.own() => {
                match page.search().own_matches(compiled.id, page.revision()) {
                    None => "Searching…".to_owned(),
                    Some((matches, truncated)) => {
                        let count = matches.len();
                        let (start, end) = self.search_anchor(&page);
                        let position = page
                            .search()
                            .own_position(start.char_offset(), end.char_offset());
                        let more = if truncated { "+" } else { "" };
                        match (count, position) {
                            (0, _) => "No matches".to_owned(),
                            (count, Some(position)) => format!("{position} of {count}{more}"),
                            (1, None) => "1 match".to_owned(),
                            (count, None) => format!("{count}{more} matches"),
                        }
                    }
                }
            }
            (Some(page), Some(_)) => {
                let context = self.context_for(&page);
                let count = context.occurrences_count();
                let (start, end) = self.search_anchor(&page);
                let position = context.occurrence_position(&start, &end);
                match count {
                    -1 => "Searching…".to_owned(),
                    0 => "No matches".to_owned(),
                    count if position > 0 => format!("{position} of {count}"),
                    1 => "1 match".to_owned(),
                    count => format!("{count} matches"),
                }
            }
        };
        find.set_count(&label);
        if let Some(page) = self.current_page() {
            page.keep_current_match_on_top();
        }
    }

    /// The histories, for the session to save: find and replace map to
    /// `session.json`'s `find_history` and `replace_history`.
    pub fn search_history(&self) -> SearchHistory {
        self.inner.find.history()
    }

    /// Restores the histories, for example from the session.
    pub fn set_search_history(&self, history: SearchHistory) {
        self.inner.find.set_history(history);
    }
}
