//! Count, Replace, Replace All (in the current or every open document) and Find All, and the
//! results panel's commands. Matching and template expansion run in our own engine on a
//! snapshot, on a worker; replacements are applied on the GTK thread in one user action per
//! document, as one bulk edit above 2,000 edits (ADR-003 and ADR-005 amendments).

use super::Window;
use super::find::{BarMode, Field, Tone};
use super::results::{FileNode, FileTarget, Jump};
use crate::editor::{EditorPage, read_only_message};
use crate::edits;
use crate::page_search::Compiled;
use crate::worker;
use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, glib};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::ops::Range;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};
use stet_domain::actions::ActionId;
use stet_domain::document::tilde;
use stet_domain::location::LineCol;
use stet_domain::search::report::{
    count_message, replace_in_documents_message, replace_message, results_heading,
};
use stet_domain::search::{ScopeKind, SearchOptions};
use stet_infrastructure::find_in_files::DEFAULT_MAX_HITS;
use stet_infrastructure::search::{Match, SearchError, document_hits};

const SCOPE_START_MARK: &str = "stet-scope-start";
const SCOPE_END_MARK: &str = "stet-scope-end";
/// How long a jump waits for a file it opened to finish loading.
const LOAD_WAIT: Duration = Duration::from_secs(10);
/// The results panel's height when it first opens.
pub const RESULTS_HEIGHT: i32 = 220;

/// What the last Replace All cost, for the self-test's performance report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplaceStats {
    pub count: usize,
    pub edits: usize,
    pub bulk: bool,
    /// Taking the snapshot, on the GTK thread.
    pub snapshot: Duration,
    /// Matching and expanding, on a worker.
    pub search: Duration,
    /// Changing the buffer, on the GTK thread.
    pub apply: Duration,
    /// From the command to the applied result.
    pub total: Duration,
}

impl Window {
    /// The find bar's commands and the results panel's.
    pub fn run_search_action(&self, id: ActionId) {
        match id {
            ActionId::Replace => self.show_find_mode(BarMode::Replace),
            ActionId::FindInFiles => self.show_find_mode(BarMode::Files),
            ActionId::ReplaceNext => self.replace_next(true),
            ActionId::ReplaceAll => self.replace_all(),
            ActionId::ReplaceAllInOpenDocuments => {
                let window = self.clone();
                self.spawn(async move { window.replace_all_in_open_documents().await });
            }
            ActionId::Count => self.count(),
            ActionId::FindAllInDocument => self.find_all(false),
            ActionId::FindAllInOpenDocuments => self.find_all(true),
            ActionId::StopFindInFiles => {
                self.stop_find_in_files();
                self.stop_replace_in_files();
            }
            ActionId::ReplaceInFiles => self.replace_in_files(),
            ActionId::SearchResults => self.toggle_results_focus(),
            ActionId::NextSearchResult => self.step_result(true),
            ActionId::PreviousSearchResult => self.step_result(false),
            ActionId::CopySearchResults => self.copy_results(),
            ActionId::ClearSearchResults => self.clear_results(),
            ActionId::CloseSearchResults => self.hide_results(),
            _ => {}
        }
    }

    /// The query for a command, remembered in the history; `None` (with the bar showing why)
    /// when the find field is empty or invalid.
    pub(super) fn query_for_command(&self) -> Option<Rc<Compiled>> {
        let find = &self.inner.find;
        if find.compiled().is_none() {
            find.compile();
        }
        match find.compiled() {
            Some(compiled) => {
                find.remember(Field::Find, &compiled.query.pattern);
                Some(compiled)
            }
            None => {
                if !find.is_open() {
                    self.show_find();
                }
                if find.pattern_error().is_none() {
                    find.set_message("Type what to search for", Tone::Info);
                }
                None
            }
        }
    }

    /// Replacing needs the replace row on screen; otherwise the command opens it.
    fn replace_row_shown(&self) -> bool {
        let find = &self.inner.find;
        if find.is_open() && find.bar_mode() == BarMode::Replace {
            return true;
        }
        self.show_find_mode(BarMode::Replace);
        false
    }

    /// Whether `page` can be changed now; the bar says why not.
    pub(super) fn editable_page(&self, page: &EditorPage) -> bool {
        if page.is_loading() || page.is_busy() {
            self.inner
                .find
                .set_message("The document is still loading", Tone::Error);
            return false;
        }
        if let Some(reason) = page.read_only() {
            self.inner
                .find
                .set_message(read_only_message(reason), Tone::Error);
            return false;
        }
        true
    }

