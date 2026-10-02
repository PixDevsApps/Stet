//! The editor window (DESIGN.md "Window layout"): the tab strip with the hamburger menu on
//! top, the editor with its banners, the find bar, the search results panel and the status
//! bar below; the command palette and the language and encoding pickers as popovers. There
//! is no toolbar and no title bar.

mod a11y;
mod banners;
mod compare;
mod default_editor;
pub mod dialogs;
mod disk;
mod encoding;
mod encoding_picker;
mod fif;
mod files;
mod find;
mod focus;
mod indentation;
mod language;
mod marking;
mod nav;
mod palette;
mod pins;
mod quick_open;
mod replace;
mod results;
mod rif;
mod session;
mod settings;
mod smart;
mod split;
mod status;
mod tabs;
mod tool_dialogs;
mod toolbox;

use crate::actions;
use crate::config::AppConfig;
use crate::editor::{DocState, EditorPage, ReadOnly, ViewSettings};
use crate::preferences::Preferences;
use crate::theme::Appearance;
use crate::worker;
use dialogs::SaveChoice;
use gtk4 as gtk;
use gtk4::{gdk, gio, glib};
use libadwaita as adw;
use libadwaita::prelude::*;
use sourceview5::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::OsString;
use std::future::Future;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;
use stet_domain::actions::{Accelerator, ActionId, ActionKind};
use stet_domain::document::{next_untitled_number, tab_title, window_title};
use stet_domain::location::{self, LineCol};
use stet_domain::recent::RecentFiles;
use stet_domain::text::Position;

pub use compare::CompareRun;
pub use encoding::grouped_entries;
pub use encoding_picker::{EncodingPicker, PickerMode};
pub use fif::FifStats;
pub use find::{BarMode, Field, FindBar, Tone};
pub use indentation::{IndentChoice, IndentPicker};
pub use language::LanguagePicker;
pub use palette::{Command as PaletteCommand, Palette};
pub use pins::tab_menu_items;
pub use replace::ReplaceStats;
pub use results::ResultsPanel;

pub use session::Phase as SessionPhase;
pub use status::StatusBar;
pub use toolbox::SYNC_BYTES;

/// A file to open, with an optional position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub path: PathBuf,
    pub position: Option<LineCol>,
    /// Open an empty document for a path that does not exist yet (command line), instead of
    /// reporting an error (recent files, drops).
    pub create: bool,
}

/// A closed tab that Restore Closed Tab can bring back.
#[derive(Debug, Clone)]
enum ClosedTab {
    File {
        path: PathBuf,
        position: Position,
    },
    Untitled {
        text: String,
        position: Position,
        language: Option<String>,
        name: Option<String>,
    },
}

const CLOSED_LIMIT: usize = 20;

/// State shared by the whole application.
pub struct Shared {
    pub config: AppConfig,
    pub appearance: Rc<Appearance>,
    /// `config.toml` and `keys.toml` (M5).
    pub preferences: Rc<Preferences>,
    /// The editors' shortcuts, shared by every editor's controller (`actions`).
    pub editor_shortcuts: gio::ListStore,
    /// While the self-test asks for it, desktop commands are recorded instead of run.
    launches: RefCell<Option<Vec<Vec<OsString>>>>,
    /// The recent files, kept in the session (M2; M1 wrote `recent.json`).
    recent: RefCell<RecentFiles>,
    recent_listeners: RefCell<Vec<Box<dyn Fn()>>>,
}

impl Shared {
    pub fn new(config: AppConfig, appearance: Rc<Appearance>) -> Rc<Self> {
        let preferences = Preferences::start(config.dirs.config.clone());
        let editor_shortcuts = gio::ListStore::new::<gtk::Shortcut>();
        actions::fill_editor_shortcuts(&editor_shortcuts, &preferences.keymap());
        Rc::new(Self {
            config,
            appearance,
            preferences,
            editor_shortcuts,
            launches: RefCell::new(None),
            recent: RefCell::new(RecentFiles::default()),
            recent_listeners: RefCell::new(Vec::new()),
        })
    }

    pub fn recent(&self) -> RecentFiles {
        self.recent.borrow().clone()
    }

    /// The list the session restored, with files opened meanwhile in front.
    pub fn set_recent(&self, restored: RecentFiles) {
        let merged = RecentFiles::new(
            self.recent
                .borrow()
                .paths()
                .iter()
                .chain(restored.paths())
                .cloned(),
        );
        self.recent.replace(merged);
        self.notify_recent();
    }

    /// Records desktop commands (a terminal, the file manager) instead of running them.
    pub fn record_launches(&self) {
        self.launches.replace(Some(Vec::new()));
    }

    /// The commands recorded since [`Self::record_launches`].
    pub fn launches(&self) -> Vec<Vec<OsString>> {
        self.launches.borrow().clone().unwrap_or_default()
    }

    pub fn connect_recent_changed(&self, listener: impl Fn() + 'static) {
        self.recent_listeners.borrow_mut().push(Box::new(listener));
    }

    pub fn add_recent(self: &Rc<Self>, path: PathBuf) {
        if self.recent.borrow_mut().add(path) {
            self.recent_changed();
        }
    }

    pub fn remove_recent(self: &Rc<Self>, path: &Path) {
        if self.recent.borrow_mut().remove(path) {
            self.recent_changed();
        }
    }

    pub fn clear_recent(self: &Rc<Self>) {
        if self.recent.borrow_mut().clear() {
            self.recent_changed();
        }
    }

    fn notify_recent(&self) {
        for listener in self.recent_listeners.borrow().iter() {
            listener();
        }
    }

    /// The next session commit writes the list.
    fn recent_changed(self: &Rc<Self>) {
        self.notify_recent();
    }
}

#[derive(Clone)]
pub struct Window {
    pub(crate) inner: Rc<Inner>,
}

/// One of the window's two views (M7): a tab strip over its tabs. The second view shows only
/// while the window is split, and either view hides when its last tab goes.
pub struct ViewPane {
    pub tabs: adw::TabView,
    pub bar: adw::TabBar,
}

impl ViewPane {
    fn new() -> Self {
        let tabs = adw::TabView::new();
        tabs.set_shortcuts(adw::TabViewShortcuts::empty());
        let bar = adw::TabBar::builder()
            .view(&tabs)
            .autohide(false)
            .expand_tabs(false)
            .build();
        bar.add_css_class("stet-mono");
        Self { tabs, bar }
    }
}

pub struct Inner {
    pub shared: Rc<Shared>,
    pub window: adw::ApplicationWindow,
    /// The main view and the second one (M7).
    pub views: [ViewPane; 2],
    /// The view in front: where commands act, new tabs open, and what the status bar and the
    /// window title show.
    active: Cell<usize>,
    /// The views side by side, divided where the tab strips above them are (`top_paned` in
    /// [`Window::new`]).
    views_paned: gtk::Paned,
    /// The hamburger menu (and the window controls the decoration layout puts first, outside
    /// Hyprland), at the start of the first strip on screen (1.3).
    start: gtk::Box,
    /// The window controls outside Hyprland, at the end of the last strip on screen.
    end: gtk::Box,
    content: gtk::Box,
    toasts: adw::ToastOverlay,
    menu_button: gtk::MenuButton,
    pub status: StatusBar,
    pub find: FindBar,
    pub results: Rc<ResultsPanel>,
    /// The editor (and find bar) above, the results panel below.
    paned: gtk::Paned,
    results_height: Cell<i32>,
    fif: RefCell<Option<fif::FifRun>>,
    fif_runs: Cell<u64>,
    fif_stats: RefCell<Option<FifStats>>,
    pub palette: Rc<Palette>,
    pub languages: Rc<LanguagePicker>,
    pub encodings: Rc<EncodingPicker>,
    pub indentation: Rc<IndentPicker>,
    /// The text toolbox's dialogs' values and last statistics (M5).
    pub tools: toolbox::ToolState,
    /// The tab whose context menu is open.
    menu_page: RefCell<Option<adw::TabPage>>,
    /// The tab the last press on a tab strip was on, to tell a double-click on a tab.
    pressed_tab: glib::WeakRef<adw::TabPage>,
    closed: RefCell<Vec<ClosedTab>>,
    settings: Cell<ViewSettings>,
    dialog: RefCell<Option<adw::AlertDialog>>,
    closing_window: Cell<bool>,
    closing_without_prompt: Cell<bool>,
    /// A tab is moving to the other view: its detach and attach are not a close and an open.
    moving: Cell<bool>,
    /// A tab's close is being finished: its detach is a close, not a tab dragged to the other
    /// view's strip.
    finishing_close: Cell<bool>,
    actions: RefCell<HashMap<ActionId, gio::SimpleAction>>,
    last_focus_check: Cell<Option<Instant>>,
    last_toast: RefCell<String>,
    session: session::WindowSession,
    /// Navigation, pinned tabs, quick open and Replace in Files (M8).
    pub(crate) nav: nav::Nav,
    /// Split view's synchronised scrolling (M7).
    pub(crate) split: split::SplitState,
    /// The comparison on screen (M7).
    pub(crate) comparing: compare::CompareState,
    /// Mark's options and the last Mark's statistics (M7).
    pub(crate) marking: marking::MarkState,
}

