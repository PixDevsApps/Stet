//! What `config.toml` and `keys.toml` do to the window (ADR-017): the keymap's accelerators,
//! editor shortcuts and menus, the font, the view defaults, indentation and smart
//! highlighting, applied when Stet starts and whenever a file changes. Also the palette's
//! Open Settings and Open Keyboard Shortcuts, and the read-only Keyboard Shortcuts overview.

use super::{Location, Window};
use crate::actions;
use crate::editor::{EditorPage, IndentSource};
use crate::preferences::Update;
use crate::worker;
use gtk4::glib;
use libadwaita as adw;
use libadwaita::prelude::*;
use std::time::Duration;
use stet_domain::actions::{ActionId, KeyScope, Menu};
use stet_domain::settings::keymap::default_keys_file;
use stet_domain::settings::{Keymap, Settings, default_config_file};
use stet_infrastructure::settings_files::create_if_missing;

/// How long the text must rest before its words are counted again.
const WORD_COUNT_DELAY: Duration = Duration::from_millis(250);

/// Words: runs of characters between white space.
fn count_words(text: &str) -> usize {
    text.split_whitespace().count()
}

impl Window {
    /// The settings in use.
    pub fn settings(&self) -> Settings {
        self.inner.shared.preferences.settings()
    }

    /// The keymap in use.
    pub fn keymap(&self) -> Keymap {
        self.inner.shared.preferences.keymap()
    }