    /// Remembers a selection the bar opened with, so that In selection still has it after the
    /// incremental search moves the caret.
    pub(super) fn remember_search_scope(&self, page: &EditorPage) {
        let buffer = page.buffer();
        match buffer.selection_bounds() {
            Some((start, end)) => {
                buffer.create_mark(Some(SCOPE_START_MARK), &start, true);
                buffer.create_mark(Some(SCOPE_END_MARK), &end, false);
            }
            None => forget_search_scope(page),
        }
    }

    /// Where Count and Replace All work (the scope table, [`SearchOptions::scope`]), and
    /// how to describe it. In selection uses the selection, or the one the bar opened with.
    pub(super) fn command_scope(
        &self,
        page: &EditorPage,
        options: &SearchOptions,
    ) -> (Range<usize>, ScopeKind) {
        let buffer = page.buffer();
        let caret = buffer.iter_at_mark(&buffer.get_insert()).offset() as usize;
        let live = buffer
            .selection_bounds()
            .map(|(start, end)| start.offset() as usize..end.offset() as usize)
            .filter(|range| range.start < range.end);
        let remembered = || {
            let start = buffer.mark(SCOPE_START_MARK)?;
            let end = buffer.mark(SCOPE_END_MARK)?;
            let range = buffer.iter_at_mark(&start).offset() as usize
                ..buffer.iter_at_mark(&end).offset() as usize;
            (range.start < range.end).then_some(range)
        };
        let selection = match live {
            Some(range) => Some(range),
            None if options.in_selection => remembered(),
            None => None,
        };
        let len = buffer.char_count() as usize;
        (
            options.scope(caret, selection.clone(), len),
            options.scope_kind(selection.is_some()),
        )
    }

    /// Count: the matches in the scope, reported in the bar.
    fn count(&self) {
        let Some(compiled) = self.query_for_command() else {
            return;
        };
        let Some(page) = self.current_page() else {
            return;
        };
        let find = &self.inner.find;
        if !find.is_open() {
            self.show_find();
        }
        let options = find.options();
        let (range, scope) = self.command_scope(&page, &options);
        let text = page.text();
        let matcher = Arc::clone(&compiled.matcher);
        find.set_message("Counting…", Tone::Info);
        let window = self.clone();
        self.spawn(async move {
            let cancel = AtomicBool::new(false);
            let counted = worker::run(move || {
                matcher
                    .find_all(&text, Some(range), usize::MAX, &cancel)
                    .map(|found| found.matches.len())
            })
            .await;
            let find = &window.inner.find;
            match counted {
                Some(Ok(count)) => find.set_message(&count_message(count, scope), Tone::Info),
                Some(Err(error)) => find.set_message(&error.to_string(), Tone::Error),
                None => find.set_message("Counting failed", Tone::Error),
            }
        });
    }

    /// Replace: if the current match (or the selection) is a match, replace it and find the
    /// next; otherwise just find the next.
    pub(super) fn replace_next(&self, forward: bool) {
        if !self.replace_row_shown() {
            return;
        }
        let Some(compiled) = self.query_for_command() else {
            return;
        };
        let template = match self.template() {
            Ok(template) => template,
            Err(message) => {
                self.inner.find.set_message(&message, Tone::Error);
                return;
            }
        };
        let find = &self.inner.find;
        find.remember(Field::Replace, &find.replace_entry.text());
        let Some(page) = self.current_page() else {
            return;
        };
        if !self.editable_page(&page) {
            return;
        }
        let forward = forward != find.backward.is_active();
        let has_match = page.current_match().is_some();
        let (start, end) = self.search_anchor(&page);
        if start == end && !has_match {
            self.search_from(page, start.offset(), forward);
            return;
        }
        let text = page.text();
        let revision = page.revision();
        let found = Match {
            start: start.offset() as usize,
            end: end.offset() as usize,
            line: start.line() as usize,
        };
        let matcher = Arc::clone(&compiled.matcher);
        let window = self.clone();
        self.spawn(async move {
            let edit = worker::run(move || matcher.replace_one(&text, &found, &template)).await;
            if page.revision() != revision || window.tab_page(&page).is_none() {
                return;
            }
            match edit {
                Some(Ok(edit)) => {
                    page.clear_current_match();
                    edits::apply_edits(&page.buffer(), std::slice::from_ref(&edit));
                    let after = edit.start + edit.insert.chars().count();
                    let from = if forward { after } else { edit.start };
                    window.search_from(page, from as i32, forward);
                }
                Some(Err(SearchError::NotAMatch)) => {
                    window.search_from(page, found.start as i32, forward);
                }
                Some(Err(error)) => window
                    .inner
                    .find
                    .set_message(&error.to_string(), Tone::Error),
                None => {}
            }
        });
    }

