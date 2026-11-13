//! Count, Replace, Replace All (in the current or every open document) and Find All. Matching and template expansion run in our own engine on a
//! snapshot, on a worker; replacements are applied on the GTK thread in one user action per
//! document, as one bulk edit above 2,000 edits (ADR-003 and ADR-005 amendments).

use super::bar::{BarMode, Field, Tone};
use super::results::{FileNode, FileTarget};
use crate::convert::{BufferPositions, IterPosition};
use crate::editor::EditorPage;
use crate::editor::edits;
use crate::editor::search::Compiled;
use crate::runtime::worker;
use crate::window::Window;
use crate::window::snapshot::{PageSnapshot, Stale};
use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
#[cfg(feature = "selftest")]
use std::cell::Cell;
use std::fmt::Write as _;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};
use stet_domain::document::tilde;
use stet_domain::search::SearchOptions;
use stet_domain::search::report::ScopeKind;
use stet_domain::search::report::{
    count_message, replace_in_documents_message, replace_message, results_heading,
};
use stet_domain::text::{CharOffset, counted};
use stet_infrastructure::files::find::DEFAULT_MAX_HITS;
use stet_infrastructure::search::{Match, SearchError, document_hits};

const SCOPE_START_MARK: &str = "stet-scope-start";
const SCOPE_END_MARK: &str = "stet-scope-end";

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

/// What Replace All keeps for the self-test: the last one's cost.
#[cfg(feature = "selftest")]
#[derive(Debug, Default)]
pub struct ReplaceState {
    stats: Cell<Option<ReplaceStats>>,
}

