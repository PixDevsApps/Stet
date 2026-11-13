//! The tab strip's context menu and the File-menu commands behind it: copy the path, the name
//! or the folder; open the folder or a terminal there; rename the file, or an untitled tab;
//! move it to the trash; close the other tabs or those to the right; save. From the tab menu a
//! command acts on the tab that was clicked, otherwise on the current one (INTEGRATIONS.md
//! section 7). A double-click on a tab renames it too.
//!
//! The tabs' own state is here too ([`TabsState`]), with what the views report about their
//! tabs: a close asked for, a tab detached, attached or selected, a tab dropped after the last.

use crate::actions;
use crate::convert::to_i32;
use crate::editor::EditorPage;
use crate::runtime::{Flag, Raised, worker};
use crate::window::dialogs::SaveChoice;
use crate::window::{Window, page_of};
use gtk4 as gtk;
use gtk4::{gio, glib};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use stet_domain::actions::ActionId;
use stet_domain::nav::{ClosedTab, ClosedTabs};
use stet_infrastructure::desktop;
use stet_infrastructure::fs::rename_no_replace;

/// The response that renames.
pub const RENAME: &str = "rename";

/// The commands of the tab strip: switching, moving, closing every tab and reopening one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::window) enum TabCommand {
    Next,
    Previous,
    MoveForward,
    MoveBackward,
    /// Closes every tab but the pinned ones.
    CloseAll,
    RestoreClosed,
}

/// The tab menu's commands, which act on the tab that was clicked, else on the current one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::window) enum TabTool {
    Close,
    CloseOthers,
    CloseToTheRight,
    CopyFullPath,
    CopyFileName,
    CopyDirectoryPath,
    OpenContainingFolder,
    OpenTerminalHere,
    /// Renames the file, or names an untitled tab.
    Rename,
    MoveToTrash,
}

/// The tabs' state that the views' signals don't tell: the tab whose menu is open, the last
/// press on a strip, the closed tabs, and what the window itself is doing to its tabs.
pub(in crate::window) struct TabsState {
    /// The tab whose context menu is open.
    menu_page: RefCell<Option<adw::TabPage>>,
    /// The tab the last press on a tab strip was on, to tell a double-click on a tab.
    pressed_tab: glib::WeakRef<adw::TabPage>,
    /// What Restore Closed Tab brings back.
    closed: RefCell<ClosedTabs>,
    /// The window is closing: its tabs' detaches are closes, and the last one leaves no new
    /// untitled tab behind.
    closing_window: Cell<bool>,
    /// Tabs close without asking about unsaved changes.
    closing_without_prompt: Flag,
    /// A tab is moving to the other view: its detach and attach are not a close and an open.
    moving: Flag,
    /// A tab's close is being finished: its detach is a close, not a tab dragged to the other
    /// view's strip.
    finishing_close: Flag,
    /// The view whose strip took a dropped tab after its last tab, until libadwaita hands the
    /// tab over.
    dropped_on: Cell<Option<usize>>,
    /// Where libadwaita puts that tab, always first; from here it moves to the end of the
    /// view's tabs. Never on screen.
    drop_stage: adw::TabView,
}

impl Default for TabsState {
    fn default() -> Self {
        Self {
            menu_page: RefCell::default(),
            pressed_tab: glib::WeakRef::new(),
            closed: RefCell::default(),
            closing_window: Cell::new(false),
            closing_without_prompt: Flag::default(),
            moving: Flag::default(),
            finishing_close: Flag::default(),
            dropped_on: Cell::new(None),
            drop_stage: adw::TabView::new(),
        }
    }
}

