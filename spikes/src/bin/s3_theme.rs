//! Spike S3: the Omarchy theme as a GtkSourceView 5 style scheme plus libadwaita CSS variables,
//! applied live to one realized window for every installed theme, and the GFileMonitor trigger
//! that reloads it when `omarchy-theme-set` swaps `current/theme`.
//!
//! `tools/headless.sh 13 cargo run --release -p stet-spikes --bin s3_theme`
//! prints JSON lines and writes Markdown tables to `.local/s3/results.md`. The reload test runs
//! against a fake `omarchy/current/` under `target/`; `~/.local/state/omarchy` is never touched.

use anyhow::{Context, Result, bail, ensure};
use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib, graphene, gsk, pango};
use libadwaita as adw;
use sourceview5::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use stet_domain::theme::{EditorTheme, Palette, Rgb};
use stet_infrastructure::omarchy::colors::{parse_colors, resolve_palette_with};
use stet_infrastructure::omarchy::theme_color_script;
use stet_spikes::{Report, require_headless, run_main_loop_until, wait_frames};

const ROUNDS: usize = 5;
const FRAME_TIMEOUT: Duration = Duration::from_secs(5);
const DEBOUNCE: Duration = Duration::from_millis(150);
const SETTLE: Duration = Duration::from_millis(700);
const SCENARIO_TIMEOUT: Duration = Duration::from_secs(30);
const MISSING_DIR_WAIT: Duration = Duration::from_secs(10);
const TOLERANCE: i16 = 2;
const IDLE_WAIT: Duration = Duration::from_millis(50);
const SETTLE_FRAMES: u32 = 4;
const SOURCE: &str = "fn main() {\n    let n: i32 = 42; // note\n    let s = \"hi\";\n}\n";
const SEMANTIC_KEYS: [&str; 27] = [
    "mode",
    "accent",
    "background",
    "dark_background",
    "darker_background",
    "lighter_background",
    "foreground",
    "dark_foreground",
    "light_foreground",
    "bright_foreground",
    "muted",
    "selection",
    "selection_foreground",
    "red",
    "yellow",
    "orange",
    "green",
    "cyan",
    "blue",
    "magenta",
    "brown",
    "bright_red",
    "bright_yellow",
    "bright_green",
    "bright_cyan",
    "bright_blue",
    "bright_magenta",
];

/// The swap `omarchy-theme-set` performs in `current/`, line for line, minus the shell IPC,
/// app restarts and hooks (which never touch `current/`). Arguments: current dir, skip the
/// background link (1/0), delete `theme.name` first (1/0), then `<theme dir> <name>` pairs.
const SWAP_SCRIPT: &str = r#"
swap() {
  local cur=$1 src=$2 name=$3 skip_bg=$4
  local next="$cur/next-theme" bg
  rm -rf "$next"
  mkdir -p "$next"
  cp -r "$src/"* "$next/" 2>/dev/null
  [[ -f $next/helix.toml ]] || printf '# omarchy-theme-set-templates output\n' >"$next/helix.toml"
  rm -rf "$cur/theme"
  mv "$next" "$cur/theme"
  echo "$name" >"$cur/theme.name"
  if [[ $skip_bg != 1 ]]; then
    bg=$(find -L "$cur/theme/backgrounds/" -maxdepth 1 -type f 2>/dev/null | sort | head -n 1)
    [[ -n $bg ]] && ln -nsf "$bg" "$cur/background"
  fi
  return 0
}
cur=$1 skip_bg=$2 fresh=$3
shift 3
mkdir -p "$cur"
if [[ $fresh == 1 ]]; then
  rm -f "$cur/theme.name"
  sleep 0.3
fi
while (($# >= 2)); do
  swap "$cur" "$1" "$2" "$skip_bg"
  shift 2
done
"#;

static GLIB_WARNINGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn capture_glib_warnings() {
    glib::log_set_writer_func(|level, fields| {
        if matches!(
            level,
            glib::LogLevel::Warning | glib::LogLevel::Critical | glib::LogLevel::Error
        ) {
            let field = |key: &str| {
                fields
                    .iter()
                    .find(|field| field.key() == key)
                    .and_then(|field| field.value_str())
                    .unwrap_or_default()
                    .to_owned()
            };
            let message = format!("{}: {}", field("GLIB_DOMAIN"), field("MESSAGE"));
            if let Ok(mut warnings) = GLIB_WARNINGS.lock() {
                warnings.push(message);
            }
        }
        glib::log_writer_default(level, fields)
    });
}

fn glib_warnings_since(start: usize) -> Vec<String> {
    GLIB_WARNINGS
        .lock()
        .map(|warnings| warnings.get(start..).unwrap_or_default().to_vec())
        .unwrap_or_default()
}

fn glib_warning_count() -> usize {
    GLIB_WARNINGS
        .lock()
        .map(|warnings| warnings.len())
        .unwrap_or(0)
}

#[derive(Clone, Copy)]
struct Stats {
    median: f64,
    min: f64,
    max: f64,
}

impl Stats {
    fn of(samples: &[Duration]) -> Self {
        let mut ms: Vec<f64> = samples
            .iter()
            .map(|sample| sample.as_secs_f64() * 1000.0)
            .collect();
        ms.sort_by(f64::total_cmp);
        let median = match ms.len() {
            0 => f64::NAN,
            n if n % 2 == 1 => ms[n / 2],
            n => (ms[n / 2 - 1] + ms[n / 2]) / 2.0,
        };
        Self {
            median,
            min: ms.first().copied().unwrap_or(f64::NAN),
            max: ms.last().copied().unwrap_or(f64::NAN),
        }
    }

    fn cell(self) -> String {
        if self.max < 1.0 {
            format!("{:.3} ({:.3}–{:.3})", self.median, self.min, self.max)
        } else {
            format!("{:.1} ({:.1}–{:.1})", self.median, self.min, self.max)
        }
    }

    fn json(self) -> serde_json::Value {
        serde_json::json!({ "median_ms": self.median, "min_ms": self.min, "max_ms": self.max })
    }
}

struct ThemeDir {
    name: String,
    dir: PathBuf,
    colors: PathBuf,
    builtin: bool,
}

fn discover_themes() -> Result<Vec<ThemeDir>> {
    let home = std::env::home_dir().context("no home directory")?;
    let mut themes = Vec::new();
    for (root, builtin) in [
        (PathBuf::from("/usr/share/omarchy/themes"), true),
        (home.join(".config/omarchy/themes"), false),
    ] {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        let mut found: Vec<ThemeDir> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|dir| dir.join("colors.toml").is_file())
            .map(|dir| ThemeDir {
                name: dir
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                colors: dir.join("colors.toml"),
                dir,
                builtin,
            })
            .collect();
        found.sort_by(|a, b| a.name.cmp(&b.name));
        themes.extend(found);
    }
    ensure!(!themes.is_empty(), "no Omarchy themes with a colors.toml");
    Ok(themes)
}

struct Agreement {
    script: Option<Stats>,
    builtin: Stats,
    maps_equal: Option<bool>,
    differing_keys: Vec<String>,
    palettes_equal: Option<bool>,
    missing: Vec<&'static str>,
}

fn compare_resolvers(theme: &ThemeDir, script: Option<&Path>) -> Result<Agreement> {
    let mut builtin_times = Vec::new();
    let mut builtin = BTreeMap::new();
    for _ in 0..ROUNDS {
        let started = Instant::now();
        builtin = resolve_palette_with(&theme.colors, None)?;
        builtin_times.push(started.elapsed());
    }
    let mut script_result = None;
    if let Some(script) = script {
        let mut times = Vec::new();
        let mut resolved = BTreeMap::new();
        for _ in 0..ROUNDS {
            let started = Instant::now();
            resolved = resolve_palette_with(&theme.colors, Some(script))?;
            times.push(started.elapsed());
        }
        script_result = Some((resolved, Stats::of(&times)));
    }
    let raw = parse_colors(&std::fs::read_to_string(&theme.colors)?);
    let present = |key: &str| {
        let has = |key: &str| raw.get(key).is_some_and(|value| !value.is_empty());
        has(key) || (key == "selection" && has("selection_background"))
    };
    let missing = SEMANTIC_KEYS
        .into_iter()
        .filter(|&key| !present(key))
        .collect();
    let (maps_equal, differing_keys, palettes_equal, script_stats) = match &script_result {
        Some((resolved, stats)) => {
            let differing = resolved
                .keys()
                .chain(builtin.keys())
                .filter(|key| resolved.get(*key) != builtin.get(*key))
                .cloned()
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let palettes = Palette::from_resolved(resolved) == Palette::from_resolved(&builtin);
            (
                Some(differing.is_empty()),
                differing,
                Some(palettes),
                Some(*stats),
            )
        }
        None => (None, Vec::new(), None, None),
    };
    Ok(Agreement {
        script: script_stats,
        builtin: Stats::of(&builtin_times),
        maps_equal,
        differing_keys,
        palettes_equal,
        missing,
    })
}

struct Frame {
    bytes: glib::Bytes,
    stride: usize,
    width: i32,
    height: i32,
}

impl Frame {
    fn pixel(&self, point: graphene::Point) -> Option<Rgb> {
        let (x, y) = (point.x().floor() as i32, point.y().floor() as i32);
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return None;
        }
        let offset = y as usize * self.stride + x as usize * 4;
        let pixel = self.bytes.get(offset..offset + 4)?;
        Some(Rgb {
            r: pixel[0],
            g: pixel[1],
            b: pixel[2],
        })
    }
}

