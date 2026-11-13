//! The editor window (DESIGN.md "Window layout"): the tab strip with the hamburger menu on
//! top, the editor with its banners, the find bar, the search results panel and the status
//! bar below; the command palette and the language and encoding pickers as popovers. There
//! is no toolbar and no title bar.
//!
//! This module builds the window, has each feature connect its signals, and closes it. The
//! features live in their own modules: the tabs and views in [`workspace`], documents on disk
//! and in the session in [`documents`], search in [`search`], the text commands in [`text`],
//! the tools beside the text in [`tools`], and the status bar, pickers and menus in
//! [`chrome`]; [`dispatch`] routes the actions to them.

mod chrome;
pub mod dialogs;
mod dispatch;
mod documents;
mod focus;
mod layout;
#[cfg(feature = "selftest")]
mod probe;
mod search;
mod snapshot;
mod text;
mod tools;
mod workspace;

use crate::actions;
use crate::config::AppConfig;
use crate::editor::{EditorPage, ViewSettings};
use crate::preferences::Preferences;
use crate::runtime::Listeners;
use crate::theme::Appearance;
use dialogs::{Messages, SaveChoice};
use gtk4 as gtk;
use gtk4::{gio, glib};
use layout::{Layout, ViewPane};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
#[cfg(feature = "selftest")]
use std::ffi::OsString;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::Instant;
use stet_domain::actions::ActionId;
use stet_domain::document::location::LineCol;
use stet_domain::nav::RecentFiles;

pub use chrome::encoding_picker::{EncodingPicker, PickerMode};
#[cfg(feature = "selftest")]
pub use chrome::indentation::IndentChoice;
pub use chrome::indentation::IndentPicker;
pub use chrome::language::LanguagePicker;
#[cfg(feature = "selftest")]
pub use chrome::menu::menus_in;
pub use chrome::palette::Palette;
pub use documents::encoding::grouped_entries;
#[cfg(feature = "selftest")]
pub use search::SearchCommand;
#[cfg(feature = "selftest")]
pub use search::bar::{BarMode, Field};
pub use search::{FindBar, ResultsPanel};
#[cfg(feature = "selftest")]
pub use tools::compare::CompareRun;

pub use chrome::status::StatusBar;
pub use documents::session::Phase as SessionPhase;

/// A file to open, with an optional position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub path: PathBuf,
    pub position: Option<LineCol>,
    /// Open an empty document for a path that does not exist yet (command line), instead of
    /// reporting an error (recent files, drops).
    pub create: bool,
}

/// State shared by the whole application.
pub struct Shared {
    pub config: AppConfig,
    pub appearance: Rc<Appearance>,
    /// `config.toml` and `keys.toml`.
    pub preferences: Rc<Preferences>,
    /// The editors' shortcuts, shared by every editor's controller (`actions`).
    pub editor_shortcuts: gio::ListStore,
    /// While the self-test asks for it, desktop commands are recorded instead of run.
    #[cfg(feature = "selftest")]
    launches: RefCell<Option<Vec<Vec<OsString>>>>,
    /// The recent files, kept in the session.
    recent: RefCell<RecentFiles>,
    recent_listeners: Listeners<()>,
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
            #[cfg(feature = "selftest")]
            launches: RefCell::new(None),
            recent: RefCell::new(RecentFiles::default()),
            recent_listeners: Listeners::default(),
        })
    }

    pub fn recent(&self) -> RecentFiles {
        self.recent.borrow().clone()
    }

    /// The list the session restored, with files opened meanwhile in front.
    pub fn set_recent(&self, restored: &RecentFiles) {
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

    pub fn connect_recent_changed(&self, listener: impl Fn() + 'static) {
        self.recent_listeners.connect(move |()| listener());
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
        self.recent_listeners.notify(&());
    }

    /// The next session commit writes the list.
    fn recent_changed(self: &Rc<Self>) {
        self.notify_recent();
    }
}

/// The editor window: a cheap handle, cloned into whatever needs the window.
#[derive(Clone)]
pub struct Window {
    inner: Rc<Inner>,
}

/// A handle that doesn't keep the window alive, for signal handlers and timers: the window owns
/// its widgets, so a strong handle in their handlers would keep both alive forever. With
/// `glib::clone!`, `#[weak(rename_to = window)] self` captures one and upgrades it.
#[derive(Clone)]
pub struct WeakWindow(Weak<Inner>);

impl WeakWindow {
    /// The window, while it is open.
    pub fn upgrade(&self) -> Option<Window> {
        self.0.upgrade().map(|inner| Window { inner })
    }
}

impl Window {
    /// A handle for a signal handler or a timer, which doesn't keep the window alive.
    pub fn downgrade(&self) -> WeakWindow {
        WeakWindow(Rc::downgrade(&self.inner))
    }
}

impl glib::clone::Downgrade for Window {
    type Weak = WeakWindow;

    fn downgrade(&self) -> WeakWindow {
        Window::downgrade(self)
    }
}

impl glib::clone::Upgrade for WeakWindow {
    type Strong = Window;

    fn upgrade(&self) -> Option<Window> {
        WeakWindow::upgrade(self)
    }
}

