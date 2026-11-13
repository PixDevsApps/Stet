//! `stet --self-test <script>`: runs the real app (window and all) in a scratch directory,
//! executes a `.stet-test` script against it and prints a TAP report. Exits non-zero when a
//! step fails. It refuses to run on a live display unless `STET_SELFTEST_ALLOW_LIVE=1`; run it
//! through `tools/headless.sh`. The format is documented in docs/TESTING.md; the commands are
//! declared in [`registry`] and its areas' tables.

mod actions;
mod broadway;
mod column;
mod dialogs;
mod documents;
mod editing;
mod files;
mod focus;
mod generate;
mod nav;
mod registry;
mod screenshot;
mod script;
mod search;
mod session;
mod theme;
mod timing;
mod tools;
mod views;

use crate::application;
use crate::config::{APP_ID, APP_ID_ENV, AppConfig};
use crate::editor::EditorPage;
use crate::runtime::worker;
use crate::window::Window;
use gtk4 as gtk;
use gtk4::{gio, glib};
use libadwaita as adw;
use libadwaita::prelude::*;
use script::Step;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};
use stet_domain::actions::{ActionId, ActionKind, ActionScope};
use stet_domain::text::{Line, LineNumber};
use stet_domain::theme::Rgb;
use stet_infrastructure::xdg::StetDirs;

pub const ALLOW_LIVE_ENV: &str = "STET_SELFTEST_ALLOW_LIVE";
const WAIT_TIMEOUT: Duration = Duration::from_secs(15);
/// Loading a generated 200 MB file in a debug build takes a while.
const IDLE_TIMEOUT: Duration = Duration::from_secs(180);
/// A step that runs longer has hung (a popover that never opens, a dialog nobody answers): it
/// fails and the rest of the script is not run. Longer than `IDLE_TIMEOUT`, so that a slow load
/// reports itself first. `STET_SELFTEST_STEP_TIMEOUT` overrides it, in seconds.
const STEP_TIMEOUT: Duration = Duration::from_secs(300);
const STEP_TIMEOUT_ENV: &str = "STET_SELFTEST_STEP_TIMEOUT";

pub fn run(script_path: &Path, started: Instant) -> glib::ExitCode {
    let broadway = std::env::var_os("GDK_BACKEND").is_some_and(|backend| backend == "broadway");
    let allowed = std::env::var_os(ALLOW_LIVE_ENV).is_some_and(|value| value == "1");
    if !broadway && !allowed {
        eprintln!(
            "stet: --self-test opens windows; run it headless with \
             tools/headless.sh <display> stet --self-test <script> (or set {ALLOW_LIVE_ENV}=1)"
        );
        return glib::ExitCode::from(2);
    }
    let Prepared {
        root,
        steps,
        launch,
    } = match prepare(script_path) {
        Ok(prepared) => prepared,
        Err(message) => {
            eprintln!("stet: {message}");
            return glib::ExitCode::from(2);
        }
    };

    // A child self-test (see `child-start`) gets its id from its parent.
    let app_id = std::env::var(APP_ID_ENV)
        .ok()
        .filter(|id| gio::Application::id_is_valid(id))
        .unwrap_or_else(|| format!("{APP_ID}.SelfTest"));
    let config = AppConfig {
        app_id: app_id.clone(),
        dirs: StetDirs::under(&root.path),
        omarchy_dir: root.path.join("omarchy"),
        fontconfig_dir: root.path.join("fontconfig"),
        exit_after: None,
        non_unique: false,
    };
    let (app, state) = application::build(config, started);
    if launch.len() > 1 {
        hold_until_open(&state, launch[1..].iter().map(PathBuf::from).collect());
    }
    let outcome: Rc<Cell<Option<bool>>> = Rc::new(Cell::new(None));
    let harness_root = root.path.clone();
    let harness_state = state.clone();
    state.connect_first_window(glib::clone!(
        #[strong]
        outcome,
        #[weak]
        app,
        move |window| {
            glib::spawn_future_local(async move {
                let harness = Harness {
                    window,
                    app: app.clone(),
                    root: harness_root,
                    app_id,
                    heartbeat: RefCell::new(None),
                    state: harness_state,
                    started,
                    processes: RefCell::new(HashMap::new()),
                    bus: RefCell::new(None),
                    children: Cell::new(0),
                };
                let passed = harness.run(&steps).await;
                harness.stop_processes().await;
                outcome.set(Some(passed));
                harness.end(steps.last());
            });
        }
    ));
    app.run_with_args(&launch);
    drop(root);
    match outcome.get() {
        Some(true) => glib::ExitCode::SUCCESS,
        Some(false) => glib::ExitCode::FAILURE,
        None => {
            eprintln!("stet: the self-test never started");
            glib::ExitCode::FAILURE
        }
    }
}

