//! Compare (M7): two documents side by side in the split view, the one in front as the new
//! version and the other as the old one. Lines only in the new one are added, lines only in
//! the old one removed, pairs of versions of a line changed (with the characters that differ
//! marked), and blocks that moved are moved; each side is padded where the other has lines it
//! lacks, so that equal lines face each other and the views scroll level (`crate::compare`
//! presents one side).
//!
//! The other document is the second view's, or, without a split, the tab used before, which
//! moves there; or a text the comparison opens itself, read-only, in the other view: a file
//! chosen in a dialog, the clipboard, or the file as last saved (also the Compare button of
//! the changed-on-disk and restored-conflict banners). Clear Compare closes those.
//!
//! The diff runs on a worker over snapshots and again shortly after either document changes;
//! a result for texts that changed meanwhile is dropped. Word wrap is off in compared views, so
//! the two sides' lines stay level.

use super::Window;
use crate::editor::{DocState, EditorPage, ReadOnly};
use crate::loader;
use crate::worker;
use gtk4::prelude::*;
use gtk4::{gio, glib};

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};
use stet_domain::actions::ActionId;
use stet_domain::diff::{self, CompareOptions, Comparison, IgnoreWhitespace, Side};
use stet_domain::text::{LONG_LINE_CHARS, line_count};
use stet_infrastructure::fs::LoadError;

pub use stet_domain::diff::CompareStats;

/// How long after the last edit a comparison runs again.
const RECOMPARE_DELAY: Duration = Duration::from_millis(300);

/// The last comparison run, for the self-test's report.
#[derive(Debug, Clone, Copy, Default)]
pub struct CompareRun {
    pub stats: CompareStats,
    /// Lines of the old and the new text.
    pub lines: (usize, usize),
    /// Taking both snapshots, on the GTK thread.
    pub snapshot: Duration,
    /// The diff, on a worker, with the trip there and back.
    pub diff: Duration,
    /// Tagging and padding both documents, on the GTK thread.
    pub present: Duration,
    /// From the snapshots to the presented comparison.
    pub total: Duration,
    /// From the snapshots to the end of the first frame painted after it.
    pub painted: Option<Duration>,
    /// Runs since the comparison started: re-comparisons after edits count.
    pub runs: u32,
}

/// The comparison on screen.
struct Comparing {
    old: EditorPage,
    new: EditorPage,
    comparison: Option<Rc<Comparison>>,
    /// Texts the comparison opened, closed with it.
    temporary: Vec<EditorPage>,
    handlers: Vec<(sourceview5::Buffer, glib::SignalHandlerId)>,
    runs: u32,
}

/// The window's comparison and its options.
#[derive(Default)]
pub struct CompareState {
    active: RefCell<Option<Comparing>>,
    ignore_whitespace: Cell<bool>,
    ignore_case: Cell<bool>,
    /// Bumped by every run and by Clear Compare: older results are dropped.
    generation: Cell<u64>,
    pending: RefCell<Option<glib::SourceId>>,
    /// A run's diff is on its way.
    running: Cell<bool>,
    last: Cell<Option<CompareRun>>,
}

impl Window {
    pub(super) fn run_compare_action(&self, id: ActionId) {
        match id {
            ActionId::Compare => self.compare_with_other_view(),
            ActionId::CompareWithFile => {
                let window = self.clone();
                self.spawn(async move { window.compare_with_chosen_file().await });
            }
            ActionId::CompareWithClipboard => {
                let window = self.clone();
                self.spawn(async move { window.compare_with_clipboard().await });
            }
            ActionId::CompareWithSaved => {
                if let Some(page) = self.current_page() {
                    let window = self.clone();
                    self.spawn(async move { window.compare_with_saved(page).await });
                }
            }
            ActionId::NextDifference | ActionId::PreviousDifference => {
                self.step_difference(id == ActionId::NextDifference);
            }
            ActionId::CompareIgnoreWhitespace | ActionId::CompareIgnoreCase => {
                let state = &self.inner.comparing;
                let cell = if id == ActionId::CompareIgnoreWhitespace {
                    &state.ignore_whitespace
                } else {
                    &state.ignore_case
                };
                cell.set(!cell.get());
                if let Some(action) = self.action(id) {
                    action.set_state(&cell.get().to_variant());
                }
                if self.comparing() {
                    self.recompare();
                }
            }
            ActionId::ClearCompare => self.clear_compare(),
            _ => {}
        }
    }