    /// Replace All in the scope of the current document, as one undo step.
    fn replace_all(&self) {
        if !self.replace_row_shown() {
            return;
        }
        let Some(compiled) = self.query_for_command() else {
            return;
        };
        let template = match self.template() {
            Ok(template) => template,
            Err(message) => {
                self.inner.find.set_message(&message, Tone::Error);
                return;
            }
        };
        let find = &self.inner.find;
        find.remember(Field::Replace, &find.replace_entry.text());
        let Some(page) = self.current_page() else {
            return;
        };
        if !self.editable_page(&page) {
            return;
        }
        let started = Instant::now();
        let options = find.options();
        let (range, scope) = self.command_scope(&page, &options);
        let text = page.text();
        let snapshot = started.elapsed();
        let revision = page.revision();
        let matcher = Arc::clone(&compiled.matcher);
        find.set_message("Replacing…", Tone::Info);
        find.stats.replace(None);
        let window = self.clone();
        self.spawn(async move {
            let searching = Instant::now();
            let replaced = worker::run(move || {
                let cancel = AtomicBool::new(false);
                let result = matcher.replace_all(&text, &template, Some(range), &cancel);
                (text, result)
            })
            .await;
            let search = searching.elapsed();
            let find = &window.inner.find;
            let Some((text, result)) = replaced else {
                find.set_message("Replace All failed", Tone::Error);
                return;
            };
            if window.tab_page(&page).is_none() {
                find.set_message("The document was closed; nothing was replaced", Tone::Info);
                return;
            }
            if page.revision() != revision {
                find.set_message(
                    "The document changed while replacing; nothing was replaced",
                    Tone::Error,
                );
                return;
            }
            match result {
                Ok(result) => {
                    page.clear_current_match();
                    let applied = apply_tracked(&page, &result, &text);
                    drop(text);
                    let stats = ReplaceStats {
                        count: result.count,
                        edits: applied.edits,
                        bulk: applied.bulk,
                        snapshot,
                        search,
                        apply: applied.took,
                        total: started.elapsed(),
                    };
                    tracing::info!(
                        count = stats.count,
                        edits = stats.edits,
                        bulk = stats.bulk,
                        snapshot_ms = stats.snapshot.as_secs_f64() * 1e3,
                        search_ms = stats.search.as_secs_f64() * 1e3,
                        apply_ms = stats.apply.as_secs_f64() * 1e3,
                        total_ms = stats.total.as_secs_f64() * 1e3,
                        "replace all"
                    );
                    find.stats.replace(Some(stats));
                    find.set_message(&replace_message(result.count, scope), Tone::Info);
                }
                Err(error) => find.set_message(&error.to_string(), Tone::Error),
            }
            window.update_match_count();
        });
    }