    /// Applies what is loaded now, then every later change.
    pub(super) fn connect_preferences(&self) {
        let preferences = self.inner.shared.preferences.clone();
        let mut problems = Vec::new();
        for (file, errors) in [
            (stet_domain::settings::CONFIG_FILE, &preferences.errors()[0]),
            (stet_domain::settings::KEYS_FILE, &preferences.errors()[1]),
        ] {
            if let Some(first) = errors.first() {
                problems.push(format!("{file} not applied: {first}"));
            }
        }
        self.apply_preferences(
            &Update {
                settings_before: None,
                keymap_changed: true,
                problems,
            },
            true,
        );
        preferences.connect_changed(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |update| Window { inner }.apply_preferences(update, false)
        ));
    }

    fn apply_preferences(&self, update: &Update, initial: bool) {
        if update.keymap_changed {
            let keymap = self.keymap();
            if let Some(app) = self
                .inner
                .window
                .application()
                .and_downcast::<adw::Application>()
            {
                actions::apply_accels(&app, &keymap);
            }
            actions::fill_editor_shortcuts(&self.inner.shared.editor_shortcuts, &keymap);
            self.rebuild_menu();
            let tab_menu = actions::tab_menu(&keymap);
            for view in &self.inner.views {
                view.tabs.set_menu_model(Some(&tab_menu));
            }
            let editor_menu = actions::editor_menu(&keymap);
            for page in self.pages() {
                page.view().set_extra_menu(Some(&editor_menu));
            }
        }
        if initial || update.settings_before.is_some() {
            let settings = self.settings();
            let before = update.settings_before.clone().unwrap_or_default();
            self.inner
                .shared
                .appearance
                .set_font_settings(settings.font.clone(), settings.font_size as i32);
            let mut view = self.inner.settings.get();
            if initial || before.word_wrap != settings.word_wrap {
                view.wrap = settings.word_wrap;
            }
            if initial || before.show_whitespace != settings.show_whitespace {
                view.whitespace = settings.show_whitespace;
            }
            if initial || before.document_map != settings.document_map {
                view.map = settings.document_map;
            }
            self.set_view_settings(view);
            if !initial
                && (before.backups != settings.backups
                    || before.backup_interval != settings.backup_interval)
            {
                self.backup_settings_changed();
            }
            for page in self.pages() {
                if page.indent_source() == IndentSource::Default {
                    self.refresh_indentation(&page);
                }
                if !settings.smart_highlight {
                    page.search().cancel_smart_timeout();
                    page.search().set_smart(&page.buffer(), None, None);
                }
            }
            if settings.smart_highlight
                && !before.smart_highlight
                && let Some(page) = self.current_page()
            {
                self.update_smart_highlight(&page);
            }
            self.inner.status.show_words(settings.word_count);
            self.update_status();
        }
        for problem in &update.problems {
            self.toast(problem);
        }
    }

    /// The status bar's word count (config.toml's `word_count`): counted 250 ms after the
    /// text last changed, on a worker for a large document; not in large-file mode.
    pub(super) fn update_word_count(&self, page: &EditorPage) {
        let status = &self.inner.status;
        if page.large_file_mode() {
            status.set_words(None);
            return;
        }
        let revision = page.revision();
        let cached = self
            .inner
            .tools
            .words
            .borrow()
            .as_ref()
            .filter(|(counted, at, _)| counted.upgrade().as_ref() == Some(page) && *at == revision)
            .map(|(_, _, words)| *words);
        if let Some(words) = cached {
            status.set_words(Some(words));
            return;
        }
        status.set_words(None);
        if let Some(pending) = self.inner.tools.words_pending.take() {
            pending.remove();
        }
        let window = self.clone();
        let page = page.clone();
        let source = glib::timeout_add_local_once(WORD_COUNT_DELAY, move || {
            window.inner.tools.words_pending.replace(None);
            if page.revision() != revision || window.tab_page(&page).is_none() {
                return;
            }
            let text = page.text();
            let inner = window.clone();
            window.spawn(async move {
                let words = if text.len() <= super::SYNC_BYTES {
                    Some(count_words(&text))
                } else {
                    worker::run(move || count_words(&text)).await
                };
                if let Some(words) = words
                    && page.revision() == revision
                {
                    inner
                        .inner
                        .tools
                        .words
                        .replace(Some((page.downgrade(), revision, words)));
                    if inner.current_page().as_ref() == Some(&page) {
                        inner.inner.status.set_words(Some(words));
                    }
                }
            });
        });
        self.inner.tools.words_pending.replace(Some(source));
    }

    /// Open Settings and Open Keyboard Shortcuts: the file in a tab, created with every
    /// default commented out when it does not exist yet.
    pub(super) fn open_settings_file(&self, keys: bool) {
        let preferences = &self.inner.shared.preferences;
        let path = if keys {
            preferences.keys_file()
        } else {
            preferences.config_file()
        };
        let window = self.clone();
        self.spawn(async move {
            let file = path.clone();
            let created = worker::run(move || {
                let text = if keys {
                    default_keys_file()
                } else {
                    default_config_file()
                };
                create_if_missing(&file, &text)
            })
            .await;
            match created {
                Some(Ok(created)) => {
                    window.open_locations(vec![Location {
                        path: path.clone(),
                        position: None,
                        create: true,
                    }]);
                    if created {
                        let name = path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        window.toast(&format!(
                            "Created {name}; every line is a default, commented out"
                        ));
                    }
                }
                Some(Err(error)) => {
                    window.toast(&format!("Could not create {}: {error}", path.display()));
                }
                None => {}
            }
        });
    }

    /// The read-only Keyboard Shortcuts overview, from the registry and `keys.toml`.
    pub(super) fn show_keyboard_shortcuts(&self) {
        let keymap = self.keymap();
        let dialog = adw::ShortcutsDialog::new();
        let mut sections: Vec<(String, Vec<ActionId>)> = Menu::ALL
            .into_iter()
            .map(|menu| (menu.label().to_owned(), Vec::new()))
            .collect();
        sections.push(("Command palette".to_owned(), Vec::new()));
        for id in ActionId::ALL {
            if keymap.keys(id).is_empty() {
                continue;
            }
            let index = id
                .spec()
                .menu
                .and_then(|place| Menu::ALL.iter().position(|menu| *menu == place.menu))
                .unwrap_or(Menu::ALL.len());
            sections[index].1.push(id);
        }
        for (title, ids) in sections {
            if ids.is_empty() {
                continue;
            }
            let section = adw::ShortcutsSection::new(Some(&title));
            for id in ids {
                let item = adw::ShortcutsItem::new(id.label(), &keymap.keys(id).join(" "));
                let mut subtitle = id
                    .spec()
                    .menu
                    .and_then(|place| place.submenu)
                    .map(str::to_owned)
                    .unwrap_or_default();
                if id.key_scope() == KeyScope::Editor {
                    if !subtitle.is_empty() {
                        subtitle.push_str(" · ");
                    }
                    subtitle.push_str("in the editor");
                }
                item.set_subtitle(&subtitle);
                section.add(item);
            }
            dialog.add(section);
        }
        dialog.connect_closed(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |_| Window { inner }.restore_focus_later()
        ));
        dialog.present(Some(&self.inner.window));
    }
}
