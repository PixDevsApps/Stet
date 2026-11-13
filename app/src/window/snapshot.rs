//! Work on a snapshot of a document's text: small texts at once on the GTK thread, larger ones
//! on a worker, and a result dropped when the page closed or its text changed meanwhile.

use super::Window;
use crate::editor::EditorPage;
use crate::runtime::worker;

/// Texts up to this many bytes are worked on at once on the GTK thread; larger ones on a
/// worker, as an integer sort of a million lines takes about 0.6 s.
pub(super) const SYNC_BYTES: usize = 512 * 1024;

/// Whether `text` is worked on at once, on the GTK thread.
pub(super) fn is_small(text: &str) -> bool {
    text.len() <= SYNC_BYTES
}

/// Runs `job` over `text`: at once when it is small, else on a worker. `None` when the job
/// panicked.
pub(super) async fn run_sized<T: Send + 'static>(
    text: String,
    job: impl FnOnce(&str) -> T + Send + 'static,
) -> Option<T> {
    if is_small(&text) {
        Some(job(&text))
    } else {
        worker::run(move || job(&text)).await
    }
}

/// Why a job's result no longer applies to the page it was computed for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stale {
    /// The job panicked: there is no result.
    Panicked,
    /// The page left its tab.
    Closed,
    /// The page's text changed since it was read.
    Edited,
}

/// The page and revision a snapshot of its text was read at, to check a result against.
#[derive(Debug)]
pub(super) struct PageSnapshot {
    page: EditorPage,
    revision: u64,
}

impl PageSnapshot {
    /// Reads `page`'s text, with the snapshot to check results on it against.
    pub(super) fn take(page: &EditorPage) -> (Self, String) {
        let snapshot = Self {
            page: page.clone(),
            revision: page.revision(),
        };
        (snapshot, page.text())
    }

    /// The page's text changed since the snapshot.
    pub(super) fn edited(&self) -> bool {
        self.page.revision() != self.revision
    }
}

impl Window {
    /// Whether a result computed on `snapshot` still applies: [`Stale::Closed`] once the page
    /// left its tab, else [`Stale::Edited`] once its text changed.
    pub(super) fn check_snapshot(&self, snapshot: &PageSnapshot) -> Result<(), Stale> {
        if self.tab_page(&snapshot.page).is_none() {
            Err(Stale::Closed)
        } else if snapshot.edited() {
            Err(Stale::Edited)
        } else {
            Ok(())
        }
    }

    /// Runs `job` (which owns the snapshot's text) on a worker, then checks its result against
    /// `snapshot`: [`Stale::Panicked`], then [`Self::check_snapshot`]'s answer, else the result.
    pub(super) async fn on_snapshot<T: Send + 'static>(
        &self,
        snapshot: &PageSnapshot,
        job: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, Stale> {
        let result = worker::run(job).await.ok_or(Stale::Panicked)?;
        self.check_snapshot(snapshot)?;
        Ok(result)
    }
}
