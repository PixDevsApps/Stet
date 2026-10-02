//! The GApplication: single instance, the command line of every launch (a second `stet a.rs
//! b.txt:40` is handled by the running primary instance), D-Bus activation's `open` and
//! `activate`, startup and the first frame. The first window restores the session (M2) before
//! it opens a command line's files and before it is shown. A `--wait` command line is held
//! until its tabs close (ADR-011). SIGTERM, SIGHUP and SIGINT save the session and quit.

use crate::actions;
use crate::cli;
use crate::config::{AppConfig, EXIT_AFTER_ENV};
use crate::theme::Appearance;
use crate::window::{Location, Shared, Window};
use gtk4::{gio, glib};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// How long the first window waits for the theme and font before it appears anyway.
const THEME_WAIT: Duration = Duration::from_millis(500);

/// How long the first window waits for the restored active tab's text before it appears
/// anyway; a large file shows its first screen and keeps loading.
const RESTORE_WAIT: Duration = Duration::from_millis(1000);

/// The signals that end a session: uwsm stops app units with SIGTERM at logout and reboot, a
/// closing terminal sends SIGHUP, Ctrl+C SIGINT. Linux's numbers.
const QUIT_SIGNALS: [(i32, &str); 3] = [(15, "SIGTERM"), (1, "SIGHUP"), (2, "SIGINT")];

type WindowHook = Box<dyn FnOnce(Window)>;
type PresentGate = Pin<Box<dyn Future<Output = ()>>>;

pub struct AppState {
    config: AppConfig,
    shared: RefCell<Option<Rc<Shared>>>,
    window: RefCell<Option<Window>>,
    started: Cell<Option<Instant>>,
    first_frame: Rc<Cell<Option<Instant>>>,
    on_first_window: RefCell<Option<WindowHook>>,
    present_gate: RefCell<Option<PresentGate>>,
}

impl AppState {
    pub fn window(&self) -> Option<Window> {
        self.window.borrow().clone()
    }

    /// Runs `hook` once the first window has been presented (the self-test starts there).
    pub fn connect_first_window(&self, hook: impl FnOnce(Window) + 'static) {
        self.on_first_window.replace(Some(Box::new(hook)));
    }

    /// Keeps the first window hidden until `gate` resolves as well as the theme. The self-test
    /// opens its startup files first that way, as a slow theme load does in a live session.
    pub fn hold_first_window(&self, gate: impl Future<Output = ()> + 'static) {
        self.present_gate.replace(Some(Box::pin(gate)));
    }

    /// When the first window's first frame was painted.
    pub fn first_frame(&self) -> Option<Instant> {
        self.first_frame.get()
    }
}

pub fn build(config: AppConfig, started: Instant) -> (adw::Application, Rc<AppState>) {
    let mut flags =
        gio::ApplicationFlags::HANDLES_COMMAND_LINE | gio::ApplicationFlags::HANDLES_OPEN;
    if config.non_unique {
        flags |= gio::ApplicationFlags::NON_UNIQUE;
    }
    let app = adw::Application::builder()
        .application_id(config.app_id.as_str())
        .flags(flags)
        .build();
    let state = Rc::new(AppState {
        config,
        shared: RefCell::new(None),
        window: RefCell::new(None),
        started: Cell::new(Some(started)),
        first_frame: Rc::new(Cell::new(None)),
        on_first_window: RefCell::new(None),
        present_gate: RefCell::new(None),
    });

    app.connect_startup(glib::clone!(
        #[strong]
        state,
        move |app| startup(app, &state)
    ));
    app.connect_command_line(glib::clone!(
        #[strong]
        state,
        move |app, command_line| command_line_received(app, &state, command_line)
    ));
    // D-Bus activation from a launcher or a file manager (DBusActivatable=true).
    app.connect_activate(glib::clone!(
        #[strong]
        state,
        move |app| {
            let window = window_or_new(app, &state, true);
            present(&state, &window);
        }
    ));
    app.connect_open(glib::clone!(
        #[strong]
        state,
        move |app, files, _| {
            let window = window_or_new(app, &state, true);
            present(&state, &window);
            let locations: Vec<Location> = files
                .iter()
                .filter_map(|file| file.path())
                .map(|path| Location {
                    path,
                    position: None,
                    create: true,
                })
                .collect();
            glib::spawn_future_local(async move {
                window.session_ready(RESTORE_WAIT).await;
                window.open_locations(locations);
            });
        }
    ));
    (app, state)
}

pub fn run(config: AppConfig, started: Instant) -> glib::ExitCode {
    let (app, _state) = build(config, started);
    app.run()
}