    /// Replace All in every open document that can be changed, after a confirmation; each
    /// document gets its own undo step.
    async fn replace_all_in_open_documents(&self) {
        if !self.replace_row_shown() {
            return;
        }
        let Some(compiled) = self.query_for_command() else {
            return;
        };
        let template = match self.template() {
            Ok(template) => template,
            Err(message) => {
                self.inner.find.set_message(&message, Tone::Error);
                return;
            }
        };
        let find = &self.inner.find;
        find.remember(Field::Replace, &find.replace_entry.text());
        // Restored tabs that were not shown yet load their text first (M2).
        self.load_stubs().await;
        let pages: Vec<EditorPage> = self
            .documents()
            .into_iter()
            .filter(|page| !page.is_loading() && page.read_only().is_none())
            .collect();
        if pages.is_empty() {
            find.set_message("No open document can be changed", Tone::Error);
            return;
        }
        if !self
            .ask_replace_in_documents(&compiled.query.pattern, pages.len())
            .await
        {
            return;
        }
        find.set_message("Replacing…", Tone::Info);
        let (mut total, mut documents, mut changed_meanwhile) = (0, 0, 0);
        let mut failed: Vec<String> = Vec::new();
        for page in pages {
            let text = page.text();
            let revision = page.revision();
            let matcher = Arc::clone(&compiled.matcher);
            let template = template.clone();
            let replaced = worker::run(move || {
                let cancel = AtomicBool::new(false);
                let result = matcher.replace_all(&text, &template, None, &cancel);
                (text, result)
            })
            .await;
            let (text, result) = match replaced {
                Some((text, Ok(result))) => (text, result),
                Some((_, Err(error))) => {
                    failed.push(format!("{}: {error}", page.name()));
                    continue;
                }
                None => {
                    failed.push(page.name());
                    continue;
                }
            };
            if self.tab_page(&page).is_none() {
                continue;
            }
            if page.revision() != revision {
                changed_meanwhile += 1;
                continue;
            }
            if result.count > 0 {
                page.clear_current_match();
                apply_tracked(&page, &result, &text);
                total += result.count;
                documents += 1;
            }
        }
        let mut message = replace_in_documents_message(total, documents);
        if changed_meanwhile > 0 {
            message.push_str(&format!(
                "; {changed_meanwhile} changed while replacing and were left alone"
            ));
        }
        if !failed.is_empty() {
            message.push_str(&format!("; not replaced in {}", failed.join(", ")));
        }
        let tone = if failed.is_empty() {
            Tone::Info
        } else {
            Tone::Error
        };
        find.set_message(&message, tone);
        self.update_match_count();
    }

    async fn ask_replace_in_documents(&self, pattern: &str, documents: usize) -> bool {
        let heading = if documents == 1 {
            "Replace in 1 open document?".to_owned()
        } else {
            format!("Replace in {documents} open documents?")
        };
        let body = format!(
            "Every match of “{pattern}” is replaced. Each document can be undone on its own."
        );
        let dialog = adw::AlertDialog::new(Some(&heading), Some(&body));
        dialog.add_responses(&[("cancel", "_Cancel"), ("replace", "_Replace All")]);
        dialog.set_response_appearance("replace", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("replace"));
        dialog.set_close_response("cancel");
        self.ask(&dialog).await == "replace"
    }

    /// Find All in the current document or in every open document: the results panel lists
    /// the matching lines.
    fn find_all(&self, all_documents: bool) {
        let Some(compiled) = self.query_for_command() else {
            return;
        };
        // Restored tabs that were not shown yet load their text first (M2).
        if all_documents && self.has_stubs() {
            let window = self.clone();
            self.spawn(async move {
                window.load_stubs().await;
                window.find_all(true);
            });
            return;
        }
        let pages: Vec<EditorPage> = if all_documents {
            self.documents()
                .into_iter()
                .filter(|page| !page.is_loading())
                .collect()
        } else {
            self.current_page().into_iter().collect()
        };
        let home = std::env::home_dir();
        let mut documents: Vec<(EditorPage, String)> = Vec::with_capacity(pages.len());
        let mut texts: Vec<String> = Vec::with_capacity(pages.len());
        for page in pages {
            let label = page
                .path()
                .map_or_else(|| page.name(), |path| tilde(&path, home.as_deref()));
            texts.push(page.text());
            documents.push((page, label));
        }
        let searched = documents.len();
        let matcher = Arc::clone(&compiled.matcher);
        let pattern = compiled.query.pattern.clone();
        self.inner.find.set_message("Searching…", Tone::Info);
        let window = self.clone();
        self.spawn(async move {
            let found = worker::run(move || {
                let cancel = AtomicBool::new(false);
                texts
                    .iter()
                    .map(|text| {
                        matcher
                            .find_all(text, None, DEFAULT_MAX_HITS, &cancel)
                            .map(|found| (document_hits(text, &found.matches), found.truncated))
                    })
                    .collect::<Vec<_>>()
            })
            .await;
            let find = &window.inner.find;
            let Some(found) = found else {
                find.set_message("Find All failed", Tone::Error);
                return;
            };
            let mut files = Vec::new();
            let mut truncated = false;
            for ((page, label), result) in documents.into_iter().zip(found) {
                match result {
                    Ok((hits, cut)) => {
                        truncated |= cut;
                        if !hits.is_empty() {
                            files.push(FileNode {
                                label,
                                target: FileTarget::Page {
                                    page: page.downgrade(),
                                    path: page.path(),
                                },
                                hits,
                                summary: None,
                            });
                        }
                    }
                    Err(error) => {
                        find.set_message(&error.to_string(), Tone::Error);
                        return;
                    }
                }
            }
            let results = &window.inner.results;
            let search = results.begin(String::new());
            let file_count = files.len();
            results.add_files(&search, files);
            let heading = results_heading(
                &pattern,
                search.hits(),
                file_count,
                all_documents.then_some(searched),
            );
            results.set_heading(&search, heading.clone());
            let status = if truncated {
                format!("{heading}; stopped at {DEFAULT_MAX_HITS} hits in a document")
            } else {
                heading.clone()
            };
            results.status.set_label(&status);
            find.set_message(&heading, Tone::Info);
            window.show_results(false);
        });
    }

