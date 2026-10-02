//! One tab: a GtkSourceView over its document's buffer, and the banners above it; and the
//! document itself: the file, how it is read and written, and what loading found (M3). The
//! buffer is the source of truth for the text (ADR-003) and holds LF line endings only
//! (ADR-008). Since M7 a document can be shown by two tabs, one in each view of a split
//! window: a clone shares the [`Document`] and its buffer, and has a view, a caret and banners
//! of its own.

use crate::banner::{Banner, BannerButton, BannerKind};
use crate::column::StetView;
use crate::loader::FormatKind;
use crate::marks::DocMarks;
use crate::monitor::FileWatch;
use crate::page_search::{OWN_MATCH_TAG, PageSearch, style_tag};
use crate::session::TabSession;
use crate::tabstops::TabStops;
use gtk4 as gtk;
use gtk4::glib;
use gtk4::subclass::prelude::*;
use libadwaita::prelude::*;
use sourceview5::prelude::*;
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};
use stet_domain::document::{tilde, untitled_name};
use stet_domain::indent::Indentation;
use stet_domain::session::{Fingerprint, TabRecord};
use stet_domain::text::{EolStats, Position};
use stet_domain::theme::{BOOKMARK_STYLE, SEARCH_CURRENT_STYLE};
use stet_domain::untitled::{FIRST_LINE_READ, FIRST_LINE_SCAN, first_line_name};
use stet_domain::view::{DEFAULT_TAB_WIDTH, MAX_UNDO_LEVELS};
use stet_infrastructure::encoding::DetectionSource;
use stet_infrastructure::fs::{DocumentFormat, EncodingChoice, SizeClass};

const CURRENT_MATCH_TAG: &str = "stet-search-current";
const MATCH_START_MARK: &str = "stet-match-start";
const MATCH_END_MARK: &str = "stet-match-end";

/// The pause between two inserted pieces of a file (S1: a 16 ms timer loads 100 MB in about
/// a second and leaves room for frames; an idle is starved by GtkTextView's validation).
pub const CHUNK_INTERVAL: Duration = Duration::from_millis(16);

/// Why the editor refuses edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOnly {
    /// The file contains NUL bytes; "Edit Anyway" lifts it when the file round-trips.
    Binary,
    /// This user may not write the file; "Edit Anyway" lifts it, for Save As.
    NotWritable,
    /// Long lines were broken for display, so the text isn't the file's; saving is off.
    DisplayBreaks,
    /// A text a comparison opened (the clipboard, the saved version); it closes with the
    /// comparison (M7).
    Comparison,
}

/// Where the buffer's text came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextOrigin {
    /// Decoded from the file as it is, or typed into a new document.
    #[default]
    File,
    /// Pretty-printed by the long-line path: saving asks before writing it.
    Formatted(FormatKind),
    /// Long lines broken for display: read-only.
    DisplayBreaks,
}

/// Why the last save could not write the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unwritable {
    Permission,
    ReadOnlyFilesystem,
}

/// The file on disk, as last compared with the baseline fingerprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiskState {
    #[default]
    Same,
    /// Changed while the document had unsaved changes; the banner asks what to keep.
    Changed,
    /// Deleted or moved away. `clean_revision` is the buffer's revision at that moment when
    /// it had no unsaved changes, so a file that comes back can reload quietly.
    Deleted { clean_revision: Option<u64> },
}

/// What the tab is backed by, and how it is written back (ADR-007, ADR-008).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocState {
    /// `None` for an untitled document.
    pub path: Option<PathBuf>,
    /// `path` with symlinks resolved, once the file was loaded or saved.
    pub canonical: Option<PathBuf>,
    /// The number of an untitled document.
    pub untitled: Option<u32>,
    /// Encoding, BOM and line ending for the next save; Convert changes them.
    pub format: DocumentFormat,
    /// An encoding the user picked with Reinterpret, kept when the file is reloaded.
    pub chosen: Option<EncodingChoice>,
    /// How the encoding was found; `None` for new documents.
    pub detection: Option<DetectionSource>,
    pub eol_stats: EolStats,
    /// The file had mixed line endings and nothing has normalized them yet.
    pub mixed_eol: bool,
    /// The user chose to keep the mixed line endings for now (the banner is gone; saving
    /// still warns).
    pub mixed_kept: bool,
    pub had_errors: bool,
    pub lossy_roundtrip: bool,
    pub binary: bool,
    pub nul_count: usize,
    pub placeholder_conflict: bool,
    pub class: SizeClass,
    pub read_only: Option<ReadOnly>,
    pub unwritable: Option<Unwritable>,
    pub origin: TextOrigin,
    /// The file as last read or written: changes against it are outside changes.
    pub baseline: Option<Fingerprint>,
    pub disk: DiskState,
    /// The save format differs from the file's (Convert, a line-ending change).
    pub format_changed: bool,
    /// The user already agreed to save despite a lossy decode or formatted text.
    pub save_confirmed: bool,
    /// Highlighting was left off because the file is over the ADR-014 limits.
    pub highlight_limited: bool,
    pub loading: bool,
    pub saving: bool,
    /// What a text a comparison opened is called (M7): it has no file, stays out of the
    /// session and closes with the comparison.
    pub temporary: Option<String>,
    /// The name the user gave an untitled document with Rename… (1.2), in place of its first
    /// line; a file's name is its file's.
    pub custom_name: Option<String>,
}