    /// Whether a comparison is on screen.
    pub fn comparing(&self) -> bool {
        self.inner.comparing.active.borrow().is_some()
    }

    /// Whether the views show the two compared documents, which then scroll level.
    pub(super) fn comparison_shown(&self) -> bool {
        let active = self.inner.comparing.active.borrow();
        let Some(active) = active.as_ref() else {
            return false;
        };
        let shown = self.shown_pages();
        shown.len() == 2
            && shown.iter().any(|page| page.same_document(&active.old))
            && shown.iter().any(|page| page.same_document(&active.new))
    }

    fn compare_options(&self) -> CompareOptions {
        let state = &self.inner.comparing;
        CompareOptions {
            ignore_whitespace: if state.ignore_whitespace.get() {
                IgnoreWhitespace::All
            } else {
                IgnoreWhitespace::None
            },
            ignore_case: state.ignore_case.get(),
            ..CompareOptions::default()
        }
    }

    /// Compare: the current document with the one in front of the other view; without a split,
    /// with the tab used before, which moves to the other view.
    fn compare_with_other_view(&self) {
        let Some(new) = self.current_page() else {
            return;
        };
        let Some(view) = self.view_of(&new) else {
            return;
        };
        let old = match self.view_page(1 - view) {
            Some(old) => old,
            None => {
                let previous = self
                    .recent_pages()
                    .into_iter()
                    .find(|page| !page.same_document(&new) && self.tab_page(page).is_some());
                let Some(previous) = previous else {
                    self.toast("Open a second document to compare with");
                    return;
                };
                self.move_to_other_view(&previous);
                self.select(&new);
                previous
            }
        };
        if old.same_document(&new) {
            self.toast("Both views show the same document");
            return;
        }
        let window = self.clone();
        self.spawn(async move { window.start_compare(new, old, Vec::new()).await });
    }

    /// Compare with File…: a file chosen in a dialog, opened read-only for the comparison.
    async fn compare_with_chosen_file(&self) {
        let Some(new) = self.current_page() else {
            return;
        };
        let folder = new
            .path()
            .and_then(|path| path.parent().map(std::path::Path::to_path_buf));
        let dialog = gtk4::FileDialog::builder()
            .title("Compare with File")
            .modal(true)
            .build();
        if let Some(folder) = folder {
            dialog.set_initial_folder(Some(&gio::File::for_path(folder)));
        }
        let chosen = dialog.open_future(Some(&self.inner.window)).await;
        self.restore_focus_later();
        let path = match chosen {
            Ok(file) => file.path(),
            Err(error) => {
                if !error.matches(gtk4::DialogError::Dismissed) {
                    self.toast(&format!("Could not open the file chooser: {error}"));
                }
                None
            }
        };
        let Some(path) = path else {
            return;
        };
        let label = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        self.compare_with_file_text(new, path, label, None).await;
    }

    /// The file as last saved, beside the document (also the banners' Compare button).
    pub(super) async fn compare_with_saved(&self, page: EditorPage) {
        let Some(path) = page.path() else {
            self.toast(&format!("“{}” has not been saved yet", page.name()));
            return;
        };
        let label = format!("{} (saved)", page.name());
        let choice = page.state().chosen;
        self.compare_with_file_text(page, path, label, choice).await;
    }

    async fn compare_with_file_text(
        &self,
        new: EditorPage,
        path: PathBuf,
        label: String,
        choice: Option<stet_infrastructure::fs::EncodingChoice>,
    ) {
        let read = worker::run(move || loader::read(&path, choice)).await;
        let read = match read {
            Some(Ok(read)) => read,
            Some(Err(LoadError::NotFound { .. })) => {
                self.toast(&format!("“{label}” is not on disk"));
                return;
            }
            Some(Err(error)) => {
                self.toast(&format!("Could not read “{label}”: {error}"));
                return;
            }
            None => {
                self.toast(&format!("Could not read “{label}”"));
                return;
            }
        };
        if read.longest.chars > LONG_LINE_CHARS {
            self.toast(&format!(
                "“{label}” has very long lines, which Stet can't compare"
            ));
            return;
        }
        if self.tab_page(&new).is_none() {
            return;
        }
        let old = self.open_temporary(&new, label, &read.text);
        self.start_compare(new, old.clone(), vec![old]).await;
    }

