//! The document a tab shows, whichever views show it: the buffer, its state, and the
//! buffer-signal plumbing that keeps the marks and the revision in step with the text.

use super::load::LoadStats;
use super::marks::DocMarks;
use super::page::EditorPage;
use super::search::{OWN_MATCH_TAG, PageSearch};
use super::session::TabSession;
use super::state::DocState;
use crate::convert::IterPosition;
use crate::monitor::FileWatch;
use crate::runtime::{Debounce, Notify};
use gtk4 as gtk;
use gtk4::glib;
use sourceview5::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;
use stet_domain::document::indent::Indentation;
use stet_domain::text::CharOffset;
use stet_domain::view::{DEFAULT_TAB_WIDTH, MAX_UNDO_LEVELS};

/// The tag that shows the current search match.
pub(super) const CURRENT_MATCH_TAG: &str = "stet-search-current";

/// Where a document's indentation came from; a choice in the status bar's popover sticks, and
/// language or settings changes only reach the defaults.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum IndentSource {
    /// The settings, for the document's language.
    #[default]
    Default,
    /// The file's own indentation.
    Detected,
    /// Chosen in the status bar's popover.
    User,
}

/// What a tab shows, whichever views show it: the buffer and everything about the document.
/// A clone in the other view is a second [`EditorPage`] over the same document.
#[derive(Debug)]
pub(super) struct Document {
    pub(super) buffer: sourceview5::Buffer,
    /// What the buffer's views share (column mode's redo workaround and owner).
    pub(super) shared: Rc<super::column::edit::BufferShared>,
    pub(super) state: RefCell<DocState>,
    pub(super) revision: Cell<u64>,
    pub(super) search: PageSearch,
    pub(super) indentation: Cell<Indentation>,
    pub(super) indent_source: Cell<IndentSource>,
    pub(super) busy: Cell<bool>,
    pub(super) highlighting: Cell<bool>,
    pub(super) language: RefCell<Option<sourceview5::Language>>,
    pub(super) watch: RefCell<Option<FileWatch>>,
    pub(super) load_stats: Cell<LoadStats>,
    pub(super) session: RefCell<TabSession>,
    pub(super) last_change: Cell<Option<Instant>>,
    /// An untitled document's first-line name and the revision it was read at.
    pub(super) first_line: RefCell<Option<(u64, Option<String>)>>,
    /// Bookmarks across Undo and Redo, and the Mark and style-token ranges.
    pub(super) marks: DocMarks,
    /// The marks' tags follow the views in an idle once something moved them.
    retag: Debounce,
    /// Wakes whoever waits for the document's text: it started or stopped loading, a piece of
    /// it went in, its session record changed, or a tab of it closed.
    pub(super) load_changed: Notify,
    /// The pages that show it, in the order they were made.
    pub(super) pages: RefCell<Vec<glib::WeakRef<EditorPage>>>,
    /// The page whose caret and selection the buffer has.
    pub(super) caret_owner: glib::WeakRef<EditorPage>,
    /// Edits on their way into the buffer: where an insertion starts, and the range a deletion
    /// takes, seen before the change and counted after it (a handler may stop it).
    pending_insert: Cell<Option<CharOffset>>,
    pending_delete: Cell<Option<(CharOffset, CharOffset)>>,
}

impl Document {
    pub(super) fn new(state: DocState) -> Rc<Self> {
        let buffer = sourceview5::Buffer::new(None);
        buffer.set_max_undo_levels(MAX_UNDO_LEVELS);
        buffer.set_highlight_matching_brackets(true);
        buffer.create_tag(Some(OWN_MATCH_TAG), &[]);
        buffer.create_tag(Some(CURRENT_MATCH_TAG), &[]);
        super::marks::create_tags(&buffer);
        // Before any view: the buffer's hooks run ahead of the views'.
        let shared = super::column::edit::connect_history(&buffer);
        let document = Rc::new(Self {
            buffer,
            shared,
            state: RefCell::new(state),
            revision: Cell::new(0),
            search: PageSearch::default(),
            indentation: Cell::new(Indentation {
                tab_width: DEFAULT_TAB_WIDTH,
                insert_spaces: false,
            }),
            indent_source: Cell::new(IndentSource::Default),
            busy: Cell::new(false),
            highlighting: Cell::new(true),
            language: RefCell::new(None),
            watch: RefCell::new(None),
            load_stats: Cell::new(LoadStats::default()),
            session: RefCell::new(TabSession::default()),
            last_change: Cell::new(None),
            first_line: RefCell::new(None),
            marks: DocMarks::default(),
            retag: Debounce::default(),
            load_changed: Notify::default(),
            pages: RefCell::new(Vec::new()),
            caret_owner: glib::WeakRef::new(),
            pending_insert: Cell::new(None),
            pending_delete: Cell::new(None),
        });
        Self::connect(&document);
        document
    }

