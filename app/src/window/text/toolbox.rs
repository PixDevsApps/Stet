//! The text toolbox: the line, case, comment and blank operations, prefix/suffix
//! and numbers, the JSON and XML tools, brace jumps and Select and Find, through the pure
//! operations in `stet_domain::ops`. An operation runs on a snapshot of the text, on a worker
//! when the document is large, and its result is applied in one user action (ADR-003): one
//! bulk edit above 2,000 edits, and a large result inserted in pieces so the window keeps
//! painting (ADR-003 amendment on text tools).

use crate::convert::{BufferPositions, IterPosition};
use crate::editor::EditorPage;
use crate::editor::edits;
use crate::window::Window;
use crate::window::snapshot::{PageSnapshot, Stale, is_small, run_sized};
use gtk4::glib;
use gtk4::prelude::*;
use sourceview5::prelude::*;
use std::cell::RefCell;
use std::time::{Duration, Instant};
use stet_domain::actions::ActionId;
use stet_domain::document::ReadOnly;
use stet_domain::document::indent::Indentation;
use stet_domain::marks::Footprint;
use stet_domain::ops::case::{self, Case};
use stet_domain::ops::comment::{self, CommentTokens};
use stet_domain::ops::lines::{
    self, Duplicates, EmptyLines, Order, Scope as Columns, SortKey, Trim,
};
use stet_domain::ops::numbers::NumberSequence;
use stet_domain::ops::{Change, Indent, SyntaxError, Target, json, xml};
use stet_domain::search::SearchMode;
use stet_domain::text::{LineIndex, Selection, TextEdit};

/// A result larger than this is inserted in pieces of about this size, 16 ms apart, inside
/// the one user action, so a 20 MB JSON file formats without blocking the window.
pub const PIECE_BYTES: usize = 1024 * 1024;
/// The pause between two pieces of a large result, as when a file loads.
const PIECE_INTERVAL: Duration = crate::editor::load::CHUNK_INTERVAL;
/// How many characters of the old text one step deletes before a large result goes in.
const DELETE_CHARS: usize = 2 * 1024 * 1024;

/// What the last text tool did, for the self-test.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ToolStats {
    pub edits: usize,
    /// Applied as one replacement of the range the edits span.
    pub bulk: bool,
    /// Computed on a worker.
    pub worker: bool,
    /// Pieces the result went in as; 1 unless it was large.
    pub pieces: usize,
    pub took: Duration,
}

/// The toolbox's memory: the dialogs' last values and the last run's statistics.
#[derive(Debug, Default)]
pub struct ToolState {
    pub prefix: RefCell<String>,
    pub suffix: RefCell<String>,
    pub numbering: RefCell<NumberSequence>,
    pub stats: RefCell<Option<ToolStats>>,
}

/// What a tool works on when nothing is selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// The caret's line or word (duplicate, move, join, case, comments).
    Caret,
    /// The whole document (sort, dedupe, empty lines, trim, tabs, prefix/suffix).
    Document,
}

/// Declares [`TextTool`] with one variant per text-tool action, named as the action is, and
/// the way back to the action, whose label names the tool in messages.
macro_rules! text_tools {
    ($($tool:ident),+ $(,)?) => {
        /// The text tools of the Edit menu and the editor's context menu.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub(in crate::window) enum TextTool {
            $($tool,)+
        }

        impl TextTool {
            /// The registry action that runs the tool.
            const fn action(self) -> ActionId {
                match self {
                    $(Self::$tool => ActionId::$tool,)+
                }
            }
        }
    };
}

