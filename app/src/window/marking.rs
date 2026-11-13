//! Bookmarks, Mark and style tokens: Search › Bookmark, the find bar's Mark row, and
//! Search › Style All Occurrences of Token with the jumps between styled text.
//!
//! - **Bookmarks** are GtkSourceView marks in the gutter (`crate::marks`); a click in the gutter
//!   toggles one. The bookmarked-line operations compute their edit with `stet_domain::marks`
//!   on a snapshot, on a worker for large documents, and apply it as one undo step (one bulk
//!   edit above 2,000 edits, ADR-003) through the document's bookmark tracking, so Undo and Redo
//!   bring the bookmarks back.
//! - **Mark** finds every match of the find field's query with our own engine, on a worker for
//!   large documents, and marks them in the chosen style; with Bookmark line it bookmarks their
//!   lines too.
//! - **Style tokens** mark every occurrence of the selection, or of the word at the caret as a
//!   whole word, case-sensitively, in one of five styles.

use super::Window;
use super::search::bar::{BarMode, Tone};
use super::snapshot::{PageSnapshot, Stale, is_small};
use crate::editor::EditorPage;
use crate::edits;
use crate::runtime::worker;
use gtk4::prelude::*;

use std::cell::Cell;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};
use stet_domain::actions::ActionId;
use stet_domain::marks::{self, Bookmarks, LineEdit, MarkStyle, bookmark_lines_of};
use stet_domain::ops::Change;
use stet_domain::search::report::counted;
use stet_domain::search::{Query, SearchMode, SearchOptions};
use stet_domain::text::{BULK_EDIT_THRESHOLD, is_word_char};
use stet_domain::text::{LineIndex, TextEdit};
use stet_infrastructure::search::{Matcher, SearchError};

/// Mark and the style tokens stop after this many matches in a document.
pub const MARK_LIMIT: usize = 1_000_000;

/// What the last Mark All or style token did, for the self-test.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MarkStats {
    pub style: Option<MarkStyle>,
    pub matches: usize,
    /// More matches than [`MARK_LIMIT`].
    pub truncated: bool,
    /// Lines bookmarked (Bookmark line).
    pub bookmarked: usize,
    /// Matched on a worker.
    pub worker: bool,
    pub took: Duration,
}

/// What the last bookmarked-line operation did, for the self-test.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LineOpStats {
    pub edits: usize,
    pub bulk: bool,
    pub worker: bool,
    pub took: Duration,
}

/// The window's Mark state.
#[derive(Debug, Default)]
pub struct MarkState {
    pub stats: Cell<Option<MarkStats>>,
    pub line_stats: Cell<Option<LineOpStats>>,
}

/// A bookmarked-line operation, run on a snapshot: the edit, and text for the clipboard.
type LineOp = Box<dyn FnOnce(&str, &LineIndex, &Bookmarks) -> (LineEdit, Option<String>) + Send>;

/// The commands of Search › Bookmark, the find bar's Mark row and Search › Style All
/// Occurrences of Token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MarkCommand {
    ToggleBookmark,
    NextBookmark,
    PreviousBookmark,
    ClearBookmarks,
    CutBookmarkedLines,
    CopyBookmarkedLines,
    PasteToBookmarkedLines,
    RemoveBookmarkedLines,
    RemoveUnbookmarkedLines,
    InverseBookmarks,
    /// Opens the find bar's Mark row.
    Mark,
    MarkAll,
    ClearMarks,
    CopyMarkedText,
    /// Styles every occurrence of the token at the caret in a token style.
    StyleToken(MarkStyle),
    /// Clears one style.
    ClearStyle(MarkStyle),
    /// Clears the five token styles.
    ClearAllStyles,
    /// Jumps to the next text in a style, `down` or up.
    JumpToStyle {
        style: MarkStyle,
        down: bool,
    },
}