    /// Compare with Clipboard: the clipboard's text, opened read-only for the comparison.
    async fn compare_with_clipboard(&self) {
        let Some(new) = self.current_page() else {
            return;
        };
        let text = match new.view().clipboard().read_text_future().await {
            Ok(Some(text)) => stet_domain::text::normalize_to_lf(&text).into_owned(),
            _ => {
                self.toast("The clipboard has no text");
                return;
            }
        };
        if self.tab_page(&new).is_none() {
            return;
        }
        let old = self.open_temporary(&new, "Clipboard".to_owned(), &text);
        self.start_compare(new, old.clone(), vec![old]).await;
    }

    /// A read-only tab with `text` in the other view of `beside`, named `label`, in its
    /// language; it stays out of the session and closes with the comparison.
    fn open_temporary(&self, beside: &EditorPage, label: String, text: &str) -> EditorPage {
        let view = 1 - self.view_of(beside).unwrap_or(0);
        let page = self.add_page_in(
            view,
            DocState {
                temporary: Some(label),
                read_only: Some(ReadOnly::Comparison),
                ..DocState::default()
            },
            false,
        );
        let buffer = page.buffer();
        buffer.begin_irreversible_action();
        buffer.set_text(text);
        buffer.end_irreversible_action();
        buffer.set_modified(false);
        buffer.place_cursor(&buffer.start_iter());
        page.set_language(beside.language().as_ref());
        page.set_read_only(Some(ReadOnly::Comparison));
        if let Some((index, tab_page)) = self.locate(&page) {
            self.inner.views[index].tabs.set_selected_page(&tab_page);
        }
        self.refresh_page(&page);
        page
    }

    /// Starts comparing `new` with `old`, once both have their text; `temporary` close with
    /// the comparison.
    async fn start_compare(&self, new: EditorPage, old: EditorPage, temporary: Vec<EditorPage>) {
        let mut kept = temporary.clone();
        kept.extend([new.clone(), old.clone()]);
        self.clear_compare_keeping(&kept);
        for page in [&new, &old] {
            if !self.ensure_loaded(page).await {
                return;
            }
            let started = Instant::now();
            while page.is_loading() && started.elapsed() < Duration::from_secs(30) {
                glib::timeout_future(Duration::from_millis(10)).await;
            }
            if page.large_file_mode() {
                self.toast("Large files can't be compared");
                return;
            }
        }
        let mut handlers = Vec::new();
        for page in [&new, &old] {
            let buffer = page.buffer();
            let handler = buffer.connect_changed(glib::clone!(
                #[weak(rename_to = inner)]
                self.inner,
                move |_| Window { inner }.schedule_recompare()
            ));
            handlers.push((buffer, handler));
            for view in page.document_pages() {
                view.set_compared(true);
            }
        }
        self.inner.comparing.active.replace(Some(Comparing {
            old,
            new,
            comparison: None,
            temporary,
            handlers,
            runs: 0,
        }));
        self.recompare();
        self.update_compare_actions();
    }

    /// Runs the comparison again shortly after the last edit.
    fn schedule_recompare(&self) {
        let state = &self.inner.comparing;
        if let Some(source) = state.pending.take() {
            source.remove();
        }
        let weak = Rc::downgrade(&self.inner);
        let source = glib::timeout_add_local_once(RECOMPARE_DELAY, move || {
            if let Some(inner) = weak.upgrade() {
                inner.comparing.pending.replace(None);
                Window { inner }.recompare();
            }
        });
        state.pending.replace(Some(source));
    }