fn to_rgb(rgba: &gdk::RGBA) -> Rgb {
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgb {
        r: channel(rgba.red()),
        g: channel(rgba.green()),
        b: channel(rgba.blue()),
    }
}

fn close(a: Rgb, b: Rgb) -> bool {
    let diff = |x: u8, y: u8| (i16::from(x) - i16::from(y)).abs();
    diff(a.r, b.r) <= TOLERANCE && diff(a.g, b.g) <= TOLERANCE && diff(a.b, b.b) <= TOLERANCE
}

struct Applied {
    theme: EditorTheme,
    scheme: sourceview5::StyleScheme,
    resolve: Duration,
    generate: Duration,
    scheme_load: Duration,
    css_load: Duration,
    apply: Duration,
    css_errors: Vec<String>,
}

struct Ui {
    window: adw::Window,
    view: sourceview5::View,
    buffer: sourceview5::Buffer,
    headerbar: adw::HeaderBar,
    spacer: gtk::Box,
    card: gtk::Box,
    accent: gtk::Button,
    status: gtk::Label,
    renderer: gsk::Renderer,
    paintable: gtk::WidgetPaintable,
    manager: sourceview5::StyleSchemeManager,
    styles_dir: PathBuf,
    script: Option<PathBuf>,
    provider: RefCell<Option<gtk::CssProvider>>,
    scheme_file: RefCell<Option<PathBuf>>,
    frame_work: Rc<RefCell<FrameWork>>,
    computed_style_at_once: Cell<usize>,
}

/// Time GTK spends inside one frame, from before-paint to after-paint. Unlike the wall-clock
/// wait for a frame, it excludes Broadway's throttling while no browser is connected.
#[derive(Default)]
struct FrameWork {
    started: Option<Instant>,
    last: Option<Duration>,
}