impl Default for DocState {
    fn default() -> Self {
        Self {
            path: None,
            canonical: None,
            untitled: None,
            format: DocumentFormat::default(),
            chosen: None,
            detection: None,
            eol_stats: EolStats::default(),
            mixed_eol: false,
            mixed_kept: false,
            had_errors: false,
            lossy_roundtrip: false,
            binary: false,
            nul_count: 0,
            placeholder_conflict: false,
            class: SizeClass::Normal,
            read_only: None,
            unwritable: None,
            origin: TextOrigin::File,
            baseline: None,
            disk: DiskState::Same,
            format_changed: false,
            save_confirmed: false,
            highlight_limited: false,
            loading: false,
            saving: false,
            temporary: None,
            custom_name: None,
        }
    }
}

impl DocState {
    /// Saving can't reproduce what was read: decoding errors or a lossy encoding.
    pub fn lossy(&self) -> bool {
        self.had_errors || self.lossy_roundtrip
    }

    /// Large-file mode (≥ 50 MiB): no highlighting, wrapping, whitespace or bracket matching.
    pub fn large(&self) -> bool {
        self.class == SizeClass::Large
    }
}

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

/// Window-wide view settings, applied to every editor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewSettings {
    pub wrap: bool,
    pub whitespace: bool,
    /// The document map (GtkSourceMap) beside the text; never in large-file mode.
    pub map: bool,
}

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
/// A clone in the other view (M7) is a second [`EditorPage`] over the same document.
pub struct Document {
    buffer: sourceview5::Buffer,
    /// What the buffer's views share (column mode's redo workaround and owner).
    shared: Rc<crate::column::edit::BufferShared>,
    state: RefCell<DocState>,
    revision: Cell<u64>,
    search: PageSearch,
    indentation: Cell<Indentation>,
    indent_source: Cell<IndentSource>,
    busy: Cell<bool>,
    highlighting: Cell<bool>,
    language: RefCell<Option<sourceview5::Language>>,
    watch: RefCell<Option<FileWatch>>,
    load_stats: Cell<LoadStats>,
    session: RefCell<TabSession>,
    last_change: Cell<Option<Instant>>,
    /// An untitled document's first-line name and the revision it was read at (M8).
    first_line: RefCell<Option<(u64, Option<String>)>>,
    /// Bookmarks across Undo and Redo, and the Mark and style-token ranges (M7).
    marks: DocMarks,
    /// The marks' tags follow the views in an idle once something moved them.
    retag: RefCell<Option<glib::SourceId>>,
    /// The pages that show it, in the order they were made.
    pages: RefCell<Vec<glib::WeakRef<EditorPage>>>,
    /// The page whose caret and selection the buffer has (M7).
    caret_owner: glib::WeakRef<EditorPage>,
    /// Edits on their way into the buffer: where an insertion starts, and the range a deletion
    /// takes, seen before the change and counted after it (a handler may stop it).
    pending_insert: Cell<Option<usize>>,
    pending_delete: Cell<Option<(usize, usize)>>,
}