impl Window {
    /// Runs a bookmark, Mark or style-token command.
    pub(super) fn run_mark(&self, command: MarkCommand) {
        match command {
            MarkCommand::ToggleBookmark => self.toggle_bookmark(),
            MarkCommand::NextBookmark => self.jump_to_bookmark(true),
            MarkCommand::PreviousBookmark => self.jump_to_bookmark(false),
            MarkCommand::ClearBookmarks => {
                if let Some(page) = self.current_page() {
                    page.set_bookmarks(&Bookmarks::new());
                    self.update_mark_actions();
                }
            }
            MarkCommand::InverseBookmarks => {
                if let Some(page) = self.current_page() {
                    let lines = page.buffer().line_count().max(1) as usize;
                    page.set_bookmarks(&page.bookmarks().inverted(lines));
                    self.update_mark_actions();
                }
            }
            MarkCommand::CopyBookmarkedLines => self.copy_bookmarked_lines(),
            MarkCommand::CutBookmarkedLines => self.run_line_op(
                Box::new(|text, index, bookmarks| {
                    let cut = marks::cut_bookmarked_lines(text, index, bookmarks);
                    (cut.edit, Some(cut.clipboard))
                }),
                "Cut Bookmarked Lines",
            ),
            MarkCommand::RemoveBookmarkedLines => self.run_line_op(
                Box::new(|_, index, bookmarks| {
                    (marks::delete_bookmarked_lines(index, bookmarks), None)
                }),
                "Remove Bookmarked Lines",
            ),
            MarkCommand::RemoveUnbookmarkedLines => self.run_line_op(
                Box::new(|_, index, bookmarks| {
                    (marks::delete_unbookmarked_lines(index, bookmarks), None)
                }),
                "Remove Non-Bookmarked Lines",
            ),
            MarkCommand::PasteToBookmarkedLines => self.paste_to_bookmarked_lines(),
            MarkCommand::Mark => self.show_find_mode(BarMode::Mark),
            MarkCommand::MarkAll => self.mark_all(),
            MarkCommand::ClearMarks => self.clear_marks(),
            MarkCommand::CopyMarkedText => self.copy_marked_text(),
            MarkCommand::ClearAllStyles => {
                if let Some(page) = self.current_page() {
                    page.marks().update_marks(|marks| {
                        for style in MarkStyle::TOKENS {
                            marks.clear(style);
                        }
                    });
                    page.retag_marks();
                    self.update_mark_actions();
                }
            }
            MarkCommand::StyleToken(style) => self.style_token(style),
            MarkCommand::ClearStyle(style) => {
                if let Some(page) = self.current_page() {
                    page.marks().update_marks(|marks| marks.clear(style));
                    page.retag_marks();
                    self.update_mark_actions();
                }
            }
            MarkCommand::JumpToStyle { style, down } => self.jump_to_style(style, down),
        }
    }

    // ----- bookmarks -------------------------------------------------------------------------

    fn toggle_bookmark(&self) {
        let Some(page) = self.current_page() else {
            return;
        };
        let buffer = page.buffer();
        let line = buffer.iter_at_mark(&buffer.get_insert()).line().max(0) as usize;
        page.toggle_bookmark(line);
        self.update_mark_actions();
    }

    /// Next Bookmark (`forward`) or Previous Bookmark, wrapping around; a jump to come back
    /// from.
    fn jump_to_bookmark(&self, forward: bool) {
        let Some(page) = self.current_page() else {
            return;
        };
        let bookmarks = page.bookmarks();
        let buffer = page.buffer();
        let line = buffer.iter_at_mark(&buffer.get_insert()).line().max(0) as usize;
        let target = if forward {
            bookmarks.next(line)
        } else {
            bookmarks.prev(line)
        };
        let Some(target) = target else {
            self.toast("There are no bookmarks");
            return;
        };
        self.note_jump();
        if let Some(iter) = buffer.iter_at_line(target as i32) {
            buffer.place_cursor(&iter);
            page.view()
                .scroll_to_mark(&buffer.get_insert(), 0.1, true, 0.0, 0.3);
        }
        page.view().grab_focus();
    }