impl Ui {
    fn new(styles_dir: PathBuf, script: Option<PathBuf>) -> Result<Rc<Self>> {
        let buffer = sourceview5::Buffer::new(None);
        buffer.set_language(
            sourceview5::LanguageManager::default()
                .language("rust")
                .as_ref(),
        );
        buffer.set_highlight_syntax(true);
        buffer.set_text(SOURCE);
        buffer.place_cursor(&buffer.start_iter());
        let view = sourceview5::View::with_buffer(&buffer);
        view.set_monospace(true);
        view.set_show_line_numbers(true);
        view.set_highlight_current_line(true);
        let scroller = gtk::ScrolledWindow::builder()
            .child(&view)
            .vexpand(true)
            .build();

        let status = gtk::Label::new(Some("Ln 1, Col 1"));
        let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        spacer.set_size_request(-1, 24);
        let card = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        card.add_css_class("card");
        card.set_size_request(60, 24);
        let accent = gtk::Button::new();
        accent.add_css_class("suggested-action");
        accent.set_size_request(60, 24);
        let statusbar = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .margin_start(6)
            .margin_end(6)
            .margin_top(6)
            .margin_bottom(6)
            .build();
        statusbar.append(&status);
        statusbar.append(&spacer);
        statusbar.append(&card);
        statusbar.append(&accent);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&scroller);
        content.append(&statusbar);
        let headerbar = adw::HeaderBar::new();
        headerbar.set_show_title(false);
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&headerbar);
        toolbar.set_top_bar_style(adw::ToolbarStyle::Raised);
        toolbar.set_content(Some(&content));
        let window = adw::Window::builder()
            .default_width(900)
            .default_height(600)
            .content(&toolbar)
            .build();
        window.present();
        run_main_loop_until(FRAME_TIMEOUT, || window.is_mapped()).context("window never mapped")?;
        wait_frames(&window, 2, FRAME_TIMEOUT)?;
        view.grab_focus();
        let frame_work = Rc::new(RefCell::new(FrameWork::default()));
        let clock = window.frame_clock().context("no frame clock")?;
        clock.connect_before_paint(glib::clone!(
            #[strong]
            frame_work,
            move |_| frame_work.borrow_mut().started = Some(Instant::now())
        ));
        clock.connect_after_paint(glib::clone!(
            #[strong]
            frame_work,
            move |_| {
                let mut work = frame_work.borrow_mut();
                if let Some(started) = work.started.take() {
                    work.last = Some(started.elapsed());
                }
            }
        ));

        let renderer: gsk::Renderer = gsk::CairoRenderer::new().upcast();
        renderer.realize_for_display(&WidgetExt::display(&window))?;
        let paintable = gtk::WidgetPaintable::new(Some(&window));
        let manager = sourceview5::StyleSchemeManager::default();
        manager.append_search_path(&styles_dir.to_string_lossy());
        Ok(Rc::new(Self {
            window,
            view,
            buffer,
            headerbar,
            spacer,
            card,
            accent,
            status,
            renderer,
            paintable,
            manager,
            styles_dir,
            script,
            provider: RefCell::new(None),
            scheme_file: RefCell::new(None),
            frame_work,
            computed_style_at_once: Cell::new(0),
        }))
    }

    /// Resolve, generate, load and apply one theme, as the app would on a reload. Does not wait
    /// for a frame.
    fn apply_theme(&self, colors: &Path, name: &str) -> Result<Applied> {
        let started = Instant::now();
        let resolved = resolve_palette_with(colors, self.script.as_deref())?;
        let resolve = started.elapsed();

        let theme = EditorTheme::new(&Palette::from_resolved(&resolved));
        let id = theme.scheme_id();
        let xml = theme.style_scheme_xml(&id, &format!("Stet ({name})"));
        let css = theme.adwaita_css();
        let generate = started.elapsed();

        let path = self.styles_dir.join(format!("{id}.xml"));
        std::fs::write(&path, xml)?;
        if let Some(previous) = self.scheme_file.replace(Some(path.clone()))
            && previous != path
        {
            std::fs::remove_file(previous)?;
        }
        self.manager.force_rescan();
        let scheme = self
            .manager
            .scheme(&id)
            .with_context(|| format!("scheme {id} did not load for {name}"))?;
        let scheme_load = started.elapsed();

        let provider = gtk::CssProvider::new();
        let css_errors = Rc::new(RefCell::new(Vec::new()));
        provider.connect_parsing_error(glib::clone!(
            #[strong]
            css_errors,
            move |_, section, error| {
                css_errors
                    .borrow_mut()
                    .push(format!("{}: {}", section.to_str(), error.message()));
            }
        ));
        provider.load_from_string(&css);
        let css_load = started.elapsed();

        self.buffer.set_style_scheme(Some(&scheme));
        let display = WidgetExt::display(&self.window);
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        if let Some(old) = self.provider.replace(Some(provider)) {
            gtk::style_context_remove_provider_for_display(&display, &old);
        }
        adw::StyleManager::default().set_color_scheme(if theme.is_dark() {
            adw::ColorScheme::ForceDark
        } else {
            adw::ColorScheme::ForceLight
        });
        let apply = started.elapsed();
        let css_errors = css_errors.borrow().clone();
        Ok(Applied {
            theme,
            scheme,
            resolve,
            generate: generate - resolve,
            scheme_load: scheme_load - generate,
            css_load: css_load - scheme_load,
            apply: apply - css_load,
            css_errors,
        })
    }

    fn render(&self) -> Result<Frame> {
        let (width, height) = (self.window.width(), self.window.height());
        let snapshot = gtk::Snapshot::new();
        self.paintable
            .snapshot(&snapshot, f64::from(width), f64::from(height));
        let node = snapshot.to_node().context("the window drew nothing")?;
        let viewport = graphene::Rect::new(0.0, 0.0, width as f32, height as f32);
        let texture = self.renderer.render_texture(&node, Some(&viewport));
        let mut downloader = gdk::TextureDownloader::new(&texture);
        downloader.set_format(gdk::MemoryFormat::R8g8b8a8);
        let (bytes, stride) = downloader.download_bytes();
        Ok(Frame {
            bytes,
            stride,
            width: texture.width(),
            height: texture.height(),
        })
    }

    fn point(&self, widget: &impl IsA<gtk::Widget>, x: f32, y: f32) -> Option<graphene::Point> {
        widget.compute_point(&self.window, &graphene::Point::new(x, y))
    }

    fn center(&self, widget: &impl IsA<gtk::Widget>) -> Option<graphene::Point> {
        let widget = widget.as_ref();
        self.point(
            widget,
            widget.width() as f32 / 2.0,
            widget.height() as f32 / 2.0,
        )
    }

    /// Pixel samples and their expected colours.
    fn samples(&self, theme: &EditorTheme) -> Result<Vec<(&'static str, Option<Rgb>, Rgb)>> {
        let palette = &theme.palette;
        let frame = self.render()?;
        let (line_y, line_height) = self.view.line_yrange(&self.buffer.start_iter());
        let (_, first_line) = self.view.buffer_to_window_coords(
            gtk::TextWindowType::Widget,
            0,
            line_y + line_height / 2,
        );
        let first_line = first_line as f32;
        let (view_width, view_height) = (self.view.width() as f32, self.view.height() as f32);
        let gutter_left = gtk::prelude::TextViewExt::gutter(&self.view, gtk::TextWindowType::Left)
            .and_then(|gutter| gutter.compute_point(&self.view, &graphene::Point::zero()))
            .map_or(0.0, |point| point.x());
        let sample = |point: Option<graphene::Point>| point.and_then(|point| frame.pixel(point));
        Ok(vec![
            (
                "header bar (--headerbar-bg-color)",
                sample(self.center(&self.headerbar)),
                theme.headerbar,
            ),
            (
                "window (--window-bg-color)",
                sample(self.center(&self.spacer)),
                palette.background,
            ),
            (
                "card (--card-bg-color)",
                sample(self.center(&self.card)),
                theme.card,
            ),
            (
                "suggested button (--accent-bg-color)",
                sample(self.center(&self.accent)),
                palette.accent,
            ),
            (
                "label colour (--window-fg-color)",
                Some(to_rgb(&self.status.color())),
                palette.foreground,
            ),
            (
                "text area (scheme text)",
                sample(self.point(&self.view, view_width - 30.0, view_height - 20.0)),
                palette.background,
            ),
            (
                "current line (scheme current-line)",
                sample(self.point(&self.view, view_width - 30.0, first_line)),
                theme.current_line,
            ),
            (
                "gutter (scheme line-numbers)",
                sample(self.point(&self.view, gutter_left + 2.0, view_height - 20.0)),
                palette.background,
            ),
            (
                "current line number (scheme current-line-number)",
                sample(self.point(&self.view, gutter_left + 2.0, first_line)),
                theme.current_line,
            ),
        ])
    }

    /// Checks the scheme, the highlighting tags and the painted pixels. Call it after at least
    /// one frame has been painted since `apply_theme`.
    fn verify(&self, applied: &Applied) -> Result<Verification> {
        let styles = check_styles(&applied.scheme, &applied.theme);
        let tags = check_tags(&self.buffer, &applied.theme.palette);
        let (pixels, settle) = self.settle(&applied.theme)?;
        let scheme_in_use = self
            .buffer
            .style_scheme()
            .is_some_and(|scheme| scheme.id() == applied.scheme.id());
        Ok(Verification {
            styles,
            tags,
            pixels,
            settle,
            scheme_in_use,
        })
    }

    /// Samples until every pixel matches: right away, after idle callbacks, then after up to
    /// `SETTLE_FRAMES` more frames. Returns the last mismatches and when the match happened.
    fn settle(&self, theme: &EditorTheme) -> Result<(Vec<String>, Settle)> {
        let mut settle = Settle::Immediate;
        loop {
            let samples = self.samples(theme)?;
            if settle == Settle::Immediate {
                let computed = samples
                    .iter()
                    .find(|(what, _, _)| what.starts_with("label colour"))
                    .is_some_and(|(_, got, want)| got.is_some_and(|got| close(got, *want)));
                self.computed_style_at_once
                    .set(self.computed_style_at_once.get() + usize::from(computed));
            }
            let failures: Vec<String> = samples
                .iter()
                .filter(|(_, got, want)| !got.is_some_and(|got| close(got, *want)))
                .map(|(what, got, want)| {
                    let got = got.map_or_else(|| "nothing".to_owned(), |got| got.to_string());
                    format!("{what}: got {got}, want {want}")
                })
                .collect();
            settle = match settle {
                _ if failures.is_empty() => return Ok((failures, settle)),
                Settle::Immediate => {
                    let _ = run_main_loop_until(IDLE_WAIT, || false);
                    Settle::AfterIdle
                }
                Settle::AfterIdle => Settle::Frames(1),
                Settle::Frames(frames) if frames >= SETTLE_FRAMES => {
                    return Ok((failures, Settle::Never));
                }
                Settle::Frames(frames) => Settle::Frames(frames + 1),
                Settle::Never => return Ok((failures, settle)),
            };
            if let Settle::Frames(_) = settle {
                wait_frames(&self.window, 1, FRAME_TIMEOUT)?;
            }
        }
    }
}

/// When the rendered window first showed the new theme, after the frame that followed `apply`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Settle {
    Immediate,
    AfterIdle,
    Frames(u32),
    Never,
}

impl Settle {
    fn label(self) -> String {
        match self {
            Self::Immediate => "at once".to_owned(),
            Self::AfterIdle => "after idle".to_owned(),
            Self::Frames(frames) => format!("+{frames} frame(s)"),
            Self::Never => "never".to_owned(),
        }
    }
}

struct Verification {
    styles: Vec<String>,
    tags: Vec<String>,
    pixels: Vec<String>,
    settle: Settle,
    scheme_in_use: bool,
}

