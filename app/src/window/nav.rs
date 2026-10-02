//! Navigation (M8): the Ctrl+Tab switcher in most-recently-used order, back and forward
//! through the places that jumps leave (Alt+Left and Alt+Right, the mouse's back and forward
//! buttons), and untitled tabs named after their first line.
//!
//! **Ctrl+Tab.** `next-recent-tab` and `previous-recent-tab` own Ctrl+Tab and Ctrl+Shift+Tab as
//! application accelerators (the registry), and AdwTabView's own shortcuts have been off since
//! M1. GTK runs application accelerators in the window's capture phase, before the tab view or
//! an editor's key handling, so nothing else sees those keys first. Each press steps through
//! the tabs in most-recently-used order; a key controller on the window, also in the capture
//! phase, sees Ctrl go up and switches to the selected tab, and Escape cancels. A press without
//! Ctrl held (the palette, a menu, a quick tap's release already seen) switches at once, to
//! the previous tab. The switcher shows after [`SWITCHER_DELAY`], so a tap never flashes it.
//!
//! **Back and forward.** Jumps record the place they leave: a tab switch, Go to Line, Find
//! Next and Previous, opening the find bar, the results panel and Next/Previous Search Result,
//! quick open, opening a file at a line ([`Window::note_jump`], which M5's brace jump can call
//! too). Places near each other (10 lines) merge; edits move them along; closing a tab drops
//! its places (`stet_domain::nav::History`). Alt+Left and Alt+Right are application
//! accelerators as well, which takes them from GtkSourceView's `move-words`.

use super::Window;
use super::quick_open::QuickOpen;
use super::rif::{RifRun, RifStats};
use crate::editor::EditorPage;
use gtk4 as gtk;
use gtk4::{gdk, glib, pango};
use libadwaita::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};
use stet_domain::document::tilde;
use stet_domain::nav::{History, Location, Mru};
use stet_domain::text::TextEdit;
use stet_domain::untitled::{extension_from_globs, save_file_name};

/// How long Ctrl+Tab has to be held before the switcher shows, so a quick tap never flashes it.
pub const SWITCHER_DELAY: Duration = Duration::from_millis(120);

/// How long going back waits for a restored tab to load before it places the caret.
const LOAD_WAIT: Duration = Duration::from_secs(10);

/// The Ctrl+Tab overlay: the tabs in most-recently-used order, the selection highlighted. It
/// never takes the focus; the window's key controller drives it.
pub struct Switcher {
    pub root: gtk::Box,
    list: gtk::ListBox,
    rows: RefCell<Vec<EditorPage>>,
}

impl Switcher {
    fn new() -> Self {
        let heading = gtk::Label::builder()
            .label("Switch to")
            .xalign(0.0)
            .css_classes(["stet-switcher-heading", "dim-label"])
            .build();
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .can_focus(false)
            .can_target(false)
            .build();
        let root = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .can_focus(false)
            .can_target(false)
            .visible(false)
            .css_classes(["stet-switcher", "stet-mono"])
            .build();
        root.append(&heading);
        root.append(&list);
        Self {
            root,
            list,
            rows: RefCell::new(Vec::new()),
        }
    }

    /// Shows `pages` with the one at `selected` highlighted.
    fn fill(&self, pages: Vec<EditorPage>, selected: Option<usize>) {
        self.list.remove_all();
        let home = std::env::home_dir();
        for page in &pages {
            let name = gtk::Label::builder()
                .label(stet_domain::document::tab_title(
                    &page.name(),
                    page.is_dirty(),
                ))
                .xalign(0.0)
                .max_width_chars(48)
                .ellipsize(pango::EllipsizeMode::End)
                .build();
            let dir = gtk::Label::builder()
                .label(
                    page.path()
                        .and_then(|path| path.parent().map(|dir| tilde(dir, home.as_deref())))
                        .unwrap_or_default(),
                )
                .xalign(0.0)
                .hexpand(true)
                .max_width_chars(48)
                .ellipsize(pango::EllipsizeMode::Start)
                .css_classes(["dim-label"])
                .build();
            let content = gtk::Box::new(gtk::Orientation::Horizontal, 16);
            content.append(&name);
            content.append(&dir);
            self.list
                .append(&gtk::ListBoxRow::builder().child(&content).build());
        }
        match selected.and_then(|index| self.list.row_at_index(index as i32)) {
            Some(row) => self.list.select_row(Some(&row)),
            None => self.list.unselect_all(),
        }
        self.rows.replace(pages);
    }

    pub fn is_shown(&self) -> bool {
        self.root.is_visible()
    }