text_tools! {
    Uppercase,
    Lowercase,
    ProperCase,
    ProperCaseBlend,
    SentenceCase,
    SentenceCaseBlend,
    InvertCase,
    DuplicateLine,
    CutLine,
    CopyLine,
    DeleteLine,
    TransposeLine,
    MoveLineUp,
    MoveLineDown,
    JoinLines,
    BlankLineAbove,
    BlankLineBelow,
    RemoveDuplicateLines,
    RemoveConsecutiveDuplicateLines,
    RemoveEmptyLines,
    RemoveBlankLines,
    ReverseLines,
    SortLexicalAscending,
    SortLexicalDescending,
    SortIgnoreCaseAscending,
    SortIgnoreCaseDescending,
    SortIntegerAscending,
    SortIntegerDescending,
    SortDecimalCommaAscending,
    SortDecimalCommaDescending,
    SortDecimalDotAscending,
    SortDecimalDotDescending,
    SortLengthAscending,
    SortLengthDescending,
    ToggleComment,
    CommentLines,
    UncommentLines,
    ToggleBlockComment,
    TrimTrailing,
    TrimLeading,
    TrimBoth,
    TabsToSpaces,
    SpacesToTabs,
    SpacesToTabsLeading,
    AddPrefixSuffix,
    InsertNumbers,
    GoToMatchingBrace,
    SelectToMatchingBrace,
    SelectAndFindNext,
    SelectAndFindPrevious,
    FormatJson,
    MinifyJson,
    ValidateJson,
    FormatXml,
    ValidateXml,
}

type Operation = Box<dyn FnOnce(&str, Target) -> Result<Change, SyntaxError> + Send>;

impl Window {
    /// Runs one of the text tools on the current tab.
    pub(in crate::window) fn run_text_tool(&self, tool: TextTool) {
        let Some(page) = self.current_page() else {
            return;
        };
        if let Some((reach, operation)) = self.text_edit(&page, tool) {
            self.run_tool(&page, reach, operation, describe(tool));
        }
    }

