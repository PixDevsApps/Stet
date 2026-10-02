//! Quick open (Ctrl+P, M8): a popup like the command palette that lists the open documents
//! (most recently used first), then the recent files, then the files under the project root
//! (the git root of the current document's folder, else that folder), ranked by
//! `stet_domain::fuzzy` with the matched characters in bold. `name:line[:col]` opens at a
//! position and `:line` goes to a line of the current document.
//!
//! Nothing here blocks typing: the project's files come from a walk on a thread of their own
//! (`stet_infrastructure::project`, capped at [`PROJECT_FILE_CAP`]) and stream in batches; a
//! ranking thread ranks a snapshot of the candidates for the newest query, and a ranking for
//! another query than the one in the field is dropped.

use super::{Location, Window};
use crate::editor::EditorPage;
use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, glib, pango};
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use stet_domain::document::tilde;
use stet_domain::location::LineCol;
use stet_domain::quick_open::{
    PROJECT_FILE_CAP, Request, Row, SHOWN_LIMIT, Source, rank_groups, runs,
};
use stet_infrastructure::project::{WalkSummary, project_root, walk_project};

/// How often streamed files are ranked again while the walk goes on.
const RERANK_INTERVAL: Duration = Duration::from_millis(60);

/// Where a candidate leads.
#[derive(Clone)]
pub enum Target {
    Page(glib::WeakRef<EditorPage>),
    Path(PathBuf),
}

/// What Enter chose.
pub enum Choice {
    /// A file or open document, at a position when the query had one.
    Open {
        target: Target,
        position: Option<LineCol>,
    },
    /// `:line[:col]` in the current document.
    Line(LineCol),
}

/// Timings of the last opening, for the self-test's report.
#[derive(Debug, Clone, Copy, Default)]
pub struct QuickOpenStats {
    /// The first rows on screen, from the popup's opening.
    pub first_rows: Option<Duration>,
    /// The first rows on screen that included project files.
    pub first_project_rows: Option<Duration>,
    pub walk: Option<WalkSummary>,
    /// The project files listed so far.
    pub files: usize,
    /// The longest ranking so far, on the ranking thread.
    pub slowest_rank: Duration,
    /// From the newest request to its rows on screen, once it was shown.
    pub last_rows: Option<Duration>,
    /// Rankings shown.
    pub rankings: usize,
}

struct RankRequest {
    generation: u64,
    pattern: String,
    open: Arc<Vec<String>>,
    recent: Arc<Vec<String>>,
    project: Vec<Arc<Vec<String>>>,
}

struct RankReply {
    generation: u64,
    pattern: String,
    rows: Vec<Row>,
    took: Duration,
}

enum WalkMessage {
    Batch(u64, Vec<(PathBuf, String)>),
    Done(u64, WalkSummary),
}

#[derive(Default)]
struct State {
    open: Arc<Vec<String>>,
    open_targets: Vec<Target>,
    recent: Arc<Vec<String>>,
    recent_paths: Vec<PathBuf>,
    project: Vec<Arc<Vec<String>>>,
    project_paths: Vec<Arc<Vec<PathBuf>>>,
    /// Open and recent files, which the project's list leaves out.
    known: HashSet<PathBuf>,
    root: Option<PathBuf>,
    walk_id: u64,
    walk_cancel: Option<Arc<AtomicBool>>,
    walking: bool,
    generation: u64,
    shown_generation: u64,
    /// The newest ranking asked for has been shown.
    settled: bool,
    /// The field's text the newest ranking was asked for.
    requested: String,
    rows: Vec<Row>,
    opened: Option<Instant>,
    last_request: Option<Instant>,
    rerank_pending: Option<glib::SourceId>,
    stats: QuickOpenStats,
}

type OnChoose = Box<dyn Fn(Choice)>;

pub struct QuickOpen {
    pub popover: gtk::Popover,
    pub entry: gtk::SearchEntry,
    list: gtk::ListBox,
    scrolled: gtk::ScrolledWindow,
    /// What the walk has found, under the list.
    pub note: gtk::Label,
    state: RefCell<State>,
    requests: mpsc::Sender<RankRequest>,
    on_choose: RefCell<Option<OnChoose>>,
}