    fn copy_bookmarked_lines(&self) {
        let Some(page) = self.current_page() else {
            return;
        };
        let bookmarks = page.bookmarks();
        if bookmarks.is_empty() {
            self.toast("There are no bookmarked lines");
            return;
        }
        let text = page.text();
        let index = LineIndex::new(&text);
        let copied = marks::copy_bookmarked_lines(&text, &index, &bookmarks);
        page.view().clipboard().set_text(&copied);
        self.toast(&lines_message("Copied", bookmarks.len()));
    }

    fn paste_to_bookmarked_lines(&self) {
        let Some(page) = self.current_page() else {
            return;
        };
        let clipboard = page.view().clipboard();
        let window = self.clone();
        self.spawn(async move {
            let pasted = match clipboard.read_text_future().await {
                Ok(Some(text)) => stet_domain::text::normalize_to_lf(&text).into_owned(),
                _ => {
                    window.toast("The clipboard has no text");
                    return;
                }
            };
            if window.current_page().as_ref() != Some(&page) {
                return;
            }
            window.run_line_op(
                Box::new(move |_, index, bookmarks| {
                    (
                        marks::paste_to_bookmarked_lines(index, bookmarks, &pasted),
                        None,
                    )
                }),
                "Paste to (Replace) Bookmarked Lines",
            );
        });
    }