    /// The edit `tool` makes to `page`'s text and how far it reaches; `None` when the tool
    /// acts at once instead (copying, moving the caret, asking first, validating) or has
    /// nothing to act on.
    #[expect(
        clippy::too_many_lines,
        reason = "one flat, exhaustive table from each text tool to its edit; a wildcard would \
                  let a new tool compile unhandled"
    )]
    fn text_edit(&self, page: &EditorPage, tool: TextTool) -> Option<(Reach, Operation)> {
        let tab_width = page.indentation().tab_width as usize;
        let simple = |op: fn(&str, &Target) -> Change| -> Operation {
            Box::new(move |text, target| Ok(op(text, &target)))
        };
        Some(match tool {
            TextTool::Uppercase => Self::case_tool(Case::Upper),
            TextTool::Lowercase => Self::case_tool(Case::Lower),
            TextTool::ProperCase => Self::case_tool(Case::Proper),
            TextTool::ProperCaseBlend => Self::case_tool(Case::ProperBlend),
            TextTool::SentenceCase => Self::case_tool(Case::Sentence),
            TextTool::SentenceCaseBlend => Self::case_tool(Case::SentenceBlend),
            TextTool::InvertCase => Self::case_tool(Case::Invert),
            TextTool::DuplicateLine => (Reach::Caret, simple(lines::duplicate)),
            TextTool::DeleteLine => (Reach::Caret, simple(lines::delete)),
            TextTool::MoveLineUp => (Reach::Caret, simple(lines::move_up)),
            TextTool::MoveLineDown => (Reach::Caret, simple(lines::move_down)),
            TextTool::JoinLines => (Reach::Caret, simple(lines::join)),
            TextTool::BlankLineAbove => (Reach::Caret, simple(lines::insert_blank_above)),
            TextTool::BlankLineBelow => (Reach::Caret, simple(lines::insert_blank_below)),
            TextTool::TransposeLine => (Reach::Caret, simple(transpose)),
            TextTool::CutLine => {
                if !Self::copy_lines(page) {
                    return None;
                }
                (Reach::Caret, simple(lines::delete))
            }
            TextTool::CopyLine => {
                Self::copy_lines(page);
                return None;
            }
            TextTool::RemoveDuplicateLines => document_tool(move |text, target| {
                lines::remove_duplicates(text, target, Duplicates::All)
            }),
            TextTool::RemoveConsecutiveDuplicateLines => document_tool(move |text, target| {
                lines::remove_duplicates(text, target, Duplicates::Consecutive)
            }),
            TextTool::RemoveEmptyLines => document_tool(move |text, target| {
                lines::remove_empty(text, target, EmptyLines::Empty)
            }),
            TextTool::RemoveBlankLines => document_tool(move |text, target| {
                lines::remove_empty(text, target, EmptyLines::Blank)
            }),
            TextTool::ReverseLines => (Reach::Document, simple(lines::reverse)),
            TextTool::SortLexicalAscending => sort(SortKey::Lexical, Order::Ascending),
            TextTool::SortLexicalDescending => sort(SortKey::Lexical, Order::Descending),
            TextTool::SortIgnoreCaseAscending => sort(SortKey::LexicalIgnoreCase, Order::Ascending),
            TextTool::SortIgnoreCaseDescending => {
                sort(SortKey::LexicalIgnoreCase, Order::Descending)
            }
            TextTool::SortIntegerAscending => sort(SortKey::Integer, Order::Ascending),
            TextTool::SortIntegerDescending => sort(SortKey::Integer, Order::Descending),
            TextTool::SortDecimalCommaAscending => sort(SortKey::DecimalComma, Order::Ascending),
            TextTool::SortDecimalCommaDescending => sort(SortKey::DecimalComma, Order::Descending),
            TextTool::SortDecimalDotAscending => sort(SortKey::DecimalDot, Order::Ascending),
            TextTool::SortDecimalDotDescending => sort(SortKey::DecimalDot, Order::Descending),
            TextTool::SortLengthAscending => sort(SortKey::Length, Order::Ascending),
            TextTool::SortLengthDescending => sort(SortKey::Length, Order::Descending),
            TextTool::ToggleComment => self.comment_tool(page, comment::toggle_line)?,
            TextTool::CommentLines => self.comment_tool(page, comment::comment_lines)?,
            TextTool::UncommentLines => self.comment_tool(page, comment::uncomment_lines)?,
            TextTool::ToggleBlockComment => self.comment_tool(page, comment::toggle_block)?,
            TextTool::TrimTrailing => trim(Trim::Trailing),
            TextTool::TrimLeading => trim(Trim::Leading),
            TextTool::TrimBoth => trim(Trim::Both),
            TextTool::TabsToSpaces => document_tool(move |text, target| {
                lines::tabs_to_spaces(text, target, tab_width, Columns::All)
            }),
            TextTool::SpacesToTabs | TextTool::SpacesToTabsLeading => {
                let scope = if tool == TextTool::SpacesToTabs {
                    Columns::All
                } else {
                    Columns::Leading
                };
                document_tool(move |text, target| {
                    lines::spaces_to_tabs(text, target, tab_width, scope)
                })
            }
            TextTool::AddPrefixSuffix => {
                let (window, page) = (self.clone(), page.clone());
                self.spawn(async move { window.ask_prefix_suffix(&page).await });
                return None;
            }
            TextTool::InsertNumbers => {
                let (window, page) = (self.clone(), page.clone());
                self.spawn(async move { window.ask_numbers(&page).await });
                return None;
            }
            TextTool::GoToMatchingBrace | TextTool::SelectToMatchingBrace => {
                self.to_matching_brace(page, tool == TextTool::SelectToMatchingBrace);
                return None;
            }
            TextTool::SelectAndFindNext => {
                self.select_and_find(page, true);
                return None;
            }
            TextTool::SelectAndFindPrevious => {
                self.select_and_find(page, false);
                return None;
            }
            TextTool::FormatJson => {
                let indent = indent_unit(page.indentation());
                (
                    Reach::Document,
                    Box::new(move |text, target| json::format_target(text, &target, indent)),
                )
            }
            TextTool::MinifyJson => (
                Reach::Document,
                Box::new(|text, target| json::minify_target(text, &target)),
            ),
            TextTool::FormatXml => {
                let indent = indent_unit(page.indentation());
                (
                    Reach::Document,
                    Box::new(move |text, target| xml::format_target(text, &target, indent)),
                )
            }
            TextTool::ValidateJson | TextTool::ValidateXml => {
                self.validate(page, tool == TextTool::ValidateJson);
                return None;
            }
        })
    }

    /// Go to (or with `select`, Select to) Matching Brace.
    fn to_matching_brace(&self, page: &EditorPage, select: bool) {
        self.note_jump();
        page.view().emit_move_to_matching_bracket(select);
        let buffer = page.buffer();
        if select {
            // GtkSourceView stops before the far bracket; both brackets are selected.
            let mut insert = buffer.iter_at_mark(&buffer.get_insert());
            let bound = buffer.iter_at_mark(&buffer.selection_bound());
            if insert > bound && matches!(insert.char(), ')' | ']' | '}' | '>') {
                insert.forward_char();
                buffer.move_mark(&buffer.get_insert(), &insert);
            }
        }
        page.view().scroll_mark_onscreen(&buffer.get_insert());
    }

    fn case_tool(case: Case) -> (Reach, Operation) {
        (
            Reach::Caret,
            Box::new(move |text, target| Ok(case::convert(text, &target, case))),
        )
    }

    /// A comment tool, when the document's language has comment syntax; a toast says when
    /// it has none.
    fn comment_tool(
        &self,
        page: &EditorPage,
        apply: fn(&str, &Target, &CommentTokens) -> Change,
    ) -> Option<(Reach, Operation)> {
        let tokens = self.comment_tokens(page)?;
        Some((
            Reach::Caret,
            Box::new(move |text, target| Ok(apply(text, &target, &tokens))),
        ))
    }

    /// Whether a tool may change `page` now; a toast says why not.
    pub(in crate::window) fn tool_allowed(&self, page: &EditorPage) -> bool {
        let reason = if page.is_loading() {
            Some("The document is still loading")
        } else if page.is_busy() {
            Some("The previous text tool is still running")
        } else {
            page.read_only().map(ReadOnly::message)
        };
        match reason {
            Some(reason) => {
                self.toast(reason);
                false
            }
            None => true,
        }
    }

    /// The selection, or the caret, as a target: `reach` says what a caret stands for.
    fn target(page: &EditorPage, reach: Reach) -> Target {
        let selected = edits::selection(&page.buffer()).range();
        match reach {
            _ if !selected.is_empty() => Target::Selection(selected),
            Reach::Caret => Target::caret(selected.start),
            Reach::Document => Target::Document,
        }
    }

    /// Runs `operation` on a snapshot of `page` and applies its change: at once for small
    /// documents, on a worker with a revision check for large ones.
    pub(in crate::window) fn run_tool(
        &self,
        page: &EditorPage,
        reach: Reach,
        operation: Operation,
        what: &'static str,
    ) {
        self.run_on_target(page, Self::target(page, reach), operation, what);
    }

    pub(in crate::window) fn run_on_target(
        &self,
        page: &EditorPage,
        target: Target,
        operation: Operation,
        what: &'static str,
    ) {
        if !self.tool_allowed(page) {
            return;
        }
        let started = Instant::now();
        let (snapshot, text) = PageSnapshot::take(page);
        if is_small(&text) {
            let result = operation(&text, target);
            self.finish_tool(page, &text, result, started, false, what);
            return;
        }
        page.set_busy(true);
        let window = self.clone();
        let page = page.clone();
        self.spawn(async move {
            let computed = window
                .on_snapshot(&snapshot, move || {
                    let result = operation(&text, target);
                    // A bulk change, or one with a large result, becomes one replacement here,
                    // off the GTK thread.
                    let bulk = match &result {
                        Ok(change) if change.is_bulk() || inserted_bytes(change) > PIECE_BYTES => {
                            change.coalesce(&text)
                        }
                        _ => None,
                    };
                    (text, result, bulk)
                })
                .await;
            page.set_busy(false);
            let (text, result, bulk) = match computed {
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
            match (result, bulk) {
                (Ok(change), Some(edit)) if edit.insert.len() > PIECE_BYTES => {
                    // The bookmarks need the old text's lines; the text itself can go.
                    let index = page.has_bookmarks().then(|| LineIndex::new(&text));
                    drop(text);
                    window
                        .apply_in_pieces(&page, &change, edit, index, started)
                        .await;
                }
                (result, _) => window.finish_tool(&page, &text, result, started, true, what),
            }
        });
    }

    /// Applies a tool's result, or shows its syntax error at its place.
    fn finish_tool(
        &self,
        page: &EditorPage,
        text: &str,
        result: Result<Change, SyntaxError>,
        started: Instant,
        worker: bool,
        what: &'static str,
    ) {
        let change = match result {
            Ok(change) => change,
            Err(error) => {
                self.show_syntax_error(page, what, &error);
                return;
            }
        };
        if change.is_empty() {
            self.inner.tools.stats.replace(Some(ToolStats {
                worker,
                took: started.elapsed(),
                ..ToolStats::default()
            }));
            return;
        }
        let buffer = page.buffer();
        page.clear_current_match();
        // Bookmarks and marks follow the edits, and come back with Undo and Redo.
        let applied = page.track_edit(text, &change.edits, None, || {
            edits::apply_change(&buffer, &change, text)
        });
        page.view().scroll_mark_onscreen(&buffer.get_insert());
        self.inner.tools.stats.replace(Some(ToolStats {
            edits: applied.edits,
            bulk: applied.bulk,
            worker,
            pieces: 1,
            took: started.elapsed(),
        }));
    }

    /// Applies a large result as one bulk edit whose text goes in a piece at a time, inside
    /// one user action, with the view read-only meanwhile: one undo step, and the main loop
    /// is never blocked for long.
    async fn apply_in_pieces(
        &self,
        page: &EditorPage,
        change: &Change,
        edit: TextEdit,
        index: Option<LineIndex>,
        started: Instant,
    ) {
        let buffer = page.buffer();
        let before = edits::selection(&buffer);
        let after = change.selection_after(before.range());
        page.clear_current_match();
        page.set_busy(true);
        let begun = page.marks().begin(&buffer);
        buffer.begin_user_action();
        let mut pieces = 0;
        let mut left = edit.end - edit.start;
        while left > 0 {
            if pieces > 0 {
                glib::timeout_future(PIECE_INTERVAL).await;
            }
            let count = left.min(DELETE_CHARS);
            let mut start = buffer.iter_at_char_offset(edit.start);
            let mut end = buffer.iter_at_char_offset(edit.start + count);
            buffer.delete(&mut start, &mut end);
            left -= count;
            pieces += 1;
        }
        let mut at = edit.start;
        for piece in pieces_of(&edit.insert, PIECE_BYTES) {
            if pieces > 0 {
                glib::timeout_future(PIECE_INTERVAL).await;
                if self.tab_page(page).is_none() {
                    break;
                }
            }
            let mut iter = buffer.iter_at_char_offset(at);
            buffer.insert(&mut iter, piece);
            at += piece.chars().count();
            pieces += 1;
        }
        buffer.end_user_action();
        page.marks().finish(
            &buffer,
            begun,
            index.as_ref(),
            &change.edits,
            None,
            Footprint::of(std::slice::from_ref(&edit)),
        );
        page.retag_marks();
        page.set_busy(false);
        edits::select(&buffer, before.with_range(after));
        page.view().scroll_mark_onscreen(&buffer.get_insert());
        self.inner.tools.stats.replace(Some(ToolStats {
            edits: change.edits.len(),
            bulk: true,
            worker: true,
            pieces,
            took: started.elapsed(),
        }));
    }

    /// Moves the caret to a JSON or XML error and says what it is.
    fn show_syntax_error(&self, page: &EditorPage, what: &str, error: &SyntaxError) {
        let buffer = page.buffer();
        let iter = buffer.iter_at_char_offset(error.offset);
        buffer.place_cursor(&iter);
        page.view().scroll_mark_onscreen(&buffer.get_insert());
        self.toast(&format!(
            "{what}: {} at line {}, column {}",
            error.message,
            iter.line() + 1,
            iter.line_offset() + 1
        ));
        page.view().grab_focus();
    }

    /// Validate JSON or XML: the selection, or the whole document.
    fn validate(&self, page: &EditorPage, is_json: bool) {
        let selected = edits::selection(&page.buffer()).range();
        let what = if is_json { "JSON" } else { "XML" };
        let operation: Operation = Box::new(move |text, target| {
            let index = LineIndex::new(text);
            let range = match target {
                Target::Selection(range) if !range.is_empty() => range,
                _ => 0..index.len_chars(),
            };
            let slice =
                &text[index.char_to_byte(text, range.start)..index.char_to_byte(text, range.end)];
            let checked = if is_json {
                json::validate(slice)
            } else {
                xml::validate(slice)
            };
            checked
                .map(|()| Change::default())
                .map_err(|error| SyntaxError {
                    offset: range.start + error.offset,
                    ..error
                })
        });
        let target = if selected.is_empty() {
            Target::Document
        } else {
            Target::Selection(selected)
        };
        if !self.tool_allowed_to_read(page) {
            return;
        }
        let text = page.text();
        let window = self.clone();
        let page = page.clone();
        self.spawn(async move {
            let checked = run_sized(text, move |text| operation(text, target)).await;
            match checked {
                Some(Ok(_)) => window.toast(if is_json {
                    "Valid JSON"
                } else {
                    "Well-formed XML"
                }),
                Some(Err(error)) => {
                    window.show_syntax_error(&page, &format!("Invalid {what}"), &error);
                }
                None => window.toast(&format!("Validate {what} failed")),
            }
        });
    }

    fn tool_allowed_to_read(&self, page: &EditorPage) -> bool {
        if page.is_loading() {
            self.toast("The document is still loading");
            return false;
        }
        true
    }

    /// The comment tokens of the document's language (ADR-014: the document's, not the
    /// buffer's, which has none while highlighting is off).
    fn comment_tokens(&self, page: &EditorPage) -> Option<CommentTokens> {
        let Some(language) = page.language() else {
            self.toast("Plain text has no comments; choose a language in the status bar first");
            return None;
        };
        let metadata = |key: &str| {
            language
                .metadata(key)
                .map(|value| value.to_string())
                .filter(|value| !value.is_empty())
        };
        let tokens = CommentTokens {
            line: metadata("line-comment-start"),
            block: metadata("block-comment-start").zip(metadata("block-comment-end")),
        };
        if tokens.line.is_none() && tokens.block.is_none() {
            self.toast(&format!("{} has no comment syntax", language.name()));
            return None;
        }
        Some(tokens)
    }

    /// Copy Current Line: the lines of the selection, or the caret's line, with a line
    /// break after the last. Returns false when there is nothing.
    pub(in crate::window) fn copy_lines(page: &EditorPage) -> bool {
        let buffer = page.buffer();
        let selected = edits::selection(&buffer).range();
        let first = buffer.iter_at_char_offset(selected.start);
        let mut last = buffer.iter_at_char_offset(selected.end);
        if last.line() > first.line() && last.starts_line() {
            last.backward_line();
        }
        let start = buffer.iter_at_line(first.line()).unwrap_or(first);
        let mut end = last;
        if !end.ends_line() {
            end.forward_to_line_end();
        }
        let mut text = buffer.text(&start, &end, true).to_string();
        text.push('\n');
        page.view().clipboard().set_text(&text);
        true
    }

    /// Select and Find Next or Previous: the selection, or the word at the caret, becomes the
    /// query in Normal mode, and the next match is selected.
    fn select_and_find(&self, page: &EditorPage, forward: bool) {
        let buffer = page.buffer();
        let selected = edits::selection(&buffer).range();
        let start = buffer.iter_at_char_offset(selected.start);
        let end = buffer.iter_at_char_offset(selected.end);
        let query = if selected.is_empty() {
            let line_start = buffer.iter_at_line(start.line()).unwrap_or(start);
            let mut line_end = line_start;
            if !line_end.ends_line() {
                line_end.forward_to_line_end();
            }
            let line = buffer.text(&line_start, &line_end, true).to_string();
            let column = selected.start - line_start.char_offset();
            match case::word_at(&line, column) {
                Some(word) => {
                    let base = line_start.char_offset();
                    let (from, to) = (base + word.start, base + word.end);
                    edits::select(&buffer, Selection::forward(from..to));
                    let from = buffer.iter_at_char_offset(from);
                    let to = buffer.iter_at_char_offset(to);
                    buffer.text(&from, &to, true).to_string()
                }
                None => String::new(),
            }
        } else {
            buffer.text(&start, &end, true).to_string()
        };
        if query.is_empty() || query.contains('\n') {
            self.toast("Select text on one line, or put the caret in a word");
            return;
        }
        let find = &self.inner.find;
        find.set_search_mode(SearchMode::Normal);
        find.set_query_text(&query);
        find.compile();
        self.find_step(forward);
    }

    /// Hooks a new editor to the toolbox: the shared editor shortcuts in the capture phase,
    /// Copy and Cut of the caret's line when nothing is selected (and no column rectangle is),
    /// and the context menu.
    pub(in crate::window) fn connect_page_tools(&self, page: &EditorPage) {
        let view = page.view();
        view.add_controller(crate::actions::editor_controller(
            &self.inner.shared.editor_shortcuts,
        ));
        self.connect_editor_menu_names(view.upcast_ref());
        for signal in ["copy-clipboard", "cut-clipboard"] {
            let cut = signal == "cut-clipboard";
            view.connect_local(
                signal,
                false,
                glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    #[weak]
                    page,
                    #[upgrade_or]
                    None,
                    move |_| {
                        // A selection, or a column-mode rectangle, is copied as it is.
                        if page.buffer().has_selection() || page.column_view().rect().is_some() {
                            return None;
                        }
                        if cut {
                            window.run_text_tool(TextTool::CutLine);
                        } else {
                            Self::copy_lines(&page);
                        }
                        page.view().stop_signal_emission_by_name(signal);
                        None
                    }
                ),
            );
        }
        view.set_extra_menu(Some(&crate::actions::editor_menu(&self.keymap())));
    }
}