impl QuickOpen {
    pub fn new() -> Rc<Self> {
        let entry = gtk::SearchEntry::builder()
            .placeholder_text("Open a file by name; name:line opens at a line, :line goes to one")
            .search_delay(0)
            .hexpand(true)
            .build();
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Browse)
            .build();
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(480)
            .build();
        let note = gtk::Label::builder()
            .xalign(0.0)
            .ellipsize(pango::EllipsizeMode::Start)
            .css_classes(["stet-quick-open-note", "dim-label"])
            .build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&entry);
        content.append(&scrolled);
        content.append(&note);
        let popover = gtk::Popover::builder()
            .child(&content)
            .has_arrow(false)
            .position(gtk::PositionType::Bottom)
            .build();
        popover.add_css_class("stet-palette");
        popover.add_css_class("stet-quick-open");
        popover.add_css_class("stet-mono");
        let (requests, receiver) = mpsc::channel::<RankRequest>();
        let (replies, reply_receiver) = async_channel::unbounded::<RankReply>();
        let spawned = std::thread::Builder::new()
            .name("stet-quick-open-rank".into())
            .spawn(move || rank_loop(receiver, replies));
        if let Err(error) = spawned {
            tracing::error!(%error, "could not start the quick open ranking thread");
        }
        let quick_open = Rc::new(Self {
            popover,
            entry,
            list,
            scrolled,
            note,
            state: RefCell::new(State::default()),
            requests,
            on_choose: RefCell::new(None),
        });
        quick_open.connect_signals();
        let weak = Rc::downgrade(&quick_open);
        glib::spawn_future_local(async move {
            while let Ok(reply) = reply_receiver.recv().await {
                let Some(quick_open) = weak.upgrade() else {
                    break;
                };
                quick_open.show_reply(reply);
            }
        });
        quick_open
    }

    pub fn connect_choose(&self, on_choose: impl Fn(Choice) + 'static) {
        self.on_choose.replace(Some(Box::new(on_choose)));
    }

    fn connect_signals(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.entry.connect_search_changed(move |_| {
            if let Some(quick_open) = weak.upgrade() {
                quick_open.request_rank();
            }
        });
        let weak = Rc::downgrade(self);
        self.entry.connect_activate(move |_| {
            if let Some(quick_open) = weak.upgrade() {
                quick_open.activate_selected();
            }
        });
        let weak = Rc::downgrade(self);
        self.entry.connect_stop_search(move |_| {
            if let Some(quick_open) = weak.upgrade() {
                quick_open.popover.popdown();
            }
        });
        let weak = Rc::downgrade(self);
        self.list.connect_row_activated(move |_, row| {
            if let Some(quick_open) = weak.upgrade() {
                quick_open.activate_row(row.index());
            }
        });
        let weak = Rc::downgrade(self);
        self.popover.connect_closed(move |_| {
            if let Some(quick_open) = weak.upgrade() {
                quick_open.stop_walk();
            }
        });
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(quick_open) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            match key {
                gdk::Key::Down => quick_open.move_selection(1),
                gdk::Key::Up => quick_open.move_selection(-1),
                gdk::Key::Page_Down => quick_open.move_selection(10),
                gdk::Key::Page_Up => quick_open.move_selection(-10),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        self.entry.add_controller(keys);
    }

    /// Opens over `parent`, top-centre, with the open documents and recent files, and starts
    /// listing the files under `root`.
    fn open(
        self: &Rc<Self>,
        parent: &gtk::Widget,
        open: Vec<(String, Target)>,
        recent: Vec<(String, PathBuf)>,
        known: HashSet<PathBuf>,
        root: Option<PathBuf>,
        text: &str,
    ) {
        if self.popover.parent().as_ref() != Some(parent) {
            if self.popover.parent().is_some() {
                self.popover.unparent();
            }
            self.popover.set_parent(parent);
        }
        let width = parent.width().max(1);
        self.popover
            .set_pointing_to(Some(&gdk::Rectangle::new(width / 2, 8, 1, 1)));
        self.stop_walk();
        {
            let mut state = self.state.borrow_mut();
            let (open_labels, open_targets): (Vec<String>, Vec<Target>) = open.into_iter().unzip();
            let (recent_labels, recent_paths): (Vec<String>, Vec<PathBuf>) =
                recent.into_iter().unzip();
            *state = State {
                open: Arc::new(open_labels),
                open_targets,
                recent: Arc::new(recent_labels),
                recent_paths,
                known,
                root: root.clone(),
                walk_id: state.walk_id + 1,
                generation: state.generation,
                shown_generation: state.generation,
                opened: Some(Instant::now()),
                ..State::default()
            };
        }
        self.entry.set_text(text);
        self.request_rank();
        self.popover.popup();
        self.entry.grab_focus();
        self.entry.set_position(-1);
        match root {
            Some(root) => self.start_walk(root),
            None => self
                .note
                .set_label("Open a file from a folder to list its project's files"),
        }
    }

    fn start_walk(self: &Rc<Self>, root: PathBuf) {
        let cancel = Arc::new(AtomicBool::new(false));
        let id = {
            let mut state = self.state.borrow_mut();
            state.walk_cancel = Some(Arc::clone(&cancel));
            state.walking = true;
            state.walk_id
        };
        let home = std::env::home_dir();
        self.note.set_label(&format!(
            "Listing the files in {}…",
            tilde(&root, home.as_deref())
        ));
        let (sender, receiver) = async_channel::unbounded::<WalkMessage>();
        let spawned = std::thread::Builder::new()
            .name("stet-quick-open-walk".into())
            .spawn(move || {
                let summary = walk_project(&root, PROJECT_FILE_CAP, &cancel, |batch| {
                    let files = batch
                        .into_iter()
                        .map(|file| (file.path, file.relative))
                        .collect();
                    let _ = sender.send_blocking(WalkMessage::Batch(id, files));
                });
                let _ = sender.send_blocking(WalkMessage::Done(id, summary));
            });
        if let Err(error) = spawned {
            tracing::error!(%error, "could not start the quick open walk");
            return;
        }
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            while let Ok(message) = receiver.recv().await {
                let Some(quick_open) = weak.upgrade() else {
                    break;
                };
                if !quick_open.take_walk_message(message) {
                    break;
                }
            }
        });
    }

    /// Adds a batch of the walk's files, or ends the walk. False once the walk is over or
    /// another one has started.
    fn take_walk_message(self: &Rc<Self>, message: WalkMessage) -> bool {
        let current = self.state.borrow().walk_id;
        match message {
            WalkMessage::Batch(id, files) if id == current => {
                {
                    let mut state = self.state.borrow_mut();
                    let (paths, labels): (Vec<PathBuf>, Vec<String>) = files
                        .into_iter()
                        .filter(|(path, _)| !state.known.contains(path))
                        .unzip();
                    state.stats.files += labels.len();
                    state.project.push(Arc::new(labels));
                    state.project_paths.push(Arc::new(paths));
                }
                self.update_note();
                self.rerank_soon();
                true
            }
            WalkMessage::Done(id, summary) if id == current => {
                {
                    let mut state = self.state.borrow_mut();
                    state.walking = false;
                    state.stats.walk = Some(summary);
                }
                self.update_note();
                self.request_rank();
                false
            }
            _ => false,
        }
    }

    fn update_note(&self) {
        let state = self.state.borrow();
        let Some(root) = &state.root else {
            return;
        };
        let home = std::env::home_dir();
        let root = tilde(root, home.as_deref());
        let files = super::dialogs::grouped(state.stats.files);
        let label = match state.stats.walk {
            None => format!("Listing the files in {root}… {files} so far"),
            Some(summary) if summary.truncated => {
                format!("{root}: the first {files} files (more are left out)")
            }
            Some(_) => format!("{root}: {files} files"),
        };
        self.note.set_label(&label);
    }

    fn stop_walk(&self) {
        let mut state = self.state.borrow_mut();
        if let Some(cancel) = state.walk_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        state.walking = false;
        if let Some(source) = state.rerank_pending.take() {
            source.remove();
        }
    }

    /// Ranks again at most every [`RERANK_INTERVAL`] while files stream in.
    fn rerank_soon(self: &Rc<Self>) {
        let wait = {
            let state = self.state.borrow();
            if state.rerank_pending.is_some() {
                return;
            }
            state.last_request.map_or(Duration::ZERO, |last| {
                RERANK_INTERVAL.saturating_sub(last.elapsed())
            })
        };
        if wait.is_zero() {
            self.request_rank();
            return;
        }
        let weak = Rc::downgrade(self);
        let source = glib::timeout_add_local_once(wait, move || {
            if let Some(quick_open) = weak.upgrade() {
                quick_open.state.borrow_mut().rerank_pending = None;
                quick_open.request_rank();
            }
        });
        self.state.borrow_mut().rerank_pending = Some(source);
    }

    /// Asks the ranking thread for the field's query over everything listed so far.
    fn request_rank(&self) {
        let text = self.entry.text().to_string();
        let request = {
            let mut state = self.state.borrow_mut();
            state.generation += 1;
            state.settled = false;
            state.requested = text.clone();
            state.last_request = Some(Instant::now());
            let pattern = match Request::parse(&text) {
                Request::File { pattern, .. } => pattern,
                Request::Line(_) => String::new(),
            };
            RankRequest {
                generation: state.generation,
                pattern,
                open: Arc::clone(&state.open),
                recent: Arc::clone(&state.recent),
                project: state.project.clone(),
            }
        };
        if let Request::Line(target) = Request::parse(&text) {
            self.show_line_row(target);
            let mut state = self.state.borrow_mut();
            state.shown_generation = state.generation;
            state.settled = true;
            return;
        }
        let _ = self.requests.send(request);
    }

    fn current_pattern(&self) -> String {
        match Request::parse(&self.entry.text()) {
            Request::File { pattern, .. } => pattern,
            Request::Line(_) => String::new(),
        }
    }

    fn show_reply(&self, reply: RankReply) {
        if matches!(Request::parse(&self.entry.text()), Request::Line(_))
            || reply.pattern != self.current_pattern()
        {
            return;
        }
        {
            let mut state = self.state.borrow_mut();
            if reply.generation <= state.shown_generation {
                return;
            }
            state.shown_generation = reply.generation;
            state.settled = reply.generation == state.generation;
            if state.settled {
                state.stats.last_rows = state.last_request.map(|asked| asked.elapsed());
            }
            state.stats.slowest_rank = state.stats.slowest_rank.max(reply.took);
            state.stats.rankings += 1;
            if let Some(opened) = state.opened {
                state
                    .stats
                    .first_rows
                    .get_or_insert_with(|| opened.elapsed());
                if reply.rows.iter().any(|row| row.source == Source::Project) {
                    state
                        .stats
                        .first_project_rows
                        .get_or_insert_with(|| opened.elapsed());
                }
            }
        }
        self.fill(reply.rows);
    }

    fn fill(&self, rows: Vec<Row>) {
        self.list.remove_all();
        {
            let state = self.state.borrow();
            for row in &rows {
                let (text, detail) = match row.source {
                    Source::Open => (state.open[row.index].as_str(), "open"),
                    Source::Recent => (state.recent[row.index].as_str(), "recent"),
                    Source::Project => (project_label(&state, row.index), ""),
                };
                self.list.append(&list_row(text, &row.positions, detail));
            }
        }
        if rows.is_empty() {
            let empty = gtk::Label::builder()
                .label("No file matches")
                .css_classes(["dim-label"])
                .build();
            self.list.append(
                &gtk::ListBoxRow::builder()
                    .child(&empty)
                    .activatable(false)
                    .selectable(false)
                    .build(),
            );
        } else if let Some(first) = self.list.row_at_index(0) {
            self.list.select_row(Some(&first));
        }
        self.state.borrow_mut().rows = rows;
    }

    fn show_line_row(&self, target: Option<LineCol>) {
        self.list.remove_all();
        let label = match target {
            Some(LineCol { line, column: None }) => format!("Go to line {line}"),
            Some(LineCol {
                line,
                column: Some(column),
            }) => format!("Go to line {line}, column {column}"),
            None => "Type a line number, or line:column".to_owned(),
        };
        let row = list_row(&label, &[], "");
        row.set_activatable(target.is_some());
        self.list.append(&row);
        self.list.select_row(Some(&row));
        self.state.borrow_mut().rows = Vec::new();
    }

    fn move_selection(&self, step: i32) {
        let count = self.state.borrow().rows.len() as i32;
        if count == 0 {
            return;
        }
        let current = self.list.selected_row().map_or(0, |row| row.index());
        let next = (current + step).clamp(0, count - 1);
        if let Some(row) = self.list.row_at_index(next) {
            self.list.select_row(Some(&row));
            super::language::scroll_to(&self.scrolled, &self.list, &row);
        }
    }

    /// Selects row `index`, as Down and Up do, for the self-test.
    pub fn select_row(&self, index: usize) -> Result<(), String> {
        let row = i32::try_from(index)
            .ok()
            .and_then(|index| self.list.row_at_index(index))
            .ok_or_else(|| format!("there is no row {index}"))?;
        self.list.select_row(Some(&row));
        Ok(())
    }

    /// Runs the selected row, as Enter does.
    pub fn activate_selected(&self) {
        let index = self.list.selected_row().map_or(0, |row| row.index());
        self.activate_row(index);
    }

    fn activate_row(&self, index: i32) {
        let request = Request::parse(&self.entry.text());
        let choice = match request {
            Request::Line(Some(target)) => Choice::Line(target),
            Request::Line(None) => return,
            Request::File { position, .. } => {
                let state = self.state.borrow();
                let Some(row) = usize::try_from(index)
                    .ok()
                    .and_then(|index| state.rows.get(index))
                else {
                    return;
                };
                let target = match row.source {
                    Source::Open => state.open_targets[row.index].clone(),
                    Source::Recent => Target::Path(state.recent_paths[row.index].clone()),
                    Source::Project => Target::Path(project_path(&state, row.index).to_path_buf()),
                };
                Choice::Open { target, position }
            }
        };
        self.popover.popdown();
        if let Some(on_choose) = self.on_choose.borrow().as_ref() {
            on_choose(choice);
        }
    }

    /// The rows shown, as their text, for the self-test.
    pub fn shown_labels(&self) -> Vec<String> {
        let state = self.state.borrow();
        state
            .rows
            .iter()
            .map(|row| match row.source {
                Source::Open => state.open[row.index].clone(),
                Source::Recent => state.recent[row.index].clone(),
                Source::Project => project_label(&state, row.index).to_owned(),
            })
            .collect()
    }

    /// The matched characters of shown row `index`.
    pub fn shown_positions(&self, index: usize) -> Option<Vec<usize>> {
        self.state
            .borrow()
            .rows
            .get(index)
            .map(|row| row.positions.clone())
    }

    /// The walk is over and the ranking for the field's text is on screen.
    pub fn is_settled(&self) -> bool {
        let state = self.state.borrow();
        state.settled && !state.walking && state.requested == self.entry.text().as_str()
    }

    pub fn stats(&self) -> QuickOpenStats {
        self.state.borrow().stats
    }
}