/// What a run sets up before the app starts.
struct Prepared {
    root: TempRoot,
    steps: Vec<Step>,
    /// The app's first command line: `stet` and the `launch-file` files.
    launch: Vec<String>,
}

/// Reads and parses the script, creates the scratch directory with its fake Omarchy state,
/// sandboxes the home for `sandbox-home` and writes the `launch-file` files. The error is the
/// message to print after `stet: `.
fn prepare(script_path: &Path) -> Result<Prepared, String> {
    let text = fs::read_to_string(script_path)
        .map_err(|error| format!("cannot read {}: {error}", script_path.display()))?;
    let script_dir = fs::canonicalize(script_path)
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let root = TempRoot::create()
        .map_err(|error| format!("cannot create a scratch directory: {error}"))?;
    // A fake Omarchy state directory without `current/`: the theme appears when a script
    // runs `theme-set`, which is the watch-before-`current/` case of the ADR-004 amendment.
    fs::create_dir_all(root.path.join("omarchy"))
        .map_err(|error| format!("cannot create the fake Omarchy directory: {error}"))?;
    let tmp = root.path.to_string_lossy().into_owned();
    let dir = script_dir.to_string_lossy().into_owned();
    let steps = script::parse(&text, &[("TMP", &tmp), ("DIR", &dir)])
        .map_err(|error| format!("{}: {error}", script_path.display()))?;

    if steps
        .first()
        .is_some_and(|step| step.command == "sandbox-home")
    {
        tools::sandbox_home(&root.path)
            .map_err(|error| format!("cannot create the sandboxed home: {error}"))?;
    }

    // `launch-file` steps come first: their files are written now and given to the app's
    // first command line, as `stet a.txt` would.
    let mut launch = vec!["stet".to_owned()];
    for step in steps
        .iter()
        .skip_while(|step| step.command == "sandbox-home")
        .take_while(|step| step.command == "launch-file")
    {
        write_file(Path::new(&step.args[0]), step.args[1].as_bytes())
            .map_err(|error| format!("{}: line {}: {error}", script_path.display(), step.line))?;
        launch.push(step.args[0].clone());
    }
    Ok(Prepared {
        root,
        steps,
        launch,
    })
}

/// The live session opens the command line's files while the theme still loads, before the
/// window is shown; this holds the window back until `paths` are open, to do the same.
fn hold_until_open(state: &Rc<application::AppState>, paths: Vec<PathBuf>) {
    let weak = Rc::downgrade(state);
    state.hold_first_window(async move {
        let started = Instant::now();
        while started.elapsed() < WAIT_TIMEOUT {
            let window = weak.upgrade().and_then(|state| state.window());
            let open = window.is_some_and(|window| {
                let pages = window.pages();
                paths.iter().all(|path| {
                    pages
                        .iter()
                        .any(|page| page.shows(path, None) && page.shows_text())
                })
            });
            if open {
                return;
            }
            glib::timeout_future(Duration::from_millis(5)).await;
        }
    });
}

/// A scratch directory, removed when dropped unless a parent self-test gave it
/// (`STET_SELFTEST_ROOT`), so that the next child finds what this one left.
struct TempRoot {
    path: PathBuf,
    keep: bool,
}