    /// The tab names in the switcher, and the selected one, for the self-test.
    pub fn shown(&self) -> (Vec<String>, Option<String>) {
        let rows = self.rows.borrow();
        let selected = self
            .list
            .selected_row()
            .and_then(|row| rows.get(row.index() as usize))
            .map(EditorPage::name);
        (rows.iter().map(EditorPage::name).collect(), selected)
    }
}

/// The window's navigation state (M8): the switcher, the back and forward history, quick open,
/// the clicked tab of the tab menu, and Replace in Files.
pub struct Nav {
    mru: RefCell<Mru<EditorPage>>,
    history: RefCell<History<EditorPage>>,
    /// The tab shown last: a switch to another tab records its caret as the place left.
    shown: glib::WeakRef<EditorPage>,
    /// Back or forward is moving the caret; that is not a jump to record.
    navigating: Cell<bool>,
    ctrl_held: Cell<bool>,
    show_pending: RefCell<Option<glib::SourceId>>,
    pub switcher: Switcher,
    /// The window's key controller for the switcher (Ctrl released, Escape, Enter), in the
    /// capture phase; the self-test emits key events on it.
    pub keys: gtk::EventControllerKey,
    pub back_button: gtk::GestureClick,
    pub forward_button: gtk::GestureClick,
    pub quick_open: Rc<QuickOpen>,
    pub(super) rif: RefCell<Option<RifRun>>,
    pub(super) rif_runs: Cell<u64>,
    pub(super) rif_stats: RefCell<Option<RifStats>>,
}

impl Nav {
    pub fn new() -> Self {
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let button = |number: u32| {
            let gesture = gtk::GestureClick::new();
            gesture.set_button(number);
            gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
            gesture
        };
        Self {
            mru: RefCell::new(Mru::new()),
            history: RefCell::new(History::new()),
            shown: glib::WeakRef::new(),
            navigating: Cell::new(false),
            ctrl_held: Cell::new(false),
            show_pending: RefCell::new(None),
            switcher: Switcher::new(),
            keys,
            back_button: button(8),
            forward_button: button(9),
            quick_open: QuickOpen::new(),
            rif: RefCell::new(None),
            rif_runs: Cell::new(0),
            rif_stats: RefCell::new(None),
        }
    }
}

fn caret(page: &EditorPage) -> usize {
    let buffer = page.buffer();
    usize::try_from(buffer.iter_at_mark(&buffer.get_insert()).offset()).unwrap_or(0)
}

fn line_of(page: &EditorPage, offset: usize) -> usize {
    let offset = i32::try_from(offset).unwrap_or(i32::MAX);
    usize::try_from(page.buffer().iter_at_offset(offset).line()).unwrap_or(0)
}