/// The project file at `index` of the concatenated batches.
fn project_label(state: &State, mut index: usize) -> &str {
    for chunk in &state.project {
        if index < chunk.len() {
            return &chunk[index];
        }
        index -= chunk.len();
    }
    ""
}

fn project_path(state: &State, mut index: usize) -> &Path {
    for chunk in &state.project_paths {
        if index < chunk.len() {
            return &chunk[index];
        }
        index -= chunk.len();
    }
    Path::new("")
}

/// A row: the candidate with its matched characters in bold, and where it comes from.
fn list_row(text: &str, positions: &[usize], detail: &str) -> gtk::ListBoxRow {
    let markup = highlighted(text, positions);
    let label = gtk::Label::builder()
        .use_markup(true)
        .label(&markup)
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(pango::EllipsizeMode::Start)
        .build();
    let detail = gtk::Label::builder()
        .label(detail)
        .xalign(1.0)
        .css_classes(["stet-detail"])
        .build();
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    content.append(&label);
    content.append(&detail);
    gtk::ListBoxRow::builder().child(&content).build()
}

/// `text` as Pango markup with the characters at `positions` in bold.
fn highlighted(text: &str, positions: &[usize]) -> String {
    let bytes: Vec<usize> = text
        .char_indices()
        .map(|(index, _)| index)
        .chain([text.len()])
        .collect();
    let mut out = String::with_capacity(text.len() + positions.len() * 8);
    let mut copied = 0;
    for run in runs(positions) {
        let (Some(&start), Some(&end)) = (bytes.get(run.start), bytes.get(run.end)) else {
            continue;
        };
        out.push_str(&glib::markup_escape_text(&text[copied..start]));
        out.push_str("<b>");
        out.push_str(&glib::markup_escape_text(&text[start..end]));
        out.push_str("</b>");
        copied = end;
    }
    out.push_str(&glib::markup_escape_text(&text[copied..]));
    out
}