impl TabsState {
    /// Tabs close without asking until the guard drops.
    pub(in crate::window) fn closing_without_prompt(&self) -> Raised<'_> {
        self.closing_without_prompt.raise()
    }

    pub(in crate::window) fn is_closing_without_prompt(&self) -> bool {
        self.closing_without_prompt.is_raised()
    }

    /// A tab moves to the other view until the guard drops.
    pub(in crate::window) fn moving(&self) -> Raised<'_> {
        self.moving.raise()
    }

    pub(in crate::window) fn is_moving(&self) -> bool {
        self.moving.is_raised()
    }

    /// A tab's close is finished until the guard drops.
    pub(in crate::window) fn finishing_close(&self) -> Raised<'_> {
        self.finishing_close.raise()
    }

    /// From now on the window is closing.
    pub(in crate::window) fn set_closing_window(&self) {
        self.closing_window.set(true);
    }

    pub(in crate::window) fn is_closing_window(&self) -> bool {
        self.closing_window.get()
    }

    /// Whether a tab that leaves its view now is closed, rather than dragged elsewhere.
    pub(in crate::window) fn is_closing(&self) -> bool {
        self.finishing_close.is_raised() || self.closing_window.get()
    }

    pub(in crate::window) fn remember_closed(&self, tab: ClosedTab) {
        self.closed.borrow_mut().push(tab);
    }

    /// The most recently closed tab, which Restore Closed Tab brings back.
    pub(in crate::window) fn take_closed(&self) -> Option<ClosedTab> {
        self.closed.borrow_mut().pop()
    }

    pub(in crate::window) fn has_closed(&self) -> bool {
        !self.closed.borrow().is_empty()
    }

    #[cfg(feature = "selftest")]
    pub(in crate::window) fn closed_count(&self) -> usize {
        self.closed.borrow().len()
    }
}

/// The tab strips' menus and double-clicks, and what the views report about their tabs:
/// a close asked for, a tab detached or attached, another tab selected, a tab dropped after
/// the last one; and the + after the tabs.
pub(in crate::window) fn connect(window: &Window) {
    connect_menus(window);
    connect_double_click(window);
    connect_views(window);
    connect_drop_stage(window);
    for (index, view) in window.inner.views.iter().enumerate() {
        view.new_tab.connect_clicked(glib::clone!(
            #[weak]
            window,
            move |_| {
                window.set_active_view(index, false);
                let name = actions::detailed_name(ActionId::NewTab);
                let _ = WidgetExt::activate_action(&window.inner.window, &name, None);
            }
        ));
    }
}

fn connect_menus(window: &Window) {
    let menu = actions::tab_menu(&window.keymap());
    for view in &window.inner.views {
        let tabs = &view.tabs;
        tabs.set_menu_model(Some(&menu));
        tabs.connect_setup_menu(glib::clone!(
            #[weak]
            window,
            move |_, tab_page| match tab_page {
                Some(tab_page) => {
                    window.inner.tabs.menu_page.replace(Some(tab_page.clone()));
                    window.update_actions();
                    window.name_menu_items_soon();
                }
                // The menu closed; its command, if any, has run by now or runs from this
                // event, so the clicked tab is forgotten once the event is done.
                None => {
                    let weak = window.downgrade();
                    glib::idle_add_local_once(move || {
                        if let Some(window) = weak.upgrade() {
                            window.inner.tabs.menu_page.replace(None);
                            window.update_actions();
                            window.restore_focus();
                        }
                    });
                }
            }
        ));
    }
}

/// A double-click on a tab opens Rename…: for its file, or for an untitled tab's name.
fn connect_double_click(window: &Window) {
    for view in &window.inner.views {
        let click = gtk::GestureClick::builder()
            .button(gtk::gdk::BUTTON_PRIMARY)
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        click.connect_pressed(glib::clone!(
            #[weak]
            window,
            move |gesture, presses, x, y| {
                let Some(bar) = gesture.widget() else {
                    return;
                };
                if window.tab_pressed(&bar, presses, x, y) {
                    // Neither the strip nor a window handle around it takes the press.
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                }
            }
        ));
        view.bar.add_controller(click);
    }
}

/// What each view reports about its tabs.
fn connect_views(window: &Window) {
    for (index, view) in window.inner.views.iter().enumerate() {
        view.tabs.connect_close_page(glib::clone!(
            #[weak]
            window,
            #[upgrade_or]
            glib::Propagation::Stop,
            move |tabs, tab_page| {
                window.on_close_page(tabs, tab_page);
                glib::Propagation::Stop
            }
        ));
        view.tabs.connect_page_detached(glib::clone!(
            #[weak]
            window,
            move |_, tab_page, _| window.on_page_detached(&page_of(tab_page))
        ));
        view.tabs.connect_page_attached(glib::clone!(
            #[weak]
            window,
            move |_, _, _| {
                if !window.inner.tabs.is_moving() {
                    window.update_layout();
                }
            }
        ));
        view.tabs.connect_selected_page_notify(glib::clone!(
            #[weak]
            window,
            move |_| window.on_view_selected(index)
        ));
        // A tab dropped after the strip's last tab: libadwaita then asks the tab's view for a
        // new window's tab view to put it in, which is the stage.
        view.tab_drop.connect_drop(glib::clone!(
            #[weak]
            window,
            #[upgrade_or]
            false,
            move |target, _, _, _| {
                let tabs = &window.inner.tabs;
                tabs.dropped_on.set(Some(index));
                if let Some(drag) = target.current_drop().and_then(|drop| drop.drag()) {
                    drag.connect_dnd_finished(glib::clone!(
                        #[weak]
                        window,
                        move |_| window.inner.tabs.dropped_on.set(None)
                    ));
                    drag.connect_cancel(glib::clone!(
                        #[weak]
                        window,
                        move |_, _| window.inner.tabs.dropped_on.set(None)
                    ));
                }
                true
            }
        ));
        view.tabs.connect_create_window(glib::clone!(
            #[weak]
            window,
            #[upgrade_or]
            None,
            move |_| {
                let tabs = &window.inner.tabs;
                tabs.dropped_on.get().map(|_| tabs.drop_stage.clone())
            }
        ));
    }
}