impl Window {
    pub fn new(app: &adw::Application, shared: Rc<Shared>) -> Self {
        let views = [ViewPane::new(), ViewPane::new()];
        let menu_button = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .tooltip_text("Menu")
            .build();
        menu_button.add_css_class("flat");
        let start = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let end = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let tiling = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
            || std::env::var("XDG_CURRENT_DESKTOP")
                .is_ok_and(|desktop| desktop.contains("Hyprland"));
        if !tiling {
            start.append(&gtk::WindowControls::new(gtk::PackType::Start));
            end.append(&gtk::WindowControls::new(gtk::PackType::End));
        }
        start.append(&menu_button);
        views[0].bar.set_start_action_widget(Some(&start));
        views[0].bar.set_end_action_widget(Some(&end));
        // The strips sit side by side over the views, divided where the views are (M7).
        let split_paned = |start: &gtk::Widget, end: &gtk::Widget| {
            gtk::Paned::builder()
                .orientation(gtk::Orientation::Horizontal)
                .start_child(start)
                .end_child(end)
                .resize_start_child(true)
                .resize_end_child(true)
                .shrink_start_child(false)
                .shrink_end_child(false)
                .wide_handle(false)
                .build()
        };
        let top_paned = split_paned(views[0].bar.upcast_ref(), views[1].bar.upcast_ref());
        let views_paned = split_paned(views[0].tabs.upcast_ref(), views[1].tabs.upcast_ref());
        views_paned.set_vexpand(true);
        views[1].bar.set_visible(false);
        views[1].tabs.set_visible(false);
        top_paned
            .bind_property("position", &views_paned, "position")
            .bidirectional()
            .sync_create()
            .build();
        let top: gtk::Widget = if tiling {
            top_paned.clone().upcast()
        } else {
            gtk::WindowHandle::builder()
                .child(&top_paned)
                .build()
                .upcast()
        };

        let languages = LanguagePicker::new();
        let encodings = EncodingPicker::new();
        let indentation = IndentPicker::new();
        let status = StatusBar::new(
            &languages.popover,
            actions::eol_menu().upcast_ref(),
            &encodings.popover,
            &indentation.popover,
        );
        let find = FindBar::new();
        let results = ResultsPanel::new();
        let palette = Palette::new();

        // The find bar stays right below the editor; the results panel opens below both.
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&views_paned);
        content.append(&find.revealer);
        let paned = gtk::Paned::builder()
            .orientation(gtk::Orientation::Vertical)
            .start_child(&content)
            .end_child(&results.root)
            .resize_start_child(true)
            .resize_end_child(false)
            .shrink_start_child(false)
            .shrink_end_child(false)
            .wide_handle(false)
            .vexpand(true)
            .build();
        let toolbar = adw::ToolbarView::builder()
            .content(&paned)
            .top_bar_style(adw::ToolbarStyle::RaisedBorder)
            .bottom_bar_style(adw::ToolbarStyle::RaisedBorder)
            .build();
        toolbar.add_top_bar(&top);
        toolbar.add_bottom_bar(&status.root);
        // The Ctrl+Tab switcher floats over everything but the toasts (M8).
        let nav = nav::Nav::new();
        let overlay = gtk::Overlay::builder().child(&toolbar).build();
        overlay.add_overlay(&nav.switcher.root);
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&overlay));
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Stet")
            .default_width(1100)
            .default_height(720)
            .content(&toasts)
            .build();

        let this = Self {
            inner: Rc::new(Inner {
                shared,
                window,
                views,
                active: Cell::new(0),
                views_paned,
                start,
                end,
                content,
                toasts,
                menu_button,
                status,
                find,
                results,
                paned,
                results_height: Cell::new(replace::RESULTS_HEIGHT),
                fif: RefCell::new(None),
                fif_runs: Cell::new(0),
                fif_stats: RefCell::new(None),
                palette,
                languages,
                encodings,
                indentation,
                tools: toolbox::ToolState::default(),
                menu_page: RefCell::new(None),
                pressed_tab: glib::WeakRef::new(),
                closed: RefCell::new(Vec::new()),
                settings: Cell::new(ViewSettings::default()),
                dialog: RefCell::new(None),
                closing_window: Cell::new(false),
                closing_without_prompt: Cell::new(false),
                moving: Cell::new(false),
                finishing_close: Cell::new(false),
                actions: RefCell::new(HashMap::new()),
                last_focus_check: Cell::new(None),
                last_toast: RefCell::new(String::new()),
                session: session::WindowSession::default(),
                nav,
                split: split::SplitState::default(),
                comparing: compare::CompareState::default(),
                marking: marking::MarkState::default(),
            }),
        };
        actions::install_window_actions(&this);
        this.connect_preferences();
        this.connect_tab_menu();
        this.connect_tab_double_click();
        this.connect_menu_names();
        this.connect_indentation();
        this.connect_signals();
        this.connect_find_bar();
        this.connect_results_panel();
        this.connect_focus();
        this.connect_nav();
        this.connect_split();
        this.rebuild_menu();
        this.new_untitled();
        this.update_actions();
        this
    }

    pub fn window(&self) -> &adw::ApplicationWindow {
        &self.inner.window
    }

    pub fn shared(&self) -> &Rc<Shared> {
        &self.inner.shared
    }

    pub fn spawn(&self, future: impl Future<Output = ()> + 'static) {
        glib::spawn_future_local(future);
    }

    pub(crate) fn register_action(&self, id: ActionId, action: gio::SimpleAction) {
        self.inner.window.add_action(&action);
        self.inner.actions.borrow_mut().insert(id, action);
    }

    pub fn action(&self, id: ActionId) -> Option<gio::SimpleAction> {
        self.inner.actions.borrow().get(&id).cloned()
    }

    fn connect_signals(&self) {
        let inner = &self.inner;
        for (index, view) in inner.views.iter().enumerate() {
            view.tabs.connect_close_page(glib::clone!(
                #[weak(rename_to = inner)]
                self.inner,
                #[upgrade_or]
                glib::Propagation::Stop,
                move |tabs, tab_page| {
                    Window { inner }.on_close_page(tabs, tab_page);
                    glib::Propagation::Stop
                }
            ));
            view.tabs.connect_page_detached(glib::clone!(
                #[weak(rename_to = inner)]
                self.inner,
                move |_, tab_page, _| Window { inner }.on_page_detached(&page_of(tab_page))
            ));
            view.tabs.connect_page_attached(glib::clone!(
                #[weak(rename_to = inner)]
                self.inner,
                move |_, _, _| {
                    let window = Window { inner };
                    if !window.inner.moving.get() {
                        window.update_layout();
                    }
                }
            ));
            view.tabs.connect_selected_page_notify(glib::clone!(
                #[weak(rename_to = inner)]
                self.inner,
                move |_| Window { inner }.on_view_selected(index)
            ));
        }
        inner.window.connect_close_request(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_| Window { inner }.on_close_request()
        ));
        inner.window.connect_fullscreened_notify(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |window| {
                if let Some(action) = (Window { inner }).action(ActionId::FullScreen) {
                    action.set_state(&window.is_fullscreen().to_variant());
                }
            }
        ));
        // File systems without change events (network mounts) are checked on focus (M3).
        inner.window.connect_is_active_notify(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |window| {
                if window.is_active() {
                    Window { inner }.check_all_files();
                }
            }
        ));
        inner.window.connect_realize(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |window| {
                if let Some(clock) = window.frame_clock() {
                    clock.connect_layout(glib::clone!(
                        #[weak]
                        inner,
                        move |_| Window { inner }.on_layout()
                    ));
                }
            }
        ));

        let drop = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
        drop.set_propagation_phase(gtk::PropagationPhase::Capture);
        drop.connect_drop(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            #[upgrade_or]
            false,
            move |_, value, _, _| match value.get::<gdk::FileList>() {
                Ok(files) => Window { inner }.drop_files(files.files()),
                Err(_) => false,
            }
        ));
        inner.window.add_controller(drop);

        inner.shared.appearance.connect_scheme_changed(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |scheme| {
                for page in (Window { inner }).pages() {
                    page.set_scheme(scheme);
                }
            }
        ));
        inner.shared.connect_recent_changed(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move || {
                let window = Window { inner };
                window.rebuild_menu();
                window.update_actions();
            }
        ));
        inner.palette.connect_command(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |command| Window { inner }.run_palette_command(command)
        ));
        inner.languages.connect_chosen(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |id| {
                let window = Window { inner };
                if let Some(page) = window.current_page() {
                    window.set_language(&page, id.as_deref());
                    page.view().grab_focus();
                }
            }
        ));
        // Through the actions, so their enablement applies (M3).
        inner.encodings.connect_chosen(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |mode, entry| {
                let id = match mode {
                    PickerMode::Reinterpret => ActionId::Reinterpret,
                    PickerMode::Convert => ActionId::ConvertEncoding,
                };
                let window = Window { inner };
                let _ = WidgetExt::activate_action(
                    &window.inner.window,
                    &id.detailed_name(),
                    Some(&entry.id().to_variant()),
                );
                if let Some(page) = window.current_page() {
                    page.view().grab_focus();
                }
            }
        ));
    }

    // ----- pages -------------------------------------------------------------------------

    /// Every tab: the main view's, then the second view's (M7).
    pub fn pages(&self) -> Vec<EditorPage> {
        self.inner
            .views
            .iter()
            .flat_map(|view| view_pages(&view.tabs))
            .collect()
    }

    /// One tab per open document, in tab order: the current tab for its document, else the
    /// first that shows it. Operations on documents (saving, searching every document) use
    /// it, so a document in both views (M7) counts once.
    pub fn documents(&self) -> Vec<EditorPage> {
        let current = self.current_page();
        let mut documents: Vec<EditorPage> = Vec::new();
        for page in self.pages() {
            if documents.iter().any(|known| known.same_document(&page)) {
                continue;
            }
            match &current {
                Some(current) if current.same_document(&page) => documents.push(current.clone()),
                _ => documents.push(page),
            }
        }
        documents
    }

    /// The tab in front of the active view: where commands act.
    pub fn current_page(&self) -> Option<EditorPage> {
        self.inner.views[self.inner.active.get()]
            .tabs
            .selected_page()
            .map(|page| page_of(&page))
    }

    fn tab_page(&self, page: &EditorPage) -> Option<adw::TabPage> {
        self.locate(page).map(|(_, tab_page)| tab_page)
    }

    /// The view that shows `page`, and its tab there.
    fn locate(&self, page: &EditorPage) -> Option<(usize, adw::TabPage)> {
        let widget = page.upcast_ref::<gtk::Widget>();
        self.inner
            .views
            .iter()
            .enumerate()
            .find_map(|(index, view)| {
                let tabs = &view.tabs;
                (0..tabs.n_pages())
                    .map(|position| tabs.nth_page(position))
                    .find(|tab_page| tab_page.child() == *widget)
                    .map(|tab_page| (index, tab_page))
            })
    }

    /// Tab `number` (from 1) across the views, with its view's tab view, for the self-test.
    pub fn tab_at(&self, number: usize) -> Option<(adw::TabView, adw::TabPage)> {
        let mut left = number.checked_sub(1)?;
        for view in &self.inner.views {
            let count = usize::try_from(view.tabs.n_pages()).unwrap_or(0);
            if left < count {
                return Some((view.tabs.clone(), view.tabs.nth_page(left as i32)));
            }
            left -= count;
        }
        None
    }

    /// Shows `page` and makes its view the active one.
    pub fn select(&self, page: &EditorPage) {
        let Some((index, tab_page)) = self.locate(page) else {
            return;
        };
        let tabs = &self.inner.views[index].tabs;
        let changes = tabs.selected_page().as_ref() != Some(&tab_page);
        if index != self.inner.active.get() {
            self.inner.active.set(index);
            self.style_views();
            if !changes {
                self.current_changed(true);
            }
        }
        if changes {
            tabs.set_selected_page(&tab_page);
        }
    }

    fn add_page(&self, state: DocState) -> EditorPage {
        self.add_page_with(state, true)
    }

    /// Appends a tab for `state` to the active view, selecting it when `select` is set (a
    /// restored session's tabs are not, so that only the active one loads).
    fn add_page_with(&self, state: DocState, select: bool) -> EditorPage {
        self.add_page_in(self.target_view(), state, select)
    }

    /// The view new tabs open in: the active one.
    fn target_view(&self) -> usize {
        let active = self.inner.active.get();
        if self.inner.views[active].tabs.n_pages() > 0 || self.pages().is_empty() {
            active
        } else {
            1 - active
        }
    }

    /// Appends a tab for `state` to `view`.
    pub(super) fn add_page_in(&self, view: usize, state: DocState, select: bool) -> EditorPage {
        let page = EditorPage::new(
            state,
            self.inner.shared.appearance.scheme().as_ref(),
            self.inner.settings.get(),
        );
        self.attach_page(view, &page, select);
        page
    }

    /// Appends `page`, a new tab or a clone (M7), to `view`, with the window's hooks.
    pub(super) fn attach_page(&self, view: usize, page: &EditorPage, select: bool) {
        self.install_page(page);
        self.on_nav_page_attached(page);
        let tabs = &self.inner.views[view].tabs;
        let tab_page = tabs.append(page);
        tab_page.set_title(&tab_title(&page.name(), false));
        self.update_layout();
        if select {
            self.select(page);
        }
        self.refresh_page(page);
    }

    /// The window's hooks on a new tab's view and on its buffer.
    fn install_page(&self, page: &EditorPage) {
        let page = page.clone();
        let buffer = page.buffer();
        buffer.connect_modified_changed(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            #[weak]
            page,
            move |_| Window { inner }.refresh_page(&page)
        ));
        let refresh_status = glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            #[weak]
            page,
            move || {
                let window = Window { inner };
                if window.current_page().as_ref() == Some(&page) {
                    window.update_status();
                }
            }
        );
        let on_cursor = refresh_status.clone();
        buffer.connect_cursor_position_notify(move |_| on_cursor());
        let on_mark = refresh_status.clone();
        buffer.connect_mark_set(move |_, _, mark| {
            if mark.name().as_deref() == Some("selection_bound") {
                on_mark();
            }
        });
        self.connect_page_search(&page);
        let on_language = refresh_status.clone();
        buffer.connect_language_notify(move |_| on_language());
        let on_column = refresh_status.clone();
        page.column_view()
            .connect_column_changed(move |_| on_column());
        page.view()
            .connect_overwrite_notify(move |_| refresh_status());

        // Ctrl+scroll zooms, through the same actions as Ctrl++ and Ctrl+-.
        let scroll = gtk::EventControllerScroll::new(
            gtk::EventControllerScrollFlags::VERTICAL | gtk::EventControllerScrollFlags::DISCRETE,
        );
        scroll.set_propagation_phase(gtk::PropagationPhase::Capture);
        scroll.connect_scroll(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |controller, _, dy| {
                if !controller
                    .current_event_state()
                    .contains(gdk::ModifierType::CONTROL_MASK)
                    || dy == 0.0
                {
                    return glib::Propagation::Proceed;
                }
                let id = if dy < 0.0 {
                    ActionId::ZoomIn
                } else {
                    ActionId::ZoomOut
                };
                let _ = WidgetExt::activate_action(&inner.window, &id.detailed_name(), None);
                glib::Propagation::Stop
            }
        ));
        page.view().add_controller(scroll);
        self.connect_page_tools(&page);
        self.connect_page_split(&page);
        self.refresh_indentation(&page);
    }

    pub fn new_untitled(&self) -> EditorPage {
        let used = self
            .pages()
            .into_iter()
            .filter_map(|page| page.state().untitled);
        let number = next_untitled_number(used);
        let page = self.add_page(DocState {
            untitled: Some(number),
            ..DocState::default()
        });
        page.view().grab_focus();
        page
    }

    /// Updates the tab label, tooltip, and the window title and status when it is current; for
    /// every tab of the document (M7).
    fn refresh_page(&self, page: &EditorPage) {
        for page in page.document_pages() {
            let Some(tab_page) = self.tab_page(&page) else {
                continue;
            };
            tab_page.set_title(&tab_title(&page.name(), page.is_dirty()));
            let tooltip = page
                .path()
                .map_or_else(|| page.name(), |path| path.display().to_string());
            tab_page.set_tooltip(&glib::markup_escape_text(&tooltip));
            tab_page.set_loading(page.is_loading());
            if tab_page.is_pinned() {
                self.update_pin_icon(&page);
            }
        }
        if self
            .current_page()
            .is_some_and(|current| current.same_document(page))
        {
            self.update_title();
            self.update_status();
        }
    }

    /// The selected tab of view `index` changed.
    fn on_view_selected(&self, index: usize) {
        if self.inner.moving.get() {
            return;
        }
        // A restored tab loads its text when it is first shown, in either view (M2).
        if let Some(page) = self.inner.views[index].tabs.selected_page() {
            self.session_page_selected(&page_of(&page));
        }
        if index == self.inner.active.get() {
            self.on_page_selected();
        }
    }

    fn on_page_selected(&self) {
        self.current_changed(true);
    }

    /// Another tab is current: in the active view, or another view became the active one.
    /// With `take_focus` its editor takes the focus, unless the user is in the find bar, the
    /// results panel, a popover or a dialog; switching with Ctrl+Tab from the find field keeps
    /// it.
    fn current_changed(&self, take_focus: bool) {
        // A document in both views has one caret in its buffer: the current view's (M7).
        if let Some(page) = self.current_page() {
            page.take_caret();
        }
        self.update_title();
        self.update_status();
        self.update_actions();
        self.sync_search_highlight();
        self.update_match_count();
        if take_focus {
            self.restore_focus();
        }
        if let Some(page) = self.current_page() {
            self.session_page_selected(&page);
        }
        self.on_nav_page_selected();
    }

    /// A tab left its view: closed, or moved to the other view (Move to Other View, or dragged
    /// to the other view's strip), which attaches it there next.
    fn on_page_detached(&self, page: &EditorPage) {
        let closed = self.inner.finishing_close.get() || self.inner.closing_window.get();
        if self.inner.moving.get() || !closed {
            return;
        }
        self.on_nav_page_detached(page);
        self.compare_page_closed(page);
        if self.pages().is_empty() {
            if !self.inner.closing_window.get() {
                self.new_untitled();
            }
        } else {
            self.update_layout();
        }
        self.update_actions();
    }

    pub fn title(&self) -> String {
        self.inner
            .window
            .title()
            .map(|title| title.to_string())
            .unwrap_or_default()
    }

    fn update_title(&self) {
        let title = match self.current_page() {
            Some(page) => {
                window_title(&page.name(), page.dir_display().as_deref(), page.is_dirty())
            }
            None => stet_domain::document::APP_NAME.to_owned(),
        };
        self.inner.window.set_title(Some(&title));
    }

    pub fn update_status(&self) {
        let Some(page) = self.current_page() else {
            return;
        };
        let buffer = page.buffer();
        let view = page.view();
        let cursor = buffer.iter_at_mark(&buffer.get_insert());
        let (selected, lines) = buffer.selection_bounds().map_or((0, 0), |(start, end)| {
            let last = if end.line() > start.line() && end.starts_line() {
                end.line() - 1
            } else {
                end.line()
            };
            (
                (end.offset() - start.offset()) as usize,
                (last - start.line() + 1) as usize,
            )
        });
        let status = &self.inner.status;
        match page.column_view().rect() {
            Some(rect) => status.set_column_position(
                rect.cursor.line + 1,
                rect.cursor.column + 1,
                rect.lines().len(),
                rect.width(),
            ),
            None => status.set_position(
                cursor.line() as usize + 1,
                view.visual_column(&cursor) as usize + 1,
                selected,
                lines,
            ),
        }
        status.set_length(buffer.line_count() as usize, buffer.char_count() as usize);
        status.set_indentation(&page.indentation().label());
        if status.words_shown() {
            self.update_word_count(&page);
        }
        status.set_language(&page.language().map_or_else(
            || "Plain Text".to_owned(),
            |language| language.name().to_string(),
        ));
        let state = page.state();
        let (encoding, eol) = encoding::format_labels(&state.format, state.mixed_eol);
        status.set_eol(eol);
        status.set_encoding(&encoding);
        status.set_overwrite(view.overwrites());
        self.inner.encodings.prepare(
            stet_infrastructure::encoding::entry_for(state.format.encoding, state.format.bom),
            state.path.is_some(),
        );
    }

    /// Exact tab stops for the visible editor; runs in every layout phase (ADR-015).
    /// Runs in every layout phase of the window: exact tab stops for the visible editor
    /// (ADR-015), and the current search match kept above GtkSourceView's match style.
    fn on_layout(&self) {
        for page in self.shown_pages() {
            page.tab_stops().ensure(page.view());
        }
        if let Some(page) = self.current_page() {
            page.keep_current_match_on_top();
        }
    }

    // ----- opening -------------------------------------------------------------------------

    /// Opens files from a command line: `args` as typed, relative to `cwd`, with `-n`/`-c`.
    /// Paths are resolved on a worker, since `file:line` needs a look at the disk. Returns the
    /// tabs, for `--wait`.
    pub async fn open_command_line(
        &self,
        cwd: PathBuf,
        args: Vec<OsString>,
        line: Option<u32>,
        column: Option<u32>,
    ) -> Vec<EditorPage> {
        if args.is_empty() {
            return Vec::new();
        }
        let default = location::from_options(line, column);
        let locations = worker::run(move || {
            args.iter()
                .map(|arg| resolve_argument(&cwd, arg, default))
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        self.open_locations(locations)
    }

    // ----- languages -------------------------------------------------------------------------

    pub fn set_language(&self, page: &EditorPage, id: Option<&str>) {
        let language = id.and_then(|id| sourceview5::LanguageManager::default().language(id));
        page.set_language(language.as_ref());
        self.refresh_indentation(page);
        self.refresh_banners(page);
        self.update_status();
    }

    // ----- saving ----------------------------------------------------------------------------

    async fn save_all(&self) {
        for page in self.documents() {
            if self.has_unsaved_changes(&page) {
                self.select(&page);
                self.save(&page).await;
            }
        }
    }

    // ----- closing ---------------------------------------------------------------------------

    fn on_close_page(&self, tabs: &adw::TabView, tab_page: &adw::TabPage) {
        let page = page_of(tab_page);
        let tabs = tabs.clone();
        // A clone closes alone: its document stays open in the other view (M7).
        if self.other_page(&page).is_some() {
            self.finish_close(&tabs, tab_page, true);
            return;
        }
        let unsaved = self.has_unsaved_changes(&page);
        if !unsaved || self.inner.closing_without_prompt.get() {
            self.remember_closed(&page);
            self.tab_closed(&page, unsaved || page.is_loading());
            self.finish_close(&tabs, tab_page, true);
            return;
        }
        let window = self.clone();
        let tab_page = tab_page.clone();
        self.spawn(async move {
            window.select(&page);
            let choice = window.ask_save_changes(&[page.name()]).await;
            let confirmed = match choice {
                SaveChoice::Save => window.save(&page).await,
                SaveChoice::Discard => true,
                SaveChoice::Cancel => false,
            };
            if confirmed {
                window.remember_closed(&page);
                window.tab_closed(&page, choice == SaveChoice::Discard);
            }
            window.finish_close(&tabs, &tab_page, confirmed);
        });
    }

    /// Ends a tab's close, which detaches it from its view when `confirmed`.
    fn finish_close(&self, tabs: &adw::TabView, tab_page: &adw::TabPage, confirmed: bool) {
        self.inner.finishing_close.set(confirmed);
        tabs.close_page_finish(tab_page, confirmed);
        self.inner.finishing_close.set(false);
    }

    fn close_without_prompt(&self, page: &EditorPage) {
        if let Some((index, tab_page)) = self.locate(page) {
            self.inner.closing_without_prompt.set(true);
            self.inner.views[index].tabs.close_page(&tab_page);
            self.inner.closing_without_prompt.set(false);
        }
    }

    /// Closes every tab of `page`'s document without asking (M7).
    fn close_document(&self, page: &EditorPage) {
        for view_page in page.document_pages() {
            self.close_without_prompt(&view_page);
        }
    }

    /// Another tab that shows `page`'s document, in either view (M7).
    pub(super) fn other_page(&self, page: &EditorPage) -> Option<EditorPage> {
        page.document_pages()
            .into_iter()
            .find(|other| other != page && self.tab_page(other).is_some())
    }

    /// The documents that closing `pages` closes, one tab each: those with no tab left open.
    pub(super) fn closing_documents(&self, pages: &[EditorPage]) -> Vec<EditorPage> {
        let mut documents: Vec<EditorPage> = Vec::new();
        for page in pages {
            if documents.iter().any(|known| known.same_document(page)) {
                continue;
            }
            let all_closing = page
                .document_pages()
                .iter()
                .filter(|other| self.tab_page(other).is_some())
                .all(|other| pages.contains(other));
            if all_closing {
                documents.push(page.clone());
            }
        }
        documents
    }

    fn remember_closed(&self, page: &EditorPage) {
        if page.is_loading() || page.state().temporary.is_some() {
            return;
        }
        let position = page.cursor();
        let entry = match page.path() {
            Some(path) => ClosedTab::File { path, position },
            None => {
                let text = page.text();
                if text.is_empty() {
                    return;
                }
                ClosedTab::Untitled {
                    text,
                    position,
                    language: page.language().map(|language| language.id().to_string()),
                    name: page.state().custom_name.clone(),
                }
            }
        };
        let mut closed = self.inner.closed.borrow_mut();
        closed.push(entry);
        if closed.len() > CLOSED_LIMIT {
            closed.remove(0);
        }
    }

    /// Close All: every tab but the pinned ones, asking once about those with unsaved changes.
    async fn close_all(&self) {
        let closing = self.closable_by_bulk_commands(self.pages());
        let dirty: Vec<EditorPage> = self
            .closing_documents(&closing)
            .into_iter()
            .filter(|page| self.has_unsaved_changes(page))
            .collect();
        if !dirty.is_empty() {
            let names: Vec<String> = dirty.iter().map(EditorPage::name).collect();
            match self.ask_save_changes(&names).await {
                SaveChoice::Save => {
                    for page in &dirty {
                        self.select(page);
                        if !self.save(page).await {
                            return;
                        }
                    }
                }
                SaveChoice::Discard => {}
                SaveChoice::Cancel => return,
            }
        }
        for page in closing {
            self.close_without_prompt(&page);
        }
    }

    pub fn restore_closed(&self) {
        let Some(entry) = self.inner.closed.borrow_mut().pop() else {
            return;
        };
        match entry {
            ClosedTab::File { path, position } => {
                self.open_locations(vec![Location {
                    path,
                    position: Some(LineCol::new(
                        position.line as u32 + 1,
                        Some(position.column as u32 + 1),
                    )),
                    create: false,
                }]);
            }
            ClosedTab::Untitled {
                text,
                position,
                language,
                name,
            } => {
                let page = self.new_untitled();
                page.update_state(|state| state.custom_name = name);
                let buffer = page.buffer();
                buffer.begin_irreversible_action();
                buffer.set_text(&text);
                buffer.end_irreversible_action();
                buffer.set_modified(true);
                self.set_language(&page, language.as_deref());
                page.go_to(position);
                self.refresh_page(&page);
            }
        }
        self.update_actions();
    }

    pub fn closed_count(&self) -> usize {
        self.inner.closed.borrow().len()
    }

    /// Closing the window. With the session on it never asks, except about documents in
    /// large-file mode (M2); without one it asks about every document with unsaved changes.
    fn on_close_request(&self) -> glib::Propagation {
        if self.inner.closing_window.get() {
            return glib::Propagation::Proceed;
        }
        if self.session_closes_window() {
            let window = self.clone();
            self.spawn(async move { window.quit_with_session().await });
            return glib::Propagation::Stop;
        }
        let dirty: Vec<EditorPage> = self
            .documents()
            .into_iter()
            .filter(|page| self.has_unsaved_changes(page))
            .collect();
        if dirty.is_empty() {
            self.answer_waits();
            self.inner.closing_window.set(true);
            return glib::Propagation::Proceed;
        }
        let window = self.clone();
        self.spawn(async move {
            let names: Vec<String> = dirty.iter().map(EditorPage::name).collect();
            match window.ask_save_changes(&names).await {
                SaveChoice::Save => {
                    for page in &dirty {
                        window.select(page);
                        if !window.save(page).await {
                            return;
                        }
                    }
                }
                SaveChoice::Discard => {}
                SaveChoice::Cancel => return,
            }
            window.answer_waits();
            window.inner.closing_window.set(true);
            window.inner.window.close();
        });
        glib::Propagation::Stop
    }

    // ----- actions ---------------------------------------------------------------------------

    /// Runs a window action. `param` is the string parameter of `WithString` actions.
    pub fn run_action(&self, id: ActionId, param: Option<String>) {
        let page = self.current_page();
        // Column mode (M6) closes its typing burst first, and ends for actions that don't know
        // about columns.
        if let Some(page) = &page
            && crate::column::before_action(page.column_view(), id)
        {
            return;
        }
        match id {
            ActionId::NewTab => {
                self.new_untitled();
            }
            ActionId::Open => {
                let window = self.clone();
                self.spawn(async move { window.open_with_dialog().await });
            }
            ActionId::QuickOpen => self.open_quick_open(""),
            ActionId::NextRecentTab | ActionId::PreviousRecentTab => {
                self.switch_recent(id == ActionId::NextRecentTab);
            }
            ActionId::GoBack | ActionId::GoForward => self.navigate(id == ActionId::GoBack),
            ActionId::PinTab | ActionId::UnpinTab => self.run_pin_action(id),
            ActionId::OpenRecent => {
                if let Some(path) = param {
                    self.open_locations(vec![Location {
                        path: PathBuf::from(path),
                        position: None,
                        create: false,
                    }]);
                }
            }
            ActionId::ClearRecent => self.inner.shared.clear_recent(),
            // From the tab menu, the tab that was clicked.
            ActionId::Save | ActionId::SaveAs => {
                if let Some(page) = self.context_page() {
                    let window = self.clone();
                    self.spawn(async move {
                        if id == ActionId::Save {
                            window.save(&page).await;
                        } else {
                            window.save_as(&page).await;
                        }
                    });
                }
            }
            ActionId::SaveAll => {
                let window = self.clone();
                self.spawn(async move { window.save_all().await });
            }
            ActionId::CloseTab => self.run_tab_tool(id),
            ActionId::CloseAll => {
                let window = self.clone();
                self.spawn(async move { window.close_all().await });
            }
            ActionId::RestoreClosedTab => self.restore_closed(),
            ActionId::ForgetDrafts => self.forget_drafts(),
            ActionId::Quit => self.inner.window.close(),
            ActionId::Undo
            | ActionId::Redo
            | ActionId::Cut
            | ActionId::Copy
            | ActionId::Paste
            | ActionId::Delete
            | ActionId::SelectAll => {
                if let Some(page) = page {
                    edit(&page, id);
                }
            }
            ActionId::Find => self.show_find(),
            ActionId::FindNext => self.find_step(true),
            ActionId::FindPrevious => self.find_step(false),
            ActionId::FindInFiles
            | ActionId::Replace
            | ActionId::ReplaceNext
            | ActionId::ReplaceAll
            | ActionId::ReplaceAllInOpenDocuments
            | ActionId::Count
            | ActionId::FindAllInDocument
            | ActionId::FindAllInOpenDocuments
            | ActionId::StopFindInFiles
            | ActionId::ReplaceInFiles
            | ActionId::SearchResults
            | ActionId::NextSearchResult
            | ActionId::PreviousSearchResult
            | ActionId::CopySearchResults
            | ActionId::ClearSearchResults
            | ActionId::CloseSearchResults => self.run_search_action(id),
            ActionId::GoToLine => self.open_palette(":"),
            ActionId::WordWrap | ActionId::ShowWhitespace => self.toggle_setting(id),
            ActionId::ZoomIn | ActionId::ZoomOut | ActionId::ZoomReset => {
                let appearance = &self.inner.shared.appearance;
                let zoom = appearance.zoom();
                appearance.set_zoom(match id {
                    ActionId::ZoomIn => zoom.zoom_in(),
                    ActionId::ZoomOut => zoom.zoom_out(),
                    _ => stet_domain::view::Zoom::default(),
                });
            }
            ActionId::FullScreen => {
                let window = &self.inner.window;
                if window.is_fullscreen() {
                    window.unfullscreen();
                } else {
                    window.fullscreen();
                }
            }
            ActionId::NextTab | ActionId::PreviousTab => self.cycle_tabs(id == ActionId::NextTab),
            ActionId::MoveTabForward | ActionId::MoveTabBackward => {
                let tabs = self.active_tabs();
                if let Some(tab_page) = tabs.selected_page() {
                    if id == ActionId::MoveTabForward {
                        tabs.reorder_forward(&tab_page);
                    } else {
                        tabs.reorder_backward(&tab_page);
                    }
                }
            }
            ActionId::CommandPalette => self.open_palette(""),
            ActionId::ChooseLanguage => self.choose_language(),
            ActionId::SetLanguage => {
                if let Some(page) = page {
                    let id = param.filter(|id| !id.is_empty());
                    self.set_language(&page, id.as_deref());
                }
            }
            ActionId::ReloadFromDisk => {
                if let Some(page) = page {
                    let window = self.clone();
                    self.spawn(async move { window.reload_requested(&page).await });
                }
            }
            ActionId::EolCrLf
            | ActionId::EolLf
            | ActionId::EolCr
            | ActionId::EncodeUtf8
            | ActionId::EncodeUtf8Bom
            | ActionId::EncodeUtf16BeBom
            | ActionId::EncodeUtf16LeBom
            | ActionId::Reinterpret
            | ActionId::ConvertToAnsi
            | ActionId::ConvertToUtf8
            | ActionId::ConvertToUtf8Bom
            | ActionId::ConvertToUtf16BeBom
            | ActionId::ConvertToUtf16LeBom
            | ActionId::ConvertEncoding
            | ActionId::ChooseEncoding => self.run_encoding_action(id, param),
            ActionId::About => self.show_about(),
            ActionId::CloseOthers
            | ActionId::CloseToTheRight
            | ActionId::CopyFullPath
            | ActionId::CopyFileName
            | ActionId::CopyDirectoryPath
            | ActionId::OpenContainingFolder
            | ActionId::OpenTerminalHere
            | ActionId::RenameFile
            | ActionId::MoveToTrash => self.run_tab_tool(id),
            ActionId::Uppercase
            | ActionId::Lowercase
            | ActionId::ProperCase
            | ActionId::ProperCaseBlend
            | ActionId::SentenceCase
            | ActionId::SentenceCaseBlend
            | ActionId::InvertCase
            | ActionId::DuplicateLine
            | ActionId::CutLine
            | ActionId::CopyLine
            | ActionId::DeleteLine
            | ActionId::TransposeLine
            | ActionId::MoveLineUp
            | ActionId::MoveLineDown
            | ActionId::JoinLines
            | ActionId::BlankLineAbove
            | ActionId::BlankLineBelow
            | ActionId::RemoveDuplicateLines
            | ActionId::RemoveConsecutiveDuplicateLines
            | ActionId::RemoveEmptyLines
            | ActionId::RemoveBlankLines
            | ActionId::ReverseLines
            | ActionId::SortLexicalAscending
            | ActionId::SortLexicalDescending
            | ActionId::SortIgnoreCaseAscending
            | ActionId::SortIgnoreCaseDescending
            | ActionId::SortIntegerAscending
            | ActionId::SortIntegerDescending
            | ActionId::SortDecimalCommaAscending
            | ActionId::SortDecimalCommaDescending
            | ActionId::SortDecimalDotAscending
            | ActionId::SortDecimalDotDescending
            | ActionId::SortLengthAscending
            | ActionId::SortLengthDescending
            | ActionId::ToggleComment
            | ActionId::CommentLines
            | ActionId::UncommentLines
            | ActionId::ToggleBlockComment
            | ActionId::TrimTrailing
            | ActionId::TrimLeading
            | ActionId::TrimBoth
            | ActionId::TabsToSpaces
            | ActionId::SpacesToTabs
            | ActionId::SpacesToTabsLeading
            | ActionId::AddPrefixSuffix
            | ActionId::InsertNumbers
            | ActionId::GoToMatchingBrace
            | ActionId::SelectToMatchingBrace
            | ActionId::SelectAndFindNext
            | ActionId::SelectAndFindPrevious
            | ActionId::FormatJson
            | ActionId::MinifyJson
            | ActionId::ValidateJson
            | ActionId::FormatXml
            | ActionId::ValidateXml => self.run_text_tool(id),
            ActionId::DocumentMap => self.toggle_setting(id),
            ActionId::ChooseIndentation => self.choose_indentation(),
            ActionId::DetectIndentation => self.detect_indentation(),
            ActionId::OpenSettings => self.open_settings_file(false),
            ActionId::OpenKeyboardShortcuts => self.open_settings_file(true),
            ActionId::SetAsDefaultEditor => {
                let window = self.clone();
                self.spawn(async move { window.set_as_default_editor().await });
            }
            ActionId::KeyboardShortcuts => self.show_keyboard_shortcuts(),
            ActionId::ColumnSelectLeft
            | ActionId::ColumnSelectRight
            | ActionId::ColumnSelectUp
            | ActionId::ColumnSelectDown
            | ActionId::ColumnSelectLineStart
            | ActionId::ColumnSelectLineEnd
            | ActionId::ColumnSelectPageUp
            | ActionId::ColumnSelectPageDown
            | ActionId::ColumnBeginEndSelect => {
                if let Some(page) = page {
                    crate::column::run_action(page.column_view(), id);
                }
            }
            ActionId::ColumnEditor => {
                if let Some(page) = page {
                    crate::column::dialog::open(self, page.column_view());
                }
            }
            ActionId::ToggleBookmark
            | ActionId::NextBookmark
            | ActionId::PreviousBookmark
            | ActionId::ClearBookmarks
            | ActionId::CutBookmarkedLines
            | ActionId::CopyBookmarkedLines
            | ActionId::PasteToBookmarkedLines
            | ActionId::RemoveBookmarkedLines
            | ActionId::RemoveUnbookmarkedLines
            | ActionId::InverseBookmarks
            | ActionId::Mark
            | ActionId::MarkAll
            | ActionId::ClearMarks
            | ActionId::CopyMarkedText
            | ActionId::StyleToken1
            | ActionId::StyleToken2
            | ActionId::StyleToken3
            | ActionId::StyleToken4
            | ActionId::StyleToken5
            | ActionId::ClearStyle1
            | ActionId::ClearStyle2
            | ActionId::ClearStyle3
            | ActionId::ClearStyle4
            | ActionId::ClearStyle5
            | ActionId::ClearAllStyles
            | ActionId::JumpUp1
            | ActionId::JumpUp2
            | ActionId::JumpUp3
            | ActionId::JumpUp4
            | ActionId::JumpUp5
            | ActionId::JumpUpMark
            | ActionId::JumpDown1
            | ActionId::JumpDown2
            | ActionId::JumpDown3
            | ActionId::JumpDown4
            | ActionId::JumpDown5
            | ActionId::JumpDownMark => self.run_mark_action(id),
            ActionId::MoveToOtherView
            | ActionId::CloneToOtherView
            | ActionId::SwitchView
            | ActionId::SyncVerticalScrolling
            | ActionId::SyncHorizontalScrolling => self.run_split_action(id),
            ActionId::Compare
            | ActionId::CompareWithFile
            | ActionId::CompareWithClipboard
            | ActionId::CompareWithSaved
            | ActionId::PreviousDifference
            | ActionId::NextDifference
            | ActionId::CompareIgnoreWhitespace
            | ActionId::CompareIgnoreCase
            | ActionId::ClearCompare => self.run_compare_action(id),
        }
    }

    fn cycle_tabs(&self, forward: bool) {
        let tabs = self.active_tabs();
        let count = tabs.n_pages();
        let Some(selected) = tabs.selected_page() else {
            return;
        };
        if count < 2 {
            return;
        }
        let position = tabs.page_position(&selected);
        let next = if forward {
            (position + 1) % count
        } else {
            (position + count - 1) % count
        };
        tabs.set_selected_page(&tabs.nth_page(next));
    }

    fn toggle_setting(&self, id: ActionId) {
        let mut settings = self.inner.settings.get();
        match id {
            ActionId::WordWrap => settings.wrap = !settings.wrap,
            ActionId::DocumentMap => settings.map = !settings.map,
            _ => settings.whitespace = !settings.whitespace,
        }
        self.set_view_settings(settings);
    }

    /// Applies the window's view settings to every editor and the View menu's check marks.
    fn set_view_settings(&self, settings: ViewSettings) {
        self.inner.settings.set(settings);
        for page in self.pages() {
            page.apply_settings(settings);
        }
        for (id, on) in [
            (ActionId::WordWrap, settings.wrap),
            (ActionId::ShowWhitespace, settings.whitespace),
            (ActionId::DocumentMap, settings.map),
        ] {
            if let Some(action) = self.action(id) {
                action.set_state(&on.to_variant());
            }
        }
    }

    fn choose_language(&self) {
        let current = self
            .current_page()
            .and_then(|page| page.language())
            .map(|language| language.id().to_string());
        self.inner.status.language.popup();
        self.inner.languages.select(current.as_deref());
    }

    /// Enables the actions that have something to act on.
    fn update_actions(&self) {
        let has_closed = !self.inner.closed.borrow().is_empty();
        let has_recent = !self.inner.shared.recent().is_empty();
        let page = self.current_page();
        let ready = page.as_ref().is_some_and(|page| !page.is_loading());
        let on_disk = ready && page.as_ref().is_some_and(|page| page.path().is_some());
        let convertible = ready
            && page
                .as_ref()
                .is_some_and(|page| page.read_only() != Some(ReadOnly::DisplayBreaks));
        let mut enabled = vec![
            (ActionId::RestoreClosedTab, has_closed),
            (ActionId::ClearRecent, has_recent),
            (ActionId::ReloadFromDisk, on_disk),
            (
                ActionId::ForgetDrafts,
                self.session_phase() == SessionPhase::On,
            ),
        ];
        enabled.extend(
            [
                ActionId::EncodeUtf8,
                ActionId::EncodeUtf8Bom,
                ActionId::EncodeUtf16BeBom,
                ActionId::EncodeUtf16LeBom,
                ActionId::Reinterpret,
            ]
            .map(|id| (id, on_disk)),
        );
        enabled.extend(
            [
                ActionId::ConvertToAnsi,
                ActionId::ConvertToUtf8,
                ActionId::ConvertToUtf8Bom,
                ActionId::ConvertToUtf16BeBom,
                ActionId::ConvertToUtf16LeBom,
                ActionId::ConvertEncoding,
                ActionId::EolCrLf,
                ActionId::EolLf,
                ActionId::EolCr,
            ]
            .map(|id| (id, convertible)),
        );
        for (id, enabled) in enabled {
            if let Some(action) = self.action(id) {
                action.set_enabled(enabled);
            }
        }
        self.update_tab_actions();
        self.update_pin_actions();
        self.update_mark_actions();
        self.update_split_actions();
        self.update_compare_actions();
    }

    pub fn menu_model(&self) -> Option<gio::MenuModel> {
        self.ensure_menu();
        self.inner.menu_button.menu_model()
    }

    /// The hamburger menu's button.
    pub fn menu_button(&self) -> &gtk::MenuButton {
        &self.inner.menu_button
    }

    /// The view whose tab strip has the menu at its start.
    pub fn menu_view(&self) -> Option<usize> {
        let start = self.inner.start.upcast_ref::<gtk::Widget>();
        self.inner
            .views
            .iter()
            .position(|view| view.bar.start_action_widget().as_ref() == Some(start))
    }

    /// The menu is out of date (keys, recent files): it is built again when it next opens.
    /// Building it takes about 20 ms, which used to run twice at startup and once for every
    /// opened file.
    pub fn rebuild_menu(&self) {
        let button = &self.inner.menu_button;
        button.set_menu_model(None::<&gio::MenuModel>);
        if button.is_active() {
            self.ensure_menu();
        }
    }

    /// Builds the menu if it is out of date; the button's popup function runs it before the
    /// menu opens.
    pub(super) fn ensure_menu(&self) {
        let button = &self.inner.menu_button;
        if button.menu_model().is_none() {
            let recent = self.inner.shared.recent();
            button.set_menu_model(Some(&actions::menu_model(recent.paths(), &self.keymap())));
            if let Some(popover) = button.popover() {
                // From the button's left edge into the window (1.3.1): centred on the button at
                // the window's left end, half the menu hung outside the window.
                popover.set_halign(gtk::Align::Start);
                // Each page as wide as its own items (1.3.2), not as the widest submenu.
                if let Some(pages) = menu_part::<gtk::Stack>(&popover) {
                    pages.set_hhomogeneous(false);
                }
                popover.connect_show(glib::clone!(
                    #[weak(rename_to = inner)]
                    self.inner,
                    move |popover| Window { inner }.fit_menu(popover)
                ));
            }
        }
    }

    /// Keeps the menu above the window's bottom edge too (1.3.2): a page taller than the room
    /// below the button scrolls.
    fn fit_menu(&self, popover: &gtk::Popover) {
        let window = self.inner.window.upcast_ref::<gtk::Widget>();
        let (Some(scroller), Some(button)) = (
            menu_part::<gtk::ScrolledWindow>(popover),
            self.inner.menu_button.compute_bounds(window),
        ) else {
            return;
        };
        let below = window.height() - (button.y() + button.height()).ceil() as i32;
        scroller.set_max_content_height((below - MENU_MARGIN).max(MENU_MIN_HEIGHT));
    }

    // ----- palette ---------------------------------------------------------------------------

    pub fn open_palette(&self, text: &str) {
        let items = self.palette_items();
        self.inner.palette.open(&self.inner.content, items, text);
    }

    pub fn palette(&self) -> &Rc<Palette> {
        &self.inner.palette
    }

    fn palette_items(&self) -> Vec<palette::Item> {
        let keymap = self.keymap();
        // Pin Tab and Unpin Tab: only the one that applies (M8).
        let applies = |id: ActionId| {
            !matches!(id, ActionId::PinTab | ActionId::UnpinTab)
                || self.action(id).is_some_and(|action| action.is_enabled())
        };
        let mut items: Vec<palette::Item> = ActionId::ALL
            .into_iter()
            .filter(|id| id.spec().palette && applies(*id))
            .map(|id| {
                let on = id.kind() == ActionKind::Toggle
                    && self
                        .action(id)
                        .and_then(|action| action.state())
                        .and_then(|state| state.get::<bool>())
                        .unwrap_or(false);
                palette::Item {
                    label: if on {
                        format!("{} (on)", id.label())
                    } else {
                        id.label().to_owned()
                    },
                    detail: id.menu_path().unwrap_or_default(),
                    keys: keymap
                        .keys(id)
                        .iter()
                        .filter_map(|key| Accelerator::parse(key))
                        .map(|accel| accel.display())
                        .collect::<Vec<_>>()
                        .join("  "),
                    command: PaletteCommand::Action(id),
                }
            })
            .collect();
        let home = std::env::home_dir();
        items.extend(self.inner.shared.recent().paths().iter().map(|path| {
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            );
            let dir = path
                .parent()
                .map(|dir| stet_domain::document::tilde(dir, home.as_deref()))
                .unwrap_or_default();
            palette::Item {
                label: name,
                detail: format!("Open Recent › {dir}"),
                keys: String::new(),
                command: PaletteCommand::OpenRecent(path.clone()),
            }
        }));
        items
    }

    fn run_palette_command(&self, command: PaletteCommand) {
        match command {
            PaletteCommand::Action(id) => {
                let name = id.detailed_name();
                if let Err(error) = WidgetExt::activate_action(&self.inner.window, &name, None) {
                    tracing::warn!(%error, name, "palette action failed");
                }
            }
            PaletteCommand::OpenRecent(path) => {
                if let Some(path) = path.to_str() {
                    let _ = WidgetExt::activate_action(
                        &self.inner.window,
                        &ActionId::OpenRecent.detailed_name(),
                        Some(&path.to_variant()),
                    );
                }
            }
            PaletteCommand::GoTo(target) => {
                if let Some(page) = self.current_page() {
                    self.note_jump();
                    page.go_to(target.position());
                    page.view().grab_focus();
                }
            }
        }
    }

    pub fn present(&self) {
        self.inner.window.present();
    }
}

