//! `config.toml` and `keys.toml` (ADR-017): read on a worker when Stet starts and again
//! whenever either changes in `$XDG_CONFIG_HOME/stet/` (a file monitor, debounced). A file
//! with mistakes is not applied; the previous settings or keys stay and the listeners hear
//! what is wrong, with the line and column.

use crate::monitor::{DirWatch, TriggerRule};
use crate::runtime::{Latest, Listeners, Notify, wait_until, worker};
use gtk4 as gtk;
use gtk4::glib;
use gtk4::glib::translate::IntoGlib;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use stet_domain::settings::{CONFIG_FILE, FileError, KEYS_FILE, Keymap, Settings};
use stet_infrastructure::settings_files::read_optional;

/// What a reload found.
#[derive(Debug, Clone, Default)]
pub struct Update {
    /// The settings that were in use before, when new ones were applied.
    pub settings_before: Option<Settings>,
    pub keymap_changed: bool,
    /// One message per file that could not be applied, for a toast.
    pub problems: Vec<String>,
}

pub struct Preferences {
    config_file: PathBuf,
    keys_file: PathBuf,
    settings: RefCell<Settings>,
    keymap: RefCell<Keymap>,
    /// The text each file had at the last reload, so an unchanged file is not reported again.
    texts: RefCell<[Option<String>; 2]>,
    errors: RefCell<[Vec<FileError>; 2]>,
    loaded: Cell<bool>,
    /// Wakes [`Self::ready`]'s waiters once the files were read.
    first_read: Notify,
    /// A ticket per reload: the newest wins.
    reads: Latest,
    listeners: Listeners<Update>,
    watch: RefCell<Option<Rc<DirWatch>>>,
}

impl Preferences {
    /// Starts reading both files and watching their directory.
    pub fn start(dir: PathBuf) -> Rc<Self> {
        let this = Rc::new(Self {
            config_file: dir.join(CONFIG_FILE),
            keys_file: dir.join(KEYS_FILE),
            settings: RefCell::new(Settings::default()),
            keymap: RefCell::new(Keymap::default()),
            texts: RefCell::new([None, None]),
            errors: RefCell::new([Vec::new(), Vec::new()]),
            loaded: Cell::new(false),
            first_read: Notify::default(),
            reads: Latest::default(),
            listeners: Listeners::default(),
            watch: RefCell::new(None),
        });
        let rule: TriggerRule = Box::new(|_, file, other| {
            [Some(file), other]
                .into_iter()
                .flatten()
                .any(|name| name == CONFIG_FILE || name == KEYS_FILE)
        });
        let weak = Rc::downgrade(&this);
        let watch = DirWatch::start(dir, rule, move || {
            if let Some(this) = weak.upgrade() {
                this.reload();
            }
        });
        this.watch.replace(Some(watch));
        this.reload();
        this
    }

    pub fn settings(&self) -> Settings {
        self.settings.borrow().clone()
    }

    pub fn keymap(&self) -> Keymap {
        self.keymap.borrow().clone()
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_file.clone()
    }

    pub fn keys_file(&self) -> PathBuf {
        self.keys_file.clone()
    }

    /// The mistakes found in `config.toml` and `keys.toml` at the last reload.
    pub fn errors(&self) -> [Vec<FileError>; 2] {
        self.errors.borrow().clone()
    }

    pub fn connect_changed(&self, listener: impl Fn(&Update) + 'static) {
        self.listeners.connect(listener);
    }

    /// Resolves once the files have been read the first time, or after `timeout`.
    pub async fn ready(&self, timeout: Duration) {
        wait_until(
            Some(timeout),
            || self.loaded.get(),
            || self.first_read.notified(),
        )
        .await;
    }

    /// Reads both files again; the newest reload wins.
    pub fn reload(self: &Rc<Self>) {
        let ticket = self.reads.next();
        let paths = [self.config_file.clone(), self.keys_file.clone()];
        let this = self.clone();
        glib::spawn_future_local(async move {
            let texts = worker::run(move || paths.map(|path| read_optional(&path))).await;
            if !ticket.is_current() {
                return;
            }
            if let Some(texts) = texts {
                this.apply(texts);
            }
        });
    }

    fn apply(&self, texts: [std::io::Result<Option<String>>; 2]) {
        let mut update = Update::default();
        let [config, keys] = texts;
        let first = !self.loaded.get();
        match config {
            Ok(text) if first || self.texts.borrow()[0] != text => {
                self.texts.borrow_mut()[0].clone_from(&text);
                match Settings::parse(text.as_deref().unwrap_or("")) {
                    Ok(settings) => {
                        self.errors.borrow_mut()[0].clear();
                        if *self.settings.borrow() != settings || first {
                            update.settings_before = Some(self.settings.replace(settings));
                        }
                    }
                    Err(errors) => {
                        update.problems.push(problem(CONFIG_FILE, &errors));
                        self.errors.borrow_mut()[0] = errors;
                    }
                }
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(%error, "could not read config.toml");
                update
                    .problems
                    .push(format!("{CONFIG_FILE} could not be read: {error}"));
            }
        }
        match keys {
            Ok(text) if first || self.texts.borrow()[1] != text => {
                self.texts.borrow_mut()[1].clone_from(&text);
                match Keymap::parse(text.as_deref().unwrap_or(""), &canonical) {
                    Ok(keymap) => {
                        self.errors.borrow_mut()[1].clear();
                        if *self.keymap.borrow() != keymap || first {
                            self.keymap.replace(keymap);
                            update.keymap_changed = true;
                        }
                    }
                    Err(errors) => {
                        update.problems.push(problem(KEYS_FILE, &errors));
                        self.errors.borrow_mut()[1] = errors;
                    }
                }
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(%error, "could not read keys.toml");
                update
                    .problems
                    .push(format!("{KEYS_FILE} could not be read: {error}"));
            }
        }
        self.loaded.set(true);
        self.first_read.notify();
        if update.settings_before.is_some() || update.keymap_changed || !update.problems.is_empty()
        {
            self.listeners.notify(&update);
        }
    }
}

/// The toast for a file with mistakes: the first one, and how many more there are.
fn problem(file: &str, errors: &[FileError]) -> String {
    let more = match errors.len() {
        0 | 1 => String::new(),
        2 => " (and 1 more mistake)".to_owned(),
        count => format!(" (and {} more mistakes)", count - 1),
    };
    let first = errors.first().map_or_else(String::new, ToString::to_string);
    format!("{file} not applied: {first}{more}")
}

/// GTK's spelling of a key, or `None` when GTK can't parse it.
pub fn canonical(key: &str) -> Option<String> {
    let (keyval, modifiers) = gtk::accelerator_parse(key)?;
    if keyval.into_glib() == 0 {
        return None;
    }
    Some(gtk::accelerator_name(keyval, modifiers).to_string())
}