fn check_color(
    what: &str,
    attribute: &str,
    expected: Option<Rgb>,
    set: bool,
    actual: Option<glib::GString>,
    failures: &mut Vec<String>,
) {
    let Some(expected) = expected else {
        return;
    };
    let parsed = actual
        .as_deref()
        .and_then(|value| gdk::RGBA::parse(value).ok())
        .map(|rgba| to_rgb(&rgba));
    if !set || parsed != Some(expected) {
        failures.push(format!(
            "{what} {attribute}: got {actual:?} (set: {set}), want {expected}"
        ));
    }
}

fn check_styles(scheme: &sourceview5::StyleScheme, theme: &EditorTheme) -> Vec<String> {
    let mut failures = Vec::new();
    for expected in theme.scheme_styles() {
        let name = expected.name;
        let Some(style) = scheme.style(name) else {
            failures.push(format!("{name}: missing"));
            continue;
        };
        check_color(
            name,
            "foreground",
            expected.foreground,
            style.is_foreground_set(),
            style.foreground(),
            &mut failures,
        );
        check_color(
            name,
            "background",
            expected.background,
            style.is_background_set(),
            style.background(),
            &mut failures,
        );
        check_color(
            name,
            "underline-color",
            expected.underline_color,
            style.is_underline_color_set(),
            style.underline_color(),
            &mut failures,
        );
        let flags = [
            (
                "bold",
                expected.bold,
                style.is_bold_set() && style.is_bold(),
            ),
            (
                "italic",
                expected.italic,
                style.is_italic_set() && style.is_italic(),
            ),
            (
                "strikethrough",
                expected.strikethrough,
                style.is_strikethrough_set() && style.is_strikethrough(),
            ),
            (
                "underline",
                expected.underline.is_some(),
                style.is_underline_set(),
            ),
        ];
        for (flag, want, got) in flags {
            if want && !got {
                failures.push(format!("{name}: {flag} not set"));
            }
        }
    }
    failures
}

fn check_tags(buffer: &sourceview5::Buffer, palette: &Palette) -> Vec<String> {
    buffer.ensure_highlight(&buffer.start_iter(), &buffer.end_iter());
    let mut failures = Vec::new();
    for (needle, what, want, italic) in [
        ("fn", "keyword", palette.magenta, false),
        ("i32", "type", palette.yellow, false),
        ("42", "number", palette.orange, false),
        ("note", "comment", palette.muted, true),
        ("hi", "string", palette.green, false),
    ] {
        let offset = SOURCE.find(needle).map_or(0, |offset| offset as i32);
        let iter = buffer.iter_at_offset(offset);
        let mut color = None;
        let mut is_italic = false;
        for tag in iter.tags() {
            if tag.is_foreground_set() {
                color = tag.foreground_rgba().map(|rgba| to_rgb(&rgba));
            }
            if tag.is_style_set() && tag.style() == pango::Style::Italic {
                is_italic = true;
            }
        }
        if color != Some(want) || (italic && !is_italic) {
            let got = color.map_or_else(|| "no colour".to_owned(), |color| color.to_string());
            failures.push(format!(
                "{what} `{needle}`: got {got} italic={is_italic}, want {want} italic={italic}"
            ));
        }
    }
    failures
}

struct ThemeRuns {
    agreement: Agreement,
    theme: Option<EditorTheme>,
    mode: &'static str,
    totals: Vec<Duration>,
    loaded: usize,
    styles: Vec<String>,
    tags: Vec<String>,
    pixels: Vec<String>,
    pixel_rounds_ok: usize,
    settles: Vec<Settle>,
    scheme_in_use: usize,
    css_errors: Vec<String>,
    warnings: Vec<String>,
}

impl ThemeRuns {
    fn passed(&self) -> bool {
        self.loaded == ROUNDS
            && self.scheme_in_use == ROUNDS
            && self.styles.is_empty()
            && self.tags.is_empty()
            && self.pixel_rounds_ok == ROUNDS
            && self.css_errors.is_empty()
            && self.warnings.is_empty()
    }
}

#[derive(Default)]
struct Phases {
    resolve: Vec<Duration>,
    generate: Vec<Duration>,
    scheme_load: Vec<Duration>,
    css_load: Vec<Duration>,
    apply: Vec<Duration>,
    apply_mode_switch: Vec<Duration>,
    first_frame: Vec<Duration>,
    frame_work: Vec<Duration>,
    frame_work_mode_switch: Vec<Duration>,
    switch: Vec<Duration>,
    total: Vec<Duration>,
}

fn run_themes(ui: &Ui, themes: &[ThemeDir], report: &Report) -> Result<(Vec<ThemeRuns>, Phases)> {
    let script = ui.script.as_deref();
    let mut runs = Vec::new();
    for theme in themes {
        runs.push(ThemeRuns {
            agreement: compare_resolvers(theme, script)?,
            theme: None,
            mode: "",
            totals: Vec::new(),
            loaded: 0,
            styles: Vec::new(),
            tags: Vec::new(),
            pixels: Vec::new(),
            pixel_rounds_ok: 0,
            settles: Vec::new(),
            scheme_in_use: 0,
            css_errors: Vec::new(),
            warnings: Vec::new(),
        });
    }
    let mut phases = Phases::default();
    let mut dark = None;
    for round in 0..ROUNDS {
        for (theme, runs) in themes.iter().zip(&mut runs) {
            let before = glib_warning_count();
            ui.frame_work.borrow_mut().last = None;
            let started = Instant::now();
            let applied = match ui.apply_theme(&theme.colors, &theme.name) {
                Ok(applied) => applied,
                Err(error) => {
                    runs.styles.push(format!("round {round}: {error:#}"));
                    continue;
                }
            };
            let painted = wait_frames(&ui.window, 1, FRAME_TIMEOUT);
            let total = started.elapsed();
            let first_frame = painted?;
            let frame_work = ui
                .frame_work
                .borrow()
                .last
                .context("no frame work recorded")?;
            let mode_switch = dark.is_some_and(|dark| dark != applied.theme.is_dark());
            dark = Some(applied.theme.is_dark());
            runs.loaded += 1;
            let switch = applied.resolve
                + applied.generate
                + applied.scheme_load
                + applied.css_load
                + applied.apply
                + frame_work;
            runs.totals.push(switch);
            phases.switch.push(switch);
            runs.mode = if applied.theme.is_dark() {
                "dark"
            } else {
                "light"
            };
            runs.theme = Some(applied.theme.clone());
            phases.resolve.push(applied.resolve);
            phases.generate.push(applied.generate);
            phases.scheme_load.push(applied.scheme_load);
            phases.css_load.push(applied.css_load);
            if mode_switch {
                phases.apply_mode_switch.push(applied.apply);
                phases.frame_work_mode_switch.push(frame_work);
            } else {
                phases.apply.push(applied.apply);
                phases.frame_work.push(frame_work);
            }
            phases.first_frame.push(first_frame);
            phases.total.push(total);

            let verification = ui.verify(&applied)?;
            runs.scheme_in_use += usize::from(verification.scheme_in_use);
            runs.settles.push(verification.settle);
            if verification.pixels.is_empty() {
                runs.pixel_rounds_ok += 1;
            }
            let tagged = |round: usize, items: Vec<String>| {
                items
                    .into_iter()
                    .map(move |item| format!("round {round}: {item}"))
            };
            runs.styles.extend(tagged(round, verification.styles));
            runs.tags.extend(tagged(round, verification.tags));
            runs.pixels.extend(tagged(round, verification.pixels));
            runs.css_errors.extend(applied.css_errors);
            runs.warnings.extend(glib_warnings_since(before));
            report.record(
                &format!("apply/{}/round{round}", theme.name),
                serde_json::json!({
                    "switch_ms": switch.as_secs_f64() * 1000.0,
                    "wall_ms": total.as_secs_f64() * 1000.0,
                    "pixels_ok": runs.pixel_rounds_ok,
                }),
            );
        }
    }
    Ok((runs, phases))
}