impl Window {
    pub(super) fn connect_nav(&self) {
        let inner = &self.inner;
        let nav = &inner.nav;
        inner.window.add_controller(nav.keys.clone());
        nav.keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, _| Window { inner }.on_nav_key_pressed(key)
        ));
        nav.keys.connect_key_released(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |_, key, _, _| Window { inner }.on_nav_key_released(key)
        ));
        for (gesture, back) in [(&nav.back_button, true), (&nav.forward_button, false)] {
            gesture.connect_pressed(glib::clone!(
                #[weak(rename_to = inner)]
                self.inner,
                move |gesture, _, _, _| {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    Window { inner }.mouse_navigation(back);
                }
            ));
            inner.window.add_controller(gesture.clone());
        }
        inner.window.connect_is_active_notify(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |window| {
                if !window.is_active() {
                    let window = Window { inner };
                    window.inner.nav.ctrl_held.set(false);
                    if window.inner.nav.mru.borrow().is_switching() {
                        window.cancel_switch();
                    }
                }
            }
        ));
        self.connect_quick_open();
    }

    /// A new tab: it joins the switcher's order at the end until it is shown, its edits move
    /// the history's places along, and an untitled tab follows its first line.
    pub(super) fn on_nav_page_attached(&self, page: &EditorPage) {
        self.inner.nav.mru.borrow_mut().add_last(page.clone());
        let buffer = page.buffer();
        buffer.connect_insert_text(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            #[weak]
            page,
            move |_, iter, text| {
                let at = usize::try_from(iter.offset()).unwrap_or(0);
                Window { inner }.remap_history(&page, at, || TextEdit::insert(at, text));
            }
        ));
        buffer.connect_delete_range(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            #[weak]
            page,
            move |_, start, end| {
                let (start, end) = (
                    usize::try_from(start.offset()).unwrap_or(0),
                    usize::try_from(end.offset()).unwrap_or(0),
                );
                Window { inner }.remap_history(&page, start, || TextEdit::delete(start..end));
            }
        ));
        buffer.connect_changed(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            #[weak]
            page,
            move |_| {
                if page.path().is_none() && page.first_line_changed() {
                    Window { inner }.refresh_page(&page);
                }
            }
        ));
    }

    pub(super) fn on_nav_page_detached(&self, page: &EditorPage) {
        let nav = &self.inner.nav;
        let switching = {
            let mut mru = nav.mru.borrow_mut();
            mru.remove(page);
            mru.is_switching()
        };
        nav.history.borrow_mut().remove_document(page);
        if switching {
            self.refresh_switcher();
        } else if !nav.switcher.is_shown() {
            nav.switcher.rows.borrow_mut().retain(|row| row != page);
        }
    }

    /// A tab is shown: it becomes the most recently used one, and the tab left behind records
    /// its caret as a place to go back to.
    pub(super) fn on_nav_page_selected(&self) {
        let nav = &self.inner.nav;
        let Some(page) = self.current_page() else {
            return;
        };
        nav.mru.borrow_mut().activate(page.clone());
        let previous = nav.shown.upgrade();
        nav.shown.set(Some(&page));
        if let Some(previous) = previous
            && previous != page
            && !nav.navigating.get()
            && self.tab_page(&previous).is_some()
        {
            self.record_place(&previous);
        }
        self.update_pin_actions();
    }

    // ----- back and forward ----------------------------------------------------------------

    /// Records the current place before a jump, so Go Back returns to it. Brace jumps, search
    /// results and other jumps call it first; moves made by Go Back and Go Forward don't count.
    pub fn note_jump(&self) {
        if let Some(page) = self.current_page() {
            self.record_place(&page);
        }
    }

    fn record_place(&self, page: &EditorPage) {
        let nav = &self.inner.nav;
        if nav.navigating.get() || page.is_loading() || page.session().stub.is_some() {
            return;
        }
        nav.history
            .borrow_mut()
            .record(Location::new(page.clone(), caret(page)), line_of);
    }

    /// Moves the places in `page` through an edit at character `at`; `edit` is only built when a
    /// place lies after it.
    fn remap_history(&self, page: &EditorPage, at: usize, edit: impl FnOnce() -> TextEdit) {
        let mut history = self.inner.nav.history.borrow_mut();
        let affected = history
            .entries()
            .iter()
            .any(|entry| entry.doc == *page && entry.offset > at);
        if affected {
            history.remap(page, &[edit()]);
        }
    }

    /// Go Back (`back`) or Go Forward: to the place recorded before the current one, or after.
    pub fn navigate(&self, back: bool) {
        let Some(page) = self.current_page() else {
            return;
        };
        let here = Location::new(page.clone(), caret(&page));
        let target = {
            let mut history = self.inner.nav.history.borrow_mut();
            if back {
                history.back(here, line_of)
            } else {
                history.forward(here, line_of)
            }
        };
        if let Some(target) = target {
            self.go_to_place(target);
        }
    }

    /// The mouse's back (button 8) and forward (button 9) buttons.
    pub fn mouse_navigation(&self, back: bool) {
        self.navigate(back);
    }

    /// Shows the tab of `place` and puts the caret there, once a restored tab has loaded.
    fn go_to_place(&self, place: Location<EditorPage>) {
        let nav = &self.inner.nav;
        nav.navigating.set(true);
        self.select(&place.doc);
        nav.navigating.set(false);
        let window = self.clone();
        self.spawn(async move {
            let page = place.doc;
            if !window.ensure_loaded(&page).await {
                return;
            }
            let started = Instant::now();
            while page.is_loading() && started.elapsed() < LOAD_WAIT {
                glib::timeout_future(Duration::from_millis(10)).await;
            }
            if window.tab_page(&page).is_none() {
                return;
            }
            let buffer = page.buffer();
            let offset = i32::try_from(place.offset)
                .unwrap_or(i32::MAX)
                .min(buffer.char_count());
            window.inner.nav.navigating.set(true);
            buffer.place_cursor(&buffer.iter_at_offset(offset));
            page.view()
                .scroll_to_mark(&buffer.get_insert(), 0.1, false, 0.0, 0.0);
            window.inner.nav.navigating.set(false);
            window.restore_focus();
        });
    }

    /// The history's places as (tab name, line), and the index of the current one, for the
    /// self-test.
    pub fn history_places(&self) -> (Vec<(String, usize)>, Option<usize>) {
        let history = self.inner.nav.history.borrow();
        let places = history
            .entries()
            .iter()
            .map(|entry| (entry.doc.name(), line_of(&entry.doc, entry.offset) + 1))
            .collect::<Vec<_>>();
        let current = history.current().and_then(|current| {
            history
                .entries()
                .iter()
                .position(|entry| std::ptr::eq(entry, current))
        });
        (places, current)
    }

    // ----- the switcher ----------------------------------------------------------------------

    /// Ctrl+Tab (`forward`) or Ctrl+Shift+Tab: the next or previous tab in most-recently-used
    /// order. With Ctrl held the switcher shows and the next press moves on; without, the
    /// switch happens at once.
    pub fn switch_recent(&self, forward: bool) {
        let nav = &self.inner.nav;
        if let Some(page) = self.current_page() {
            nav.mru.borrow_mut().activate(page);
        }
        {
            let mut mru = nav.mru.borrow_mut();
            if forward {
                mru.next();
            } else {
                mru.prev();
            }
        }
        if !self.ctrl_held() {
            self.commit_switch();
            return;
        }
        self.refresh_switcher();
        if !nav.switcher.is_shown() && nav.show_pending.borrow().is_none() {
            let weak = Rc::downgrade(&self.inner);
            let source = glib::timeout_add_local_once(SWITCHER_DELAY, move || {
                if let Some(inner) = weak.upgrade() {
                    inner.nav.show_pending.replace(None);
                    if inner.nav.mru.borrow().is_switching() {
                        inner.nav.switcher.root.set_visible(true);
                    }
                }
            });
            nav.show_pending.replace(Some(source));
        }
    }

    /// Whether Ctrl is down: as the window's key controller saw it, or as the keyboard says.
    fn ctrl_held(&self) -> bool {
        self.inner.nav.ctrl_held.get()
            || gtk::prelude::WidgetExt::display(&self.inner.window)
                .default_seat()
                .and_then(|seat| seat.keyboard())
                .is_some_and(|keyboard| {
                    keyboard
                        .modifier_state()
                        .contains(gdk::ModifierType::CONTROL_MASK)
                })
    }

    fn refresh_switcher(&self) {
        let nav = &self.inner.nav;
        let (pages, selected) = {
            let mru = nav.mru.borrow();
            (mru.iter().cloned().collect(), mru.selected_index())
        };
        nav.switcher.fill(pages, selected);
    }

    /// Releasing Ctrl: shows the selected tab.
    pub fn commit_switch(&self) {
        let nav = &self.inner.nav;
        let target = nav.mru.borrow_mut().commit().cloned();
        self.hide_switcher();
        if let Some(page) = target
            && self.current_page().as_ref() != Some(&page)
        {
            self.select(&page);
        }
    }

    /// Escape: back to the tab the switch started from.
    pub fn cancel_switch(&self) {
        self.inner.nav.mru.borrow_mut().cancel();
        self.hide_switcher();
    }

    fn hide_switcher(&self) {
        let nav = &self.inner.nav;
        if let Some(source) = nav.show_pending.take() {
            source.remove();
        }
        nav.switcher.root.set_visible(false);
    }

    /// The tabs in most-recently-used order.
    pub(super) fn recent_pages(&self) -> Vec<EditorPage> {
        self.inner.nav.mru.borrow().iter().cloned().collect()
    }

    /// The tabs in most-recently-used order, for the self-test.
    pub fn recent_order(&self) -> Vec<String> {
        self.inner
            .nav
            .mru
            .borrow()
            .iter()
            .map(EditorPage::name)
            .collect()
    }

    pub fn switching(&self) -> bool {
        self.inner.nav.mru.borrow().is_switching()
    }

    fn on_nav_key_pressed(&self, key: gdk::Key) -> glib::Propagation {
        let nav = &self.inner.nav;
        match key {
            gdk::Key::Control_L | gdk::Key::Control_R => {
                nav.ctrl_held.set(true);
                glib::Propagation::Proceed
            }
            gdk::Key::Escape if self.switching() => {
                self.cancel_switch();
                glib::Propagation::Stop
            }
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter if self.switching() => {
                self.commit_switch();
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    }

    fn on_nav_key_released(&self, key: gdk::Key) {
        if matches!(key, gdk::Key::Control_L | gdk::Key::Control_R) {
            self.inner.nav.ctrl_held.set(false);
            if self.switching() {
                self.commit_switch();
            }
        }
    }

    // ----- untitled names --------------------------------------------------------------------

    /// The file name Save As proposes: an untitled document's name (its first line) made safe
    /// for a file name, with the language's extension or `.txt`; a file's own name otherwise.
    pub fn save_as_name(&self, page: &EditorPage) -> String {
        if let Some(name) = page.path().and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        }) {
            return name;
        }
        let extension = page.language().and_then(|language| {
            let globs = language.globs();
            extension_from_globs(globs.iter().map(|glob| glob.as_str()))
        });
        save_file_name(&page.name(), extension.as_deref())
            .or_else(|| save_file_name(&page.untitled_label(), extension.as_deref()))
            .unwrap_or_else(|| "untitled.txt".to_owned())
    }
}