fn page_of(tab_page: &adw::TabPage) -> EditorPage {
    tab_page
        .child()
        .downcast()
        .expect("every tab holds an EditorPage")
}

/// Between the menu button and the menu's items: the popover's arrow, border and padding, and
/// a little room above the window's bottom edge.
const MENU_MARGIN: i32 = 40;

const MENU_MIN_HEIGHT: i32 = 160;

/// The first widget of type `T` in a popover menu: its scrolled window, or the stack of its
/// pages (the main one and one per submenu, as wide as the widest unless told otherwise). GTK
/// builds both inside the popover with no API to reach them.
pub fn menu_part<T: IsA<gtk::Widget>>(popover: &gtk::Popover) -> Option<T> {
    let mut widgets = vec![popover.clone().upcast::<gtk::Widget>()];
    while let Some(widget) = widgets.pop() {
        if let Ok(part) = widget.clone().downcast::<T>() {
            return Some(part);
        }
        let mut child = widget.first_child();
        while let Some(next) = child {
            child = next.next_sibling();
            widgets.push(next);
        }
    }
    None
}

/// The tabs of one view, in order.
fn view_pages(tabs: &adw::TabView) -> Vec<EditorPage> {
    (0..tabs.n_pages())
        .map(|index| page_of(&tabs.nth_page(index)))
        .collect()
}

