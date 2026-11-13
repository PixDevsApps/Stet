//! The tab widget: a GtkSourceView over its document's buffer, with the banners above it. A
//! clone in the other view of a split window is a second page over the same [`Document`], with
//! a view, a caret and banners of its own.

use super::appearance::ViewSettings;
use super::banners::Banner;
use super::column::StetView;
use super::document::Document;
use super::session::TabSession;
use super::state::{Activity, DocState};
use super::tabstops::TabStops;
use crate::monitor::FileWatch;
use gtk4 as gtk;
use gtk4::glib;
use gtk4::subclass::prelude::*;
use libadwaita::prelude::*;
use sourceview5::prelude::*;
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;
use stet_domain::document::banners::BannerKind;
use stet_domain::document::{
    DiskState, FIRST_LINE_READ, FIRST_LINE_SCAN, ReadOnly, TextOrigin, Unwritable, first_line_name,
    tilde, untitled_name,
};
use stet_domain::session::{Fingerprint, RecordId, TabRecord};
use stet_domain::text::Eol;
use stet_infrastructure::encoding::Encoding;
use stet_infrastructure::fs::{DocInfo, DocumentFormat, EncodingChoice, SaveOutcome};

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct EditorPage {
        pub(in crate::editor) view: OnceCell<StetView>,
        pub(in crate::editor) banner_box: OnceCell<gtk::Box>,
        pub(in crate::editor) banners: RefCell<BTreeMap<BannerKind, Rc<Banner>>>,
        pub(in crate::editor) tab_stops: TabStops,
        pub(in crate::editor) settings: Cell<ViewSettings>,
        pub(in crate::editor) map: RefCell<Option<sourceview5::Map>>,
        pub(in crate::editor) body: OnceCell<gtk::Box>,
        pub(in crate::editor) document: OnceCell<Rc<Document>>,
        /// This view's caret and selection while another view of the document is in front:
        /// GTK keeps one caret per buffer.
        pub(in crate::editor) selection: RefCell<Option<(gtk::TextMark, gtk::TextMark)>>,
        /// A clone's own id in the session.
        pub(in crate::editor) clone_id: RefCell<Option<RecordId>>,
        /// The restored first visible line, kept until the view is first shown and has a
        /// scroll position of its own.
        pub(in crate::editor) top_line: Cell<Option<usize>>,
        /// A restored tab's caret and scroll position, applied once its text is in.
        pub(in crate::editor) pending: RefCell<Option<Box<TabRecord>>>,
        /// Shown in a comparison: no word wrap, so the two views' lines line up.
        pub(in crate::editor) compared: Cell<bool>,
        pub(in crate::editor) bookmark: OnceCell<sourceview5::MarkAttributes>,
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
            self.view.get().is_some_and(WidgetExt::grab_focus)
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
        Self::with_document(&Document::new(state), scheme, settings)
    }

    /// A second view of this page's document (Clone to Other View): the same buffer,
    /// with a caret, scroll position and banners of its own.
    pub fn clone_view(
        &self,
        scheme: Option<&sourceview5::StyleScheme>,
        settings: ViewSettings,
    ) -> Self {
        let page = Self::with_document(self.document(), scheme, settings);
        page.save_selection();
        page
    }

    fn with_document(
        document: &Rc<Document>,
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

        // Bookmarks in the gutter, toggled by a click there.
        let attributes = sourceview5::MarkAttributes::new();
        view.set_mark_attributes(super::marks::BOOKMARK_CATEGORY, &attributes, 10);
        view.set_show_line_marks(true);
        // Line numbers and bookmarks at the top of their line, which a comparison pads below.
        let gutter = ViewExt::gutter(
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
        let weak = Rc::downgrade(document);
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
            let weak = Rc::downgrade(document);
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
        document.sync_editable();
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

    pub(super) fn document(&self) -> &Rc<Document> {
        self.imp().document.get().expect("constructed")
    }

    /// Whether `other` shows the same document (one is a clone of the other).
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

    /// The view with column mode.
    pub fn column_view(&self) -> &StetView {
        self.imp().view.get().expect("constructed")
    }

    pub fn buffer(&self) -> sourceview5::Buffer {
        self.document().buffer.clone()
    }

    pub fn text(&self) -> String {
        let buffer = self.buffer();
        buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), true)
            .to_string()
    }

    pub fn revision(&self) -> u64 {
        self.document().revision.get()
    }

    /// When the text last changed, for the backups' idle check.
    pub fn last_change(&self) -> Option<Instant> {
        self.document().last_change.get()
    }

    pub fn set_watch(&self, watch: Option<FileWatch>) {
        self.document().watch.replace(watch);
    }

    // ----- the document's state ----------------------------------------------------------

    pub fn state(&self) -> DocState {
        self.document().state.borrow().clone()
    }

    /// A general change to the document's state, for what no transition below names; the views
    /// follow whether it can be edited.
    pub fn update_state(&self, update: impl FnOnce(&mut DocState)) {
        let document = self.document();
        update(&mut document.state.borrow_mut());
        document.sync_editable();
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

    pub fn read_only(&self) -> Option<ReadOnly> {
        self.document().state.borrow().read_only()
    }

    /// Large-file mode (≥ 50 MiB). Highlight-all and smart highlight must stay off here.
    pub fn large_file_mode(&self) -> bool {
        self.document().state.borrow().large()
    }

    /// The document is now the file `info` describes, its text from `origin`.
    pub fn set_file_info(&self, info: &DocInfo, origin: TextOrigin) {
        self.update_state(|state| state.set_file_info(info, origin));
    }

    /// "Edit Anyway": the read-only state that only protects the file is lifted.
    pub fn edit_anyway(&self) {
        self.update_state(DocState::edit_anyway);
    }

    /// A save starts writing a snapshot of the text: the file isn't checked meanwhile.
    pub fn begin_save(&self) {
        self.update_state(|state| state.set_activity(Activity::Saving));
    }

    /// The save that [`Self::begin_save`] started is over, whatever its outcome.
    pub fn end_save(&self) {
        self.update_state(|state| state.end_activity(Activity::Saving));
    }

    /// A save wrote `written` to `path`: that is the document's file and baseline now.
    pub fn saved_to(&self, path: PathBuf, outcome: &SaveOutcome, written: DocumentFormat) {
        self.update_state(|state| state.saved(path, outcome, written));
    }

    /// Why the last save could not write the file.
    pub fn set_unwritable(&self, reason: Unwritable) {
        self.update_state(|state| state.unwritable = Some(reason));
    }

    /// The user agreed to save despite a lossy decode or formatted text; the next save of this
    /// text doesn't ask again.
    pub fn confirm_save(&self) {
        self.update_state(|state| state.save.confirmed = true);
    }

    /// The next save writes `encoding`, with a BOM when `bom`.
    pub fn convert_encoding(&self, encoding: &'static Encoding, bom: bool) {
        self.update_state(|state| state.convert_encoding(encoding, bom));
    }

    /// The next save writes `eol` everywhere, which also settles mixed line endings.
    pub fn convert_eol(&self, eol: Eol) {
        self.update_state(|state| state.convert_eol(eol));
    }

    /// Keeps the mixed line endings for now: the banner goes, saving still warns.
    pub fn keep_mixed_endings(&self) {
        self.update_state(|state| state.eol.kept = true);
    }

    /// The encoding the user picked with Reinterpret, kept when the file is reloaded.
    pub fn set_encoding_choice(&self, choice: Option<EncodingChoice>) {
        self.update_state(|state| state.chosen = choice);
    }

    /// The file changed on disk while the document had unsaved changes.
    pub fn disk_changed(&self) {
        self.update_state(|state| state.disk = DiskState::Changed);
    }

    /// The file is gone; `clean_revision` is the revision now when there were no unsaved
    /// changes.
    pub fn disk_deleted(&self, clean_revision: Option<u64>) {
        self.update_state(|state| state.disk = DiskState::Deleted { clean_revision });
    }

    /// The file on disk is `now`, and the document takes it as its baseline.
    pub fn disk_settled(&self, now: Fingerprint) {
        self.update_state(|state| state.settle_disk(now));
    }

    /// The file was renamed to `path` (`canonical` with symlinks resolved).
    pub fn moved_to(&self, path: PathBuf, canonical: PathBuf) {
        self.update_state(|state| {
            state.path = Some(path);
            state.canonical = Some(canonical);
        });
    }

    /// The name the user gave an untitled document; `None` names it by its first line again.
    pub fn set_custom_name(&self, name: Option<String>) {
        self.update_state(|state| state.custom_name = name);
    }

    // ----- names -------------------------------------------------------------------------

    /// The file name; for an untitled document the name the user gave it, else its first line
    /// with text, else `Untitled N`; for a text a comparison opened, what it is.
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

    /// The first line with text of an untitled document, cut for a tab; read again only
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

    // ----- changes -----------------------------------------------------------------------

    /// Unsaved changes: edits, a changed save format, or a file deleted from disk. Never while
    /// loading, when inserting the text marks the buffer modified.
    pub fn is_dirty(&self) -> bool {
        let state = self.document().state.borrow();
        !state.is_loading()
            && (self.buffer().is_modified()
                || state.save.format_changed
                || matches!(state.disk, DiskState::Deleted { .. }))
    }

    /// An untitled tab nobody has typed in: opening a file replaces it.
    pub fn is_pristine(&self) -> bool {
        let buffer = self.buffer();
        let state = self.document().state.borrow();
        state.path.is_none()
            && state.temporary.is_none()
            && !state.is_loading()
            && !state.save.format_changed
            && !buffer.is_modified()
            && buffer.char_count() == 0
            && !buffer.can_undo()
    }

    /// A text tool is changing the document across main-loop iterations (a large result
    /// inserted in pieces): the view takes no edits and other tools wait.
    pub fn is_busy(&self) -> bool {
        self.document().busy.get()
    }

    pub fn set_busy(&self, busy: bool) {
        self.document().busy.set(busy);
        self.document().sync_editable();
    }

    // ----- the session -------------------------------------------------------------------

    /// The document's part in the session.
    pub fn session(&self) -> std::cell::Ref<'_, TabSession> {
        self.document().session.borrow()
    }

    pub fn update_session<R>(&self, update: impl FnOnce(&mut TabSession) -> R) -> R {
        let result = update(&mut self.document().session.borrow_mut());
        self.document().load_changed.notify();
        result
    }

    /// This tab's own id in the session when it is a clone, made on first use.
    pub fn clone_id(&self) -> RecordId {
        self.imp()
            .clone_id
            .borrow_mut()
            .get_or_insert_with(stet_infrastructure::session_store::new_id)
            .clone()
    }

    pub fn set_clone_id(&self, id: RecordId) {
        self.imp().clone_id.replace(Some(id));
    }

    /// The restored first visible line of this view, until it is first shown.
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
}