    /// Diffs snapshots of both documents on a worker and shows the result, unless either
    /// changed meanwhile or a newer run started.
    fn recompare(&self) {
        let state = &self.inner.comparing;
        let Some((old, new)) = state
            .active
            .borrow()
            .as_ref()
            .map(|active| (active.old.clone(), active.new.clone()))
        else {
            return;
        };
        let started = Instant::now();
        let generation = state.generation.get() + 1;
        state.generation.set(generation);
        state.running.set(true);
        let (old_text, new_text) = (old.text(), new.text());
        let snapshot = started.elapsed();
        let revisions = (old.revision(), new.revision());
        let options = self.compare_options();
        let window = self.clone();
        self.spawn(async move {
            let diffing = Instant::now();
            let result = worker::run(move || {
                let lines = (line_count(&old_text), line_count(&new_text));
                (diff::compare(&old_text, &new_text, &options), lines)
            })
            .await;
            let diff_time = diffing.elapsed();
            let state = &window.inner.comparing;
            if state.generation.get() != generation {
                return;
            }
            state.running.set(false);
            let Some((comparison, lines)) = result else {
                window.toast("The comparison failed");
                return;
            };
            if (old.revision(), new.revision()) != revisions {
                return;
            }
            let presenting = Instant::now();
            let comparison = Rc::new(comparison);
            crate::compare::present(&old, &comparison, Side::Left);
            crate::compare::present(&new, &comparison, Side::Right);
            let present = presenting.elapsed();
            let runs = {
                let mut active = state.active.borrow_mut();
                let Some(active) = active.as_mut() else {
                    return;
                };
                active.comparison = Some(comparison.clone());
                active.runs += 1;
                active.runs
            };
            let summary = comparison.stats.summary();
            window.inner.status.set_compare(Some(&summary));
            state.last.set(Some(CompareRun {
                stats: comparison.stats,
                lines,
                snapshot,
                diff: diff_time,
                present,
                total: started.elapsed(),
                painted: None,
                runs,
            }));
            window.note_painted(generation, started);
            if runs == 1 {
                window.toast(&summary);
                window.show_first_difference(&new, &comparison);
            }
            window.level_views();
            window.update_compare_actions();
        });
    }