impl Document {
    fn new(state: DocState) -> Rc<Self> {
        let buffer = sourceview5::Buffer::new(None);
        buffer.set_max_undo_levels(MAX_UNDO_LEVELS);
        buffer.set_highlight_matching_brackets(true);
        buffer.create_tag(Some(OWN_MATCH_TAG), &[]);
        buffer.create_tag(Some(CURRENT_MATCH_TAG), &[]);
        crate::marks::create_tags(&buffer);
        // Before any view: the buffer's hooks run ahead of the views'.
        let shared = crate::column::edit::connect_history(&buffer);
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
            retag: RefCell::new(None),
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
                document
                    .pending_insert
                    .set(Some(iter.offset().max(0) as usize));
            }
        });
        let weak = Rc::downgrade(document);
        buffer.connect_local("insert-text", true, move |values| {
            let document = weak.upgrade()?;
            let start = document.pending_insert.take()?;
            let end = values.get(1)?.get::<gtk::TextIter>().ok()?.offset().max(0) as usize;
            document.marks.on_insert(start, end.saturating_sub(start));
            document.schedule_retag();
            None
        });
        let weak = Rc::downgrade(document);
        buffer.connect_delete_range(move |_, start, end| {
            if let Some(document) = weak.upgrade() {
                let (start, end) = (start.offset().max(0), end.offset().max(0));
                document
                    .pending_delete
                    .set(Some((start.min(end) as usize, start.max(end) as usize)));
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
    fn live_pages(&self) -> Vec<EditorPage> {
        let mut pages = self.pages.borrow_mut();
        pages.retain(|page| page.upgrade().is_some());
        pages.iter().filter_map(glib::WeakRef::upgrade).collect()
    }

    /// Tags the marks around what the views show, once the main loop is idle.
    fn schedule_retag(self: &Rc<Self>) {
        if self.retag.borrow().is_some() || self.marks.marks().is_empty() {
            return;
        }
        let weak = Rc::downgrade(self);
        let source = glib::idle_add_local_once(move || {
            if let Some(document) = weak.upgrade() {
                document.retag.replace(None);
                document.retag_now();
            }
        });
        self.retag.replace(Some(source));
    }

    fn retag_now(&self) {
        let views: Vec<sourceview5::View> = self
            .live_pages()
            .iter()
            .filter(|page| page.is_mapped())
            .map(|page| page.view().clone())
            .collect();
        self.marks.retag(&self.buffer, &views);
    }
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct EditorPage {
        pub view: OnceCell<StetView>,
        pub banner_box: OnceCell<gtk::Box>,
        pub banners: RefCell<BTreeMap<BannerKind, Rc<Banner>>>,
        pub tab_stops: TabStops,
        pub settings: Cell<ViewSettings>,
        pub map: RefCell<Option<sourceview5::Map>>,
        pub body: OnceCell<gtk::Box>,
        pub document: OnceCell<Rc<Document>>,
        /// This view's caret and selection while another view of the document is in front
        /// (M7): GTK keeps one caret per buffer.
        pub selection: RefCell<Option<(gtk::TextMark, gtk::TextMark)>>,
        /// A clone's own id in the session (M7).
        pub clone_id: RefCell<Option<String>>,
        /// The restored first visible line, kept until the view is first shown and has a
        /// scroll position of its own (M2).
        pub top_line: Cell<Option<usize>>,
        /// A restored tab's caret and scroll position, applied once its text is in (M2, M7).
        pub pending: RefCell<Option<Box<TabRecord>>>,
        /// Shown in a comparison: no word wrap, so the two views' lines line up (M7).
        pub compared: Cell<bool>,
        pub bookmark: OnceCell<sourceview5::MarkAttributes>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for EditorPage {
        const NAME: &'static str = "StetEditorPage";
        type Type = super::EditorPage;
        type ParentType = gtk::Box;
    }

    impl ObjectImpl for EditorPage {}
    impl WidgetImpl for EditorPage {
        /// The page's focus is its editor: a click on the page's tab, and GTK moving a focus
        /// that a banner button lost, land there.
        fn grab_focus(&self) -> bool {
            self.view.get().is_some_and(|view| view.grab_focus())
        }
    }
    impl BoxImpl for EditorPage {}
}

glib::wrapper! {
    pub struct EditorPage(ObjectSubclass<imp::EditorPage>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl EditorPage {
    pub fn new(
        state: DocState,
        scheme: Option<&sourceview5::StyleScheme>,
        settings: ViewSettings,
    ) -> Self {
        Self::with_document(Document::new(state), scheme, settings)
    }

    /// A second view of this page's document (Clone to Other View, M7): the same buffer,
    /// with a caret, scroll position and banners of its own.
    pub fn clone_view(
        &self,
        scheme: Option<&sourceview5::StyleScheme>,
        settings: ViewSettings,
    ) -> Self {
        let page = Self::with_document(self.document().clone(), scheme, settings);
        page.save_selection();
        page
    }

    fn with_document(
        document: Rc<Document>,
        scheme: Option<&sourceview5::StyleScheme>,
        settings: ViewSettings,
    ) -> Self {
        let page: Self = glib::Object::builder()
            .property("orientation", gtk::Orientation::Vertical)
            .build();
        let indentation = document.indentation.get();
        let view = StetView::builder()
            .buffer(&document.buffer, &document.shared)
            .monospace(true)
            .show_line_numbers(true)
            .highlight_current_line(true)
            .auto_indent(true)
            .tab_width(indentation.tab_width)
            .insert_spaces_instead_of_tabs(indentation.insert_spaces)
            .smart_home_end(sourceview5::SmartHomeEndType::Before)
            .left_margin(4)
            .hexpand(true)
            .vexpand(true)
            .build();
        // Added, not set: setting css-classes would drop GtkSourceView's own classes.
        view.add_css_class("stet-editor");
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&view)
            .hexpand(true)
            .vexpand(true)
            .build();
        let banner_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let body = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        body.append(&scrolled);
        page.append(&banner_box);
        page.append(&body);

        let imp = page.imp();
        imp.view.set(view.clone()).expect("set once");
        imp.banner_box.set(banner_box).expect("set once");
        imp.body.set(body).expect("set once");
        document.pages.borrow_mut().push(page.downgrade());
        let _ = imp.document.set(document.clone());

        // Bookmarks in the gutter, toggled by a click there (M7).
        let attributes = sourceview5::MarkAttributes::new();
        view.set_mark_attributes(crate::marks::BOOKMARK_CATEGORY, &attributes, 10);
        view.set_show_line_marks(true);
        // Line numbers and bookmarks at the top of their line, which a comparison pads below.
        let gutter = sourceview5::prelude::ViewExt::gutter(
            view.upcast_ref::<sourceview5::View>(),
            gtk::TextWindowType::Left,
        );
        let mut child = gutter.first_child();
        while let Some(widget) = child {
            if let Some(renderer) = widget.downcast_ref::<sourceview5::GutterRenderer>() {
                renderer.set_yalign(0.0);
            }
            child = widget.next_sibling();
        }
        imp.bookmark.set(attributes).expect("set once");
        view.connect_line_mark_activated(glib::clone!(
            #[weak]
            page,
            move |_, iter, button, _, _| {
                if button == gtk::gdk::BUTTON_PRIMARY {
                    page.toggle_bookmark(iter.line().max(0) as usize);
                }
            }
        ));
        let weak = Rc::downgrade(&document);
        view.set_column_tracker(Rc::new(move |lines, logical, apply| {
            if let Some(document) = weak.upgrade() {
                document
                    .marks
                    .track_lines(&document.buffer, lines, logical, apply);
            } else {
                apply();
            }
        }));
        if let Some(adjustment) = view.vadjustment() {
            let weak = Rc::downgrade(&document);
            adjustment.connect_value_changed(move |_| {
                if let Some(document) = weak.upgrade() {
                    document.schedule_retag();
                }
            });
        }

        page.apply_settings(settings);
        if let Some(scheme) = scheme {
            page.set_scheme(scheme);
        }
        page.sync_editable();
        view.connect_tab_width_notify(glib::clone!(
            #[weak]
            page,
            move |view| {
                page.imp().tab_stops.invalidate();
                view.queue_resize();
            }
        ));
        page
    }

    fn document(&self) -> &Rc<Document> {
        self.imp().document.get().expect("constructed")
    }

    /// Whether `other` shows the same document (one is a clone of the other, M7).
    pub fn same_document(&self, other: &Self) -> bool {
        Rc::ptr_eq(self.document(), other.document())
    }

    /// Every page that shows this page's document, this one included.
    pub fn document_pages(&self) -> Vec<Self> {
        self.document().live_pages()
    }

    /// Whether any page of the document is still in a tab view.
    pub fn document_open(&self) -> bool {
        self.document_pages()
            .iter()
            .any(|page| page.parent().is_some())
    }

    pub fn view(&self) -> &sourceview5::View {
        self.column_view().upcast_ref()
    }

    /// The view with column mode (M6).
    pub fn column_view(&self) -> &StetView {
        self.imp().view.get().expect("constructed")
    }

    pub fn buffer(&self) -> sourceview5::Buffer {
        self.document().buffer.clone()
    }

    pub fn state(&self) -> DocState {
        self.document().state.borrow().clone()
    }

    pub fn update_state(&self, update: impl FnOnce(&mut DocState)) {
        update(&mut self.document().state.borrow_mut());
    }

    pub fn path(&self) -> Option<PathBuf> {
        self.document().state.borrow().path.clone()
    }

    /// Whether this tab shows `path` (as given, or with symlinks resolved).
    pub fn shows(&self, path: &Path, canonical: Option<&Path>) -> bool {
        let state = self.document().state.borrow();
        state.path.as_deref() == Some(path)
            || (canonical.is_some() && state.canonical.as_deref() == canonical)
    }

    /// The file name; for an untitled document the name the user gave it (1.2), else its first
    /// line with text (M8), else `Untitled N`; for a text a comparison opened, what it is (M7).
    /// A restored tab whose text isn't loaded yet keeps the name it had.
    pub fn name(&self) -> String {
        let document = self.document();
        if let Some(label) = document.state.borrow().temporary.clone() {
            return label;
        }
        if let Some(path) = document.state.borrow().path.as_ref() {
            return path.file_name().map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            );
        }
        if let Some(name) = document.state.borrow().custom_name.clone() {
            return name;
        }
        if let Some(stub) = document.session.borrow().stub.as_ref()
            && !stub.display_name.is_empty()
        {
            return stub.display_name.clone();
        }
        self.first_line_name()
            .unwrap_or_else(|| self.untitled_label())
    }

    /// `Untitled N`, the name of an untitled document without text.
    pub fn untitled_label(&self) -> String {
        untitled_name(self.document().state.borrow().untitled.unwrap_or(1))
    }

    /// The first line with text of an untitled document, cut for a tab (M8); read again only
    /// when the text changed.
    fn first_line_name(&self) -> Option<String> {
        let document = self.document();
        let revision = document.revision.get();
        if let Some((read_at, name)) = document.first_line.borrow().as_ref()
            && *read_at == revision
        {
            return name.clone();
        }
        let name = self.read_first_line();
        document.first_line.replace(Some((revision, name.clone())));
        name
    }

    /// The first [`FIRST_LINE_SCAN`] lines, at most [`FIRST_LINE_READ`] characters each, until
    /// one has text.
    fn read_first_line(&self) -> Option<String> {
        let buffer = self.buffer();
        let mut start = buffer.start_iter();
        for _ in 0..FIRST_LINE_SCAN {
            let mut end = start;
            let mut read = 0;
            while read < FIRST_LINE_READ && !end.ends_line() {
                end.forward_char();
                read += 1;
            }
            let line = buffer.text(&start, &end, true);
            if let Some(name) = first_line_name([line.as_str()]) {
                return Some(name);
            }
            if !start.forward_line() {
                break;
            }
        }
        None
    }

    /// Reads an untitled document's name again; true when it changed, so the tab and the
    /// window title must follow.
    pub fn first_line_changed(&self) -> bool {
        let before = self
            .document()
            .first_line
            .borrow()
            .as_ref()
            .map(|(_, name)| name.clone());
        let after = self.first_line_name();
        before != Some(after)
    }

    /// The directory for the window title, with `~` for the home directory.
    pub fn dir_display(&self) -> Option<String> {
        let path = self.path()?;
        let dir = path.parent()?;
        Some(tilde(dir, std::env::home_dir().as_deref()))
    }

    /// Unsaved changes: edits, a changed save format, or a file deleted from disk. Never while
    /// loading, when inserting the text marks the buffer modified.
    pub fn is_dirty(&self) -> bool {
        let state = self.document().state.borrow();
        !state.loading
            && (self.buffer().is_modified()
                || state.format_changed
                || matches!(state.disk, DiskState::Deleted { .. }))
    }

    pub fn is_loading(&self) -> bool {
        self.document().state.borrow().loading
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

    pub fn read_only(&self) -> Option<ReadOnly> {
        self.document().state.borrow().read_only
    }

    /// Large-file mode (≥ 50 MiB). M4's highlight-all and smart highlight must stay off here.
    pub fn large_file_mode(&self) -> bool {
        self.document().state.borrow().large()
    }

    /// An untitled tab nobody has typed in: opening a file replaces it.
    pub fn is_pristine(&self) -> bool {
        let buffer = self.buffer();
        let state = self.document().state.borrow();
        state.path.is_none()
            && state.temporary.is_none()
            && !state.loading
            && !state.format_changed
            && !buffer.is_modified()
            && buffer.char_count() == 0
            && !buffer.can_undo()
    }

    pub fn revision(&self) -> u64 {
        self.document().revision.get()
    }

    /// When the text last changed, for the backups' idle check (M2).
    pub fn last_change(&self) -> Option<Instant> {
        self.document().last_change.get()
    }

    /// The document's part in the session (M2).
    pub fn session(&self) -> std::cell::Ref<'_, TabSession> {
        self.document().session.borrow()
    }

    pub fn update_session<R>(&self, update: impl FnOnce(&mut TabSession) -> R) -> R {
        update(&mut self.document().session.borrow_mut())
    }

    /// This tab's own id in the session when it is a clone, made on first use (M7).
    pub fn clone_id(&self) -> String {
        self.imp()
            .clone_id
            .borrow_mut()
            .get_or_insert_with(stet_infrastructure::session_store::new_id)
            .clone()
    }

    pub fn set_clone_id(&self, id: String) {
        self.imp().clone_id.replace(Some(id));
    }

    /// The restored first visible line of this view, until it is first shown (M2).
    pub fn top_line(&self) -> Option<usize> {
        self.imp().top_line.get()
    }

    pub fn set_top_line(&self, line: Option<usize>) {
        self.imp().top_line.set(line);
    }

    /// The session record whose caret and scroll position this tab takes once its text is in.
    pub fn set_pending_record(&self, record: TabRecord) {
        self.imp().pending.replace(Some(Box::new(record)));
    }

    pub fn take_pending_record(&self) -> Option<Box<TabRecord>> {
        self.imp().pending.take()
    }

    pub fn text(&self) -> String {
        let buffer = self.buffer();
        buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), true)
            .to_string()
    }

    pub fn set_read_only(&self, reason: Option<ReadOnly>) {
        self.update_state(|state| state.read_only = reason);
        self.sync_editable();
    }

    pub fn set_loading(&self, loading: bool) {
        self.update_state(|state| state.loading = loading);
        self.sync_editable();
    }

    /// A text tool is changing the document across main-loop iterations (a large result
    /// inserted in pieces): the view takes no edits and other tools wait.
    pub fn is_busy(&self) -> bool {
        self.document().busy.get()
    }

    pub fn set_busy(&self, busy: bool) {
        self.document().busy.set(busy);
        self.sync_editable();
    }

    fn sync_editable(&self) {
        let document = self.document();
        let editable = {
            let state = document.state.borrow();
            !state.loading && state.read_only.is_none() && !document.busy.get()
        };
        for page in document.live_pages() {
            page.view().set_editable(editable);
        }
    }

    // ----- this view's caret (M7) --------------------------------------------------------

    /// Keeps the caret and selection the buffer has now as this view's, while another view of
    /// the document is in front.
    pub fn save_selection(&self) {
        let buffer = self.buffer();
        let insert = buffer.iter_at_mark(&buffer.get_insert());
        let bound = buffer.iter_at_mark(&buffer.selection_bound());
        let mut saved = self.imp().selection.borrow_mut();
        match saved.as_ref() {
            Some((insert_mark, bound_mark)) => {
                buffer.move_mark(insert_mark, &insert);
                buffer.move_mark(bound_mark, &bound);
            }
            None => {
                *saved = Some((
                    buffer.create_mark(None, &insert, false),
                    buffer.create_mark(None, &bound, false),
                ));
            }
        }
    }

    /// This view is the one in front: the buffer gets its caret and selection, and keeps those
    /// of the view of the document that had them.
    pub fn take_caret(&self) {
        let document = self.document();
        let owner = document.caret_owner.upgrade();
        if owner.as_ref() == Some(self) {
            return;
        }
        if let Some(owner) = owner {
            owner.save_selection();
        }
        self.restore_selection();
        document.caret_owner.set(Some(self));
    }

    /// Whether the buffer has this view's caret: it is the view in front, or no view is yet.
    fn has_caret(&self) -> bool {
        self.document()
            .caret_owner
            .upgrade()
            .is_none_or(|owner| owner == *self)
    }

    /// This view's caret and selection, in characters: (caret, selection).
    pub fn own_selection(&self) -> (usize, Option<(usize, usize)>) {
        let buffer = self.buffer();
        let saved = self.imp().selection.borrow().clone();
        let (insert, bound) = match saved {
            Some((insert, bound)) if !self.has_caret() => {
                (buffer.iter_at_mark(&insert), buffer.iter_at_mark(&bound))
            }
            _ => (
                buffer.iter_at_mark(&buffer.get_insert()),
                buffer.iter_at_mark(&buffer.selection_bound()),
            ),
        };
        let (insert, bound) = (
            insert.offset().max(0) as usize,
            bound.offset().max(0) as usize,
        );
        (
            insert,
            (insert != bound).then(|| (insert.min(bound), insert.max(bound))),
        )
    }

    /// Puts this view's caret at `caret` with the selection reaching to `anchor` (characters):
    /// in the buffer when it has this view's caret, else in the marks this view keeps.
    pub fn place_selection(&self, anchor: usize, caret: usize) {
        let buffer = self.buffer();
        let at = |chars: usize| buffer.iter_at_offset(i32::try_from(chars).unwrap_or(i32::MAX));
        let document = self.document();
        if document.caret_owner.upgrade().is_none() {
            document.caret_owner.set(Some(self));
        }
        if self.has_caret() {
            buffer.select_range(&at(caret), &at(anchor));
            return;
        }
        let mut saved = self.imp().selection.borrow_mut();
        match saved.as_ref() {
            Some((insert, bound)) => {
                buffer.move_mark(insert, &at(caret));
                buffer.move_mark(bound, &at(anchor));
            }
            None => {
                *saved = Some((
                    buffer.create_mark(None, &at(caret), false),
                    buffer.create_mark(None, &at(anchor), false),
                ));
            }
        }
    }

    /// Puts this view's caret and selection back in the buffer, without ending column mode in
    /// any view of the document.
    pub fn restore_selection(&self) {
        let Some((insert, bound)) = self.imp().selection.borrow().clone() else {
            return;
        };
        let buffer = self.buffer();
        let (insert, bound) = (buffer.iter_at_mark(&insert), buffer.iter_at_mark(&bound));
        let views: Vec<StetView> = self
            .document_pages()
            .iter()
            .map(|page| page.column_view().clone())
            .collect();
        quietly(&views, &|| buffer.select_range(&insert, &bound));
    }

    // ----- bookmarks and marks (M7) ------------------------------------------------------

    pub fn bookmarks(&self) -> stet_domain::marks::Bookmarks {
        crate::marks::bookmarks(&self.buffer())
    }

    pub fn set_bookmarks(&self, bookmarks: &stet_domain::marks::Bookmarks) {
        crate::marks::set_bookmarks(&self.buffer(), bookmarks);
    }

    pub fn has_bookmarks(&self) -> bool {
        crate::marks::has_bookmarks(&self.buffer())
    }

    /// The picture this view's gutter draws for a bookmark.
    pub fn bookmark_pixbuf(&self) -> Option<gtk::gdk_pixbuf::Pixbuf> {
        Some(self.imp().bookmark.get()?.pixbuf())
    }

    /// Toggle Bookmark on `line`; returns whether it is bookmarked now.
    pub fn toggle_bookmark(&self, line: usize) -> bool {
        crate::marks::toggle_bookmark(&self.buffer(), line)
    }

    /// The document's bookmark history and marks.
    pub fn marks(&self) -> &DocMarks {
        &self.document().marks
    }

    /// The marks changed: their tags follow now.
    pub fn retag_marks(&self) {
        self.document().retag_now();
    }

    /// Runs one of Stet's own edits through the document's bookmark and mark tracking
    /// ([`DocMarks::track`]).
    pub fn track_edit(
        &self,
        text: &str,
        edits: &[stet_domain::text::TextEdit],
        after: Option<stet_domain::marks::Bookmarks>,
        apply: impl FnOnce() -> stet_domain::marks::Footprint,
    ) {
        let document = self.document();
        document
            .marks
            .track(&document.buffer, text, edits, after, apply);
        document.schedule_retag();
    }

    // ----- banners -----------------------------------------------------------------------

    /// Shows the banner of `kind` with `title` and `buttons`, in its place in the stack.
    pub fn show_banner(&self, kind: BannerKind, title: &str, buttons: Vec<BannerButton>) {
        self.banner(kind).show(title, buttons);
    }

    pub fn hide_banner(&self, kind: BannerKind) {
        if let Some(banner) = self.imp().banners.borrow().get(&kind) {
            banner.hide();
        }
    }

    fn banner(&self, kind: BannerKind) -> Rc<Banner> {
        let imp = self.imp();
        if let Some(banner) = imp.banners.borrow().get(&kind) {
            return banner.clone();
        }
        let banner = Rc::new(Banner::new());
        let container = imp.banner_box.get().expect("constructed");
        let before = imp
            .banners
            .borrow()
            .range(..kind)
            .next_back()
            .map(|(_, banner)| banner.widget().clone());
        container.insert_child_after(banner.widget(), before.as_ref());
        imp.banners.borrow_mut().insert(kind, banner.clone());
        banner
    }

    /// The titles of the banners on screen, top to bottom.
    pub fn banner_texts(&self) -> Vec<String> {
        self.imp()
            .banners
            .borrow()
            .values()
            .filter(|banner| banner.is_shown())
            .map(|banner| banner.title())
            .collect()
    }

    /// Clicks the button labelled `label` on a banner on screen.
    pub fn click_banner(&self, label: &str) -> bool {
        let banners: Vec<Rc<Banner>> = self.imp().banners.borrow().values().cloned().collect();
        banners
            .iter()
            .filter(|banner| banner.is_shown())
            .any(|banner| banner.click(label))
    }

    /// The labels of the buttons on the banners on screen.
    pub fn banner_buttons(&self) -> Vec<String> {
        self.imp()
            .banners
            .borrow()
            .values()
            .filter(|banner| banner.is_shown())
            .flat_map(|banner| banner.button_labels())
            .collect()
    }

    // ----- view settings and large files -------------------------------------------------

    pub fn apply_settings(&self, settings: ViewSettings) {
        self.imp().settings.set(settings);
        let large = self.large_file_mode();
        let view = self.view();
        let wrap = settings.wrap && !large && !self.imp().compared.get();
        view.set_wrap_mode(if wrap {
            gtk::WrapMode::WordChar
        } else {
            gtk::WrapMode::None
        });
        let drawer = view.space_drawer();
        drawer.set_types_for_locations(
            sourceview5::SpaceLocationFlags::ALL,
            sourceview5::SpaceTypeFlags::SPACE
                | sourceview5::SpaceTypeFlags::TAB
                | sourceview5::SpaceTypeFlags::NBSP,
        );
        drawer.set_enable_matrix(settings.whitespace && !large);
        self.buffer().set_highlight_matching_brackets(!large);
        self.show_map(settings.map && !large);
    }

    /// Whether the view is in a comparison, which keeps word wrap off (M7).
    pub fn set_compared(&self, compared: bool) {
        self.imp().compared.set(compared);
        self.apply_settings(self.imp().settings.get());
    }

    pub fn is_compared(&self) -> bool {
        self.imp().compared.get()
    }

    /// Shows or hides the document map. Hiding removes it: GtkSourceMap shown again after
    /// being hidden sets its scroll position from an empty allocation, and GTK logs a
    /// CRITICAL ("gtk_adjustment_set_value: assertion 'isfinite (value)' failed").
    fn show_map(&self, show: bool) {
        let imp = self.imp();
        if !show {
            if let Some(map) = imp.map.take() {
                imp.body.get().expect("constructed").remove(&map);
            }
            return;
        }
        let mut map = imp.map.borrow_mut();
        let map = map.get_or_insert_with(|| {
            let map = sourceview5::Map::new();
            map.set_view(self.view());
            map.add_css_class("stet-map");
            imp.body.get().expect("constructed").append(&map);
            map
        });
        map.set_visible(true);
    }

    /// Whether the document map is on screen.
    pub fn map_shown(&self) -> bool {
        self.imp()
            .map
            .borrow()
            .as_ref()
            .is_some_and(|map| map.is_visible())
    }

    // ----- indentation ---------------------------------------------------------------------

    pub fn indentation(&self) -> Indentation {
        self.document().indentation.get()
    }

    pub fn indent_source(&self) -> IndentSource {
        self.document().indent_source.get()
    }

    /// Sets the tab width and what Tab inserts, in every view of the document. The exact tab
    /// stops follow at the next layout (ADR-015, `notify::tab-width`).
    pub fn set_indentation(&self, indentation: Indentation, source: IndentSource) {
        let document = self.document();
        document.indentation.set(indentation);
        document.indent_source.set(source);
        for page in document.live_pages() {
            let view = page.view();
            if view.tab_width() != indentation.tab_width {
                view.set_tab_width(indentation.tab_width);
            }
            view.set_insert_spaces_instead_of_tabs(indentation.insert_spaces);
            view.set_indent_width(-1);
        }
    }

    /// Whether syntax highlighting is on for this document (ADR-014): set when it opens,
    /// by size, and by "Highlight Anyway".
    pub fn highlighting(&self) -> bool {
        self.document().highlighting.get()
    }

    /// Switches highlighting on or off. Off detaches the language from the buffer: with a
    /// language GtkSourceView analyses the whole text even when it draws no highlighting,
    /// and that analysis is what holds off frames (ADR-014 amendment).
    pub fn set_highlighting(&self, on: bool) {
        self.document().highlighting.set(on);
        self.sync_language();
    }

    /// The document's language: what the status bar shows and the picker sets. The buffer
    /// has it only while highlighting is on.
    pub fn language(&self) -> Option<sourceview5::Language> {
        self.document().language.borrow().clone()
    }

    pub fn set_language(&self, language: Option<&sourceview5::Language>) {
        self.document().language.replace(language.cloned());
        self.sync_language();
    }

    fn sync_language(&self) {
        let document = self.document();
        let on = document.highlighting.get() && !document.state.borrow().loading;
        let buffer = self.buffer();
        let wanted = if on {
            document.language.borrow().clone()
        } else {
            None
        };
        if buffer.language() != wanted {
            buffer.set_language(wanted.as_ref());
        }
        buffer.set_highlight_syntax(on);
    }

    // ----- loading -----------------------------------------------------------------------

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
    }

    /// After the first piece of a load: waits (up to a second) until the window is shown, then
    /// lets idle work run once. Once the rest of the file is in, GtkTextView's validation of
    /// its lines starves idles until it is done (S1: about 40 s for 1M lines), and GTK's
    /// accessibility registration of a new window is one of them.
    async fn settle_before_streaming(&self) {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(1)
            && !self.root().is_some_and(|root| root.is_mapped())
        {
            glib::timeout_future(Duration::from_millis(5)).await;
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
        let handler = Rc::new(RefCell::new(None));
        let id = clock.connect_after_paint(glib::clone!(
            #[weak(rename_to = page)]
            self,
            #[strong]
            handler,
            move |clock| {
                if let Some(id) = handler.take() {
                    clock.disconnect(id);
                }
                page.update_load_stats(|stats| stats.first_frame = Some(Instant::now()));
            }
        ));
        handler.replace(Some(id));
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

    pub fn set_watch(&self, watch: Option<FileWatch>) {
        self.document().watch.replace(watch);
    }

    /// The vertical scroll position, to keep it across a reload.
    pub fn scroll_position(&self) -> f64 {
        self.view()
            .vadjustment()
            .map_or(0.0, |adjustment| adjustment.value())
    }

    pub fn set_scroll_position(&self, value: f64) {
        if let Some(adjustment) = self.view().vadjustment() {
            adjustment.set_value(value);
        }
    }

    /// Moves the cursor to `position`, clamped to the text, and scrolls it into view.
    pub fn go_to(&self, position: Position) {
        let buffer = self.buffer();
        let Some(iter) = self.iter_at(position) else {
            return;
        };
        buffer.place_cursor(&iter);
        self.view()
            .scroll_to_mark(&buffer.get_insert(), 0.1, true, 0.0, 0.3);
    }

    /// Moves the cursor to `position`, clamped to the text, without scrolling.
    pub fn place_cursor(&self, position: Position) {
        if let Some(iter) = self.iter_at(position) {
            self.buffer().place_cursor(&iter);
        }
    }

    fn iter_at(&self, position: Position) -> Option<gtk::TextIter> {
        let buffer = self.buffer();
        let last_line = (buffer.line_count() - 1).max(0);
        let line = i32::try_from(position.line)
            .unwrap_or(i32::MAX)
            .min(last_line);
        let mut iter = buffer.iter_at_line(line)?;
        let mut column = position.column;
        while column > 0 && !iter.ends_line() {
            iter.forward_char();
            column -= 1;
        }
        Some(iter)
    }

    /// Selects the character at `offset` and shows it (the unmappable dialog's Go To).
    pub fn select_char(&self, offset: usize) {
        let buffer = self.buffer();
        let start = buffer.iter_at_offset(i32::try_from(offset).unwrap_or(i32::MAX));
        let mut end = start;
        end.forward_char();
        buffer.select_range(&start, &end);
        self.view()
            .scroll_to_mark(&buffer.get_insert(), 0.1, true, 0.0, 0.3);
        self.view().grab_focus();
    }

    /// The cursor as a zero-based line and character column.
    pub fn cursor(&self) -> Position {
        let buffer = self.buffer();
        let iter = buffer.iter_at_mark(&buffer.get_insert());
        Position::new(iter.line() as usize, iter.line_offset() as usize)
    }

    pub fn tab_stops(&self) -> &TabStops {
        &self.imp().tab_stops
    }

    /// Applies a theme's scheme, including the search, mark and comparison styles (DESIGN.md),
    /// and the bookmark in the gutter.
    pub fn set_scheme(&self, scheme: &sourceview5::StyleScheme) {
        let buffer = self.buffer();
        buffer.set_style_scheme(Some(scheme));
        let table = buffer.tag_table();
        for (tag, style) in [
            (CURRENT_MATCH_TAG, SEARCH_CURRENT_STYLE),
            (OWN_MATCH_TAG, "search-match"),
        ] {
            if let Some(tag) = table.lookup(tag) {
                style_tag(&tag, scheme, style);
            }
        }
        for style in stet_domain::marks::MarkStyle::ALL {
            if let Some(tag) = table.lookup(crate::marks::tag_name(style)) {
                style_tag(&tag, scheme, crate::marks::scheme_style(style));
            }
        }
        crate::compare::recolor(&buffer, scheme);
        self.document().search.set_scheme(scheme);
        let color = scheme
            .style(BOOKMARK_STYLE)
            .and_then(|style| style.foreground())
            .and_then(|color| gtk::gdk::RGBA::parse(color.as_str()).ok())
            .unwrap_or_else(|| self.view().color());
        if let Some(attributes) = self.imp().bookmark.get() {
            attributes.set_pixbuf(&crate::marks::bookmark_pixbuf(&color, 64));
        }
        self.view().queue_draw();
    }

    /// Marks `start..end` as the current search match: highlighted, and remembered with marks
    /// so the next search continues from it.
    pub fn set_current_match(&self, start: &gtk::TextIter, end: &gtk::TextIter) {
        self.clear_current_match();
        let buffer = self.buffer();
        if let Some(tag) = buffer.tag_table().lookup(CURRENT_MATCH_TAG) {
            buffer.apply_tag(&tag, start, end);
        }
        buffer.create_mark(Some(MATCH_START_MARK), start, true);
        buffer.create_mark(Some(MATCH_END_MARK), end, false);
        self.keep_current_match_on_top();
    }

    /// GtkSourceView raises its own search-match tag to the top priority whenever it scans,
    /// which would hide the current match's style; put ours back above it.
    pub fn keep_current_match_on_top(&self) {
        let buffer = self.buffer();
        if buffer.mark(MATCH_START_MARK).is_none() {
            return;
        }
        let table = buffer.tag_table();
        if let Some(tag) = table.lookup(CURRENT_MATCH_TAG)
            && tag.priority() < table.size() - 1
        {
            tag.set_priority(table.size() - 1);
        }
    }

    pub fn current_match(&self) -> Option<(gtk::TextIter, gtk::TextIter)> {
        let buffer = self.buffer();
        let start = buffer.mark(MATCH_START_MARK)?;
        let end = buffer.mark(MATCH_END_MARK)?;
        Some((buffer.iter_at_mark(&start), buffer.iter_at_mark(&end)))
    }

    pub fn clear_current_match(&self) {
        let buffer = self.buffer();
        buffer.remove_tag_by_name(CURRENT_MATCH_TAG, &buffer.start_iter(), &buffer.end_iter());
        for name in [MATCH_START_MARK, MATCH_END_MARK] {
            if let Some(mark) = buffer.mark(name) {
                buffer.delete_mark(&mark);
            }
        }
    }

    /// The document's search state: its search contexts and our own matcher's matches.
    pub fn search(&self) -> &PageSearch {
        &self.document().search
    }
}

/// Runs `f` with every view in `views` letting caret moves through.
fn quietly(views: &[StetView], f: &dyn Fn()) {
    match views.split_first() {
        Some((view, rest)) => view.quietly(|| quietly(rest, f)),
        None => f(),
    }
}

/// Why a save was refused before writing anything.
pub fn read_only_message(reason: ReadOnly) -> &'static str {
    match reason {
        ReadOnly::Binary => {
            "This binary file is read-only; choose Edit Anyway on its banner to edit it."
        }
        ReadOnly::NotWritable => {
            "You may not write this file; save a copy with Save As, or edit it with sudoedit."
        }
        ReadOnly::DisplayBreaks => {
            "Long lines are broken for display, so this text isn't the file's and can't be saved."
        }
        ReadOnly::Comparison => {
            "This copy was opened for the comparison and can't be changed; Clear Compare closes it."
        }
    }
}
