//! Quitting: closing the window, Quit, and SIGTERM, SIGHUP and SIGINT commit one last time and
//! wait for the disk.

use crate::editor::EditorPage;
use crate::runtime::{wait_until, worker};
use crate::window::Window;
use crate::window::dialogs::SaveChoice;
use libadwaita::prelude::*;
use std::sync::Arc;
use std::time::Instant;

use super::{FLUSH_TIMEOUT, Phase};

impl Window {
    /// Whether closing the window goes through the session: no prompts, a last commit.
    pub(in crate::window) fn session_closes_window(&self) -> bool {
        self.inner.session.phase.get() != Phase::Off
    }

    /// Closing the window with the session on: asks only about unsaved changes that have no
    /// backups (documents in large-file mode, or every document when config.toml turns
    /// backups off), then commits one last time, waits for the disk and goes.
    pub(in crate::window) async fn quit_with_session(&self) {
        let session = &self.inner.session;
        if session.quitting.replace(true) {
            return;
        }
        wait_until(
            None,
            || session.phase.get() != Phase::Pending,
            || session.changed.notified(),
        )
        .await;
        let backups = self.settings().backups;
        let large: Vec<EditorPage> = self
            .documents()
            .into_iter()
            .filter(|page| {
                page.session().waits.is_empty()
                    && (page.large_file_mode() || !backups)
                    && page.is_dirty()
            })
            .collect();
        if !large.is_empty() {
            let names: Vec<String> = large.iter().map(EditorPage::name).collect();
            match self.ask_save_changes(&names).await {
                SaveChoice::Save => {
                    for page in &large {
                        self.select(page);
                        if !self.save(page).await {
                            session.quitting.set(false);
                            return;
                        }
                    }
                }
                SaveChoice::Discard => {
                    for page in &large {
                        page.update_session(|session| session.discarded = true);
                    }
                }
                SaveChoice::Cancel => {
                    session.quitting.set(false);
                    return;
                }
            }
        }
        self.leave().await;
    }

    /// SIGTERM, SIGHUP or SIGINT: commit everything, large documents included, and go. With
    /// backups off nothing keeps unsaved changes, and there is no one to ask.
    pub fn shutdown_for_signal(&self) {
        if self.inner.session.quitting.replace(true) {
            return;
        }
        if !self.settings().backups {
            let unsaved = self
                .documents()
                .iter()
                .filter(|page| page.is_dirty())
                .count();
            if unsaved > 0 {
                tracing::warn!(unsaved, "backups are off: unsaved changes are lost");
            }
        }
        let window = self.clone();
        self.spawn(async move { window.leave().await });
    }

    /// The window disappears, the session is committed and flushed, the `--wait` command
    /// lines are answered and the window is destroyed, which ends the application.
    async fn leave(&self) {
        self.inner.window.set_visible(false);
        self.end_session().await;
        self.answer_waits();
        self.inner.tabs.set_closing_window();
        self.inner.window.destroy();
    }

    /// The last commit, then a wait of at most [`FLUSH_TIMEOUT`] for it to reach the disk,
    /// on a worker. The store is closed, so the lock is free for the next instance.
    async fn end_session(&self) {
        let session = &self.inner.session;
        if let Some(tick) = session.tick.take() {
            tick.remove();
        }
        if session.phase.get() != Phase::On {
            return;
        }
        self.commit_session(true);
        session.set_phase(Phase::Off);
        let Some(store) = session.store.take() else {
            return;
        };
        let started = Instant::now();
        let flushed = worker::run(move || match Arc::try_unwrap(store) {
            Ok(store) => store.close(FLUSH_TIMEOUT),
            Err(store) => store.flush(FLUSH_TIMEOUT),
        })
        .await;
        let ms = started.elapsed().as_secs_f64() * 1e3;
        match flushed {
            Some(Ok(())) => tracing::info!(ms, "the session is saved"),
            Some(Err(error)) => tracing::warn!(%error, ms, "the session may be incomplete"),
            None => tracing::warn!("the session writer failed"),
        }
    }
}