/// A tab dropped after a strip's last tab lands on the stage, and moves on to the end of that
/// strip's view.
fn connect_drop_stage(window: &Window) {
    window
        .inner
        .tabs
        .drop_stage
        .connect_page_attached(glib::clone!(
            #[weak]
            window,
            move |stage, tab_page, _| {
                let index = window
                    .inner
                    .tabs
                    .dropped_on
                    .take()
                    .unwrap_or_else(|| window.inner.active.get());
                let (stage, tab_page) = (stage.clone(), tab_page.clone());
                // Once libadwaita has finished attaching it here.
                glib::spawn_future_local(async move {
                    let target = &window.inner.views[index].tabs;
                    let position = if tab_page.is_pinned() {
                        target.n_pinned_pages()
                    } else {
                        target.n_pages()
                    };
                    stage.transfer_page(&tab_page, target, position);
                });
            }
        ));
}

impl Window {
    /// A press at (`x`, `y`) on tab strip `bar`, the `presses`th of a click: the second one on
    /// the same tab renames it. Returns whether it did.
    pub fn tab_pressed(&self, bar: &gtk::Widget, presses: i32, x: f64, y: f64) -> bool {
        let tab_page = tab_under(bar, x, y);
        // Both presses on the tab itself: a quick second click on a close button lands on the
        // tab that moves in under it.
        let pressed = &self.inner.tabs.pressed_tab;
        let previous = pressed.upgrade();
        pressed.set(tab_page.as_ref());
        match tab_page {
            Some(tab_page) if presses == 2 && previous.as_ref() == Some(&tab_page) => {
                self.rename_from_tab(&page_of(&tab_page))
            }
            _ => false,
        }
    }

    /// The strip that shows `tab_page` and the tab on it (for the self-test).
    #[cfg(feature = "selftest")]
    pub fn tab_widget(&self, tab_page: &adw::TabPage) -> Option<(gtk::Widget, gtk::Widget)> {
        self.inner.views.iter().find_map(|view| {
            let bar = view.bar.upcast_ref::<gtk::Widget>();
            let mut stack = vec![bar.clone()];
            while let Some(widget) = stack.pop() {
                if tab_page_of(&widget).as_ref() == Some(tab_page) {
                    return Some((bar.clone(), widget));
                }
                let mut child = widget.first_child();
                while let Some(next) = child {
                    child = next.next_sibling();
                    stack.push(next);
                }
            }
            None
        })
    }

    /// Rename… for the tab's document. A tab still loading and a text a comparison opened have
    /// no name to change.
    fn rename_from_tab(&self, page: &EditorPage) -> bool {
        if page.is_loading() || page.state().temporary.is_some() {
            return false;
        }
        let window = self.clone();
        let page = page.clone();
        self.spawn(async move { window.rename(&page).await });
        true
    }

    /// The tab a tab command acts on: the one whose menu is open, else the current one.
    pub(in crate::window) fn context_page(&self) -> Option<EditorPage> {
        self.inner
            .tabs
            .menu_page
            .borrow()
            .as_ref()
            .map(page_of)
            .or_else(|| self.current_page())
    }