/// The ranking thread: always ranks the newest request waiting, so typing never queues work.
fn rank_loop(requests: mpsc::Receiver<RankRequest>, replies: async_channel::Sender<RankReply>) {
    while let Ok(mut request) = requests.recv() {
        while let Ok(newer) = requests.try_recv() {
            request = newer;
        }
        let started = Instant::now();
        let project: Vec<&str> = request
            .project
            .iter()
            .flat_map(|chunk| chunk.iter().map(String::as_str))
            .collect();
        let rows = rank_groups(
            &request.pattern,
            &request.open,
            &request.recent,
            &project,
            SHOWN_LIMIT,
        );
        let reply = RankReply {
            generation: request.generation,
            pattern: request.pattern,
            rows,
            took: started.elapsed(),
        };
        if replies.send_blocking(reply).is_err() {
            break;
        }
    }
}

impl Window {
    pub(super) fn connect_quick_open(&self) {
        let quick_open = &self.inner.nav.quick_open;
        quick_open.connect_choose(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |choice| Window { inner }.quick_open_chosen(choice)
        ));
        quick_open.popover.connect_closed(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |_| Window { inner }.restore_focus_later()
        ));
    }

    /// Quick Open (Ctrl+P), with `text` in its field.
    pub fn open_quick_open(&self, text: &str) {
        self.open_quick_open_from(text, None);
    }

    /// Quick Open listing the project of `folder`, or when `None` of the current document's
    /// folder, else of the most recently used one with a file.
    pub fn open_quick_open_from(&self, text: &str, folder: Option<PathBuf>) {
        let home = std::env::home_dir();
        let current = self.current_page();
        let mut known = HashSet::new();
        let mut open = Vec::new();
        let order = self.pages_by_recent_use();
        for page in &order {
            let label = match page.path() {
                Some(path) => {
                    known.insert(path.clone());
                    if let Some(canonical) = page.state().canonical {
                        known.insert(canonical);
                    }
                    tilde(&path, home.as_deref())
                }
                None => page.name(),
            };
            open.push((label, Target::Page(page.downgrade())));
        }
        let recent: Vec<(String, PathBuf)> = self
            .inner
            .shared
            .recent()
            .paths()
            .iter()
            .filter(|path| !known.contains(*path))
            .map(|path| (tilde(path, home.as_deref()), path.clone()))
            .collect();
        known.extend(recent.iter().map(|(_, path)| path.clone()));
        let folder = folder.or_else(|| {
            current
                .iter()
                .chain(order.iter())
                .find_map(|page| page.path())
                .or_else(|| self.inner.shared.recent().paths().first().cloned())
                .and_then(|path| path.parent().map(Path::to_path_buf))
        });
        let quick_open = Rc::clone(&self.inner.nav.quick_open);
        let parent: gtk::Widget = self.inner.content.clone().upcast();
        match folder {
            Some(folder) => {
                // Finding the project root looks at the disk: on a worker, then the popup.
                let text = text.to_owned();
                self.spawn(async move {
                    let root = crate::worker::run(move || project_root(&folder)).await;
                    quick_open.open(&parent, open, recent, known, root, &text);
                });
            }
            None => quick_open.open(&parent, open, recent, known, None, text),
        }
    }

    /// The tabs, most recently used first.
    fn pages_by_recent_use(&self) -> Vec<EditorPage> {
        let mut order: Vec<EditorPage> = Vec::new();
        // A document in both views (M7) is listed once.
        for page in self.recent_pages().into_iter().chain(self.pages()) {
            if !order.iter().any(|known| known.same_document(&page)) {
                order.push(page);
            }
        }
        order
    }

    fn quick_open_chosen(&self, choice: Choice) {
        self.note_jump();
        match choice {
            Choice::Line(target) => {
                if let Some(page) = self.current_page() {
                    page.go_to(target.position());
                    page.view().grab_focus();
                }
            }
            Choice::Open {
                target: Target::Page(page),
                position,
            } => {
                if let Some(page) = page.upgrade().filter(|page| self.tab_page(page).is_some()) {
                    self.select(&page);
                    if let Some(position) = position {
                        page.go_to(position.position());
                    }
                    self.focus_editor();
                }
            }
            Choice::Open {
                target: Target::Path(path),
                position,
            } => {
                self.open_locations(vec![Location {
                    path,
                    position,
                    create: false,
                }]);
            }
        }
    }
}
