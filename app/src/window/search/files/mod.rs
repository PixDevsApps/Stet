//! Find in Files and Replace in Files: the find bar's files row starts a job on threads of its
//! own (`stet_infrastructure::files`), and its events stream into the results panel about 30
//! times a second as a [`StreamingRun`]. What the two runs share lives here: the checks
//! before a run, the folder field's default and chooser, and the run itself, from the job's
//! channel to the panel's status line and the Stop button. Each run's own part is a [`Feed`]
//! ([`find`] and [`replace`]).

mod find;
mod replace;

use super::bar::{BarMode, FilesRequest, Tone};
use super::results::{FileNode, SearchNode};
use crate::runtime::{Latest, Ticket, worker};
use crate::window::Window;
use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gio, glib};
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};
use stet_domain::actions::ActionId;
use stet_domain::document::tilde;
use stet_infrastructure::files::{JobHandle, StartError};

/// How often a run's events reach the panel: about 30 times a second.
const DRAIN_INTERVAL: Duration = Duration::from_millis(33);
/// About the most result lines added to the panel per drain (whole files at a time), so a
/// burst never stalls a frame.
const LINES_PER_DRAIN: usize = 1_000;

/// The window's files searches: the Find in Files and the Replace in Files run, each with how
/// its last run went.
#[derive(Default)]
pub struct FilesSearch {
    find: RunSlot<find::FindFeed>,
    replace: RunSlot<replace::ReplaceFeed>,
}

/// Where one kind of run lives: the run in progress, the tickets that tell the newest run, and
/// how the last run went.
struct RunSlot<F: Feed> {
    run: RefCell<Option<StreamingRun<F>>>,
    runs: Latest,
    stats: RefCell<Option<F::Stats>>,
}

impl<F: Feed> Default for RunSlot<F> {
    fn default() -> Self {
        Self {
            run: RefCell::new(None),
            runs: Latest::default(),
            stats: RefCell::new(None),
        }
    }
}

/// What one kind of run adds to a [`StreamingRun`]: how it takes the job's events, what the
/// panel says while it runs, and how it ends.
trait Feed: Sized + 'static {
    type Event: Send + 'static;
    /// What the job returns.
    type Summary: Send + 'static;
    /// What the job shares while it runs.
    type Progress: 'static;
    /// How the run went, for the self-test's report.
    type Stats: Clone;
    /// Once the job has ended after Stop, the events still waiting are dropped, so the panel
    /// stops at once.
    const DROPS_AFTER_STOP: bool;

    fn slot(files: &FilesSearch) -> &RunSlot<Self>;

    /// Takes one event: the row it adds to the panel, if any, with the lines it costs.
    fn absorb(&mut self, event: Self::Event, stopped: bool) -> Option<(FileNode, usize)>;

    /// The run's heading in the panel; `searched` counts the files searched once it is done.
    fn heading(run: &StreamingRun<Self>, searched: Option<usize>) -> String;

    /// The status line after a drain that `added` rows, while the job has not `finished`;
    /// `None` keeps it.
    fn progress(run: &StreamingRun<Self>, added: bool, finished: bool) -> Option<String>;

    /// Work of the run's own still to start after a drain.
    fn advance(_run: &mut StreamingRun<Self>, _window: &Window, _ticket: &Ticket) {}

    /// The run has work of its own that is not done yet.
    fn busy(_run: &StreamingRun<Self>) -> bool {
        false
    }

    /// Stop was asked for.
    fn stop(&mut self) {}

    /// Ends the run in the panel and the bar, once the job, its events and the run's own work
    /// are done: returns how it went.
    fn finish(run: StreamingRun<Self>, window: &Window) -> Self::Stats;
}

