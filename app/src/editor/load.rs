//! Streaming a text into the buffer: the view takes no edits and undo is off while the pieces
//! go in, one every [`CHUNK_INTERVAL`], with the progress in a banner.

use super::page::EditorPage;
use super::state::Activity;
use crate::runtime::{Notify, connect_once, wait_until};
use gtk4::glib;
use gtk4::prelude::*;
use std::time::{Duration, Instant};
use stet_domain::document::banners::BannerKind;

/// The pause between two inserted pieces of a file (S1: a 16 ms timer loads 100 MB in about
/// a second and leaves room for frames; an idle is starved by GtkTextView's validation).
pub const CHUNK_INTERVAL: Duration = Duration::from_millis(16);

/// How long an operation on documents' text (Compare, Replace in Files) waits for one to load.
pub const LOAD_WAIT: Duration = Duration::from_secs(30);

/// How long a jump to a place in a document (going back, a search result) waits for it to load.
pub const JUMP_LOAD_WAIT: Duration = Duration::from_secs(10);

/// When the pieces of the last load went in, for the self-test's report.
#[derive(Debug, Clone, Copy, Default)]
pub struct LoadStats {
    pub started: Option<Instant>,
    /// The worker had read and decoded the file.
    pub decoded: Option<Instant>,
    pub first_insert: Option<Instant>,
    /// The first frame painted after the first piece went in.
    pub first_frame: Option<Instant>,
    pub finished: Option<Instant>,
    pub pieces: usize,
    pub bytes: usize,
    pub longest_insert: Duration,
}

impl EditorPage {
    pub fn is_loading(&self) -> bool {
        self.document().state.borrow().is_loading()
    }

    /// Whether the tab has its text to show: it is loaded, or the first piece of a load in
    /// pieces is in. A restored tab that has not started loading has not.
    pub fn shows_text(&self) -> bool {
        if self.is_loading() {
            self.load_stats().pieces > 0
        } else {
            self.session().stub.is_none()
        }
    }

    /// Marks the document as loading or not: the view takes no edits while it loads.
    pub fn set_loading(&self, loading: bool) {
        let document = self.document();
        {
            let mut state = document.state.borrow_mut();
            if loading {
                state.set_activity(Activity::Loading);
            } else {
                state.end_activity(Activity::Loading);
            }
        }
        document.sync_editable();
        document.load_changed.notify();
    }

    /// Wakes whoever waits for the document's text when it starts or stops loading, a piece of
    /// it goes in, its session record changes, or a tab of it closes; subscribe with
    /// [`Notify::notified`].
    pub fn load_changed(&self) -> &Notify {
        &self.document().load_changed
    }

    /// Resolves once the document is not loading (at once when it is not) or `limit` has
    /// passed; true when it is not loading.
    pub async fn loaded(&self, limit: Duration) -> bool {
        wait_until(
            Some(limit),
            || !self.is_loading(),
            || self.load_changed().notified(),
        )
        .await
    }

    /// Starts (re)loading: the view stops taking edits, undo stops, the language comes off
    /// the buffer, and the buffer is emptied without an undo step.
    pub fn begin_load(&self, started: Instant) {
        self.set_loading(true);
        self.sync_language();
        let decoded = self.load_stats().decoded;
        self.document().load_stats.set(LoadStats {
            started: Some(started),
            decoded,
            ..LoadStats::default()
        });
        let buffer = self.buffer();
        buffer.set_enable_undo(false);
        self.document().marks.forget_history();
        if buffer.char_count() > 0 {
            buffer.set_text("");
        }
    }

    pub fn load_stats(&self) -> LoadStats {
        self.document().load_stats.get()
    }

    pub fn update_load_stats(&self, update: impl FnOnce(&mut LoadStats)) {
        let mut stats = self.document().load_stats.get();
        update(&mut stats);
        self.document().load_stats.set(stats);
        self.document().load_changed.notify();
    }

    /// After the first piece of a load: waits (up to a second) until the window is shown, then
    /// lets idle work run once. Once the rest of the file is in, GtkTextView's validation of
    /// its lines starves idles until it is done (S1: about 40 s for 1M lines), and GTK's
    /// accessibility registration of a new window is one of them.
    async fn settle_before_streaming(&self) {
        if let Some(root) = self.root().filter(|root| !root.is_mapped()) {
            let (mapped, on_map) = async_channel::bounded(1);
            connect_once(
                &root,
                |root, run| root.connect_map(run),
                move |_| {
                    let _ = mapped.try_send(());
                },
            );
            let _ = glib::future_with_timeout(Duration::from_secs(1), on_map.recv()).await;
        }
        let (sender, receiver) = async_channel::bounded(1);
        glib::idle_add_local_once(move || {
            let _ = sender.try_send(());
        });
        let _ = receiver.recv().await;
    }

    /// Inserts the streamed pieces at the end of the buffer, the first at once and each
    /// further one [`CHUNK_INTERVAL`] later, and shows the progress in a banner. Returns
    /// false when the document was closed first.
    pub async fn feed(&self, pieces: async_channel::Receiver<String>, total: usize) -> bool {
        let buffer = self.buffer();
        let name = self.name();
        let mut inserted = 0;
        while let Ok(piece) = pieces.recv().await {
            if self.load_stats().pieces > 0 {
                glib::timeout_future(CHUNK_INTERVAL).await;
            }
            if !self.document_open() {
                return false;
            }
            let started = Instant::now();
            buffer.insert(&mut buffer.end_iter(), &piece);
            let took = started.elapsed();
            inserted += piece.len();
            let first = self.load_stats().pieces == 0;
            self.update_load_stats(|stats| {
                stats.pieces += 1;
                stats.bytes = inserted;
                stats.longest_insert = stats.longest_insert.max(took);
                if first {
                    stats.first_insert = Some(started);
                }
            });
            if first {
                self.note_first_frame();
                if inserted < total {
                    self.settle_before_streaming().await;
                }
            }
            if total > 0 && inserted < total {
                let percent = inserted * 100 / total;
                for page in self.document_pages() {
                    page.show_banner(
                        BannerKind::Loading,
                        &format!("Loading “{name}”… {percent}%"),
                        Vec::new(),
                    );
                }
            }
        }
        for page in self.document_pages() {
            page.hide_banner(BannerKind::Loading);
        }
        self.update_load_stats(|stats| stats.finished = Some(Instant::now()));
        self.document_open()
    }

    /// Records the end of the next painted frame as the load's first frame.
    fn note_first_frame(&self) {
        let Some(clock) = self.view().frame_clock() else {
            return;
        };
        connect_once(
            &clock,
            |clock, run| clock.connect_after_paint(run),
            glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |_| page.update_load_stats(|stats| stats.first_frame = Some(Instant::now()))
            ),
        );
        self.view().queue_draw();
    }

    /// Ends a load: undo starts afresh, the buffer is unmodified, the language goes back on
    /// when highlighting is on, and the view takes edits unless the document is read-only.
    pub fn end_load(&self) {
        let buffer = self.buffer();
        buffer.set_enable_undo(true);
        buffer.set_modified(false);
        self.set_loading(false);
        self.sync_language();
    }
}