    /// Enables the tab commands that make sense for the tab they would act on.
    pub(in crate::window) fn update_tab_actions(&self) {
        let page = self.context_page();
        let ready = page.as_ref().is_some_and(|page| !page.is_loading());
        let has_file = ready && page.as_ref().is_some_and(|page| page.path().is_some());
        let renamable = ready
            && page
                .as_ref()
                .is_some_and(|page| page.state().temporary.is_none());
        // The closing commands act within the tab's own view.
        let (position, count) = page.as_ref().and_then(|page| self.locate(page)).map_or(
            (None, 0),
            |(view, tab_page)| {
                let tabs = &self.inner.views[view].tabs;
                (Some(tabs.page_position(&tab_page)), tabs.n_pages())
            },
        );
        for (id, enabled) in [
            (ActionId::CopyFullPath, has_file),
            (ActionId::CopyDirectoryPath, has_file),
            (ActionId::OpenContainingFolder, has_file),
            (ActionId::OpenTerminalHere, has_file),
            (ActionId::RenameFile, renamable),
            (ActionId::MoveToTrash, has_file),
            (ActionId::CloseOthers, count > 1),
            (
                ActionId::CloseToTheRight,
                position.is_some_and(|position| position + 1 < count),
            ),
        ] {
            if let Some(action) = self.action(id) {
                action.set_enabled(enabled);
            }
        }
    }