/// A files search streaming into the results panel: the job, the channel its events come
/// through, the panel's search, Stop, and the feature's [`Feed`].
struct StreamingRun<F: Feed> {
    handle: JobHandle<F::Summary, F::Progress>,
    receiver: async_channel::Receiver<F::Event>,
    search: Rc<SearchNode>,
    started: Instant,
    stop_requested: Option<Instant>,
    /// How long after Stop the job's threads had ended, as the panel saw it.
    ended_after_stop: Option<Duration>,
    /// When the panel first showed rows.
    first_shown: Option<Duration>,
    /// The longest time one drain spent adding rows to the panel.
    slowest_drain: Duration,
    feed: F,
}

impl Window {
    /// Starts a run: `start` starts the job with the channel's sender and the run's start
    /// time. The panel then shows a new search with `heading` and `status`, and the bar
    /// `message`; when the job cannot start, the bar says why.
    fn start_run<F: Feed>(
        &self,
        feed: F,
        heading: String,
        status: &str,
        message: &str,
        start: impl FnOnce(
            async_channel::Sender<F::Event>,
            Instant,
        ) -> Result<JobHandle<F::Summary, F::Progress>, StartError>,
    ) {
        let (sender, receiver) = async_channel::unbounded();
        let started = Instant::now();
        let handle = match start(sender, started) {
            Ok(handle) => handle,
            Err(error) => {
                self.inner.find.set_message(&error.to_string(), Tone::Error);
                return;
            }
        };
        let results = &self.inner.results;
        let search = results.begin(heading);
        results.set_status(status, None);
        results.set_stoppable(true);
        if let Some(action) = self.action(ActionId::StopFindInFiles) {
            action.set_enabled(true);
        }
        self.inner.find.set_message(message, Tone::Info);
        self.show_results(false);
        let slot = F::slot(&self.inner.files);
        let ticket = slot.runs.next();
        slot.stats.replace(None);
        slot.run.replace(Some(StreamingRun {
            handle,
            receiver,
            search,
            started,
            stop_requested: None,
            ended_after_stop: None,
            first_shown: None,
            slowest_drain: Duration::ZERO,
            feed,
        }));
        glib::timeout_add_local(
            DRAIN_INTERVAL,
            glib::clone!(
                #[weak(rename_to = inner)]
                self.inner,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move || Window { inner }.drain::<F>(&ticket)
            ),
        );
    }

    /// Moves the events waiting into the panel, and finishes the run once the job, its events
    /// and the feed's own work are done. Only `ticket`'s run, the newest, is in the slot.
    fn drain<F: Feed>(&self, ticket: &Ticket) -> glib::ControlFlow {
        let slot = F::slot(&self.inner.files);
        let mut current = slot.run.borrow_mut();
        let Some(run) = current.as_mut().filter(|_| ticket.is_current()) else {
            return glib::ControlFlow::Break;
        };
        let finished = run.handle.is_finished();
        if finished && let Some(stopped) = run.stop_requested {
            run.ended_after_stop
                .get_or_insert_with(|| stopped.elapsed());
            if F::DROPS_AFTER_STOP {
                while run.receiver.try_recv().is_ok() {}
            }
        }
        let drain_started = Instant::now();
        let mut rows = Vec::new();
        let mut lines = 0;
        while lines < LINES_PER_DRAIN {
            let Ok(event) = run.receiver.try_recv() else {
                break;
            };
            let stopped = run.stop_requested.is_some();
            if let Some((row, cost)) = run.feed.absorb(event, stopped) {
                lines += cost;
                rows.push(row);
            }
        }
        let results = &self.inner.results;
        let added = !rows.is_empty();
        if added {
            run.first_shown.get_or_insert_with(|| run.started.elapsed());
            results.add_files(&run.search, rows);
            results.set_heading(&run.search, F::heading(run, None));
        }
        if let Some(status) = F::progress(run, added, finished) {
            results.set_status_text(&status);
        }
        if added {
            run.slowest_drain = run.slowest_drain.max(drain_started.elapsed());
        }
        F::advance(run, self, ticket);
        if !finished || !run.receiver.is_empty() || F::busy(run) {
            return glib::ControlFlow::Continue;
        }
        let run = current.take().expect("checked above");
        drop(current);
        let stats = F::finish(run, self);
        results.set_stoppable(false);
        if let Some(action) = self.action(ActionId::StopFindInFiles) {
            action.set_enabled(self.files_search_running());
        }
        slot.stats.replace(Some(stats));
        glib::ControlFlow::Break
    }