/// Undo, redo and the clipboard on the editor, for the menu and the palette. The same keys
/// reach the focused widget directly.
fn edit(page: &EditorPage, id: ActionId) {
    let view = page.view();
    let buffer = page.buffer();
    match id {
        ActionId::Undo if buffer.can_undo() => buffer.undo(),
        ActionId::Redo if buffer.can_redo() => buffer.redo(),
        ActionId::Cut => view.emit_cut_clipboard(),
        ActionId::Copy => view.emit_copy_clipboard(),
        ActionId::Paste => view.emit_paste_clipboard(),
        ActionId::Delete => {
            buffer.delete_selection(true, view.is_editable());
        }
        ActionId::SelectAll => view.emit_select_all(true),
        _ => {}
    }
    view.grab_focus();
}

/// One command-line argument: the literal path when it exists; otherwise a `:line[:col]`
/// suffix is split off (ADR-011, CLI).
pub fn resolve_argument(cwd: &Path, arg: &OsString, default: Option<LineCol>) -> Location {
    let literal = absolute(cwd, Path::new(arg));
    if literal.exists() {
        return Location {
            path: literal,
            position: default,
            create: true,
        };
    }
    if let Some((path, position)) = arg.to_str().and_then(location::split_location) {
        return Location {
            path: absolute(cwd, Path::new(path)),
            position: Some(position),
            create: true,
        };
    }
    Location {
        path: literal,
        position: default,
        create: true,
    }
}