    /// Runs a command of the tab strip.
    pub(in crate::window) fn run_tab(&self, command: TabCommand) {
        match command {
            TabCommand::Next => self.cycle_tabs(true),
            TabCommand::Previous => self.cycle_tabs(false),
            TabCommand::MoveForward => {
                let tabs = self.active_tabs();
                if let Some(tab_page) = tabs.selected_page() {
                    tabs.reorder_forward(&tab_page);
                }
            }
            TabCommand::MoveBackward => {
                let tabs = self.active_tabs();
                if let Some(tab_page) = tabs.selected_page() {
                    tabs.reorder_backward(&tab_page);
                }
            }
            TabCommand::CloseAll => {
                let window = self.clone();
                self.spawn(async move { window.close_all().await });
            }
            TabCommand::RestoreClosed => self.restore_closed(),
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

    /// Runs a command of the tab menu on the tab that was clicked, else on the current one.
    pub(in crate::window) fn run_tab_tool(&self, tool: TabTool) {
        let Some(page) = self.context_page() else {
            return;
        };
        match tool {
            TabTool::Close => {
                if let Some((view, tab_page)) = self.locate(&page) {
                    self.inner.views[view].tabs.close_page(&tab_page);
                }
            }
            TabTool::CloseOthers | TabTool::CloseToTheRight => {
                let Some(view) = self.view_of(&page) else {
                    return;
                };
                let pages = crate::window::view_pages(&self.inner.views[view].tabs);
                let Some(position) = pages.iter().position(|other| *other == page) else {
                    return;
                };
                let closing: Vec<EditorPage> = pages
                    .into_iter()
                    .enumerate()
                    .filter(|(index, _)| {
                        if tool == TabTool::CloseOthers {
                            *index != position
                        } else {
                            *index > position
                        }
                    })
                    .map(|(_, page)| page)
                    .collect();
                // Pinned tabs stay, as with Close All.
                let closing = self.closable_by_bulk_commands(closing);
                let window = self.clone();
                self.spawn(async move { window.close_pages(closing, &page).await });
            }
            TabTool::CopyFileName => self.copy_path_text(&page, &page.name()),
            TabTool::CopyFullPath => {
                if let Some(path) = page.path() {
                    self.copy_path_text(&page, &path.display().to_string());
                }
            }
            TabTool::CopyDirectoryPath => {
                if let Some(path) = page.path() {
                    let dir = path
                        .parent()
                        .map(|dir| dir.display().to_string())
                        .unwrap_or_default();
                    self.copy_path_text(&page, &dir);
                }
            }
            TabTool::OpenContainingFolder | TabTool::OpenTerminalHere => {
                let Some(dir) = page
                    .path()
                    .and_then(|path| path.parent().map(Path::to_path_buf))
                else {
                    return;
                };
                let terminal = tool == TabTool::OpenTerminalHere;
                let window = self.clone();
                self.spawn(async move { window.open_externally(dir, terminal).await });
            }
            TabTool::Rename => {
                let window = self.clone();
                self.spawn(async move { window.rename(&page).await });
            }
            TabTool::MoveToTrash => {
                let window = self.clone();
                self.spawn(async move { window.move_to_trash(&page).await });
            }
        }
    }

    /// Puts `text`, the tab's name or part of its path, on the clipboard.
    fn copy_path_text(&self, page: &EditorPage, text: &str) {
        page.view().clipboard().set_text(text);
        self.toast(&format!("Copied “{text}”"));
    }

    /// Closes `pages`, asking once about the unsaved ones, as Close All does. `keep` stays
    /// in front.
    async fn close_pages(&self, pages: Vec<EditorPage>, keep: &EditorPage) {
        let dirty: Vec<EditorPage> = self
            .closing_documents(&pages)
            .into_iter()
            .filter(EditorPage::is_dirty)
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
        self.select(keep);
        for page in &pages {
            self.close_without_prompt(page);
        }
        self.update_tab_actions();
        self.restore_focus();
    }

    /// Starts the file manager or a terminal in `dir`, detached from Stet.
    async fn open_externally(&self, dir: PathBuf, terminal: bool) {
        let command = worker::run(move || {
            let installed = |name: &str| desktop::installed(name);
            if terminal {
                desktop::terminal_command(&dir, &installed)
            } else {
                desktop::folder_command(&dir, &installed)
            }
        })
        .await
        .flatten();
        let Some(command) = command else {
            self.toast(if terminal {
                "No terminal to open: xdg-terminal-exec is not installed"
            } else {
                "No file manager to open: xdg-open is not installed"
            });
            return;
        };
        self.launch(command).await;
    }

    /// Runs a desktop command detached, or records it while the self-test asks for that.
    pub async fn launch(&self, command: Vec<OsString>) {
        #[cfg(feature = "selftest")]
        if let Some(recorded) = self.inner.shared.launches.borrow_mut().as_mut() {
            recorded.push(command);
            return;
        }
        let shown = command
            .iter()
            .map(|part| part.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ");
        match worker::run(move || desktop::spawn_detached(&command)).await {
            Some(Ok(())) => tracing::info!(command = shown, "started"),
            Some(Err(error)) => self.toast(&format!("Could not run {shown}: {error}")),
            None => self.toast(&format!("Could not run {shown}")),
        }
    }

    /// Rename…: a file gets a new name in its folder; an untitled document a name for its tab.
    async fn rename(&self, page: &EditorPage) {
        if page.path().is_some() {
            self.rename_file(page).await;
        } else {
            self.rename_tab(page).await;
        }
    }

    /// The Rename dialog for `name`, and its field.
    fn rename_dialog(name: &str, body: &str) -> (adw::AlertDialog, adw::EntryRow) {
        let entry = adw::EntryRow::builder()
            .title("New name")
            .text(name)
            .build();
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        list.append(&entry);
        let dialog = adw::AlertDialog::new(Some(&format!("Rename “{name}”")), Some(body));
        dialog.set_extra_child(Some(&list));
        dialog.add_responses(&[("cancel", "_Cancel"), (RENAME, "_Rename")]);
        dialog.set_response_appearance(RENAME, adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some(RENAME));
        dialog.set_close_response("cancel");
        dialog.set_focus(Some(&entry));
        crate::window::text::tool_dialogs::enter_applies(&dialog, &entry, RENAME);
        (dialog, entry)
    }

    /// An untitled document's tab gets the name typed, in place of its first line:
    /// nothing is saved, and Save As… proposes the name. An empty name follows the first line
    /// again.
    async fn rename_tab(&self, page: &EditorPage) {
        let name = page.name();
        let given = page.state().custom_name.clone();
        let (dialog, entry) = Self::rename_dialog(
            &name,
            "Only the tab's name changes; nothing is saved, and Save As… proposes the name. \
             Leave it empty to name the tab after its first line again.",
        );
        if self.ask(&dialog).await != RENAME {
            return;
        }
        let typed: String = entry
            .text()
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        let typed = typed.trim();
        let custom = if typed.is_empty() {
            None
        } else if given.is_none() && typed == name {
            // The name its first line gives it: the tab goes on following that line.
            return;
        } else {
            Some(typed.to_owned())
        };
        if custom != given {
            page.set_custom_name(custom);
            self.refresh_page(page);
        }
    }

    /// A file's new name in the same folder; the tab follows the file.
    async fn rename_file(&self, page: &EditorPage) {
        let Some(path) = page.path() else {
            return;
        };
        let name = page.name();
        let (dialog, entry) = Self::rename_dialog(
            &name,
            "The file is renamed in its folder, and its tab follows it.",
        );
        select_stem_on_focus(&entry, &name);
        if self.ask(&dialog).await != RENAME {
            return;
        }
        let new_name = entry.text().trim().to_owned();
        if new_name == name || new_name.is_empty() {
            return;
        }
        if new_name.contains('/') || new_name == "." || new_name == ".." {
            self.show_error(
                "Can't rename the file",
                &format!("“{new_name}” is not a file name; a name can't contain “/”."),
            )
            .await;
            return;
        }
        let target = path.with_file_name(&new_name);
        page.set_watch(None);
        let (from, to) = (path.clone(), target.clone());
        let renamed = worker::run(move || {
            rename_no_replace(&from, &to)?;
            std::fs::canonicalize(&to)
        })
        .await;
        match renamed {
            Some(Ok(canonical)) => {
                page.moved_to(target.clone(), canonical);
                self.watch_file(page);
                self.inner.shared.remove_recent(&path);
                self.inner.shared.add_recent(target);
                let head = {
                    let buffer = page.buffer();
                    let mut end = buffer.start_iter();
                    end.forward_chars(4096);
                    buffer.text(&buffer.start_iter(), &end, true).to_string()
                };
                self.detect_language(page, &head);
                self.refresh_indentation(page);
                self.refresh_page(page);
                self.update_tab_actions();
                self.toast(&format!("Renamed to “{new_name}”"));
            }
            Some(Err(error)) => {
                self.watch_file(page);
                let reason = if error.kind() == std::io::ErrorKind::AlreadyExists {
                    format!("“{new_name}” already exists in that folder.")
                } else {
                    error.to_string()
                };
                self.show_error(&format!("Can't rename “{name}”"), &reason)
                    .await;
            }
            None => self.watch_file(page),
        }
    }

    /// Move to Trash…: after a confirmation, the file goes to the trash and its tab closes.
    async fn move_to_trash(&self, page: &EditorPage) {
        let Some(path) = page.path() else {
            return;
        };
        let name = page.name();
        let mut body = "Its tab closes. The file can be restored from the trash.".to_owned();
        if page.is_dirty() {
            body.push_str(" Its unsaved changes are lost.");
        }
        let confirmed = self
            .confirm(
                &format!("Move “{name}” to the trash?"),
                &body,
                "_Move to Trash",
                true,
            )
            .await;
        if !confirmed {
            return;
        }
        page.set_watch(None);
        let file = gio::File::for_path(&path);
        match file.trash_future(glib::Priority::DEFAULT).await {
            Ok(()) => {
                self.inner.shared.remove_recent(&path);
                self.close_without_prompt(page);
                self.toast(&format!("Moved “{name}” to the trash"));
            }
            Err(error) => {
                self.watch_file(page);
                self.show_error(
                    &format!("Can't move “{name}” to the trash"),
                    error.message(),
                )
                .await;
            }
        }
    }
}

/// The tab at (`x`, `y`) of tab strip `bar`, unless that spot is one of its buttons.
fn tab_under(bar: &gtk::Widget, x: f64, y: f64) -> Option<adw::TabPage> {
    let mut widget = bar.pick(x, y, gtk::PickFlags::DEFAULT);
    while let Some(current) = widget {
        if current.is::<gtk::Button>() || current == *bar {
            return None;
        }
        if let Some(tab_page) = tab_page_of(&current) {
            return Some(tab_page);
        }
        widget = current.parent();
    }
    None
}

/// The page of `widget` when it is one of a tab strip's tabs: libadwaita's internal AdwTab,
/// read without a panic should a libadwaita release change it.
fn tab_page_of(widget: &gtk::Widget) -> Option<adw::TabPage> {
    if widget.type_().name() != "AdwTab" || widget.find_property("page").is_none() {
        return None;
    }
    widget.property_value("page").get().ok().flatten()
}

/// Selects `name` without its extension the first time `entry` takes the keyboard focus, so
/// typing replaces the name and keeps the extension. GtkText selects everything as it takes
/// the focus, so the selection is made once that is done. GtkEditable counts characters.
fn select_stem_on_focus(entry: &adw::EntryRow, name: &str) {
    let stem = Path::new(name).file_stem().map_or_else(
        || name.chars().count(),
        |stem| stem.to_string_lossy().chars().count(),
    );
    let Some(text) = entry.delegate() else {
        return;
    };
    let name = name.to_owned();
    // Every time the field gets the focus with the name unchanged, also when it gets it back
    // after the popover that opened the dialog closed: GTK selects everything, then this.
    text.connect_has_focus_notify(move |text| {
        if text.has_focus() && text.text() == name {
            let text = text.clone();
            glib::idle_add_local_full(glib::Priority::HIGH_IDLE, move || {
                text.select_region(0, to_i32(stem));
                glib::ControlFlow::Break
            });
        }
    });
}
