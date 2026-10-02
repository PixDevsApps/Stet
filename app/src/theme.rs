//! The live Omarchy theme and font (ADR-004 and its amendment). The palette comes from
//! `current/theme/colors.toml` through `omarchy-theme-color`; it becomes a generated
//! GtkSourceView scheme and libadwaita CSS variables. Outside Omarchy, two built-in palettes
//! follow the system's dark preference. The font is fontconfig's `monospace` family.

use crate::monitor::{DirWatch, TriggerRule};
use crate::worker;
use gtk4 as gtk;
use gtk4::{gdk, gio, glib};
use libadwaita as adw;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};
use stet_domain::theme::{EditorTheme, Mode, Palette};
use stet_domain::view::{DEFAULT_FONT_SIZE_PT, Zoom, font_css};
use stet_infrastructure::omarchy::{colors, font};
use stet_infrastructure::xdg;

const STATIC_CSS: &str = include_str!("style.css");

type SchemeListener = Rc<dyn Fn(&sourceview5::StyleScheme)>;

/// Where the current palette came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeSource {
    /// An Omarchy theme, by its `theme.name`.
    Omarchy(String),
    /// A built-in palette: no Omarchy theme is installed.
    Builtin(Mode),
}

pub struct Appearance {
    current_dir: PathBuf,
    styles_dir: PathBuf,
    manager: sourceview5::StyleSchemeManager,
    scheme: RefCell<Option<sourceview5::StyleScheme>>,
    scheme_file: RefCell<Option<PathBuf>>,
    theme: RefCell<Option<EditorTheme>>,
    source: RefCell<Option<ThemeSource>>,
    colors: RefCell<Option<gtk::CssProvider>>,
    font: gtk::CssProvider,
    family: RefCell<Option<String>>,
    /// `font` from config.toml, which wins over fontconfig's monospace family.
    font_override: RefCell<Option<String>>,
    /// `font_size` from config.toml, in points before zoom.
    base_size: Cell<i32>,
    zoom: Cell<Zoom>,
    forced: Cell<Option<Mode>>,
    generation: Cell<u64>,
    reloads: Cell<u32>,
    font_reloads: Cell<u32>,
    scheme_listeners: RefCell<Vec<SchemeListener>>,
    theme_watch: RefCell<Option<Rc<DirWatch>>>,
    font_watch: RefCell<Option<Rc<DirWatch>>>,
}

struct OmarchyTheme {
    name: String,
    resolved: BTreeMap<String, String>,
}