impl Window {
    /// The query for a command, remembered in the history; `None` (with the bar showing why)
    /// when the find field is empty or invalid.
    pub(in crate::window) fn query_for_command(&self) -> Option<Rc<Compiled>> {
        let find = &self.inner.find;
        let compiled = find.query();
        match &compiled {
            Some(compiled) => find.remember(Field::Find, &compiled.query.pattern),
            None if !find.is_open() => self.show_find(),
            None => {}
        }
        compiled
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
    pub(in crate::window) fn editable_page(&self, page: &EditorPage) -> bool {
        if page.is_loading() || page.is_busy() {
            self.inner
                .find
                .set_message("The document is still loading", Tone::Error);
            return false;
        }
        if let Some(reason) = page.read_only() {
            self.inner.find.set_message(reason.message(), Tone::Error);
            return false;
        }
        true
    }

    /// Remembers a selection the bar opened with, so that In selection still has it after the
    /// incremental search moves the caret.
    pub(in crate::window) fn remember_search_scope(page: &EditorPage) {
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
    pub(in crate::window) fn command_scope(
        page: &EditorPage,
        options: SearchOptions,
    ) -> (Range<CharOffset>, ScopeKind) {
        let buffer = page.buffer();
        let caret = buffer.iter_at_mark(&buffer.get_insert()).char_offset();
        let live = buffer
            .selection_bounds()
            .map(|(start, end)| start.char_offset()..end.char_offset())
            .filter(|range| range.start < range.end);
        let remembered = || {
            let start = buffer.mark(SCOPE_START_MARK)?;
            let end = buffer.mark(SCOPE_END_MARK)?;
            let range =
                buffer.iter_at_mark(&start).char_offset()..buffer.iter_at_mark(&end).char_offset();
            (range.start < range.end).then_some(range)
        };
        let selection = match live {
            Some(range) => Some(range),
            None if options.in_selection => remembered(),
            None => None,
        };
        (
            options.scope(caret, selection.clone(), buffer.end_char()),
            options.scope_kind(selection.is_some()),
        )
    }

    /// Count: the matches in the scope, reported in the bar.
    pub(super) fn count(&self) {
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
        let (range, scope) = Self::command_scope(&page, options);
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
    pub(in crate::window) fn replace_next(&self, forward: bool) {
        if !self.replace_row_shown() {
            return;
        }
        let Some(compiled) = self.query_for_command() else {
            return;
        };
        let find = &self.inner.find;
        let template = match find.replace_template() {
            Ok(template) => template,
            Err(error) => {
                find.set_message(&error.to_string(), Tone::Error);
                return;
            }
        };
        find.remember(Field::Replace, &find.replace_text());
        let Some(page) = self.current_page() else {
            return;
        };
        if !self.editable_page(&page) {
            return;
        }
        let forward = forward != find.options().backward;
        let has_match = page.current_match().is_some();
        let (start, end) = self.search_anchor(&page);
        if start == end && !has_match {
            self.search_from(page, start.char_offset(), forward);
            return;
        }
        let (snapshot, text) = PageSnapshot::take(&page);
        let found = Match {
            start: start.char_offset(),
            end: end.char_offset(),
            line: start.buffer_line(),
        };
        let matcher = Arc::clone(&compiled.matcher);
        let window = self.clone();
        self.spawn(async move {
            let edit = window
                .on_snapshot(&snapshot, move || {
                    matcher.replace_one(&text, &found, &template)
                })
                .await;
            let Ok(edit) = edit else {
                return;
            };
            match edit {
                Ok(edit) => {
                    page.clear_current_match();
                    edits::apply_edits(&page.buffer(), std::slice::from_ref(&edit));
                    let after = edit.start() + edit.insert.chars().count();
                    let from = if forward { after } else { edit.start() };
                    window.search_from(page, from, forward);
                }
                Err(SearchError::NotAMatch) => {
                    window.search_from(page, found.start, forward);
                }
                Err(error) => window
                    .inner
                    .find
                    .set_message(&error.to_string(), Tone::Error),
            }
        });
    }

    /// Replace All in the scope of the current document, as one undo step.
    pub(super) fn replace_all(&self) {
        if !self.replace_row_shown() {
            return;
        }
        let Some(compiled) = self.query_for_command() else {
            return;
        };
        let find = &self.inner.find;
        let template = match find.replace_template() {
            Ok(template) => template,
            Err(error) => {
                find.set_message(&error.to_string(), Tone::Error);
                return;
            }
        };
        find.remember(Field::Replace, &find.replace_text());
        let Some(page) = self.current_page() else {
            return;
        };
        if !self.editable_page(&page) {
            return;
        }
        let started = Instant::now();
        let options = find.options();
        let (range, scope) = Self::command_scope(&page, options);
        let (taken, text) = PageSnapshot::take(&page);
        let snapshot = started.elapsed();
        let matcher = Arc::clone(&compiled.matcher);
        find.set_message("Replacing…", Tone::Info);
        #[cfg(feature = "selftest")]
        self.inner.replace.stats.set(None);
        let window = self.clone();
        self.spawn(async move {
            let searching = Instant::now();
            let replaced = window
                .on_snapshot(&taken, move || {
                    let cancel = AtomicBool::new(false);
                    let result = matcher.replace_all(&text, &template, Some(range), &cancel);
                    (text, result)
                })
                .await;
            let search = searching.elapsed();
            let find = &window.inner.find;
            let (text, result) = match replaced {
                Ok(replaced) => replaced,
                Err(Stale::Panicked) => {
                    find.set_message("Replace All failed", Tone::Error);
                    return;
                }
                Err(Stale::Closed) => {
                    find.set_message("The document was closed; nothing was replaced", Tone::Info);
                    return;
                }
                Err(Stale::Edited) => {
                    find.set_message(
                        "The document changed while replacing; nothing was replaced",
                        Tone::Error,
                    );
                    return;
                }
            };
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
                    #[cfg(feature = "selftest")]
                    window.inner.replace.stats.set(Some(stats));
                    find.set_message(&replace_message(result.count, scope), Tone::Info);
                }
                Err(error) => find.set_message(&error.to_string(), Tone::Error),
            }
            window.update_match_count();
        });
    }

    /// Replace All in every open document that can be changed, after a confirmation; each
    /// document gets its own undo step.
    pub(super) async fn replace_all_in_open_documents(&self) {
        if !self.replace_row_shown() {
            return;
        }
        let Some(compiled) = self.query_for_command() else {
            return;
        };
        let find = &self.inner.find;
        let template = match find.replace_template() {
            Ok(template) => template,
            Err(error) => {
                find.set_message(&error.to_string(), Tone::Error);
                return;
            }
        };
        find.remember(Field::Replace, &find.replace_text());
        // Restored tabs that were not shown yet load their text first.
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
            let (snapshot, text) = PageSnapshot::take(&page);
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
            match self.check_snapshot(&snapshot) {
                Ok(()) => {}
                Err(Stale::Closed) => continue,
                Err(_) => {
                    changed_meanwhile += 1;
                    continue;
                }
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
            let _ = write!(
                message,
                "; {changed_meanwhile} changed while replacing and were left alone"
            );
        }
        if !failed.is_empty() {
            let _ = write!(message, "; not replaced in {}", failed.join(", "));
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
        let heading = format!(
            "Replace in {}?",
            counted(documents, "open document", "open documents")
        );
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
    pub(super) fn find_all(&self, all_documents: bool) {
        let Some(compiled) = self.query_for_command() else {
            return;
        };
        // Restored tabs that were not shown yet load their text first.
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
            results.set_status_text(&status);
            find.set_message(&heading, Tone::Info);
            window.show_results(false);
        });
    }
}

/// Applies a Replace All computed on `text`, the document's text, through the document's
/// bookmark and mark tracking, so bookmarks come back with Undo and Redo.
pub(in crate::window) fn apply_tracked(
    page: &EditorPage,
    result: &stet_infrastructure::search::ReplaceAll,
    text: &str,
) -> edits::Applied {
    let buffer = page.buffer();
    page.track_edit(text, &result.edits, None, || {
        edits::apply_replace_all(&buffer, result, text)
    })
}

/// Drops the selection the bar opened with.
pub(in crate::window) fn forget_search_scope(page: &EditorPage) {
    let buffer = page.buffer();
    for name in [SCOPE_START_MARK, SCOPE_END_MARK] {
        if let Some(mark) = buffer.mark(name) {
            buffer.delete_mark(&mark);
        }
    }
}

/// What the self-test reads of Replace All.
#[cfg(feature = "selftest")]
impl Window {
    /// What the last Replace All cost.
    pub fn replace_stats(&self) -> Option<ReplaceStats> {
        self.inner.replace.stats.get()
    }

    /// Forgets the last Replace All's cost.
    pub fn reset_replace_stats(&self) {
        self.inner.replace.stats.set(None);
    }
}