struct Stale {
    first: Option<Rgb>,
    second: Option<Rgb>,
    same_object: bool,
    old_object_after: Option<Rgb>,
}

/// Rewrites a scheme file under the same id and rescans, to see whether the manager serves
/// the new colours or a cached scheme.
fn stale_id_experiment(ui: &Ui, a: &Path, b: &Path) -> Result<Stale> {
    let text_background = |scheme: &sourceview5::StyleScheme| {
        scheme
            .style("text")
            .and_then(|style| style.background())
            .and_then(|value| gdk::RGBA::parse(value.as_str()).ok())
            .map(|rgba| to_rgb(&rgba))
    };
    let id = "stet-s3-stale";
    let path = ui.styles_dir.join(format!("{id}.xml"));
    let theme = |colors: &Path| -> Result<EditorTheme> {
        Ok(EditorTheme::new(&Palette::from_resolved(
            &resolve_palette_with(colors, ui.script.as_deref())?,
        )))
    };
    std::fs::write(&path, theme(a)?.style_scheme_xml(id, "stale"))?;
    ui.manager.force_rescan();
    let first = ui.manager.scheme(id).context("stale scheme missing")?;
    std::fs::write(&path, theme(b)?.style_scheme_xml(id, "stale"))?;
    ui.manager.force_rescan();
    let second = ui.manager.scheme(id).context("stale scheme missing")?;
    std::fs::remove_file(&path)?;
    ui.manager.force_rescan();
    Ok(Stale {
        first: text_background(&first),
        second: text_background(&second),
        same_object: first == second,
        old_object_after: text_background(&first),
    })
}

/// Loads legacy `@define-color` CSS into a detached provider and returns what GTK reports.
fn define_color_experiment() -> Vec<String> {
    let provider = gtk::CssProvider::new();
    let messages = Rc::new(RefCell::new(Vec::new()));
    provider.connect_parsing_error(glib::clone!(
        #[strong]
        messages,
        move |_, section, error| {
            messages
                .borrow_mut()
                .push(format!("{}: {}", section.to_str(), error.message()));
        }
    ));
    provider.load_from_string(
        "@define-color window_bg_color #101010;\nwindow { background-color: @window_bg_color; }\n",
    );
    messages.take()
}

struct EventRecord {
    at: Duration,
    kind: &'static str,
    file: String,
    other: Option<String>,
    trigger: Option<&'static str>,
}

struct ReloadRecord {
    at: Duration,
    since_trigger: Duration,
    theme: String,
    colors_present: bool,
    applied: Result<EditorTheme, String>,
}

struct MonitorLog {
    clock: Instant,
    events: Vec<EventRecord>,
    reloads: Vec<ReloadRecord>,
    pending: Option<glib::SourceId>,
    last_trigger: Option<Instant>,
}

fn event_name(event: gio::FileMonitorEvent) -> &'static str {
    match event {
        gio::FileMonitorEvent::Changed => "CHANGED",
        gio::FileMonitorEvent::ChangesDoneHint => "CHANGES_DONE_HINT",
        gio::FileMonitorEvent::Deleted => "DELETED",
        gio::FileMonitorEvent::Created => "CREATED",
        gio::FileMonitorEvent::AttributeChanged => "ATTRIBUTE_CHANGED",
        gio::FileMonitorEvent::PreUnmount => "PRE_UNMOUNT",
        gio::FileMonitorEvent::Unmounted => "UNMOUNTED",
        gio::FileMonitorEvent::Moved => "MOVED",
        gio::FileMonitorEvent::Renamed => "RENAMED",
        gio::FileMonitorEvent::MovedIn => "MOVED_IN",
        gio::FileMonitorEvent::MovedOut => "MOVED_OUT",
        _ => "UNKNOWN",
    }
}

/// The plan's rule (`theme.name` CHANGES_DONE_HINT, or a rename whose new name is `theme`),
/// plus the watched `current/` itself appearing, which GLib reports after polling for it.
fn trigger_rule(
    event: gio::FileMonitorEvent,
    file: &str,
    other: Option<&str>,
) -> Option<&'static str> {
    match event {
        gio::FileMonitorEvent::ChangesDoneHint if file == "theme.name" => Some("plan: theme.name"),
        gio::FileMonitorEvent::Renamed if other == Some("theme") => Some("plan: renamed to theme"),
        gio::FileMonitorEvent::Created if file == "current" => Some("extra: current/ appeared"),
        _ => None,
    }
}