/// A tool over the whole document, or the selection, that cannot fail.
fn document_tool(
    edit: impl FnOnce(&str, &Target) -> Change + Send + 'static,
) -> (Reach, Operation) {
    (
        Reach::Document,
        Box::new(move |text, target| Ok(edit(text, &target))),
    )
}

fn sort(key: SortKey, order: Order) -> (Reach, Operation) {
    (
        Reach::Document,
        Box::new(move |text, target| Ok(lines::sort(text, &target, key, order))),
    )
}

fn trim(which: Trim) -> (Reach, Operation) {
    (
        Reach::Document,
        Box::new(move |text, target| Ok(lines::trim(text, &target, which))),
    )
}

/// Transpose Current Line (Scintilla's line transpose): the caret's line swaps with the line
/// above, and the caret goes to the start of its line.
fn transpose(text: &str, target: &Target) -> Change {
    let Target::Selection(range) = target else {
        return Change::default();
    };
    let index = LineIndex::new(text);
    let line = index.line_of_char(range.start.min(range.end));
    if line == 0 {
        return Change::default();
    }
    let mut change = lines::move_down(text, &Target::Lines(line - 1..line));
    let length = index.line_chars(line).len();
    let caret = index.line_chars(line - 1).start + length + 1;
    change.selection = (!change.is_empty()).then_some(caret..caret);
    change
}

