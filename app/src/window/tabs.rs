//! The tab strip's context menu (M5) and the File-menu commands behind it: copy the path, the
//! name or the folder; open the folder or a terminal there; rename the file, or an untitled
//! tab (1.2); move it to the trash; close the other tabs or those to the right; save. From the
//! tab menu a command acts on the tab that was clicked, otherwise on the current one
//! (INTEGRATIONS.md section 7). A double-click on a tab renames it too (1.1).

use super::Window;
use super::dialogs::SaveChoice;
use crate::actions;
use crate::editor::EditorPage;
use crate::worker;
use gtk4 as gtk;
use gtk4::{gio, glib};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use stet_domain::actions::ActionId;
use stet_infrastructure::desktop;
use stet_infrastructure::fs::rename_no_replace;

/// The response that renames.
pub const RENAME: &str = "rename";

impl Window {
    pub(super) fn connect_tab_menu(&self) {
        let menu = actions::tab_menu(&self.keymap());
        for view in &self.inner.views {
            let tabs = &view.tabs;
            tabs.set_menu_model(Some(&menu));
            tabs.connect_setup_menu(glib::clone!(
                #[weak(rename_to = inner)]
                self.inner,
                move |_, tab_page| {
                    let window = Window { inner };
                    match tab_page {
                        Some(tab_page) => {
                            window.inner.menu_page.replace(Some(tab_page.clone()));
                            window.update_actions();
                            window.name_menu_items_soon();
                        }
                        // The menu closed; its command, if any, has run by now or runs from
                        // this event, so the clicked tab is forgotten once the event is done.
                        None => {
                            let weak = std::rc::Rc::downgrade(&window.inner);
                            glib::idle_add_local_once(move || {
                                if let Some(inner) = weak.upgrade() {
                                    let window = Window { inner };
                                    window.inner.menu_page.replace(None);
                                    window.update_actions();
                                    window.restore_focus();
                                }
                            });
                        }
                    }
                }
            ));
        }
    }

    /// A double-click on a tab opens Rename…: for its file, or for an untitled tab's name.
    pub(super) fn connect_tab_double_click(&self) {
        for view in &self.inner.views {
            let click = gtk::GestureClick::builder()
                .button(gtk::gdk::BUTTON_PRIMARY)
                .propagation_phase(gtk::PropagationPhase::Capture)
                .build();
            click.connect_pressed(glib::clone!(
                #[weak(rename_to = inner)]
                self.inner,
                move |gesture, presses, x, y| {
                    let Some(bar) = gesture.widget() else {
                        return;
                    };
                    if (Window { inner }).tab_pressed(&bar, presses, x, y) {
                        // Neither the strip nor a window handle around it takes the press.
                        gesture.set_state(gtk::EventSequenceState::Claimed);
                    }
                }
            ));
            view.bar.add_controller(click);
        }
    }

    /// A press at (`x`, `y`) on tab strip `bar`, the `presses`th of a click: the second one on
    /// the same tab renames it. Returns whether it did.
    pub fn tab_pressed(&self, bar: &gtk::Widget, presses: i32, x: f64, y: f64) -> bool {
        let tab_page = tab_under(bar, x, y);
        // Both presses on the tab itself: a quick second click on a close button lands on the
        // tab that moves in under it.
        let previous = self.inner.pressed_tab.upgrade();
        self.inner.pressed_tab.set(tab_page.as_ref());
        match tab_page {
            Some(tab_page) if presses == 2 && previous.as_ref() == Some(&tab_page) => {
                self.rename_from_tab(&super::page_of(&tab_page))
            }
            _ => false,
        }
    }

    /// The strip that shows `tab_page` and the tab on it (for the self-test).
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
    pub(super) fn context_page(&self) -> Option<EditorPage> {
        self.inner
            .menu_page
            .borrow()
            .as_ref()
            .map(super::page_of)
            .or_else(|| self.current_page())
    }