fn base_name(file: &gio::File) -> String {
    file.basename()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn reload(ui: Option<&Ui>, current: &Path, log: &RefCell<MonitorLog>) {
    let now = Instant::now();
    let since_trigger = log
        .borrow()
        .last_trigger
        .map_or(Duration::ZERO, |trigger| now - trigger);
    let theme = std::fs::read_to_string(current.join("theme.name"))
        .map(|name| name.trim().to_owned())
        .unwrap_or_else(|error| format!("<{error}>"));
    let colors = current.join("theme/colors.toml");
    let applied = match ui {
        Some(ui) => ui
            .apply_theme(&colors, &theme)
            .map(|applied| applied.theme)
            .map_err(|error| format!("{error:#}")),
        None => resolve_palette_with(&colors, None)
            .map(|resolved| EditorTheme::new(&Palette::from_resolved(&resolved)))
            .map_err(|error| error.to_string()),
    };
    let mut log = log.borrow_mut();
    let at = now - log.clock;
    log.reloads.push(ReloadRecord {
        at,
        since_trigger,
        theme,
        colors_present: colors.is_file(),
        applied,
    });
}

fn watch(
    current: &Path,
    ui: Option<Rc<Ui>>,
) -> Result<(gio::FileMonitor, Rc<RefCell<MonitorLog>>)> {
    let monitor = gio::File::for_path(current).monitor_directory(
        gio::FileMonitorFlags::WATCH_MOVES,
        None::<&gio::Cancellable>,
    )?;
    let log = Rc::new(RefCell::new(MonitorLog {
        clock: Instant::now(),
        events: Vec::new(),
        reloads: Vec::new(),
        pending: None,
        last_trigger: None,
    }));
    let current = current.to_path_buf();
    monitor.connect_changed(glib::clone!(
        #[strong]
        log,
        move |_, file, other, event| {
            let name = base_name(file);
            let other_name = other.map(base_name);
            let trigger = trigger_rule(event, &name, other_name.as_deref());
            let mut state = log.borrow_mut();
            let at = state.clock.elapsed();
            state.events.push(EventRecord {
                at,
                kind: event_name(event),
                file: name,
                other: other_name,
                trigger,
            });
            if trigger.is_none() {
                return;
            }
            state.last_trigger = Some(Instant::now());
            if let Some(pending) = state.pending.take() {
                pending.remove();
            }
            let source = glib::timeout_add_local_once(
                DEBOUNCE,
                glib::clone!(
                    #[strong]
                    log,
                    #[strong]
                    ui,
                    #[strong]
                    current,
                    move || {
                        log.borrow_mut().pending = None;
                        reload(ui.as_deref(), &current, &log);
                    }
                ),
            );
            state.pending = Some(source);
        }
    ));
    Ok((monitor, log))
}

fn spawn_swap(
    current: &Path,
    skip_background: bool,
    fresh: bool,
    swaps: &[&ThemeDir],
) -> Result<Child> {
    let mut command = Command::new("bash");
    command
        .arg("-c")
        .arg(SWAP_SCRIPT)
        .arg("s3-swap")
        .arg(current)
        .arg(if skip_background { "1" } else { "0" })
        .arg(if fresh { "1" } else { "0" });
    for theme in swaps {
        command.arg(&theme.dir).arg(&theme.name);
    }
    Ok(command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()?)
}

struct Scenario {
    name: String,
    swaps: Vec<String>,
    shell: Duration,
    events: Vec<EventRecord>,
    reloads: Vec<ReloadRecord>,
    expected_reloads: &'static str,
    passed: bool,
    ui_shows: Option<bool>,
}

/// Runs one swap process while the main loop dispatches monitor events, then waits `settle`
/// after it exits so the debounce can fire.
fn run_scenario(
    log: &Rc<RefCell<MonitorLog>>,
    ui: Option<&Ui>,
    name: &str,
    current: &Path,
    (skip_background, fresh): (bool, bool),
    swaps: &[&ThemeDir],
    settle: Duration,
) -> Result<Scenario> {
    let (events_from, reloads_from) = {
        let log = log.borrow();
        (log.events.len(), log.reloads.len())
    };
    let started = Instant::now();
    let child = RefCell::new(spawn_swap(current, skip_background, fresh, swaps)?);
    let exited: Cell<Option<Instant>> = Cell::new(None);
    let failed = Cell::new(false);
    run_main_loop_until(SCENARIO_TIMEOUT, || {
        if exited.get().is_none()
            && let Ok(Some(status)) = child.borrow_mut().try_wait()
        {
            exited.set(Some(Instant::now()));
            failed.set(!status.success());
        }
        exited
            .get()
            .is_some_and(|at| at.elapsed() >= settle && log.borrow().pending.is_none())
    })
    .with_context(|| format!("scenario {name}"))?;
    ensure!(!failed.get(), "swap script failed in scenario {name}");
    let shell = exited.get().map_or(Duration::ZERO, |at| at - started);
    let offset = started - log.borrow().clock;
    let mut log = log.borrow_mut();
    let events = log
        .events
        .drain(events_from..)
        .map(|event| EventRecord {
            at: event.at.saturating_sub(offset),
            ..event
        })
        .collect();
    let reloads = log
        .reloads
        .drain(reloads_from..)
        .map(|reload| ReloadRecord {
            at: reload.at.saturating_sub(offset),
            ..reload
        })
        .collect();
    drop(log);
    let ui_shows = match ui {
        Some(ui) => Some(shows_theme(ui, swaps.last().copied())?),
        None => None,
    };
    Ok(Scenario {
        name: name.to_owned(),
        swaps: swaps.iter().map(|theme| theme.name.clone()).collect(),
        shell,
        events,
        reloads,
        expected_reloads: "",
        passed: false,
        ui_shows,
    })
}

/// Whether the window now paints the text background of `theme`.
fn shows_theme(ui: &Ui, theme: Option<&ThemeDir>) -> Result<bool> {
    let Some(theme) = theme else {
        return Ok(false);
    };
    wait_frames(&ui.window, 1, FRAME_TIMEOUT)?;
    let expected = EditorTheme::new(&Palette::from_resolved(&resolve_palette_with(
        &theme.colors,
        ui.script.as_deref(),
    )?));
    let (failures, _) = ui.settle(&expected)?;
    Ok(failures.is_empty())
}

fn find<'a>(themes: &'a [ThemeDir], name: &str) -> Result<&'a ThemeDir> {
    themes
        .iter()
        .find(|theme| theme.builtin && theme.name == name)
        .with_context(|| format!("built-in theme {name} not installed"))
}

fn judge(scenario: &mut Scenario, expected: &'static str, exact: Option<usize>) {
    let last = scenario.swaps.last().cloned().unwrap_or_default();
    let reloads_ok = match exact {
        Some(count) => scenario.reloads.len() == count,
        None => !scenario.reloads.is_empty(),
    };
    let final_ok = scenario.reloads.last().is_some_and(|reload| {
        reload.theme == last && reload.colors_present && reload.applied.is_ok()
    });
    scenario.expected_reloads = expected;
    scenario.passed = reloads_ok && final_ok && scenario.ui_shows != Some(false);
}

struct Reloads {
    scenarios: Vec<Scenario>,
    missing: Vec<Scenario>,
    missing_error: Option<String>,
}

fn reload_mechanics(ui: &Rc<Ui>, work: &Path, themes: &[ThemeDir]) -> Result<Reloads> {
    let current = work.join("state/omarchy/current");
    let initial = find(themes, "tokyo-night")?;
    let mut setup = spawn_swap(&current, false, false, &[initial])?;
    ensure!(setup.wait()?.success(), "initial theme setup failed");

    let (monitor, log) = watch(&current, Some(ui.clone()))?;
    let _ = run_main_loop_until(Duration::from_millis(300), || false);
    log.borrow_mut().events.clear();

    let mut scenarios = Vec::new();
    for (index, name) in ["catppuccin-latte", "gruvbox", "flexoki-light", "catppuccin"]
        .into_iter()
        .enumerate()
    {
        let theme = find(themes, name)?;
        let mut scenario = run_scenario(
            &log,
            Some(ui),
            &format!("swap {} → {name}", index + 1),
            &current,
            (false, false),
            &[theme],
            SETTLE,
        )?;
        judge(&mut scenario, "exactly 1", Some(1));
        scenarios.push(scenario);
    }
    let catppuccin = find(themes, "catppuccin")?;
    let mut refresh = run_scenario(
        &log,
        Some(ui),
        "refresh (same theme, no background)",
        &current,
        (true, false),
        &[catppuccin],
        SETTLE,
    )?;
    judge(&mut refresh, "exactly 1", Some(1));
    scenarios.push(refresh);

    let burst: Vec<&ThemeDir> = ["rose-pine", "white", "nord"]
        .into_iter()
        .map(|name| find(themes, name))
        .collect::<Result<_>>()?;
    let mut rapid = run_scenario(
        &log,
        Some(ui),
        "burst of 3 swaps in one process",
        &current,
        (false, false),
        &burst,
        SETTLE,
    )?;
    judge(&mut rapid, "at least 1, last = nord", None);
    scenarios.push(rapid);

    let kanagawa = find(themes, "kanagawa")?;
    let mut fresh = run_scenario(
        &log,
        Some(ui),
        "first write of theme.name",
        &current,
        (false, true),
        &[kanagawa],
        SETTLE,
    )?;
    judge(&mut fresh, "exactly 1", Some(1));
    scenarios.push(fresh);
    monitor.cancel();

    let missing_current = work.join("state-missing/omarchy/current");
    std::fs::create_dir_all(missing_current.parent().context("no parent")?)?;
    let mut missing = Vec::new();
    let (missing_monitor, missing_log) = match watch(&missing_current, None) {
        Ok(watch) => watch,
        Err(error) => {
            return Ok(Reloads {
                scenarios,
                missing,
                missing_error: Some(format!("{error:#}")),
            });
        }
    };
    for (index, name) in ["everforest", "osaka-jade"].into_iter().enumerate() {
        let theme = find(themes, name)?;
        let mut scenario = run_scenario(
            &missing_log,
            None,
            &format!("watch created before current/ exists, swap {}", index + 1),
            &missing_current,
            (false, false),
            &[theme],
            MISSING_DIR_WAIT,
        )?;
        judge(&mut scenario, "exactly 1 (not required)", Some(1));
        missing.push(scenario);
    }
    missing_monitor.cancel();
    Ok(Reloads {
        scenarios,
        missing,
        missing_error: None,
    })
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "**no**" }
}

fn verdict(value: bool) -> &'static str {
    if value { "PASS" } else { "**FAIL**" }
}