/// The formatters' indentation: the document's.
fn indent_unit(indentation: Indentation) -> Indent {
    if indentation.insert_spaces {
        Indent::Spaces(indentation.tab_width as usize)
    } else {
        Indent::Tab
    }
}

fn inserted_bytes(change: &Change) -> usize {
    change.edits.iter().map(|edit| edit.insert.len()).sum()
}

/// `text` in pieces of at most about `size` bytes, cut after a line break where there is
/// one, else at a character boundary.
fn pieces_of(text: &str, size: usize) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let mut cut = rest.len().min(size);
        while !rest.is_char_boundary(cut) {
            cut -= 1;
        }
        if cut < rest.len()
            && let Some(newline) = rest[..cut].rfind('\n')
        {
            cut = newline + 1;
        }
        let (piece, tail) = rest.split_at(cut);
        rest = tail;
        Some(piece)
    })
}

/// The tool's name for messages.
fn describe(tool: TextTool) -> &'static str {
    match tool {
        TextTool::FormatJson => "Format JSON",
        TextTool::MinifyJson => "Minify JSON",
        TextTool::FormatXml => "Format XML",
        other => other.action().label(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_end_at_line_breaks_and_cover_everything() {
        let text = "aaaa\nbb\ncccccc\nd";
        let pieces: Vec<&str> = pieces_of(text, 6).collect();
        assert_eq!(pieces.concat(), text);
        assert_eq!(pieces[0], "aaaa\n");
        assert!(pieces.iter().all(|piece| piece.len() <= 6));
        let long = "ø".repeat(10);
        let pieces: Vec<&str> = pieces_of(&long, 5).collect();
        assert_eq!(pieces.concat(), long);
        assert!(pieces.iter().all(|piece| piece.len() <= 5));
        assert_eq!(pieces_of("", 4).count(), 0);
    }

    #[test]
    fn transpose_swaps_with_the_line_above() {
        let text = "one\ntwo\nthree";
        let change = transpose(text, &Target::caret(5));
        let result = stet_domain::text::apply(text, change.edits.clone()).unwrap();
        assert_eq!(result, "two\none\nthree");
        assert_eq!(change.selection, Some(4..4));
        assert!(transpose(text, &Target::caret(1)).is_empty());
    }
}