impl Appearance {
    /// Loads the static CSS and starts resolving the theme and the font. Call once, after
    /// GTK is initialised.
    pub fn start(omarchy_dir: &Path, fontconfig_dir: &Path, styles_dir: PathBuf) -> Rc<Self> {
        let display = gdk::Display::default().expect("a display after GTK startup");
        let static_css = gtk::CssProvider::new();
        static_css.load_from_string(STATIC_CSS);
        gtk::style_context_add_provider_for_display(
            &display,
            &static_css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let font = gtk::CssProvider::new();
        gtk::style_context_add_provider_for_display(
            &display,
            &font,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let manager = sourceview5::StyleSchemeManager::default();
        manager.append_search_path(&styles_dir.to_string_lossy());

        let this = Rc::new(Self {
            current_dir: omarchy_dir.join("current"),
            styles_dir,
            manager,
            scheme: RefCell::new(None),
            scheme_file: RefCell::new(None),
            theme: RefCell::new(None),
            source: RefCell::new(None),
            colors: RefCell::new(None),
            font,
            family: RefCell::new(None),
            font_override: RefCell::new(None),
            base_size: Cell::new(DEFAULT_FONT_SIZE_PT),
            zoom: Cell::new(Zoom::default()),
            forced: Cell::new(None),
            generation: Cell::new(0),
            reloads: Cell::new(0),
            font_reloads: Cell::new(0),
            scheme_listeners: RefCell::new(Vec::new()),
            theme_watch: RefCell::new(None),
            font_watch: RefCell::new(None),
        });

        let weak = Rc::downgrade(&this);
        adw::StyleManager::default().connect_dark_notify(move |_| {
            if let Some(this) = weak.upgrade()
                && matches!(*this.source.borrow(), Some(ThemeSource::Builtin(_)))
            {
                this.reload_theme();
            }
        });

        let weak = Rc::downgrade(&this);
        let theme_watch = DirWatch::start(this.current_dir.clone(), theme_rule(), move || {
            if let Some(this) = weak.upgrade() {
                this.reload_theme();
            }
        });
        this.theme_watch.replace(Some(theme_watch));

        let weak = Rc::downgrade(&this);
        let font_rule: TriggerRule = Box::new(|event, _, _| {
            use gio::FileMonitorEvent as E;
            matches!(
                event,
                E::ChangesDoneHint
                    | E::Created
                    | E::Deleted
                    | E::Renamed
                    | E::MovedIn
                    | E::MovedOut
            )
        });
        let font_watch = DirWatch::start(fontconfig_dir.to_path_buf(), font_rule, move || {
            if let Some(this) = weak.upgrade() {
                this.reload_font();
            }
        });
        this.font_watch.replace(Some(font_watch));

        this.reload_theme();
        this.reload_font();
        this
    }

    /// Resolves once the first theme and font are applied, or after `timeout`.
    pub async fn ready(&self, timeout: Duration) {
        let started = Instant::now();
        while (self.scheme.borrow().is_none() || self.family.borrow().is_none())
            && started.elapsed() < timeout
        {
            glib::timeout_future(Duration::from_millis(2)).await;
        }
    }

    pub fn scheme(&self) -> Option<sourceview5::StyleScheme> {
        self.scheme.borrow().clone()
    }

    pub fn source(&self) -> Option<ThemeSource> {
        self.source.borrow().clone()
    }

    pub fn family(&self) -> Option<String> {
        self.family.borrow().clone()
    }

    pub fn zoom(&self) -> Zoom {
        self.zoom.get()
    }

    /// The editor's family: config.toml's `font`, else fontconfig's monospace family.
    pub fn editor_family(&self) -> Option<String> {
        self.font_override
            .borrow()
            .clone()
            .or_else(|| self.family())
    }

    /// Applies config.toml's `font` and `font_size` (ADR-017).
    pub fn set_font_settings(&self, family: Option<String>, size_pt: i32) {
        let changed = *self.font_override.borrow() != family || self.base_size.get() != size_pt;
        if changed {
            self.font_override.replace(family);
            self.base_size.set(size_pt);
            self.apply_font_css();
        }
    }

    pub fn set_zoom(&self, zoom: Zoom) {
        if zoom != self.zoom.get() {
            self.zoom.set(zoom);
            self.apply_font_css();
        }
    }

    /// Completed theme reloads (the first load included), for the self-test.
    pub fn reloads(&self) -> u32 {
        self.reloads.get()
    }

    pub fn font_reloads(&self) -> u32 {
        self.font_reloads.get()
    }

    /// Called with the new scheme after every theme change; set it on every buffer.
    pub fn connect_scheme_changed(&self, listener: impl Fn(&sourceview5::StyleScheme) + 'static) {
        self.scheme_listeners.borrow_mut().push(Rc::new(listener));
    }

    fn reload_theme(self: &Rc<Self>) {
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        let current = self.current_dir.clone();
        let this = self.clone();
        glib::spawn_future_local(async move {
            let loaded = worker::run(move || load_omarchy(&current)).await.flatten();
            if this.generation.get() != generation {
                return;
            }
            let (palette, source) = match loaded {
                Some(theme) => (
                    Palette::from_resolved(&theme.resolved),
                    ThemeSource::Omarchy(theme.name),
                ),
                None => {
                    let style = adw::StyleManager::default();
                    if this.forced.take().is_some() {
                        style.set_color_scheme(adw::ColorScheme::Default);
                    }
                    let mode = if style.is_dark() {
                        Mode::Dark
                    } else {
                        Mode::Light
                    };
                    (Palette::builtin(mode), ThemeSource::Builtin(mode))
                }
            };
            this.apply(palette, source, generation).await;
        });
    }

    async fn apply(&self, palette: Palette, source: ThemeSource, generation: u64) {
        let theme = EditorTheme::new(&palette);
        let unchanged = self.theme.borrow().as_ref() == Some(&theme)
            && self.source.borrow().as_ref() == Some(&source);
        if unchanged {
            self.reloads.set(self.reloads.get() + 1);
            tracing::info!("theme reloaded; unchanged");
            return;
        }
        let id = theme.scheme_id();
        let name = match &source {
            ThemeSource::Omarchy(name) => format!("Stet ({name})"),
            ThemeSource::Builtin(mode) => format!("Stet (built-in {})", mode.as_str()),
        };
        let xml = theme.style_scheme_xml(&id, &name);
        let styles_dir = self.styles_dir.clone();
        let file_id = id.clone();
        let written = worker::run(move || write_scheme(&styles_dir, &file_id, &xml)).await;
        if self.generation.get() != generation {
            return;
        }
        let path = match written {
            Some(Ok(path)) => path,
            Some(Err(error)) => {
                tracing::error!(%error, "could not write the style scheme");
                return;
            }
            None => return,
        };
        self.manager.force_rescan();
        let Some(scheme) = self.manager.scheme(&id) else {
            tracing::error!(id, "the generated style scheme did not load");
            return;
        };

        let provider = gtk::CssProvider::new();
        provider.connect_parsing_error(|_, section, error| {
            tracing::warn!(section = %section.to_str(), %error, "theme CSS");
        });
        provider.load_from_string(&theme.adwaita_css());
        let display = gdk::Display::default().expect("a display");
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        if let Some(old) = self.colors.replace(Some(provider)) {
            gtk::style_context_remove_provider_for_display(&display, &old);
        }

        self.scheme.replace(Some(scheme.clone()));
        self.scheme_file.replace(Some(path));
        tracing::info!(theme = %name, "theme applied");
        self.theme.replace(Some(theme));
        let omarchy = matches!(source, ThemeSource::Omarchy(_));
        // Set before the colour scheme changes: that emits `notify::dark`, which reloads
        // only while a built-in palette is in use.
        self.source.replace(Some(source));
        if omarchy && self.forced.get() != Some(palette.mode) {
            self.forced.set(Some(palette.mode));
            adw::StyleManager::default().set_color_scheme(match palette.mode {
                Mode::Dark => adw::ColorScheme::ForceDark,
                Mode::Light => adw::ColorScheme::ForceLight,
            });
        }
        self.reloads.set(self.reloads.get() + 1);
        let listeners = self.scheme_listeners.borrow().clone();
        for listener in listeners {
            listener(&scheme);
        }
    }

    fn reload_font(self: &Rc<Self>) {
        let this = self.clone();
        glib::spawn_future_local(async move {
            let family = worker::run(font::monospace_family)
                .await
                .unwrap_or_else(|| font::FALLBACK_FAMILY.to_owned());
            this.font_reloads.set(this.font_reloads.get() + 1);
            if this.family.borrow().as_deref() != Some(family.as_str()) {
                tracing::info!(%family, "editor font");
                this.family.replace(Some(family));
                this.apply_font_css();
            }
        });
    }

    fn apply_font_css(&self) {
        let family = self
            .editor_family()
            .unwrap_or_else(|| font::FALLBACK_FAMILY.to_owned());
        let size = self.zoom.get().font_size(self.base_size.get());
        self.font.load_from_string(&font_css(&family, size));
    }
}

/// `theme.name` CHANGES_DONE_HINT, or a rename to `theme` (the swap in `omarchy-theme-set`).
/// `current` appearing or disappearing is handled by [`DirWatch`] itself.
fn theme_rule() -> TriggerRule {
    Box::new(|event, file, other| {
        use gio::FileMonitorEvent as E;
        match event {
            E::ChangesDoneHint => file == "theme.name",
            E::Renamed => other == Some("theme"),
            _ => false,
        }
    })
}

fn load_omarchy(current: &Path) -> Option<OmarchyTheme> {
    let colors_file = current.join("theme/colors.toml");
    if !colors_file.is_file() {
        return None;
    }
    let resolved = match colors::resolve_palette(&colors_file) {
        Ok(resolved) => resolved,
        Err(error) => {
            tracing::warn!(%error, "could not read the Omarchy palette; using the built-in one");
            return None;
        }
    };
    let name = std::fs::read_to_string(current.join("theme.name"))
        .map(|name| name.trim().to_owned())
        .unwrap_or_default();
    Some(OmarchyTheme { name, resolved })
}

/// Writes `<id>.xml` and removes other generated schemes, so exactly one is in the search path.
fn write_scheme(dir: &Path, id: &str, xml: &str) -> io::Result<PathBuf> {
    xdg::ensure_private_dir(dir)?;
    let path = dir.join(format!("{id}.xml"));
    std::fs::write(&path, xml)?;
    for entry in std::fs::read_dir(dir)?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("stet-omarchy-") && name.ends_with(".xml") && entry.path() != path {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    Ok(path)
}