    /// Enables the tab commands that make sense for the tab they would act on.
    pub(super) fn update_tab_actions(&self) {
        let page = self.context_page();
        let ready = page.as_ref().is_some_and(|page| !page.is_loading());
        let has_file = ready && page.as_ref().is_some_and(|page| page.path().is_some());
        let renamable = ready
            && page
                .as_ref()
                .is_some_and(|page| page.state().temporary.is_none());
        // The closing commands act within the tab's own view (M7).
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

    pub(super) fn run_tab_tool(&self, id: ActionId) {
        let Some(page) = self.context_page() else {
            return;
        };
        match id {
            ActionId::CloseTab => {
                if let Some((view, tab_page)) = self.locate(&page) {
                    self.inner.views[view].tabs.close_page(&tab_page);
                }
            }
            ActionId::CloseOthers | ActionId::CloseToTheRight => {
                let Some(view) = self.view_of(&page) else {
                    return;
                };
                let pages = super::view_pages(&self.inner.views[view].tabs);
                let Some(position) = pages.iter().position(|other| *other == page) else {
                    return;
                };
                let closing: Vec<EditorPage> = pages
                    .into_iter()
                    .enumerate()
                    .filter(|(index, _)| {
                        if id == ActionId::CloseOthers {
                            *index != position
                        } else {
                            *index > position
                        }
                    })
                    .map(|(_, page)| page)
                    .collect();
                // Pinned tabs stay, as with Close All (M8).
                let closing = self.closable_by_bulk_commands(closing);
                let window = self.clone();
                self.spawn(async move { window.close_pages(closing, &page).await });
            }
            ActionId::CopyFullPath | ActionId::CopyFileName | ActionId::CopyDirectoryPath => {
                let text = match (id, page.path()) {
                    (ActionId::CopyFileName, _) => page.name(),
                    (ActionId::CopyFullPath, Some(path)) => path.display().to_string(),
                    (_, Some(path)) => path
                        .parent()
                        .map(|dir| dir.display().to_string())
                        .unwrap_or_default(),
                    (_, None) => return,
                };
                page.view().clipboard().set_text(&text);
                self.toast(&format!("Copied “{text}”"));
            }
            ActionId::OpenContainingFolder | ActionId::OpenTerminalHere => {
                let Some(dir) = page
                    .path()
                    .and_then(|path| path.parent().map(Path::to_path_buf))
                else {
                    return;
                };
                let terminal = id == ActionId::OpenTerminalHere;
                let window = self.clone();
                self.spawn(async move { window.open_externally(dir, terminal).await });
            }
            ActionId::RenameFile => {
                let window = self.clone();
                self.spawn(async move { window.rename(&page).await });
            }
            ActionId::MoveToTrash => {
                let window = self.clone();
                self.spawn(async move { window.move_to_trash(&page).await });
            }
            _ => {}
        }
    }

    /// Closes `pages`, asking once about the unsaved ones, as Close All does. `keep` stays
    /// in front.
    async fn close_pages(&self, pages: Vec<EditorPage>, keep: &EditorPage) {
        let dirty: Vec<EditorPage> = self
            .closing_documents(&pages)
            .into_iter()
            .filter(|page| page.is_dirty())
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
        super::tool_dialogs::enter_applies(&dialog, &entry, RENAME);
        (dialog, entry)
    }

    /// An untitled document's tab gets the name typed, in place of its first line (1.2):
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
            page.update_state(|state| state.custom_name = custom);
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
                page.update_state(|state| {
                    state.path = Some(target.clone());
                    state.canonical = Some(canonical);
                });
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
    let stem = Path::new(name)
        .file_stem()
        .map_or(name.chars().count(), |stem| {
            stem.to_string_lossy().chars().count()
        });
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
                text.select_region(0, stem as i32);
                glib::ControlFlow::Break
            });
        }
    });
}