/// The window's widgets and state. The features own their state; what is here is what several
/// of them share.
struct Inner {
    shared: Rc<Shared>,
    window: adw::ApplicationWindow,
    /// The main view and the second one.
    views: [ViewPane; 2],
    /// The view in front: where commands act, new tabs open, and what the status bar and the
    /// window title show.
    active: Cell<usize>,
    /// The panes and boxes around the views that features change after the window is built.
    layout: Layout,
    /// The toasts and the dialog on screen.
    messages: Messages,
    status: StatusBar,
    find: Rc<FindBar>,
    results: Rc<ResultsPanel>,
    /// Find in Files and Replace in Files.
    files: search::FilesSearch,
    /// Replace All's last cost.
    #[cfg(feature = "selftest")]
    replace: search::ReplaceState,
    palette: Rc<Palette>,
    languages: Rc<LanguagePicker>,
    encodings: Rc<EncodingPicker>,
    indentation: Rc<IndentPicker>,
    /// The text toolbox's dialogs' values and last statistics.
    tools: text::toolbox::ToolState,
    /// The tab menu's tab, the closed tabs, and what is happening to tabs.
    tabs: workspace::tabs::TabsState,
    /// Word wrap, whitespace and the document map, which every editor of the window shares.
    settings: Cell<ViewSettings>,
    /// Every window-scoped action, by id.
    actions: RefCell<HashMap<ActionId, gio::SimpleAction>>,
    /// When the open files were last checked on focus.
    last_focus_check: Cell<Option<Instant>>,
    session: documents::session::WindowSession,
    /// Navigation, pinned tabs and quick open.
    nav: workspace::nav::Nav,
    /// Split view's synchronised scrolling.
    split: workspace::split::SplitState,
    /// The comparison on screen.
    comparing: tools::compare::CompareState,
    /// Mark's options and the last Mark's statistics.
    marking: search::marking::MarkState,
    /// Check for Updates.
    updates: tools::updates::UpdateState,
}

impl Window {
    pub fn new(app: &adw::Application, shared: Rc<Shared>) -> Self {
        let views = [ViewPane::new(), ViewPane::new()];
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
        let nav = workspace::nav::Nav::new();
        let (layout, frame) = Layout::build(&views, &find, &results, &status, &nav.switcher.root);
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&frame));
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
                layout,
                messages: Messages::new(toasts),
                status,
                find,
                results,
                files: search::FilesSearch::default(),
                #[cfg(feature = "selftest")]
                replace: search::ReplaceState::default(),
                palette: Palette::new(),
                languages,
                encodings,
                indentation,
                tools: text::toolbox::ToolState::default(),
                tabs: workspace::tabs::TabsState::default(),
                settings: Cell::new(ViewSettings::default()),
                actions: RefCell::new(HashMap::new()),
                last_focus_check: Cell::new(None),
                session: documents::session::WindowSession::default(),
                nav,
                split: workspace::split::SplitState::default(),
                comparing: tools::compare::CompareState::default(),
                marking: search::marking::MarkState::default(),
                updates: tools::updates::UpdateState::default(),
            }),
        };
        // Handlers of one signal run in the order they were connected. Where two features
        // connect to the same signal, this order decides which runs first: the open files are
        // checked on focus before the tab switcher hears of it, the editors take a new colour
        // scheme before the results panel, and a tab strip's double-click sees a press before
        // the click that makes its view the active one.
        actions::install_window_actions(&this);
        tools::settings::connect(&this);
        workspace::tabs::connect(&this);
        chrome::a11y::connect(&this);
        chrome::indentation::connect(&this);
        connect(&this);
        workspace::view::connect(&this);
        documents::disk::connect(&this);
        documents::files::connect(&this);
        workspace::pages::connect(&this);
        chrome::menu::connect(&this);
        chrome::palette::connect(&this);
        chrome::language::connect(&this);
        chrome::encoding_picker::connect(&this);
        search::connect(&this);
        focus::connect(&this);
        workspace::nav::connect(&this);
        workspace::split::connect(&this);
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

    pub fn present(&self) {
        self.inner.window.present();
    }

    /// Closing the window. With the session on it never asks, except about documents in
    /// large-file mode; without one it asks about every document with unsaved changes.
    fn on_close_request(&self) -> glib::Propagation {
        if self.inner.tabs.is_closing_window() {
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
            .filter(Self::has_unsaved_changes)
            .collect();
        if dirty.is_empty() {
            self.answer_waits();
            self.inner.tabs.set_closing_window();
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
            window.inner.tabs.set_closing_window();
            window.inner.window.close();
        });
        glib::Propagation::Stop
    }

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
}

/// The window's own lifecycle: a close asked for, and every layout phase once it is realized.
fn connect(window: &Window) {
    window.inner.window.connect_close_request(glib::clone!(
        #[weak]
        window,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_| window.on_close_request()
    ));
    window.inner.window.connect_realize(glib::clone!(
        #[weak]
        window,
        move |toplevel| {
            if let Some(clock) = toplevel.frame_clock() {
                clock.connect_layout(glib::clone!(
                    #[weak]
                    window,
                    move |_| window.on_layout()
                ));
            }
        }
    ));
}

fn page_of(tab_page: &adw::TabPage) -> EditorPage {
    tab_page
        .child()
        .downcast()
        .expect("every tab holds an EditorPage")
}

/// The tabs of one view, in order.
fn view_pages(tabs: &adw::TabView) -> Vec<EditorPage> {
    (0..tabs.n_pages())
        .map(|index| page_of(&tabs.nth_page(index)))
        .collect()
}