fn startup(app: &adw::Application, state: &Rc<AppState>) {
    let config = &state.config;
    let appearance = Appearance::start(
        &config.omarchy_dir,
        &config.fontconfig_dir,
        config.dirs.styles_dir(),
    );
    state
        .shared
        .replace(Some(Shared::new(config.clone(), appearance)));
    actions::install_app_actions(app);
    for (signal, name) in QUIT_SIGNALS {
        glib_unix::unix_signal_add_local(
            signal,
            glib::clone!(
                #[weak]
                app,
                #[weak]
                state,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move || {
                    tracing::info!("{name}: saving the session and quitting");
                    match state.window() {
                        Some(window) => window.shutdown_for_signal(),
                        None => app.quit(),
                    }
                    glib::ControlFlow::Continue
                }
            ),
        );
    }
    if let Some(delay) = config.exit_after {
        glib::timeout_add_local_once(
            delay,
            glib::clone!(
                #[weak]
                app,
                move || {
                    tracing::info!("{EXIT_AFTER_ENV} elapsed; quitting");
                    app.quit();
                }
            ),
        );
    }
}

/// The window, created on the first request; `session` is false for `--no-session`.
fn window_or_new(app: &adw::Application, state: &Rc<AppState>, session: bool) -> Window {
    if let Some(window) = state.window() {
        return window;
    }
    let shared = state
        .shared
        .borrow()
        .clone()
        .expect("startup ran before the first window");
    let window = Window::new(app, shared);
    window.window().connect_destroy(glib::clone!(
        #[weak]
        state,
        move |_| {
            state.window.replace(None);
        }
    ));
    state.window.replace(Some(window.clone()));
    window.start_session(session);
    window
}

/// Shows the window; the first time, only once the theme and font are applied and the
/// session is restored (or after a short wait), so it never flashes in default colours or
/// without its tabs.
fn present(state: &Rc<AppState>, window: &Window) {
    if window.window().is_visible() {
        window.present();
        return;
    }
    let state = state.clone();
    let window = window.clone();
    glib::spawn_future_local(async move {
        window.shared().appearance.ready(THEME_WAIT).await;
        window.shared().preferences.ready(THEME_WAIT).await;
        window.session_ready(RESTORE_WAIT).await;
        if let Some(gate) = state.present_gate.take() {
            gate.await;
        }
        if let Some(started) = state.started.take()
            && let Some(page) = window.current_page()
        {
            log_first_frame(page.view(), started, state.first_frame.clone());
        }
        window.present();
        if let Some(hook) = state.on_first_window.take() {
            hook(window);
        }
    });
}

fn command_line_received(
    app: &adw::Application,
    state: &Rc<AppState>,
    command_line: &gio::ApplicationCommandLine,
) -> glib::ExitCode {
    let parsed = match cli::parse(command_line.arguments()) {
        Ok(parsed) => parsed,
        Err(error) => {
            let text = error.render().to_string();
            if error.use_stderr() {
                command_line.printerr_literal(&text);
            } else {
                command_line.print_literal(&text);
            }
            return glib::ExitCode::from(error.exit_code() as u8);
        }
    };
    if parsed.self_test.is_some() {
        command_line.printerr_literal(
            "stet: --self-test starts its own instance; it cannot be sent to a running one\n",
        );
        return glib::ExitCode::from(2);
    }
    if parsed.no_session && state.window().is_some() {
        command_line.printerr_literal(
            "stet: --no-session only applies when it starts Stet; the running Stet keeps its \
             session\n",
        );
    }
    let window = window_or_new(app, state, !parsed.no_session);
    present(state, &window);
    if !parsed.files.is_empty() {
        let cwd = command_line
            .cwd()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();
        let command_line = command_line.clone();
        glib::spawn_future_local(async move {
            // The restored tabs come first, then this command line's files.
            window.session_ready(RESTORE_WAIT).await;
            let pages = window
                .open_command_line(cwd, parsed.files, parsed.line, parsed.column)
                .await;
            // `--wait` keeps the command line, and so the caller, until those tabs close.
            if parsed.wait {
                window.add_wait(command_line, &pages);
            }
        });
    }
    glib::ExitCode::SUCCESS
}

/// Logs `first-frame <ms since process start>` after the first paint that follows `view`
/// mapping, and records when it was. The CI smoke run greps for it.
pub fn log_first_frame(
    view: &sourceview5::View,
    started: Instant,
    painted: Rc<Cell<Option<Instant>>>,
) {
    let on_map = Rc::new(RefCell::new(None));
    let handler = view.connect_map(glib::clone!(
        #[strong]
        on_map,
        move |view| {
            if let Some(handler) = on_map.take() {
                view.disconnect(handler);
            }
            let Some(clock) = view.frame_clock() else {
                tracing::warn!("mapped view has no frame clock; first-frame not measured");
                return;
            };
            let on_paint = Rc::new(RefCell::new(None));
            let painted = painted.clone();
            let handler = clock.connect_after_paint(glib::clone!(
                #[strong]
                on_paint,
                move |clock| {
                    if let Some(handler) = on_paint.take() {
                        clock.disconnect(handler);
                    }
                    painted.set(Some(Instant::now()));
                    let ms = started.elapsed().as_secs_f64() * 1000.0;
                    tracing::info!("first-frame {ms:.1}");
                }
            ));
            on_paint.replace(Some(handler));
        }
    ));
    on_map.replace(Some(handler));
}