    // ----- the results panel -----------------------------------------------------------------

    pub(super) fn connect_results_panel(&self) {
        let results = &self.inner.results;
        results.connect_jump(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |jump| Window { inner }.jump_to(jump)
        ));
        results.keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, state| Window { inner }.on_results_key(key, state)
        ));
        for (button, id) in [
            (&results.stop, ActionId::StopFindInFiles),
            (&results.copy, ActionId::CopySearchResults),
            (&results.clear, ActionId::ClearSearchResults),
            (&results.close, ActionId::CloseSearchResults),
        ] {
            button.connect_clicked(glib::clone!(
                #[weak(rename_to = inner)]
                self.inner,
                move |_| Window { inner }.run_search_action(id)
            ));
        }
        let appearance = &self.inner.shared.appearance;
        appearance.connect_scheme_changed(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |scheme| inner.results.set_match_color(search_match_color(scheme))
        ));
        if let Some(scheme) = appearance.scheme() {
            results.set_match_color(search_match_color(&scheme));
        }
        if let Some(action) = self.action(ActionId::StopFindInFiles) {
            action.set_enabled(false);
        }
    }

    /// Shows the results panel at its last height, optionally with the focus in it.
    pub fn show_results(&self, focus: bool) {
        let results = &self.inner.results;
        if !results.root.is_visible() {
            results.root.set_visible(true);
            let paned = self.inner.paned.clone();
            let height = self.inner.results_height.get();
            let place = move || {
                let total = paned.height();
                if total > height {
                    paned.set_position(total - height);
                }
            };
            if self.inner.paned.height() > 0 {
                place();
            } else {
                glib::idle_add_local_once(place);
            }
        }
        if focus {
            let position = results.list.model().map_or(0, |model| model.n_items());
            if position > 0 {
                results.list.grab_focus();
            } else {
                results.close.grab_focus();
            }
        }
    }

    pub fn hide_results(&self) {
        let results = &self.inner.results;
        if results.root.is_visible() {
            let paned = &self.inner.paned;
            let height = paned.height() - paned.position();
            if height > 40 {
                self.inner.results_height.set(height);
            }
            results.root.set_visible(false);
        }
        if let Some(page) = self.current_page() {
            page.view().grab_focus();
        }
    }

    /// Whether the focus is inside the results panel.
    pub fn results_focused(&self) -> bool {
        gtk::prelude::RootExt::focus(&self.inner.window)
            .is_some_and(|focus| focus.is_ancestor(&self.inner.results.root))
    }

    /// Search Results Window (F7, Ctrl+Alt+R): into the panel, or back to the editor.
    fn toggle_results_focus(&self) {
        if self.results_focused() {
            if let Some(page) = self.current_page() {
                page.view().grab_focus();
            }
        } else {
            self.show_results(true);
        }
    }

    /// Next or previous search result (F4, Shift+F4).
    fn step_result(&self, forward: bool) {
        match self.inner.results.step(forward) {
            Some(jump) => {
                self.show_results(false);
                self.jump_to(jump);
            }
            None => self.toast("No search results"),
        }
    }

    fn copy_results(&self) {
        let text = self.inner.results.text();
        if text.is_empty() {
            self.toast("No search results to copy");
            return;
        }
        self.inner.window.clipboard().set_text(&text);
        self.toast("Copied the search results");
    }

    fn clear_results(&self) {
        let results = &self.inner.results;
        results.clear();
        results.status.set_label("");
    }

    /// Goes to a result: selects its tab (opening the file if needed) and the match.
    pub(super) fn jump_to(&self, jump: Jump) {
        self.note_jump();
        let open_page = match &jump.target {
            FileTarget::Page { page, path } => page
                .upgrade()
                .filter(|page| self.tab_page(page).is_some())
                .or_else(|| self.page_showing(path.as_ref()?)),
            FileTarget::Path(path) => self.page_showing(path),
        };
        if let Some(page) = open_page {
            self.select_result(&page, &jump);
            return;
        }
        let path = match &jump.target {
            FileTarget::Path(path) => path.clone(),
            FileTarget::Page {
                path: Some(path), ..
            } => path.clone(),
            FileTarget::Page { path: None, .. } => {
                self.toast("That document is closed");
                return;
            }
        };
        self.open_locations(vec![super::Location {
            path: path.clone(),
            position: Some(LineCol::new(
                u32::try_from(jump.line).unwrap_or(u32::MAX),
                Some(jump.column as u32 + 1),
            )),
            create: false,
        }]);
        let window = self.clone();
        self.spawn(async move {
            let started = Instant::now();
            while started.elapsed() < LOAD_WAIT {
                if let Some(page) = window.page_showing(&path) {
                    if !page.is_loading() {
                        window.select_result(&page, &jump);
                        return;
                    }
                } else if started.elapsed() > Duration::from_millis(500) {
                    return;
                }
                glib::timeout_future(Duration::from_millis(20)).await;
            }
        });
    }

    fn page_showing(&self, path: &Path) -> Option<EditorPage> {
        self.documents()
            .into_iter()
            .find(|page| page.shows(path, None))
    }

    fn select_result(&self, page: &EditorPage, jump: &Jump) {
        self.select(page);
        let buffer = page.buffer();
        let line = i32::try_from(jump.line.saturating_sub(1)).unwrap_or(i32::MAX);
        let line = line.min((buffer.line_count() - 1).max(0));
        let Some(mut start) = buffer.iter_at_line(line) else {
            return;
        };
        for _ in 0..jump.column {
            if start.ends_line() {
                break;
            }
            start.forward_char();
        }
        let mut end = start;
        for _ in 0..jump.length {
            if end.ends_line() {
                break;
            }
            end.forward_char();
        }
        page.clear_current_match();
        buffer.select_range(&end, &start);
        page.view()
            .scroll_to_mark(&buffer.get_insert(), 0.1, true, 0.0, 0.3);
        page.view().grab_focus();
        self.update_match_count();
    }

    /// Keys inside the results panel: Escape closes it, Ctrl+C copies the selected rows.
    pub(super) fn on_results_key(
        &self,
        key: gdk::Key,
        state: gdk::ModifierType,
    ) -> glib::Propagation {
        let control = state.contains(gdk::ModifierType::CONTROL_MASK);
        match key {
            gdk::Key::Escape => {
                self.hide_results();
                glib::Propagation::Stop
            }
            gdk::Key::c | gdk::Key::C | gdk::Key::Insert if control => {
                if let Some(text) = self.inner.results.selected_text() {
                    self.inner.window.clipboard().set_text(&text);
                }
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    }
}

/// The background of the scheme's `search-match` style, which emphasizes matches.
fn search_match_color(scheme: &sourceview5::StyleScheme) -> Option<gdk::RGBA> {
    let style = scheme.style("search-match")?;
    gdk::RGBA::parse(style.background()?.as_str()).ok()
}

/// Drops the selection the bar opened with.
/// Applies a Replace All computed on `text`, the document's text, through the document's
/// bookmark and mark tracking (M7), so bookmarks come back with Undo and Redo.
pub(super) fn apply_tracked(
    page: &EditorPage,
    result: &stet_infrastructure::search::ReplaceAll,
    text: &str,
) -> edits::Applied {
    let buffer = page.buffer();
    let mut applied = None;
    page.track_edit(text, &result.edits, None, || {
        let done = edits::apply_replace_all(&buffer, result, text);
        applied = Some(done);
        done.footprint
    });
    applied.expect("the edit ran")
}

pub(super) fn forget_search_scope(page: &EditorPage) {
    let buffer = page.buffer();
    for name in [SCOPE_START_MARK, SCOPE_END_MARK] {
        if let Some(mark) = buffer.mark(name) {
            buffer.delete_mark(&mark);
        }
    }
}