fn event_sequence(events: &[EventRecord]) -> String {
    let mut text = String::new();
    for event in events {
        let other = event
            .other
            .as_deref()
            .map(|other| format!(" → {other}"))
            .unwrap_or_default();
        let trigger = event
            .trigger
            .map(|rule| format!("  ← trigger ({rule})"))
            .unwrap_or_default();
        let _ = writeln!(
            text,
            "{:>8.1} ms  {:<18} {}{other}{trigger}",
            event.at.as_secs_f64() * 1000.0,
            event.kind,
            event.file
        );
    }
    text
}

fn scenario_rows(markdown: &mut String, scenarios: &[Scenario]) {
    markdown.push_str(
        "| Scenario | Swap process | Events | Triggers | Reloads (expected) | Reloaded theme | \
         Trigger → reload | UI repainted | Verdict |\n|---|---|---|---|---|---|---|---|---|\n",
    );
    for scenario in scenarios {
        let triggers = scenario
            .events
            .iter()
            .filter(|event| event.trigger.is_some())
            .count();
        let themes: Vec<String> = scenario
            .reloads
            .iter()
            .map(|reload| match &reload.applied {
                Ok(_) if reload.colors_present => reload.theme.clone(),
                Ok(_) => format!("{} (no colors.toml)", reload.theme),
                Err(error) => format!("{} ({error})", reload.theme),
            })
            .collect();
        let latency: Vec<String> = scenario
            .reloads
            .iter()
            .map(|reload| format!("{:.0} ms", reload.since_trigger.as_secs_f64() * 1000.0))
            .collect();
        let _ = writeln!(
            markdown,
            "| {} | {:.0} ms | {} | {triggers} | {} ({}) | {} | {} | {} | {} |",
            scenario.name,
            scenario.shell.as_secs_f64() * 1000.0,
            scenario.events.len(),
            scenario.reloads.len(),
            scenario.expected_reloads,
            if themes.is_empty() {
                "—".to_owned()
            } else {
                themes.join(", ")
            },
            if latency.is_empty() {
                "—".to_owned()
            } else {
                latency.join(", ")
            },
            scenario.ui_shows.map_or("n/a", yes_no),
            verdict(scenario.passed)
        );
    }
}