/// `path` made absolute against `cwd`, without `.` components. `..` stays, as symlinks make
/// it meaningful.
fn absolute(cwd: &Path, path: &Path) -> PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    joined
        .components()
        .filter(|component| *component != Component::CurDir)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn arguments_prefer_the_literal_path() {
        let dir = std::env::temp_dir().join(format!("stet-args-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("b.txt"), "x").unwrap();
        fs::write(dir.join("odd:12"), "x").unwrap();

        let located = resolve_argument(&dir, &OsString::from("b.txt:40"), None);
        assert_eq!(located.path, dir.join("b.txt"));
        assert_eq!(located.position, Some(LineCol::new(40, None)));

        let located = resolve_argument(&dir, &OsString::from("odd:12"), None);
        assert_eq!(located.path, dir.join("odd:12"));
        assert_eq!(located.position, None);

        let default = Some(LineCol::new(5, None));
        let located = resolve_argument(&dir, &OsString::from("./b.txt"), default);
        assert_eq!(located.path, dir.join("b.txt"));
        assert_eq!(located.position, default);

        let located = resolve_argument(&dir, &OsString::from("/abs/new.rs:3:4"), None);
        assert_eq!(located.path, PathBuf::from("/abs/new.rs"));
        assert_eq!(located.position, Some(LineCol::new(3, Some(4))));
        fs::remove_dir_all(&dir).unwrap();
    }
}