    /// Stops `F`'s run, if one runs: the job stops, and what it reported so far stays in the
    /// panel.
    fn stop_run<F: Feed>(&self) {
        if let Some(run) = F::slot(&self.inner.files).run.borrow_mut().as_mut() {
            run.stop_requested.get_or_insert_with(Instant::now);
            run.feed.stop();
            run.handle.cancel();
        }
    }

    fn is_running<F: Feed>(&self) -> bool {
        F::slot(&self.inner.files).run.borrow().is_some()
    }

    /// Stop Find in Files: stops Find in Files and Replace in Files.
    pub(in crate::window) fn stop_files_searches(&self) {
        self.stop_find_in_files();
        self.stop_replace_in_files();
    }

    /// Whether Find in Files or Replace in Files runs.
    fn files_search_running(&self) -> bool {
        self.find_in_files_running() || self.replace_in_files_running()
    }

    /// Find in Files and Replace in Files need the files row on screen; otherwise the
    /// command opens it.
    fn files_row_shown(&self) -> bool {
        let find = &self.inner.find;
        if find.is_open() && find.bar_mode() == BarMode::Files {
            return true;
        }
        self.show_find_mode(BarMode::Files);
        false
    }

    /// The files row's request; `None`, with the bar saying `missing`, when it names no
    /// folder.
    fn files_request(&self, missing: &str) -> Option<FilesRequest> {
        let request = self.inner.find.files_request();
        if request.folder.is_empty() {
            self.inner.find.set_message(missing, Tone::Error);
            return None;
        }
        Some(request)
    }

    /// Whether `request`'s folder is one, looked at on a worker; the bar says so when it is
    /// not.
    async fn is_folder(&self, request: &FilesRequest) -> bool {
        let folder = request.folder_path();
        let is_folder = worker::run(move || folder.is_dir()).await.unwrap_or(false);
        if !is_folder {
            self.inner.find.set_message(
                &format!("“{}” is not a folder", request.folder),
                Tone::Error,
            );
        }
        is_folder
    }

    /// The folder field's default: the current file's folder, else the last folder searched,
    /// else the home folder.
    pub(in crate::window) fn default_search_folder(&self) {
        let find = &self.inner.find;
        let home = std::env::home_dir();
        let from_document = self
            .current_page()
            .and_then(|page| page.path())
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .map(|dir| tilde(&dir, home.as_deref()));
        let folder = from_document
            .or_else(|| find.history().directories.get(0).map(str::to_owned))
            .or_else(|| home.as_deref().map(|home| tilde(home, Some(home))));
        if let Some(folder) = folder {
            find.set_folder(&folder);
        }
    }

    /// The folder button: a folder chooser that starts at the folder field's folder.
    pub(in crate::window) async fn choose_search_folder(&self) {
        let find = &self.inner.find;
        let dialog = gtk::FileDialog::builder()
            .title("Find in Folder")
            .modal(true)
            .build();
        let current = find.files_request().folder_path();
        if !current.as_os_str().is_empty() {
            dialog.set_initial_folder(Some(&gio::File::for_path(&current)));
        }
        match dialog.select_folder_future(Some(&self.inner.window)).await {
            Ok(folder) => {
                if let Some(path) = folder.path() {
                    let home = std::env::home_dir();
                    find.set_folder(&tilde(&path, home.as_deref()));
                }
            }
            Err(error) => {
                if !error.matches(gtk::DialogError::Dismissed) {
                    self.toast(&format!("Could not open the folder chooser: {error}"));
                }
            }
        }
    }
}