impl TempRoot {
    fn create() -> std::io::Result<Self> {
        if let Some(dir) = std::env::var_os(session::ROOT_ENV) {
            fs::create_dir_all(&dir)?;
            return Ok(Self {
                path: fs::canonicalize(&dir)?,
                keep: true,
            });
        }
        let nanos = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.subsec_nanos());
        let path =
            std::env::temp_dir().join(format!("stet-selftest-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path)?;
        Ok(Self {
            path: fs::canonicalize(&path)?,
            keep: false,
        })
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

struct Harness {
    window: Window,
    app: adw::Application,
    root: PathBuf,
    app_id: String,
    heartbeat: RefCell<Option<Rc<timing::Heartbeat>>>,
    state: Rc<application::AppState>,
    /// When the process started, for the restore timing.
    started: Instant,
    /// Background `stet` processes and child self-tests, by name.
    processes: RefCell<HashMap<String, std::process::Child>>,
    bus: RefCell<Option<session::TestBus>>,
    /// Child self-tests started so far, for their ids.
    children: Cell<u32>,
}

type StepResult = Result<(), String>;

impl Harness {
    async fn run(&self, steps: &[Step]) -> bool {
        println!("TAP version 13");
        let limit = step_timeout();
        let mut failed = 0;
        let mut timed_out = None;
        for (index, step) in steps.iter().enumerate() {
            let number = index + 1;
            if let Some(hung) = timed_out {
                failed += 1;
                println!("not ok {number} - {}", step.source);
                println!("  # not run: step {hung} timed out");
                continue;
            }
            match with_timeout(self.step(step), limit).await {
                Some(Ok(())) => println!("ok {number} - {}", step.source),
                Some(Err(message)) => {
                    failed += 1;
                    println!("not ok {number} - {}", step.source);
                    for line in message.lines() {
                        println!("  # {line}");
                    }
                }
                None => {
                    failed += 1;
                    timed_out = Some(number);
                    println!("not ok {number} - {}", step.source);
                    println!("  # timed out after {} s", limit.as_secs());
                }
            }
        }
        println!("1..{}", steps.len());
        println!("# {} of {} steps passed", steps.len() - failed, steps.len());
        failed == 0
    }

    fn page(&self) -> Result<EditorPage, String> {
        self.window
            .current_page()
            .ok_or_else(|| "no tab is selected".to_owned())
    }

    /// The window's text field `name` (see [`Window::field`]).
    fn field(&self, name: &str) -> Result<gtk::Editable, String> {
        self.window
            .field(name)
            .ok_or_else(|| format!("no field {name}"))
    }

    async fn step(&self, step: &Step) -> StepResult {
        self.run_command(&step.command, &step.args).await
    }

    /// Runs the command `name` with `args`, whose count the parser checked.
    async fn run_command(&self, name: &str, args: &[String]) -> StepResult {
        match registry::find(name) {
            Some(command) => (command.run)(self, args).await,
            None => Err(format!("unknown command {name}")),
        }
    }

    /// Ends the app once the steps ran. A last `quit` step closes the window as the user does
    /// (the session's last commit and flush), `quit signal TERM` sends this process that
    /// signal; otherwise the app just stops.
    fn end(&self, last: Option<&Step>) {
        let quit = last.filter(|step| step.command == "quit");
        match quit.map(|step| step.args.as_slice()) {
            Some([how, signal]) if how == "signal" => {
                let _ = Command::new("kill")
                    .arg(format!("-{signal}"))
                    .arg(std::process::id().to_string())
                    .status();
            }
            Some(_) => self.window.window().close(),
            None => self.app.quit(),
        }
    }

    fn activate(&self, name: &str, parameter: Option<&str>) -> StepResult {
        let id = ActionId::from_name(name).ok_or_else(|| format!("no action {name}"))?;
        let parameter = parameter.map(|parameter| match id.kind() {
            ActionKind::WithPath => crate::actions::path_target(Path::new(parameter)),
            _ => parameter.to_variant(),
        });
        match id.scope() {
            ActionScope::App => {
                self.app.activate_action(id.name(), parameter.as_ref());
                Ok(())
            }
            ActionScope::Window => {
                let action = self
                    .window
                    .action(id)
                    .ok_or_else(|| format!("{name} is not installed"))?;
                if !action.is_enabled() {
                    return Err(format!("{name} is disabled"));
                }
                action.activate(parameter.as_ref());
                Ok(())
            }
        }
    }

    /// Waits until spawned tasks have started, workers have finished and no tab is loading,
    /// or until a dialog waits for an answer.
    async fn wait_idle(&self) -> StepResult {
        let started = Instant::now();
        loop {
            self.wait_until_settled().await;
            let page_loading = self.window.pages().iter().any(EditorPage::is_loading);
            if (worker::pending() == 0 && !page_loading) || self.window.open_dialog().is_some() {
                return Ok(());
            }
            if started.elapsed() > IDLE_TIMEOUT {
                return Err("background work did not finish".to_owned());
            }
            glib::timeout_future(Duration::from_millis(5)).await;
        }
    }

    /// Lets idle handlers at default priority run once.
    async fn wait_until_settled(&self) {
        let (sender, receiver) = async_channel::bounded(1);
        glib::idle_add_local_full(glib::Priority::LOW, move || {
            let _ = sender.try_send(());
            glib::ControlFlow::Break
        });
        let _ = receiver.recv().await;
    }

    async fn wait_frames(&self, frames: usize) -> StepResult {
        let window = self.window.window();
        let clock = window
            .frame_clock()
            .ok_or_else(|| "the window has no frame clock".to_owned())?;
        let (sender, receiver) = async_channel::unbounded();
        let handler = clock.connect_after_paint(move |_| {
            let _ = sender.try_send(());
        });
        clock.begin_updating();
        window.queue_draw();
        let mut result = Ok(());
        for _ in 0..frames {
            let frame = receiver.recv();
            let timeout = glib::timeout_future(WAIT_TIMEOUT);
            futures_select(frame, timeout, &mut result).await;
            if result.is_err() {
                break;
            }
        }
        clock.end_updating();
        clock.disconnect(handler);
        result
    }
}

/// The key triggers of the widget's class shortcuts, alternatives flattened.
fn class_triggers(widget: &gtk::Widget) -> Vec<gtk::ShortcutTrigger> {
    fn flatten(trigger: gtk::ShortcutTrigger, out: &mut Vec<gtk::ShortcutTrigger>) {
        match trigger.downcast::<gtk::AlternativeTrigger>() {
            Ok(alternative) => {
                flatten(alternative.first(), out);
                flatten(alternative.second(), out);
            }
            Err(trigger) => out.push(trigger),
        }
    }
    let mut triggers = Vec::new();
    let controllers = widget.observe_controllers();
    for index in 0..controllers.n_items() {
        let Some(controller) = controllers
            .item(index)
            .and_downcast::<gtk::ShortcutController>()
        else {
            continue;
        };
        for position in 0..controller.n_items() {
            if let Some(shortcut) = controller.item(position).and_downcast::<gtk::Shortcut>()
                && let Some(trigger) = shortcut.trigger()
            {
                flatten(trigger, &mut triggers);
            }
        }
    }
    triggers
}

fn step_timeout() -> Duration {
    std::env::var(STEP_TIMEOUT_ENV)
        .ok()
        .and_then(|seconds| seconds.parse().ok())
        .map_or(STEP_TIMEOUT, Duration::from_secs)
}

/// Runs `future` until it finishes or `limit` passes; `None` when it timed out (the future is
/// dropped).
async fn with_timeout<T>(future: impl Future<Output = T>, limit: Duration) -> Option<T> {
    let mut future = std::pin::pin!(future);
    let mut timeout = std::pin::pin!(glib::timeout_future(limit));
    std::future::poll_fn(|context| {
        if let std::task::Poll::Ready(value) = future.as_mut().poll(context) {
            return std::task::Poll::Ready(Some(value));
        }
        if timeout.as_mut().poll(context).is_ready() {
            return std::task::Poll::Ready(None);
        }
        std::task::Poll::Pending
    })
    .await
}

async fn futures_select(
    frame: impl Future<Output = Result<(), async_channel::RecvError>>,
    timeout: impl Future<Output = ()>,
    result: &mut StepResult,
) {
    let mut frame = std::pin::pin!(frame);
    let mut timeout = std::pin::pin!(timeout);
    let finished = std::future::poll_fn(|context| {
        if frame.as_mut().poll(context).is_ready() {
            return std::task::Poll::Ready(true);
        }
        if timeout.as_mut().poll(context).is_ready() {
            return std::task::Poll::Ready(false);
        }
        std::task::Poll::Pending
    })
    .await;
    if !finished {
        *result = Err("no frame was painted".to_owned());
    }
}

/// The text of every label under `widget`, in tree order.
fn labels_in(widget: &gtk::Widget) -> Vec<String> {
    let mut found = Vec::new();
    if let Some(label) = widget.downcast_ref::<gtk::Label>() {
        found.push(label.label().to_string());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        found.extend(labels_in(&current));
        child = current.next_sibling();
    }
    found
}

fn write_file(path: &Path, bytes: &[u8]) -> StepResult {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(path, bytes).map_err(|error| error.to_string())
}

/// `debug` or `release`, for the reports.
fn build() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

fn number(word: &str) -> Result<usize, String> {
    word.parse()
        .map_err(|_| format!("expected a number, got {word}"))
}

/// The line a script names as `number`, counting lines from 1; 0 names the first line too.
fn script_line(number: usize) -> Line {
    LineNumber::new(number).map_or(Line::ZERO, LineNumber::index)
}

fn color(word: &str) -> Result<Rgb, String> {
    Rgb::parse(word).ok_or_else(|| format!("expected a colour, got {word}"))
}

fn action_id(name: &str) -> Result<ActionId, String> {
    ActionId::from_name(name).ok_or_else(|| format!("no action {name}"))
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "the steps build the values in place; borrowing would add `&` to over a hundred calls"
)]
fn equal<T: PartialEq + std::fmt::Debug>(what: &str, actual: T, wanted: T) -> StepResult {
    if actual == wanted {
        Ok(())
    } else {
        Err(format!("{what}: got {actual:?}, wanted {wanted:?}"))
    }
}
