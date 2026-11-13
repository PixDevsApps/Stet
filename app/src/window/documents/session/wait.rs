//! Closing tabs and `--wait`: a closed tab's backup goes with the next commit, and `--wait`
//! command lines are answered when the tabs they opened close.

use crate::editor::EditorPage;
use crate::window::Window;
use gtk4::{gio, glib};
use libadwaita::prelude::*;

/// A `--wait` command line and how many of its tabs are still open.
pub(super) struct WaitGroup {
    id: u64,
    command_line: gio::ApplicationCommandLine,
    open: usize,
    unsaved: bool,
}

impl WaitGroup {
    /// Lets the caller go: status 1 when a tab closed with unsaved changes, so that git
    /// aborts instead of committing a stale message. Dropping the command line sends the
    /// answer.
    fn answer(self, unsaved: bool) {
        self.command_line
            .set_exit_code(glib::ExitCode::from(u8::from(unsaved)));
    }
}

impl Window {
    /// A tab closed: its backup and orphan file go with the next commit, and `--wait`
    /// command lines whose last tab it was are answered (status 1 when it had unsaved
    /// changes).
    pub(in crate::window) fn tab_closed(&self, page: &EditorPage, unsaved: bool) {
        let session = page.session().clone();
        let state = &self.inner.session;
        if session.backup {
            state.removed.borrow_mut().push(session.id.clone());
        }
        if let Some(orphan) = session.orphan {
            state.adopted.borrow_mut().push(orphan);
        }
        if session.waits.is_empty() {
            return;
        }
        let finished: Vec<WaitGroup> = {
            let mut waits = state.waits.borrow_mut();
            for group in waits
                .iter_mut()
                .filter(|group| session.waits.contains(&group.id))
            {
                group.open = group.open.saturating_sub(1);
                group.unsaved |= unsaved;
            }
            let (finished, open) = waits.drain(..).partition(|group| group.open == 0);
            *waits = open;
            finished
        };
        for group in finished {
            let unsaved = group.unsaved;
            group.answer(unsaved);
        }
    }

    /// Holds `command_line` until every page in `pages` closes (`--wait`).
    pub fn add_wait(&self, command_line: gio::ApplicationCommandLine, pages: &[EditorPage]) {
        let mut waited: Vec<EditorPage> = Vec::new();
        for page in pages {
            if !waited.contains(page) && self.tab_page(page).is_some() {
                waited.push(page.clone());
            }
        }
        if waited.is_empty() {
            return;
        }
        let session = &self.inner.session;
        let id = session.next_wait.get();
        session.next_wait.set(id + 1);
        for page in &waited {
            page.update_session(|session| session.waits.push(id));
        }
        session.waits.borrow_mut().push(WaitGroup {
            id,
            command_line,
            open: waited.len(),
            unsaved: false,
        });
    }

    /// `--wait` command lines still waiting.
    #[cfg(feature = "selftest")]
    pub fn waiting(&self) -> usize {
        self.inner.session.waits.borrow().len()
    }

    /// Moves the `--wait` command lines of `from`, a tab about to close in favour of `to`
    /// (the same file through another path), to `to`.
    pub(in crate::window) fn transfer_waits(from: &EditorPage, to: &EditorPage) {
        let waits = from.update_session(|session| std::mem::take(&mut session.waits));
        to.update_session(|session| session.waits.extend(waits));
    }

    /// Answers every `--wait` command line, as the window goes: status 1 where a tab it
    /// waits for has unsaved changes.
    pub(in crate::window) fn answer_waits(&self) {
        let groups = self.inner.session.waits.take();
        for group in groups {
            let unsaved = group.unsaved
                || self.pages().iter().any(|page| {
                    page.session().waits.contains(&group.id) && Self::has_unsaved_changes(page)
                });
            group.answer(unsaved);
        }
    }
}