    /// The buffer's signals that concern the document, connected before any view's.
    fn connect(document: &Rc<Self>) {
        let buffer = &document.buffer;
        let weak = Rc::downgrade(document);
        buffer.connect_changed(move |_| {
            if let Some(document) = weak.upgrade() {
                document.revision.set(document.revision.get() + 1);
                document.last_change.set(Some(Instant::now()));
            }
        });
        let weak = Rc::downgrade(document);
        buffer.connect_insert_text(move |_, iter, _| {
            if let Some(document) = weak.upgrade() {
                document.pending_insert.set(Some(iter.char_offset()));
            }
        });
        let weak = Rc::downgrade(document);
        buffer.connect_local("insert-text", true, move |values| {
            let document = weak.upgrade()?;
            let start = document.pending_insert.take()?;
            let end = values.get(1)?.get::<gtk::TextIter>().ok()?.char_offset();
            document.marks.on_insert(start, end.max(start) - start);
            document.schedule_retag();
            None
        });
        let weak = Rc::downgrade(document);
        buffer.connect_delete_range(move |_, start, end| {
            if let Some(document) = weak.upgrade() {
                let (start, end) = (start.char_offset(), end.char_offset());
                document
                    .pending_delete
                    .set(Some((start.min(end), start.max(end))));
            }
        });
        let weak = Rc::downgrade(document);
        buffer.connect_local("delete-range", true, move |_| {
            let document = weak.upgrade()?;
            let (start, end) = document.pending_delete.take()?;
            document.marks.on_delete(start, end);
            document.schedule_retag();
            None
        });
        for (signal, undo) in [("undo", true), ("redo", false)] {
            let weak = Rc::downgrade(document);
            buffer.connect_local(signal, false, move |_| {
                weak.upgrade()?.marks.on_replay_begin(undo);
                None
            });
            let weak = Rc::downgrade(document);
            buffer.connect_local(signal, true, move |_| {
                let document = weak.upgrade()?;
                document.marks.on_replay_end(&document.buffer);
                document.schedule_retag();
                None
            });
        }
    }

    /// The pages that show the document now.
    pub(super) fn live_pages(&self) -> Vec<EditorPage> {
        let mut pages = self.pages.borrow_mut();
        pages.retain(|page| page.upgrade().is_some());
        pages.iter().filter_map(glib::WeakRef::upgrade).collect()
    }

    /// Tags the marks around what the views show, once the main loop is idle.
    pub(super) fn schedule_retag(self: &Rc<Self>) {
        if self.retag.is_pending() || self.marks.marks().is_empty() {
            return;
        }
        let weak = Rc::downgrade(self);
        self.retag.schedule_idle(move || {
            if let Some(document) = weak.upgrade() {
                document.retag_now();
            }
        });
    }

    pub(super) fn retag_now(&self) {
        let views: Vec<sourceview5::View> = self
            .live_pages()
            .iter()
            .filter(|page| page.is_mapped())
            .map(|page| page.view().clone())
            .collect();
        self.marks.retag(&self.buffer, &views);
    }

    /// The view of every page takes edits unless the document is loading, read-only or busy
    /// with a text tool.
    pub(super) fn sync_editable(&self) {
        let editable = {
            let state = self.state.borrow();
            !state.is_loading() && state.read_only().is_none() && !self.busy.get()
        };
        for page in self.live_pages() {
            page.view().set_editable(editable);
        }
    }
}