    /// Runs a bookmarked-line operation on the current document: on a snapshot, on a worker
    /// when the document is large, applied as one undo step that keeps the bookmarks across
    /// Undo and Redo.
    fn run_line_op(&self, op: LineOp, what: &'static str) {
        let Some(page) = self.current_page() else {
            return;
        };
        if !self.tool_allowed(&page) {
            return;
        }
        let bookmarks = page.bookmarks();
        if bookmarks.is_empty() {
            self.toast("There are no bookmarked lines");
            return;
        }
        let started = Instant::now();
        let (snapshot, text) = PageSnapshot::take(&page);
        if is_small(&text) {
            let index = LineIndex::new(&text);
            let (edit, clipboard) = op(&text, &index, &bookmarks);
            let bulk = (edit.edits.len() > BULK_EDIT_THRESHOLD)
                .then(|| coalesce(&edit.edits, &text))
                .flatten();
            self.finish_line_op(
                &page,
                &text,
                &edit,
                bulk.as_ref(),
                clipboard,
                started,
                false,
            );
            return;
        }
        page.set_busy(true);
        let window = self.clone();
        self.spawn(async move {
            let computed = window
                .on_snapshot(&snapshot, move || {
                    let index = LineIndex::new(&text);
                    let (edit, clipboard) = op(&text, &index, &bookmarks);
                    let bulk = (edit.edits.len() > BULK_EDIT_THRESHOLD)
                        .then(|| coalesce(&edit.edits, &text))
                        .flatten();
                    (text, edit, bulk, clipboard)
                })
                .await;
            page.set_busy(false);
            let (text, edit, bulk, clipboard) = match computed {
                Ok(computed) => computed,
                Err(Stale::Panicked) => {
                    window.toast(&format!("{what} failed"));
                    return;
                }
                Err(Stale::Closed) => return,
                Err(Stale::Edited) => {
                    window.toast(&format!(
                        "The document changed while {what} ran; nothing was changed"
                    ));
                    return;
                }
            };
            window.finish_line_op(&page, &text, &edit, bulk.as_ref(), clipboard, started, true);
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_line_op(
        &self,
        page: &EditorPage,
        text: &str,
        edit: &LineEdit,
        bulk: Option<&TextEdit>,
        clipboard: Option<String>,
        started: Instant,
        worker: bool,
    ) {
        if let Some(clipboard) = clipboard {
            page.view().clipboard().set_text(&clipboard);
        }
        if edit.edits.is_empty() {
            page.set_bookmarks(&edit.bookmarks);
            self.inner.marking.line_stats.set(Some(LineOpStats {
                worker,
                took: started.elapsed(),
                ..LineOpStats::default()
            }));
            return;
        }
        let buffer = page.buffer();
        page.clear_current_match();
        let mut applied = None;
        page.track_edit(text, &edit.edits, Some(edit.bookmarks.clone()), || {
            let done = match bulk {
                Some(bulk) => edits::apply_coalesced(&buffer, &edit.edits, bulk),
                None => edits::apply_change(
                    &buffer,
                    &Change {
                        edits: edit.edits.clone(),
                        selection: None,
                    },
                    text,
                ),
            };
            applied = Some(done);
            done.footprint
        });
        page.view().scroll_mark_onscreen(&buffer.get_insert());
        if let Some(applied) = applied {
            self.inner.marking.line_stats.set(Some(LineOpStats {
                edits: applied.edits,
                bulk: applied.bulk,
                worker,
                took: started.elapsed(),
            }));
        }
        self.update_mark_actions();
    }

    // ----- Mark --------------------------------------------------------------------------

    /// Mark All: every match of the find field's query in the scope (In selection, Wrap
    /// around), in the Mark row's style; with Bookmark line their lines are bookmarked, and
    /// with Purge for each search the style's marks (and those bookmarks) go first.
    pub(super) fn mark_all(&self) {
        let Some(compiled) = self.query_for_command() else {
            return;
        };
        let Some(page) = self.current_page() else {
            return;
        };
        let find = &self.inner.find;
        if page.is_loading() {
            find.set_message("The document is still loading", Tone::Error);
            return;
        }
        let mark = find.mark_options();
        let options = find.options();
        let (range, _) = Self::command_scope(&page, options);
        let matcher = Arc::clone(&compiled.matcher);
        let request = MarkRequest {
            style: mark.style,
            bookmark: mark.bookmark_line,
            purge: mark.purge,
            range: Some(range),
        };
        self.mark_matches(&page, matcher, request, "Mark");
    }

    /// Finds the matches of `matcher` in `page` (on a worker when the document is large) and
    /// marks them as `request` says.
    fn mark_matches(
        &self,
        page: &EditorPage,
        matcher: Arc<Matcher>,
        request: MarkRequest,
        what: &'static str,
    ) {
        let started = Instant::now();
        let (snapshot, text) = PageSnapshot::take(page);
        let bookmark = request.bookmark;
        let range = request.range.clone();
        let search = move |text: &str| {
            let cancel = AtomicBool::new(false);
            matcher
                .find_all(text, range.clone(), MARK_LIMIT, &cancel)
                .map(|found| {
                    let ranges: Vec<Range<usize>> = found
                        .matches
                        .iter()
                        .map(|found| found.start..found.end)
                        .collect();
                    let lines = if bookmark {
                        bookmark_lines_of(&ranges, &LineIndex::new(text))
                    } else {
                        Vec::new()
                    };
                    Found {
                        ranges,
                        lines,
                        truncated: found.truncated,
                    }
                })
        };
        if is_small(&text) {
            let found = search(&text);
            self.finish_mark(page, &request, found, started, false, what);
            return;
        }
        self.inner
            .find
            .set_message(&format!("{what}: searching…"), Tone::Info);
        let window = self.clone();
        let page = page.clone();
        self.spawn(async move {
            let found = worker::run(move || search(&text)).await;
            match window.check_snapshot(&snapshot) {
                Ok(()) => {}
                Err(Stale::Closed) => return,
                Err(_) => {
                    window.inner.find.set_message(
                        &format!("The document changed while {what} ran; nothing was marked"),
                        Tone::Error,
                    );
                    return;
                }
            }
            match found {
                Some(found) => window.finish_mark(&page, &request, found, started, true, what),
                None => window
                    .inner
                    .find
                    .set_message(&format!("{what} failed"), Tone::Error),
            }
        });
    }

    fn finish_mark(
        &self,
        page: &EditorPage,
        request: &MarkRequest,
        found: Result<Found, SearchError>,
        started: Instant,
        worker: bool,
        what: &str,
    ) {
        let find = &self.inner.find;
        let Found {
            ranges,
            lines,
            truncated,
        } = match found {
            Ok(found) => found,
            Err(error) => {
                find.set_message(&error.to_string(), Tone::Error);
                return;
            }
        };
        let matches = ranges.len();
        page.marks().update_marks(|marks| {
            if request.purge {
                marks.clear(request.style);
            }
            marks.add(request.style, ranges);
        });
        if request.bookmark {
            let mut bookmarks = if request.purge {
                Bookmarks::new()
            } else {
                page.bookmarks()
            };
            bookmarks.extend(lines.iter().copied());
            page.set_bookmarks(&bookmarks);
        }
        page.retag_marks();
        let more = if truncated { "+" } else { "" };
        let message = match matches {
            0 => format!("{what}: no matches"),
            1 => format!("{what}: 1 match"),
            count => format!("{what}: {count}{more} matches"),
        };
        if what == "Mark" {
            find.set_message(&message, Tone::Info);
        }
        self.inner.marking.stats.set(Some(MarkStats {
            style: Some(request.style),
            matches,
            truncated,
            bookmarked: lines.len(),
            worker,
            took: started.elapsed(),
        }));
        self.update_mark_actions();
    }

    /// Clear All Marks: the Mark row's style, and with Bookmark line every bookmark.
    fn clear_marks(&self) {
        let Some(page) = self.current_page() else {
            return;
        };
        let find = &self.inner.find;
        let mark = find.mark_options();
        page.marks().update_marks(|marks| marks.clear(mark.style));
        if mark.bookmark_line {
            page.set_bookmarks(&Bookmarks::new());
        }
        page.retag_marks();
        if find.is_open() {
            find.set_message("Cleared the marks", Tone::Info);
        }
        self.update_mark_actions();
    }

    /// Copy Marked Text: the text of every mark of the Mark row's style, one per line.
    fn copy_marked_text(&self) {
        let Some(page) = self.current_page() else {
            return;
        };
        let style = self.inner.find.mark_options().style;
        let ranges = page.marks().marks().ranges(style).to_vec();
        if ranges.is_empty() {
            self.toast("Nothing is marked in that style");
            return;
        }
        let buffer = page.buffer();
        let copied: Vec<String> = ranges
            .iter()
            .map(|range| {
                buffer
                    .text(
                        &buffer.iter_at_offset(range.start as i32),
                        &buffer.iter_at_offset(range.end as i32),
                        true,
                    )
                    .to_string()
            })
            .collect();
        page.view().clipboard().set_text(&copied.join("\n"));
        self.toast(&format!(
            "Copied {}",
            counted(ranges.len(), "marked text", "marked texts")
        ));
    }

    // ----- style tokens ------------------------------------------------------------------

    /// Style All Occurrences of Token: the selection (one line) or the word at the caret,
    /// everywhere in the document, case-sensitively; the caret's word as a whole word.
    fn style_token(&self, style: MarkStyle) {
        let Some(page) = self.current_page() else {
            return;
        };
        if page.is_loading() {
            self.toast("The document is still loading");
            return;
        }
        let Some((token, whole_word)) = token_at_caret(&page) else {
            self.toast("Select text, or put the caret in a word, to style its occurrences");
            return;
        };
        let options = SearchOptions {
            mode: SearchMode::Normal,
            match_case: true,
            whole_word,
            ..SearchOptions::default()
        };
        let matcher = match Matcher::new(&Query::new(token, options)) {
            Ok(matcher) => Arc::new(matcher),
            Err(error) => {
                self.toast(&error.message);
                return;
            }
        };
        let request = MarkRequest {
            style,
            bookmark: false,
            purge: false,
            range: None,
        };
        self.mark_matches(&page, matcher, request, "Style");
    }

    /// Jump Down (`down`) or Up to the next text in `style`, wrapping around; a jump to come
    /// back from.
    fn jump_to_style(&self, style: MarkStyle, down: bool) {
        let Some(page) = self.current_page() else {
            return;
        };
        let buffer = page.buffer();
        let (selection, _) = edits::selection(&buffer);
        let target = {
            let marks = page.marks().marks();
            if down {
                marks.next(style, selection.start)
            } else {
                marks.prev(style, selection.start)
            }
        };
        let Some(target) = target else {
            self.toast("Nothing is marked in that style");
            return;
        };
        self.note_jump();
        edits::select(&buffer, target, false);
        page.view()
            .scroll_to_mark(&buffer.get_insert(), 0.1, true, 0.0, 0.3);
        page.view().grab_focus();
    }

    /// Enables the bookmarked-line operations that change the text while the document can be
    /// changed. The others stay enabled and say so when there are no bookmarks.
    pub(super) fn update_mark_actions(&self) {
        let editable = self
            .current_page()
            .is_some_and(|page| !page.is_loading() && page.read_only().is_none());
        for id in [
            ActionId::CutBookmarkedLines,
            ActionId::PasteToBookmarkedLines,
            ActionId::RemoveBookmarkedLines,
            ActionId::RemoveUnbookmarkedLines,
        ] {
            if let Some(action) = self.action(id) {
                action.set_enabled(editable);
            }
        }
    }

    /// The marks of a style in the current document, for the self-test.
    #[cfg(feature = "selftest")]
    pub fn marked(&self, style: MarkStyle) -> Vec<Range<usize>> {
        self.current_page()
            .map(|page| page.marks().marks().ranges(style).to_vec())
            .unwrap_or_default()
    }

    #[cfg(feature = "selftest")]
    pub fn mark_stats(&self) -> Option<MarkStats> {
        self.inner.marking.stats.get()
    }

    #[cfg(feature = "selftest")]
    pub fn line_op_stats(&self) -> Option<LineOpStats> {
        self.inner.marking.line_stats.get()
    }
}

/// What a Mark search found: the matches, the lines to bookmark, and whether there were more
/// than [`MARK_LIMIT`].
struct Found {
    ranges: Vec<Range<usize>>,
    lines: Vec<usize>,
    truncated: bool,
}

/// How to mark what a search finds.
struct MarkRequest {
    style: MarkStyle,
    bookmark: bool,
    purge: bool,
    range: Option<Range<usize>>,
}

/// The selection, when it is on one line, else the word at the caret (as a whole word).
fn token_at_caret(page: &EditorPage) -> Option<(String, bool)> {
    let buffer = page.buffer();
    if let Some((start, end)) = buffer.selection_bounds() {
        let selected = buffer.text(&start, &end, false).to_string();
        if !selected.contains('\n') && !selected.is_empty() {
            return Some((selected, false));
        }
    }
    // A token is a run of letters, digits and underscores (Scintilla's word characters):
    // `alpha.beta` is two.
    let caret = buffer.iter_at_mark(&buffer.get_insert());
    let mut start = caret;
    while !start.starts_line() {
        let mut before = start;
        before.backward_char();
        if !is_word_char(before.char()) {
            break;
        }
        start = before;
    }
    let mut end = caret;
    while !end.ends_line() && is_word_char(end.char()) {
        end.forward_char();
    }
    let token = buffer.text(&start, &end, false).to_string();
    (!token.is_empty()).then_some((token, true))
}

/// The one replacement `edits` amount to in `text`, for a bulk edit.
fn coalesce(edits: &[TextEdit], text: &str) -> Option<TextEdit> {
    Change {
        edits: edits.to_vec(),
        selection: None,
    }
    .coalesce(text)
}

fn lines_message(verb: &str, count: usize) -> String {
    format!(
        "{verb} {}",
        counted(count, "bookmarked line", "bookmarked lines")
    )
}