    /// Records when the first frame after run `generation`'s presentation is painted.
    fn note_painted(&self, generation: u64, started: Instant) {
        let Some(clock) = self.inner.window.frame_clock() else {
            return;
        };
        let handler = Rc::new(RefCell::new(None));
        let id = clock.connect_after_paint(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            #[strong]
            handler,
            move |clock| {
                if let Some(id) = handler.take() {
                    clock.disconnect(id);
                }
                let state = &inner.comparing;
                if state.generation.get() == generation
                    && let Some(mut run) = state.last.get()
                {
                    run.painted = Some(started.elapsed());
                    state.last.set(Some(run));
                }
            }
        ));
        handler.replace(Some(id));
        self.inner.window.queue_draw();
    }

    /// The first run puts the new document's caret on the first difference.
    fn show_first_difference(&self, new: &EditorPage, comparison: &Comparison) {
        let Some(hunk) = comparison.hunks.first() else {
            return;
        };
        let buffer = new.buffer();
        let line = hunk
            .right
            .start
            .min(buffer.line_count().max(1) as usize - 1);
        if let Some(iter) = buffer.iter_at_line(line as i32) {
            buffer.place_cursor(&iter);
            new.view()
                .scroll_to_mark(&buffer.get_insert(), 0.1, true, 0.0, 0.3);
        }
    }

    /// Puts the other view level with the active one, once both are laid out.
    fn level_views(&self) {
        let weak = Rc::downgrade(&self.inner);
        glib::idle_add_local_once(move || {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let window = Window { inner };
            if !window.comparison_shown() {
                return;
            }
            let active = window.active_view();
            let (Some(from), Some(to)) = (window.view_page(active), window.view_page(1 - active))
            else {
                return;
            };
            if let (Some(source), Some(target)) =
                (from.view().vadjustment(), to.view().vadjustment())
            {
                target.set_value(source.value());
            }
            window.reset_scroll_sync();
        });
    }

    /// Which side of the comparison `page` shows, and the comparison.
    fn compare_side(&self, page: &EditorPage) -> Option<(Side, Rc<Comparison>)> {
        let active = self.inner.comparing.active.borrow();
        let active = active.as_ref()?;
        let comparison = active.comparison.clone()?;
        if page.same_document(&active.new) {
            Some((Side::Right, comparison))
        } else if page.same_document(&active.old) {
            Some((Side::Left, comparison))
        } else {
            None
        }
    }

    /// Next Difference (`forward`) or Previous Difference, wrapping around; a jump to come back
    /// from.
    fn step_difference(&self, forward: bool) {
        let Some(page) = self.current_page() else {
            return;
        };
        let Some((side, comparison)) = self.compare_side(&page) else {
            self.toast("This document is not in the comparison");
            return;
        };
        let count = comparison.hunks.len();
        if count == 0 {
            self.toast("No differences");
            return;
        }
        let buffer = page.buffer();
        let line = buffer.iter_at_mark(&buffer.get_insert()).line().max(0) as usize;
        let last = buffer.line_count().max(1) as usize - 1;
        let target = |index: usize| comparison.hunks[index].lines(side).start.min(last);
        let mut index = if forward {
            comparison.next_hunk(side, line).unwrap_or(0)
        } else {
            comparison.prev_hunk(side, line).unwrap_or(count - 1)
        };
        // A place where the other side has lines this one lacks is on the line after it:
        // from there, the difference to go to is the one beyond.
        if target(index) == line {
            index = if forward {
                (index + 1) % count
            } else {
                (index + count - 1) % count
            };
        }
        let target = target(index);
        self.note_jump();
        if let Some(iter) = buffer.iter_at_line(target as i32) {
            buffer.place_cursor(&iter);
            page.view()
                .scroll_to_mark(&buffer.get_insert(), 0.1, true, 0.0, 0.3);
        }
        page.view().grab_focus();
    }

    /// Clear Compare: the documents lose the comparison, and the texts it opened close.
    pub(super) fn clear_compare(&self) {
        self.clear_compare_keeping(&[]);
    }

    /// Clears the comparison, closing the texts it opened except those in `keep`.
    fn clear_compare_keeping(&self, keep: &[EditorPage]) {
        let state = &self.inner.comparing;
        state.generation.set(state.generation.get() + 1);
        state.running.set(false);
        if let Some(source) = state.pending.take() {
            source.remove();
        }
        let Some(active) = state.active.take() else {
            return;
        };
        for (buffer, handler) in active.handlers {
            buffer.disconnect(handler);
        }
        for page in [&active.old, &active.new] {
            crate::compare::clear(page);
            for view in page.document_pages() {
                view.set_compared(false);
            }
        }
        for page in active.temporary {
            if !keep.contains(&page) && self.tab_page(&page).is_some() {
                self.close_without_prompt(&page);
            }
        }
        self.inner.status.set_compare(None);
        self.update_compare_actions();
    }

    /// A tab closed: a compared document that is gone ends the comparison.
    pub(super) fn compare_page_closed(&self, page: &EditorPage) {
        let involved = self
            .inner
            .comparing
            .active
            .borrow()
            .as_ref()
            .is_some_and(|active| {
                page.same_document(&active.old) || page.same_document(&active.new)
            });
        if involved && self.other_page(page).is_none() {
            self.clear_compare();
        }
    }

    pub(super) fn update_compare_actions(&self) {
        let page = self.current_page();
        let comparing = self.comparing();
        let other = self.is_split() || self.documents().len() > 1;
        for (id, enabled) in [
            (ActionId::Compare, page.is_some() && other),
            (ActionId::CompareWithFile, page.is_some()),
            (ActionId::CompareWithClipboard, page.is_some()),
            (
                ActionId::CompareWithSaved,
                page.as_ref().is_some_and(|page| page.path().is_some()),
            ),
            (ActionId::NextDifference, comparing),
            (ActionId::PreviousDifference, comparing),
            (ActionId::ClearCompare, comparing),
        ] {
            if let Some(action) = self.action(id) {
                action.set_enabled(enabled);
            }
        }
    }

    // ----- for the self-test -----------------------------------------------------------

    /// The last comparison run.
    pub fn compare_run(&self) -> Option<CompareRun> {
        self.inner.comparing.last.get()
    }

    /// Whether a comparison run is due or on its way.
    pub fn compare_busy(&self) -> bool {
        let state = &self.inner.comparing;
        state.running.get() || state.pending.borrow().is_some()
    }

    /// The comparison on screen, with the old and the new document.
    pub fn comparison(&self) -> Option<(EditorPage, EditorPage, Rc<Comparison>)> {
        let active = self.inner.comparing.active.borrow();
        let active = active.as_ref()?;
        Some((
            active.old.clone(),
            active.new.clone(),
            active.comparison.clone()?,
        ))
    }
}