/// WCAG contrast of the generated roles. Informational: the plan sets no contrast bar.
fn contrast_table(md: &mut String, themes: &[ThemeDir], runs: &[ThemeRuns]) {
    md.push_str(
        "\n### Contrast of the generated roles (WCAG ratio; **bold** is below 4.5 for text, \
         3.0 for comments and syntax colours)\n\n\
         | Theme | Text | Comment | Weakest syntax colour | Selection text | Text on accent | \
         Current match | Popover text | Current line vs background |\n\
         |---|---|---|---|---|---|---|---|---|\n",
    );
    let cell = |ratio: f64, bar: f64| {
        if ratio < bar {
            format!("**{ratio:.1}**")
        } else {
            format!("{ratio:.1}")
        }
    };
    for (dir, run) in themes.iter().zip(runs) {
        let Some(theme) = &run.theme else {
            continue;
        };
        let p = &theme.palette;
        let background = p.background;
        let (weakest, ratio) = [
            ("magenta", p.magenta),
            ("blue", p.blue),
            ("yellow", p.yellow),
            ("orange", p.orange),
            ("green", p.green),
            ("cyan", p.cyan),
            ("red", p.red),
        ]
        .into_iter()
        .map(|(name, color)| (name, color.contrast(background)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or(("none", f64::NAN));
        let _ = writeln!(
            md,
            "| {} | {} | {} | {weakest} {} | {} | {} | {} | {} | {:.2} |",
            dir.name,
            cell(p.foreground.contrast(background), 4.5),
            cell(p.muted.contrast(background), 3.0),
            cell(ratio, 3.0),
            cell(p.selection_foreground.contrast(p.selection_background), 4.5),
            cell(theme.accent_text.contrast(p.accent), 4.5),
            cell(theme.search_current_text.contrast(p.yellow), 4.5),
            cell(p.foreground.contrast(theme.popover), 4.5),
            theme.current_line.contrast(background),
        );
    }
}

struct Results<'a> {
    themes: &'a [ThemeDir],
    runs: &'a [ThemeRuns],
    phases: &'a Phases,
    stale: &'a Stale,
    define_color: &'a [String],
    reloads: &'a Reloads,
    setup_warnings: &'a [String],
    computed_style_at_once: usize,
    computed_style_checks: usize,
}

fn write_results(path: &Path, results: &Results<'_>) -> Result<()> {
    let Results {
        themes,
        runs,
        phases,
        stale,
        define_color,
        reloads,
        setup_warnings,
        computed_style_at_once,
        computed_style_checks,
    } = *results;
    let mut md = String::new();
    let script = theme_color_script();
    let _ = writeln!(
        md,
        "## S3 run\n\nThemes: {} ({} built-in, {} user). Rounds per theme: {ROUNDS}. \
         Resolver script: {}.\n",
        themes.len(),
        themes.iter().filter(|theme| theme.builtin).count(),
        themes.iter().filter(|theme| !theme.builtin).count(),
        script
            .as_deref()
            .map_or("not installed".to_owned(), |path| format!(
                "`{}`",
                path.display()
            ))
    );

    md.push_str(
        "### Themes\n\n| Theme | Source | Mode | Script = built-in resolver | Keys the file lacks \
         (filled by the cascade) | Scheme loads | Styles match | Syntax tags | CSS errors | \
         Pixels match (rounds) | Pixels correct (worst round) | Switch cost: resolve → apply + \
         first-frame GTK work, ms median (min–max) | Verdict |\n\
         |---|---|---|---|---|---|---|---|---|---|---|---|---|\n",
    );
    for (theme, run) in themes.iter().zip(runs) {
        let agreement = match (run.agreement.maps_equal, run.agreement.palettes_equal) {
            (Some(true), _) => "identical".to_owned(),
            (Some(false), Some(true)) => format!(
                "same palette; keys differ: {}",
                run.agreement.differing_keys.join(", ")
            ),
            (Some(false), _) => format!("**differs**: {}", run.agreement.differing_keys.join(", ")),
            (None, _) => "script missing".to_owned(),
        };
        let missing = if run.agreement.missing.is_empty() {
            "—".to_owned()
        } else {
            run.agreement.missing.join(", ")
        };
        let _ = writeln!(
            md,
            "| {} | {} | {} | {agreement} | {missing} | {}/{ROUNDS} | {} | {} | {} | {}/{ROUNDS} | {} | {} | {} |",
            theme.name,
            if theme.builtin { "built-in" } else { "user" },
            run.mode,
            run.loaded,
            yes_no(run.styles.is_empty()),
            yes_no(run.tags.is_empty()),
            run.css_errors.len(),
            run.pixel_rounds_ok,
            run.settles
                .iter()
                .max()
                .map_or_else(|| "—".to_owned(), |settle| settle.label()),
            Stats::of(&run.totals).cell(),
            verdict(run.passed()),
        );
    }
    let failures: Vec<String> = themes
        .iter()
        .zip(runs)
        .flat_map(|(theme, run)| {
            run.styles
                .iter()
                .chain(&run.tags)
                .chain(&run.pixels)
                .chain(&run.css_errors)
                .chain(&run.warnings)
                .map(move |failure| format!("- {}: {failure}", theme.name))
        })
        .collect();
    if !failures.is_empty() {
        md.push_str("\n**Failures and warnings:**\n\n");
        for failure in failures {
            md.push_str(&failure);
            md.push('\n');
        }
    }

    contrast_table(&mut md, themes, runs);

    let script_medians: Vec<f64> = runs
        .iter()
        .filter_map(|run| run.agreement.script.map(|stats| stats.median))
        .collect();
    let builtin_medians: Vec<f64> = runs
        .iter()
        .map(|run| run.agreement.builtin.median)
        .collect();
    let median_of = |values: &[f64]| {
        let durations: Vec<Duration> = values
            .iter()
            .map(|ms| Duration::from_secs_f64(ms / 1000.0))
            .collect();
        Stats::of(&durations)
    };
    let _ = writeln!(
        md,
        "\n### Timing across all themes and rounds (n = {})\n\n| Phase | ms median (min–max) |\n|---|---|",
        phases.total.len()
    );
    for (phase, samples) in [
        (
            "resolve: `omarchy-theme-color --all` (process spawn)",
            &phases.resolve,
        ),
        ("generate: palette → tokens → XML + CSS", &phases.generate),
        (
            "scheme: write XML, delete old, force_rescan, lookup",
            &phases.scheme_load,
        ),
        ("css: CssProvider::load_from_string", &phases.css_load),
        (
            "apply: set_style_scheme, swap provider, StyleManager (same mode)",
            &phases.apply,
        ),
        (
            "apply, when the StyleManager switches dark ↔ light",
            &phases.apply_mode_switch,
        ),
        (
            "GTK work in the first frame, before-paint → after-paint (same mode)",
            &phases.frame_work,
        ),
        (
            "GTK work in the first frame, when switching dark ↔ light",
            &phases.frame_work_mode_switch,
        ),
        (
            "wall clock until the first frame (Broadway-throttled)",
            &phases.first_frame,
        ),
        (
            "switch cost: resolve → apply + first-frame GTK work",
            &phases.switch,
        ),
        (
            "wall clock including the Broadway frame wait",
            &phases.total,
        ),
    ] {
        let _ = writeln!(md, "| {phase} | {} |", Stats::of(samples).cell());
    }
    let _ = writeln!(
        md,
        "| per-theme median, resolve via script only | {} |\n| per-theme median, built-in Rust resolver only | {} |",
        median_of(&script_medians).cell(),
        median_of(&builtin_medians).cell()
    );

    let settles: Vec<Settle> = runs
        .iter()
        .flat_map(|run| run.settles.iter().copied())
        .collect();
    let mut kinds: Vec<Settle> = settles.clone();
    kinds.sort();
    kinds.dedup();
    md.push_str(
        "\n### When the rendered window showed the new theme\n\n\
         Counted from the first frame painted after `apply`; sampled through `GtkWidgetPaintable`.\n\n\
         | Pixels correct | Theme switches |\n|---|---|\n",
    );
    for kind in kinds {
        let count = settles.iter().filter(|settle| **settle == kind).count();
        let _ = writeln!(md, "| {} | {count} |", kind.label());
    }
    let _ = writeln!(
        md,
        "\nThe label's computed colour (`gtk_widget_get_color`, no rendering involved) was already \
         the new `--window-fg-color` right after the first frame in {} of {} theme switches.",
        computed_style_at_once, computed_style_checks
    );

    let colour =
        |value: Option<Rgb>| value.map_or_else(|| "none".to_owned(), |rgb| rgb.to_string());
    let _ = writeln!(
        md,
        "\n### Same scheme id, rewritten file\n\nFirst load text background {}, after rewriting \
         the file and `force_rescan` {}; same object: {}; the first object now reports {}.",
        colour(stale.first),
        colour(stale.second),
        stale.same_object,
        colour(stale.old_object_after)
    );
    let _ = writeln!(
        md,
        "\n### Legacy `@define-color`\n\nParsing messages: {}",
        if define_color.is_empty() {
            "none".to_owned()
        } else {
            define_color.join("; ")
        }
    );
    if !setup_warnings.is_empty() {
        let _ = writeln!(
            md,
            "\nGLib warnings during setup: {}",
            setup_warnings.join("; ")
        );
    }

    md.push_str("\n### Live reload trigger\n\n");
    scenario_rows(&mut md, &reloads.scenarios);
    md.push_str("\n#### Watch created before `current/` exists\n\n");
    if let Some(error) = &reloads.missing_error {
        let _ = writeln!(md, "`monitor_directory` failed: {error}");
    }
    scenario_rows(&mut md, &reloads.missing);
    md.push_str("\n### Event sequences (times from the start of the swap process)\n");
    for scenario in reloads.scenarios.iter().chain(&reloads.missing) {
        let _ = write!(
            md,
            "\n**{}** ({}):\n\n```\n{}",
            scenario.name,
            scenario.swaps.join(", "),
            event_sequence(&scenario.events)
        );
        for reload in &scenario.reloads {
            let _ = writeln!(
                md,
                "{:>8.1} ms  RELOAD             theme.name={} ({:.1} ms after the last trigger)",
                reload.at.as_secs_f64() * 1000.0,
                reload.theme,
                reload.since_trigger.as_secs_f64() * 1000.0
            );
        }
        md.push_str("```\n");
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, md)?;
    Ok(())
}

fn main() -> Result<()> {
    require_headless()?;
    capture_glib_warnings();
    adw::init()?;
    sourceview5::init();
    let report = Report::new("s3_theme");
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("no workspace root")?
        .to_path_buf();
    std::fs::create_dir_all(root.join("target"))?;
    let work = tempfile::Builder::new()
        .prefix("s3-theme-")
        .tempdir_in(root.join("target"))?;
    let styles_dir = work.path().join("styles");
    std::fs::create_dir_all(&styles_dir)?;

    let themes = discover_themes()?;
    report.record("themes", themes.len());
    let script = theme_color_script();
    let setup_before = glib_warning_count();
    let ui = Ui::new(styles_dir, script)?;
    let setup_warnings = glib_warnings_since(setup_before);

    let (runs, phases) = run_themes(&ui, &themes, &report)?;
    let computed_style_at_once = ui.computed_style_at_once.get();
    let computed_style_checks = phases.total.len();
    for (theme, run) in themes.iter().zip(&runs) {
        report.record(
            &format!("theme/{}", theme.name),
            serde_json::json!({
                "builtin": theme.builtin,
                "passed": run.passed(),
                "resolvers_identical": run.agreement.maps_equal,
                "apply": Stats::of(&run.totals).json(),
            }),
        );
    }
    report.record("phase/total", Stats::of(&phases.total).json());

    let stale = stale_id_experiment(
        &ui,
        &find(&themes, "tokyo-night")?.colors,
        &find(&themes, "catppuccin-latte")?.colors,
    )?;
    let define_color = define_color_experiment();
    let reloads = reload_mechanics(&ui, work.path(), &themes)?;
    for scenario in reloads.scenarios.iter().chain(&reloads.missing) {
        report.record(
            &format!("reload/{}", scenario.name),
            serde_json::json!({
                "events": scenario.events.len(),
                "triggers": scenario.events.iter().filter(|event| event.trigger.is_some()).count(),
                "reloads": scenario.reloads.len(),
                "passed": scenario.passed,
            }),
        );
    }

    let results = root.join(".local/s3/results.md");
    write_results(
        &results,
        &Results {
            themes: &themes,
            runs: &runs,
            phases: &phases,
            stale: &stale,
            define_color: &define_color,
            reloads: &reloads,
            setup_warnings: &setup_warnings,
            computed_style_at_once,
            computed_style_checks,
        },
    )?;
    let builtin_passed = themes
        .iter()
        .zip(&runs)
        .filter(|(theme, _)| theme.builtin)
        .all(|(_, run)| run.passed());
    let reload_passed = reloads.scenarios.iter().all(|scenario| scenario.passed);
    report.record("builtin_themes_passed", builtin_passed);
    report.record("reload_passed", reload_passed);
    println!("results: {}", results.display());

    ui.renderer.unrealize();
    ui.window.destroy();
    if !(builtin_passed && reload_passed) {
        bail!("S3 did not pass; see {}", results.display());
    }
    Ok(())
}
