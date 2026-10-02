//! `stet --self-test <script>`: runs the real app (window and all) in a scratch directory,
//! executes a `.stet-test` script against it and prints a TAP report. Exits non-zero when a
//! step fails. It refuses to run on a live display unless `STET_SELFTEST_ALLOW_LIVE=1`; run it
//! through `tools/headless.sh`. The format is documented in docs/TESTING.md.

mod broadway;
mod column;
mod focus;
mod generate;
mod nav;
mod screenshot;
mod script;
mod search;
mod session;
mod tools;
mod views;

use crate::application;
use crate::config::{APP_ID, APP_ID_ENV, AppConfig};
use crate::editor::EditorPage;
use crate::tabstops;
use crate::window::dialogs::{GO_TO_LABEL, click_response};
use crate::window::{PickerMode, Window};
use crate::worker;
use gtk4 as gtk;
use gtk4::glib::translate::IntoGlib;
use gtk4::{gio, glib};
use libadwaita as adw;
use libadwaita::prelude::*;
use script::{Step, boolean, hex_bytes};
use sourceview5::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};
use stet_domain::actions::ActionId;
use stet_domain::language::hint_ids;
use stet_domain::theme::Rgb;
use stet_infrastructure::xdg::StetDirs;

pub const ALLOW_LIVE_ENV: &str = "STET_SELFTEST_ALLOW_LIVE";
const WAIT_TIMEOUT: Duration = Duration::from_secs(15);
/// Loading a generated 200 MB file in a debug build takes a while.
const IDLE_TIMEOUT: Duration = Duration::from_secs(180);
/// How often the heartbeat looks for main-loop blocks (as in spike S1).
const HEARTBEAT: Duration = Duration::from_millis(5);
/// A step that runs longer has hung (a popover that never opens, a dialog nobody answers): it
/// fails and the rest of the script is not run. Longer than `IDLE_TIMEOUT`, so that a slow load
/// reports itself first. `STET_SELFTEST_STEP_TIMEOUT` overrides it, in seconds.
const STEP_TIMEOUT: Duration = Duration::from_secs(300);
const STEP_TIMEOUT_ENV: &str = "STET_SELFTEST_STEP_TIMEOUT";

/// The longest gap between two runs of a 5 ms timer: how long the main loop was blocked.
struct Heartbeat {
    last: Cell<Instant>,
    longest: Cell<Duration>,
}

impl Heartbeat {
    fn start() -> Rc<Self> {
        let heartbeat = Rc::new(Self {
            last: Cell::new(Instant::now()),
            longest: Cell::new(Duration::ZERO),
        });
        let weak = Rc::downgrade(&heartbeat);
        glib::timeout_add_local(HEARTBEAT, move || {
            let Some(heartbeat) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let now = Instant::now();
            heartbeat
                .longest
                .set(heartbeat.longest.get().max(now - heartbeat.last.get()));
            heartbeat.last.set(now);
            glib::ControlFlow::Continue
        });
        heartbeat
    }

    fn longest(&self) -> Duration {
        self.longest.get().max(self.last.get().elapsed())
    }
}

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
    let text = match fs::read_to_string(script_path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("stet: cannot read {}: {error}", script_path.display());
            return glib::ExitCode::from(2);
        }
    };
    let script_dir = fs::canonicalize(script_path)
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let root = match TempRoot::create() {
        Ok(root) => root,
        Err(error) => {
            eprintln!("stet: cannot create a scratch directory: {error}");
            return glib::ExitCode::from(2);
        }
    };
    // A fake Omarchy state directory without `current/`: the theme appears when a script
    // runs `theme-set`, which is the watch-before-`current/` case of the ADR-004 amendment.
    if let Err(error) = fs::create_dir_all(root.path.join("omarchy")) {
        eprintln!("stet: cannot create the fake Omarchy directory: {error}");
        return glib::ExitCode::from(2);
    }
    let tmp = root.path.to_string_lossy().into_owned();
    let dir = script_dir.to_string_lossy().into_owned();
    let steps = match script::parse(&text, &[("TMP", &tmp), ("DIR", &dir)]) {
        Ok(steps) => steps,
        Err(error) => {
            eprintln!("stet: {}: {error}", script_path.display());
            return glib::ExitCode::from(2);
        }
    };

    if steps
        .first()
        .is_some_and(|step| step.command == "sandbox-home")
        && let Err(error) = tools::sandbox_home(&root.path)
    {
        eprintln!("stet: cannot create the sandboxed home: {error}");
        return glib::ExitCode::from(2);
    }

    // `launch-file` steps come first: their files are written now and given to the app's
    // first command line, as `stet a.txt` would.
    let mut launch = vec!["stet".to_owned()];
    for step in steps
        .iter()
        .skip_while(|step| step.command == "sandbox-home")
        .take_while(|step| step.command == "launch-file")
    {
        if let Err(error) = write_file(Path::new(&step.args[0]), step.args[1].as_bytes()) {
            eprintln!(
                "stet: {}: line {}: {error}",
                script_path.display(),
                step.line
            );
            return glib::ExitCode::from(2);
        }
        launch.push(step.args[0].clone());
    }

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
        // The live session opens the command line's files while the theme still loads, before
        // the window is shown; hold the window back until they are open, to do the same.
        let paths: Vec<PathBuf> = launch[1..].iter().map(PathBuf::from).collect();
        let weak = Rc::downgrade(&state);
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
        let nanos = std::time::SystemTime::now()
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
    heartbeat: RefCell<Option<Rc<Heartbeat>>>,
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

    async fn step(&self, step: &Step) -> StepResult {
        if let Some(result) = self.search_step(step).await {
            return result;
        }
        if let Some(result) = self.focus_step(step).await {
            return result;
        }
        if let Some(result) = self.session_step(step).await {
            return result;
        }
        if let Some(result) = self.tools_step(step).await {
            return result;
        }
        if let Some(result) = self.column_step(step).await {
            return result;
        }
        if let Some(result) = self.nav_step(step).await {
            return result;
        }
        if let Some(result) = self.views_step(step).await {
            return result;
        }
        let args = &step.args;
        let arg = |index: usize| args[index].as_str();
        match step.command.as_str() {
            // Written before the app started (see `run`).
            "launch-file" => Ok(()),
            "write" => write_file(Path::new(arg(0)), arg(1).as_bytes()),
            "write-hex" => write_file(Path::new(arg(0)), &hex_bytes(arg(1))?),
            "chmod" => {
                let mode = u32::from_str_radix(arg(1), 8).map_err(|error| error.to_string())?;
                fs::set_permissions(arg(0), fs::Permissions::from_mode(mode))
                    .map_err(|error| error.to_string())
            }
            "mkdir" => fs::create_dir_all(arg(0)).map_err(|error| error.to_string()),
            "rename" => fs::rename(arg(0), arg(1)).map_err(|error| error.to_string()),
            "remove" => fs::remove_dir_all(arg(0))
                .or_else(|_| fs::remove_file(arg(0)))
                .map_err(|error| error.to_string()),
            "open" | "open-start" => {
                let args: Vec<OsString> = args.iter().map(OsString::from).collect();
                let parsed = crate::cli::parse(std::iter::once(OsString::from("stet")).chain(args))
                    .map_err(|error| error.to_string())?;
                self.window
                    .open_command_line(self.root.clone(), parsed.files, parsed.line, parsed.column)
                    .await;
                if step.command == "open" {
                    self.wait_idle().await
                } else {
                    Ok(())
                }
            }
            "spawn-stet" => self.spawn_stet(args, 0).await,
            "spawn-stet-expect" => {
                let status = arg(0)
                    .parse::<i32>()
                    .map_err(|_| format!("expected an exit status, got {}", arg(0)))?;
                self.spawn_stet(&args[1..], status).await
            }
            "action" => {
                self.activate(arg(0), args.get(1).map(String::as_str))?;
                self.wait_until_settled().await;
                Ok(())
            }
            "type" => {
                let page = self.page()?;
                let view = page.view();
                view.grab_focus();
                page.buffer()
                    .insert_interactive_at_cursor(arg(0), view.is_editable());
                Ok(())
            }
            "cursor" => {
                let page = self.page()?;
                let (line, column) = (number(arg(0))?, number(arg(1))?);
                page.go_to(stet_domain::text::Position::new(line - 1, column - 1));
                Ok(())
            }
            "key" => self.key(arg(0), arg(1)),
            "find" => {
                self.window.show_find();
                self.window.inner.find.entry.set_text(arg(0));
                self.wait_until_settled().await;
                Ok(())
            }
            "find-case" => {
                self.window.inner.find.case.set_active(boolean(arg(0))?);
                Ok(())
            }
            "palette" => {
                self.window
                    .open_palette(args.first().map_or("", String::as_str));
                Ok(())
            }
            "palette-activate" => {
                self.window.palette().activate_selected();
                self.wait_idle().await
            }
            "palette-close" => {
                self.window.palette().popover.popdown();
                Ok(())
            }
            "choose-language" => {
                if self.window.inner.languages.choose_matching(arg(0)) {
                    Ok(())
                } else {
                    Err(format!("no language matches {}", arg(0)))
                }
            }
            "drop" => {
                let files = args.iter().map(gio::File::for_path).collect();
                if !self.window.drop_files(files) {
                    return Err("the drop was refused".to_owned());
                }
                self.wait_idle().await
            }
            "respond" => {
                let dialog = self
                    .window
                    .open_dialog()
                    .ok_or_else(|| "no dialog is open".to_owned())?;
                if !click_response(&dialog, arg(0)) {
                    return Err(format!("the dialog has no {} button", arg(0)));
                }
                self.wait_idle().await
            }
            "save-as" => {
                let page = self.page()?;
                if self.window.save_to(&page, PathBuf::from(arg(0))).await {
                    Ok(())
                } else {
                    Err("the save failed".to_owned())
                }
            }
            "set-tab-width" => {
                self.page()?.view().set_tab_width(number(arg(0))? as u32);
                Ok(())
            }
            "toggle-overwrite" => {
                // What the Insert key does in GtkTextView.
                self.page()?.view().emit_toggle_overwrite();
                Ok(())
            }
            "theme-set" => self.theme_set(Path::new(arg(0)), arg(1)).await,
            "symlink" => {
                std::os::unix::fs::symlink(arg(0), arg(1)).map_err(|error| error.to_string())
            }
            "write-generated" => {
                let path = PathBuf::from(arg(0));
                let kind = arg(1).to_owned();
                let bytes = number(arg(2))?;
                worker::run(move || {
                    let text = generate::generate(&kind, bytes)?;
                    write_file(&path, text.as_bytes())
                })
                .await
                .ok_or("the worker failed")?
            }
            "set-mtime" => {
                let ago = Duration::from_secs(number(arg(1))? as u64);
                let file = fs::File::options()
                    .write(true)
                    .open(arg(0))
                    .map_err(|error| error.to_string())?;
                file.set_modified(SystemTime::now() - ago)
                    .map_err(|error| error.to_string())
            }
            "select-tab" => {
                let index = number(arg(0))?;
                let pages = self.window.pages();
                let page = pages
                    .get(index.wrapping_sub(1))
                    .ok_or_else(|| format!("there is no tab {index}"))?;
                self.window.select(page);
                Ok(())
            }
            "banner-click" => {
                if self.page()?.click_banner(arg(0)) {
                    self.wait_until_settled().await;
                    Ok(())
                } else {
                    Err(format!(
                        "no banner button {:?}; the banners have {:?}",
                        arg(0),
                        self.page()?.banner_buttons()
                    ))
                }
            }
            "dialog-goto" => {
                let dialog = self
                    .window
                    .open_dialog()
                    .ok_or_else(|| "no dialog is open".to_owned())?;
                let index = number(arg(0))?;
                let buttons = buttons_labelled(dialog.upcast_ref(), GO_TO_LABEL);
                let button = buttons
                    .get(index.saturating_sub(1))
                    .ok_or_else(|| format!("the dialog has {} Go To buttons", buttons.len()))?;
                button.emit_clicked();
                self.wait_idle().await
            }
            "window-focus" => {
                self.window.check_all_files();
                self.wait_idle().await
            }
            "unwatch" => {
                // As on a file system that sends no change events.
                self.page()?.set_watch(None);
                Ok(())
            }
            "truncate" => {
                // A sparse file of that size, for size checks that never read it.
                let file = fs::File::create(arg(0)).map_err(|error| error.to_string())?;
                file.set_len(number(arg(1))? as u64)
                    .map_err(|error| error.to_string())
            }
            "unpace-frames" => {
                if let Some(settings) = gtk::Settings::default() {
                    settings.set_gtk_cursor_blink(false);
                }
                broadway::connect().map_err(|error| format!("Broadway web client: {error}"))
            }
            "heartbeat-start" => {
                self.heartbeat.replace(Some(Heartbeat::start()));
                Ok(())
            }
            "pick-encoding" => {
                let mode = match arg(0) {
                    "reinterpret" => PickerMode::Reinterpret,
                    "convert" => PickerMode::Convert,
                    other => return Err(format!("{other}: use reinterpret or convert")),
                };
                self.window.update_status();
                if !self.window.inner.encodings.choose_id(mode, arg(1)) {
                    return Err(format!("the picker can't {} {}", arg(0), arg(1)));
                }
                self.wait_idle().await
            }
            "type-burst" => {
                let count = number(arg(0))?;
                let interval = Duration::from_millis(number(arg(1))? as u64);
                let limit = Duration::from_millis(number(arg(2))? as u64);
                self.type_burst(count, interval, limit).await
            }
            "report-memory" => {
                let status =
                    fs::read_to_string("/proc/self/status").map_err(|error| error.to_string())?;
                let field = |name: &str| {
                    status
                        .lines()
                        .find_map(|line| line.strip_prefix(name))
                        .map_or("?", str::trim)
                        .to_owned()
                };
                println!(
                    "  # memory {}: VmRSS {}, VmHWM {}",
                    arg(0),
                    field("VmRSS:"),
                    field("VmHWM:")
                );
                Ok(())
            }
            "report-load" => {
                let stats = self.page()?.load_stats();
                let since = |at: Option<Instant>| {
                    stats.started.zip(at).map_or_else(
                        || "-".to_owned(),
                        |(started, at)| format!("{:.1} ms", (at - started).as_secs_f64() * 1e3),
                    )
                };
                println!(
                    "  # load: decoded {}, first piece in {}, first frame {}, all in {}; {} \
                     pieces, {} bytes, longest insert {:.1} ms",
                    since(stats.decoded),
                    since(stats.first_insert),
                    since(stats.first_frame),
                    since(stats.finished),
                    stats.pieces,
                    stats.bytes,
                    stats.longest_insert.as_secs_f64() * 1e3
                );
                Ok(())
            }
            "wait-idle" => self.wait_idle().await,
            "wait-frames" => self.wait_frames(number(arg(0))?).await,
            "sleep" => {
                glib::timeout_future(Duration::from_millis(number(arg(0))? as u64)).await;
                Ok(())
            }
            "screenshot" => {
                let context = self
                    .page()
                    .ok()
                    .and_then(|page| tools::context_popover(page.view().upcast_ref()))
                    .map(|menu| menu.upcast::<gtk::Popover>());
                let mut popovers = vec![
                    &self.window.palette().popover,
                    &self.window.inner.languages.popover,
                    &self.window.inner.encodings.popover,
                    &self.window.inner.indentation.popover,
                    &self.window.inner.nav.quick_open.popover,
                ];
                popovers.extend(context.as_ref());
                screenshot::capture(self.window.window(), &popovers, Path::new(arg(0)))
            }
            "dump-layout" => {
                dump_widget(
                    self.window.window().upcast_ref(),
                    self.window.window().upcast_ref(),
                    0,
                );
                Ok(())
            }
            "wait-until" => {
                let started = Instant::now();
                loop {
                    match self.check_async(&args[0], &args[1..]).await {
                        Ok(()) => return Ok(()),
                        Err(error) if started.elapsed() > WAIT_TIMEOUT => {
                            return Err(format!("still failing after {WAIT_TIMEOUT:?}: {error}"));
                        }
                        Err(_) => glib::timeout_future(Duration::from_millis(20)).await,
                    }
                }
            }
            command => self.check_async(command, args).await,
        }
    }

    /// The text of the open dialog: a question's heading, body and labels, or the labels of
    /// another dialog the window shows, such as About.
    fn dialog_text(&self) -> Result<String, String> {
        if let Some(dialog) = self.window.open_dialog() {
            return Ok(format!(
                "{}\n{}\n{}",
                dialog.heading().unwrap_or_default(),
                dialog.body(),
                labels_in(dialog.upcast_ref()).join("\n")
            ));
        }
        self.window
            .window()
            .visible_dialog()
            .map(|dialog| labels_in(dialog.upcast_ref()).join("\n"))
            .ok_or_else(|| "no dialog is open".to_owned())
    }

    /// An assertion; the ones that ask another process over D-Bus run on a worker.
    async fn check_async(&self, assertion: &str, args: &[String]) -> StepResult {
        match self.bus_check(assertion, args).await {
            Some(result) => result,
            None => self.check(assertion, args),
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
        let parameter = parameter.map(|value| value.to_variant());
        match id.scope() {
            stet_domain::actions::ActionScope::App => {
                self.app.activate_action(id.name(), parameter.as_ref());
                Ok(())
            }
            stet_domain::actions::ActionScope::Window => {
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

    fn key(&self, target: &str, key: &str) -> StepResult {
        let find = &self.window.inner.find;
        let controller = match target {
            "find" => find.keys.clone(),
            "replace" => find.replace_keys.clone(),
            "directory" => find.directory_keys.clone(),
            "filters" => find.filters_keys.clone(),
            "results" => self.window.inner.results.keys.clone(),
            other => {
                return Err(format!(
                    "no key target {other}; use find, replace, directory, filters or results"
                ));
            }
        };
        let (keyval, modifiers) =
            gtk::accelerator_parse(key).ok_or_else(|| format!("bad key {key}"))?;
        let values: [&dyn glib::value::ToValue; 3] = [&keyval.into_glib(), &0u32, &modifiers];
        controller.emit_by_name::<bool>("key-pressed", &values);
        Ok(())
    }

    /// Runs a second `stet` with the harness's application id, as a user in another terminal
    /// would; it forwards its command line to this instance and exits with `expected`.
    async fn spawn_stet(&self, args: &[String], expected: i32) -> StepResult {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        let cwd = self.root.clone();
        let app_id = self.app_id.clone();
        let args = args.to_vec();
        let output = worker::run(move || {
            Command::new(exe)
                .args(&args)
                .current_dir(cwd)
                .env(APP_ID_ENV, app_id)
                .output()
        })
        .await
        .ok_or("the worker failed")?
        .map_err(|error| error.to_string())?;
        if output.status.code() != Some(expected) {
            return Err(format!(
                "stet exited with {}, wanted {expected}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(())
    }

    /// Swaps the theme in the fake Omarchy directory the way `omarchy-theme-set` does:
    /// build `next-theme`, remove `theme`, rename `next-theme` to `theme`, write `theme.name`.
    async fn theme_set(&self, theme_dir: &Path, name: &str) -> StepResult {
        const SWAP: &str = r#"set -e
current="$1"; source="$2"; name="$3"
mkdir -p "$current"
cd "$current"
rm -rf next-theme
mkdir -p next-theme
cp -r "$source"/. next-theme/
rm -rf theme
mv next-theme theme
echo "$name" > theme.name
"#;
        let current = self.root.join("omarchy/current");
        let source = theme_dir.to_path_buf();
        let name = name.to_owned();
        let output = worker::run(move || {
            Command::new("bash")
                .arg("-c")
                .arg(SWAP)
                .arg("theme-set")
                .arg(current)
                .arg(source)
                .arg(name)
                .output()
        })
        .await
        .ok_or("the worker failed")?
        .map_err(|error| error.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        Ok(())
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

    /// Types `count` characters `interval` apart the way GtkTextView commits a key (spike S1)
    /// and measures each from the insert to the end of the next painted frame. Fails when one
    /// took longer than `limit`. Needs `unpace-frames`, or Broadway paints once a second.
    async fn type_burst(&self, count: usize, interval: Duration, limit: Duration) -> StepResult {
        let page = self.page()?;
        let view = page.view().clone();
        let buffer = page.buffer();
        let clock = self
            .window
            .window()
            .frame_clock()
            .ok_or("the window has no frame clock")?;
        let painted: Rc<RefCell<Vec<Instant>>> = Rc::new(RefCell::new(Vec::new()));
        let handler = clock.connect_after_paint(glib::clone!(
            #[strong]
            painted,
            move |_| painted.borrow_mut().push(Instant::now())
        ));
        let mut latencies = Vec::with_capacity(count);
        let mut result = Ok(());
        for _ in 0..count {
            glib::timeout_future(interval).await;
            let typed = Instant::now();
            buffer.begin_user_action();
            buffer.insert_interactive_at_cursor("x", view.is_editable());
            buffer.end_user_action();
            view.scroll_mark_onscreen(&buffer.get_insert());
            let frame = loop {
                if let Some(frame) = painted.borrow().iter().find(|frame| **frame > typed) {
                    break Some(*frame);
                }
                if typed.elapsed() > WAIT_TIMEOUT {
                    break None;
                }
                glib::timeout_future(Duration::from_millis(1)).await;
            };
            match frame {
                Some(frame) => latencies.push(frame - typed),
                None => {
                    result = Err("a keystroke was never painted".to_owned());
                    break;
                }
            }
        }
        clock.disconnect(handler);
        latencies.sort();
        let ms = |duration: Duration| duration.as_secs_f64() * 1e3;
        if let (Some(median), Some(longest)) =
            (latencies.get(latencies.len() / 2), latencies.last())
        {
            let p95 = latencies[(latencies.len() * 95).div_ceil(100).saturating_sub(1)];
            println!(
                "  # {} keystrokes, insert to painted frame: median {:.1} ms, p95 {:.1} ms, \
                 max {:.1} ms",
                latencies.len(),
                ms(*median),
                ms(p95),
                ms(*longest)
            );
            if *longest > limit {
                result = Err(format!(
                    "a keystroke took {longest:?} to paint, over {limit:?}"
                ));
            }
        }
        result
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

    fn check(&self, assertion: &str, args: &[String]) -> StepResult {
        let arg = |index: usize| args[index].as_str();
        match assertion {
            "assert-text" => equal("text", self.page()?.text(), arg(0).to_owned()),
            "assert-focus" => self.check_focus(arg(0)),
            "assert-text-contains" => {
                let text = self.page()?.text();
                if text.contains(arg(0)) {
                    Ok(())
                } else {
                    Err(format!("{:?} does not contain {:?}", text, arg(0)))
                }
            }
            "assert-selection" => {
                let buffer = self.page()?.buffer();
                let selected = buffer
                    .selection_bounds()
                    .map(|(start, end)| buffer.text(&start, &end, true).to_string())
                    .unwrap_or_default();
                equal("selection", selected, arg(0).to_owned())
            }
            "assert-current-match" => {
                let page = self.page()?;
                let buffer = page.buffer();
                let text = page
                    .current_match()
                    .map(|(start, end)| buffer.text(&start, &end, true).to_string())
                    .unwrap_or_default();
                equal("current match", text, arg(0).to_owned())
            }
            "assert-tab-count" => equal("tabs", self.window.pages().len(), number(arg(0))?),
            "assert-title" => equal("title", self.window.title(), arg(0).to_owned()),
            "assert-tab-title" => {
                let index = number(arg(0))?;
                let (_, tab) = self
                    .window
                    .tab_at(index)
                    .ok_or_else(|| format!("there is no tab {index}"))?;
                equal("tab title", tab.title().to_string(), arg(1).to_owned())
            }
            "assert-cursor" => {
                let cursor = self.page()?.cursor();
                equal(
                    "cursor (line, column)",
                    (cursor.line + 1, cursor.column + 1),
                    (number(arg(0))?, number(arg(1))?),
                )
            }
            "assert-language" => {
                let language = self
                    .page()?
                    .language()
                    .map_or_else(|| "none".to_owned(), |language| language.id().to_string());
                equal("language", language, arg(0).to_owned())
            }
            "assert-dirty" => equal("dirty", self.page()?.is_dirty(), boolean(arg(0))?),
            "assert-read-only" => {
                let page = self.page()?;
                equal(
                    "read-only",
                    page.read_only().is_some() || !page.view().is_editable(),
                    boolean(arg(0))?,
                )
            }
            "assert-banner" => {
                let banners = self.page()?.banner_texts();
                match arg(0) {
                    "none" if banners.is_empty() => Ok(()),
                    "none" => Err(format!("banners show {banners:?}")),
                    wanted if banners.iter().any(|text| text.contains(wanted)) => Ok(()),
                    wanted => Err(format!("no banner has {wanted:?}; banners: {banners:?}")),
                }
            }
            "assert-no-banner" => {
                let banners = self.page()?.banner_texts();
                match banners.iter().find(|text| text.contains(arg(0))) {
                    Some(text) => Err(format!("a banner shows {text:?}")),
                    None => Ok(()),
                }
            }
            "assert-dialog-text" => {
                let text = self.dialog_text()?;
                if text.contains(arg(0)) {
                    Ok(())
                } else {
                    Err(format!("the dialog {text:?} lacks {:?}", arg(0)))
                }
            }
            "assert-dialog-lacks" => {
                let text = self.dialog_text()?;
                if text.contains(arg(0)) {
                    Err(format!("the dialog {text:?} has {:?}", arg(0)))
                } else {
                    Ok(())
                }
            }
            "assert-highlight" => equal(
                "highlighting",
                self.page()?.buffer().is_highlight_syntax()
                    && self.page()?.buffer().language().is_some(),
                boolean(arg(0))?,
            ),
            "assert-large" => equal(
                "large-file mode",
                self.page()?.large_file_mode(),
                boolean(arg(0))?,
            ),
            "assert-editable" => equal(
                "editable",
                self.page()?.view().is_editable(),
                boolean(arg(0))?,
            ),
            "assert-max-block" => {
                let heartbeat = self
                    .heartbeat
                    .borrow()
                    .clone()
                    .ok_or("run heartbeat-start first")?;
                let longest = heartbeat.longest();
                let limit = Duration::from_millis(number(arg(0))? as u64);
                println!(
                    "  # longest main-loop block: {:.1} ms",
                    longest.as_secs_f64() * 1e3
                );
                if longest <= limit {
                    Ok(())
                } else {
                    Err(format!(
                        "the main loop was blocked for {longest:?}, over {limit:?}"
                    ))
                }
            }
            "assert-first-screen" => {
                let stats = self.page()?.load_stats();
                let screen = stats
                    .started
                    .zip(stats.first_frame)
                    .map(|(started, frame)| frame - started)
                    .ok_or("no first frame was recorded")?;
                // The bar is for release builds; a debug build decodes far slower and may
                // get its own limit as the second argument.
                let debug = cfg!(debug_assertions) && args.len() > 1;
                let limit = Duration::from_millis(number(arg(usize::from(debug)))? as u64);
                println!(
                    "  # first screen: {:.1} ms ({} build, limit {limit:?})",
                    screen.as_secs_f64() * 1e3,
                    if cfg!(debug_assertions) {
                        "debug"
                    } else {
                        "release"
                    }
                );
                if screen <= limit {
                    Ok(())
                } else {
                    Err(format!("the first screen took {screen:?}, over {limit:?}"))
                }
            }
            "assert-wrap" => equal(
                "word wrap",
                self.page()?.view().wrap_mode() != gtk::WrapMode::None,
                boolean(arg(0))?,
            ),
            "assert-toast" => {
                let toast = self.window.last_toast();
                if toast.contains(arg(0)) {
                    Ok(())
                } else {
                    Err(format!("the last toast {toast:?} lacks {:?}", arg(0)))
                }
            }
            "assert-tab-text" => {
                let index = number(arg(0))?;
                let pages = self.window.pages();
                let page = pages
                    .get(index.saturating_sub(1))
                    .ok_or_else(|| format!("there is no tab {index}"))?;
                equal("text", page.text(), arg(1).to_owned())
            }
            "assert-symlink" => {
                let target = fs::read_link(arg(0)).map_err(|error| error.to_string())?;
                equal("link target", target, PathBuf::from(arg(1)))
            }
            "assert-files-like" => {
                let count = fs::read_dir(arg(0))
                    .map_err(|error| error.to_string())?
                    .flatten()
                    .filter(|entry| entry.file_name().to_string_lossy().contains(arg(1)))
                    .count();
                equal("matching files", count, number(arg(2))?)
            }
            "assert-file" => {
                let text = fs::read_to_string(arg(0)).map_err(|error| error.to_string())?;
                equal("file contents", text, arg(1).to_owned())
            }
            "assert-file-hex" => {
                let bytes = fs::read(arg(0)).map_err(|error| error.to_string())?;
                equal("file bytes", bytes, hex_bytes(arg(1))?)
            }
            "assert-mode" => {
                let mode = fs::metadata(arg(0))
                    .map_err(|error| error.to_string())?
                    .permissions()
                    .mode()
                    & 0o7777;
                equal(
                    "mode",
                    format!("{mode:o}"),
                    arg(1).trim_start_matches('0').to_owned(),
                )
            }
            "assert-missing" => {
                if Path::new(arg(0)).exists() {
                    Err(format!("{} exists", arg(0)))
                } else {
                    Ok(())
                }
            }
            "assert-status" => {
                let status = self.window.inner.status.text();
                if status.contains(arg(0)) {
                    Ok(())
                } else {
                    Err(format!("status {status:?} lacks {:?}", arg(0)))
                }
            }
            "assert-match-count" => equal(
                "match count",
                self.window.inner.find.count.label().to_string(),
                arg(0).to_owned(),
            ),
            "assert-find-open" => equal(
                "find bar open",
                self.window.inner.find.is_open(),
                boolean(arg(0))?,
            ),
            "assert-theme" => self.check_theme(arg(0), arg(1), arg(2)),
            "assert-chrome-fg" => {
                let wanted = color(arg(0))?;
                let rgba = self.window.inner.status.root.color();
                let actual = Rgb {
                    r: (rgba.red() * 255.0).round() as u8,
                    g: (rgba.green() * 255.0).round() as u8,
                    b: (rgba.blue() * 255.0).round() as u8,
                };
                equal("status bar colour", actual.to_string(), wanted.to_string())
            }
            "assert-dark" => equal(
                "dark",
                adw::StyleManager::default().is_dark(),
                boolean(arg(0))?,
            ),
            "assert-theme-source" => {
                let source = match self.window.shared().appearance.source() {
                    Some(crate::theme::ThemeSource::Omarchy(name)) => format!("omarchy:{name}"),
                    Some(crate::theme::ThemeSource::Builtin(mode)) => {
                        format!("builtin:{}", mode.as_str())
                    }
                    None => "none".to_owned(),
                };
                equal("theme source", source, arg(0).to_owned())
            }
            "assert-reloads" => equal(
                "theme reloads",
                self.window.shared().appearance.reloads() as usize,
                number(arg(0))?,
            ),
            "assert-scheme-files" => {
                let dir = self.window.shared().config.dirs.styles_dir();
                let count = fs::read_dir(&dir)
                    .map_err(|error| error.to_string())?
                    .flatten()
                    .filter(|entry| entry.file_name().to_string_lossy().ends_with(".xml"))
                    .count();
                equal("scheme files", count, number(arg(0))?)
            }
            "assert-tab-grid" => self.check_tab_grid(),
            "assert-action-state" => {
                let id = action_id(arg(0))?;
                let state = self
                    .window
                    .action(id)
                    .and_then(|action| action.state())
                    .and_then(|state| state.get::<bool>())
                    .ok_or_else(|| format!("{} has no boolean state", arg(0)))?;
                equal("state", state, boolean(arg(1))?)
            }
            "assert-action-enabled" => {
                let id = action_id(arg(0))?;
                let enabled = self
                    .window
                    .action(id)
                    .is_some_and(|action| action.is_enabled());
                equal("enabled", enabled, boolean(arg(1))?)
            }
            "assert-accels" => self.check_accels(),
            "assert-widget-keys" => self.check_widget_keys(),
            "assert-language-hints" => {
                let manager = sourceview5::LanguageManager::default();
                let missing: Vec<&str> = hint_ids()
                    .filter(|id| manager.language(id).is_none())
                    .collect();
                if missing.is_empty() {
                    Ok(())
                } else {
                    Err(format!("not installed: {missing:?}"))
                }
            }
            "assert-recent" => {
                let recent = self.window.shared().recent();
                let first = recent.paths().first().cloned().unwrap_or_default();
                equal("most recent file", first, PathBuf::from(arg(0)))
            }
            "assert-palette-first" => {
                let labels = self.window.palette().shown_labels();
                equal(
                    "first palette row",
                    labels.first().cloned().unwrap_or_default(),
                    arg(0).to_owned(),
                )
            }
            "assert-palette-contains" => {
                let labels = self.window.palette().shown_labels();
                if labels.iter().any(|label| label == arg(0)) {
                    Ok(())
                } else {
                    Err(format!("palette rows {labels:?} lack {:?}", arg(0)))
                }
            }
            "assert-zoom" => equal(
                "zoom",
                self.window.shared().appearance.zoom().points(),
                arg(0).parse::<i32>().map_err(|error| error.to_string())?,
            ),
            "assert-font-size" => {
                let page = self.page()?;
                // GTK turns CSS points into an absolute size in pixels at 96 dpi.
                let size = page
                    .view()
                    .pango_context()
                    .font_description()
                    .map(|font| {
                        let size = f64::from(font.size()) / f64::from(gtk::pango::SCALE);
                        if font.is_size_absolute() {
                            size * 72.0 / 96.0
                        } else {
                            size
                        }
                    })
                    .unwrap_or_default();
                equal("font size (pt)", format!("{size:.0}"), arg(0).to_owned())
            }
            "assert-font-family" => equal(
                "font family",
                self.window.shared().appearance.family().unwrap_or_default(),
                arg(0).to_owned(),
            ),
            "assert-font-reloads" => equal(
                "font reloads",
                self.window.shared().appearance.font_reloads() as usize,
                number(arg(0))?,
            ),
            "assert-dialog" => equal(
                "dialog open",
                self.window.open_dialog().is_some(),
                boolean(arg(0))?,
            ),
            "assert-closed" => equal("closed tabs", self.window.closed_count(), number(arg(0))?),
            "assert-menu" => self.check_menu(),
            "assert-menu-contains" => {
                let items = self.menu_items()?;
                if items.iter().any(|(label, _)| label == arg(0)) {
                    Ok(())
                } else {
                    Err(format!("the menu has no item {:?}", arg(0)))
                }
            }
            "assert-undo-levels" => equal(
                "max undo levels",
                self.page()?.buffer().max_undo_levels() as usize,
                number(arg(0))?,
            ),
            other => self
                .search_check(other, args)
                .or_else(|| self.session_check(other, args))
                .or_else(|| self.tools_check(other, args))
                .or_else(|| self.column_check(other, args))
                .or_else(|| self.nav_check(other, args))
                .or_else(|| self.views_check(other, args))
                .unwrap_or_else(|| Err(format!("unknown command {other}"))),
        }
    }

    fn check_theme(&self, style: &str, attribute: &str, wanted: &str) -> StepResult {
        let wanted = color(wanted)?;
        let scheme = self
            .page()?
            .buffer()
            .style_scheme()
            .ok_or_else(|| "the buffer has no scheme".to_owned())?;
        let style_object = scheme
            .style(style)
            .ok_or_else(|| format!("the scheme has no style {style}"))?;
        let value = match attribute {
            "foreground" => style_object.foreground(),
            "background" => style_object.background(),
            other => return Err(format!("unknown attribute {other}")),
        }
        .ok_or_else(|| format!("{style} has no {attribute}"))?;
        let actual = color(&value)?;
        equal(
            &format!("{style} {attribute}"),
            actual.to_string(),
            wanted.to_string(),
        )
    }

    /// ADR-015: after a tab, the next character's `iter_location` x lies on the character
    /// grid (whole columns of the font's space advance), within rounding to whole pixels.
    fn check_tab_grid(&self) -> StepResult {
        let page = self.page()?;
        let view = page.view();
        let buffer = page.buffer();
        let advance = tabstops::space_advance(view) as f64 / gtk::pango::SCALE as f64;
        let tab_width = view.tab_width() as usize;
        let expected_stop = tabstops::stop_for(view);
        let stop = tabstops::current_stop(view);
        if stop != Some(expected_stop) {
            return Err(format!(
                "tab stop {stop:?} Pango units, expected {expected_stop} ({tab_width} × {:.3} px)",
                advance
            ));
        }
        let mut worst = 0.0f64;
        let mut checked = 0;
        for line in 0..buffer.line_count() {
            let Some(mut iter) = buffer.iter_at_line(line) else {
                continue;
            };
            let origin = view.iter_location(&iter).x() as f64;
            let mut column = 0usize;
            while !iter.ends_line() {
                let character = iter.char();
                if !iter.forward_char() {
                    break;
                }
                column = if character == '\t' {
                    (column / tab_width + 1) * tab_width
                } else {
                    column + 1
                };
                let x = view.iter_location(&iter).x() as f64;
                let expected = origin + column as f64 * advance;
                worst = worst.max((x - expected).abs());
                checked += 1;
            }
        }
        if checked == 0 {
            return Err("the buffer has no characters to check".to_owned());
        }
        if worst > 0.51 {
            return Err(format!(
                "off the grid by up to {worst:.2} px (advance {advance:.3} px, {checked} positions)"
            ));
        }
        println!(
            "  # tab grid: {checked} positions within {worst:.2} px; advance {advance:.3} px, \
             stop {expected_stop} Pango units, tab width {tab_width}"
        );
        Ok(())
    }

    /// Every accelerator of the keymap (the registry's, with `keys.toml`) is installed and
    /// parses in GTK.
    fn check_accels(&self) -> StepResult {
        let keymap = self.window.keymap();
        let mut problems = Vec::new();
        for id in ActionId::ALL {
            let installed: Vec<_> = self
                .app
                .accels_for_action(&id.detailed_name())
                .iter()
                .filter_map(|accel| gtk::accelerator_parse(accel.as_str()))
                .collect();
            let accels = keymap.accels(id);
            let wanted: Vec<_> = accels
                .iter()
                .map(|accel| gtk::accelerator_parse(accel).ok_or(accel.as_str()))
                .collect();
            for accel in &wanted {
                match accel {
                    Err(text) => problems.push(format!("{id:?}: GTK cannot parse {text}")),
                    Ok(parsed) if !installed.contains(parsed) => {
                        problems.push(format!("{id:?}: {parsed:?} is not installed"));
                    }
                    Ok(_) => {}
                }
            }
            if installed.len() != wanted.len() {
                problems.push(format!(
                    "{id:?}: {} installed, {} wanted",
                    installed.len(),
                    wanted.len()
                ));
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.join("\n"))
        }
    }

    /// Every (label, action) in the hamburger menu, submenus included.
    fn menu_items(&self) -> Result<Vec<(String, String)>, String> {
        fn collect(model: &gio::MenuModel, out: &mut Vec<(String, String)>) {
            let text = |index: i32, attribute: &str| {
                model
                    .item_attribute_value(index, attribute, Some(glib::VariantTy::STRING))
                    .and_then(|value| value.get::<String>())
                    .unwrap_or_default()
            };
            for index in 0..model.n_items() {
                let action = text(index, "action");
                if !action.is_empty() {
                    out.push((text(index, "label"), action));
                }
                for link in ["section", "submenu"] {
                    if let Some(child) = model.item_link(index, link) {
                        collect(&child, out);
                    }
                }
            }
        }
        let model = self
            .window
            .menu_model()
            .ok_or_else(|| "the window has no menu".to_owned())?;
        let mut items = Vec::new();
        collect(&model, &mut items);
        Ok(items)
    }

    /// The menu holds every registry action with a menu place, and nothing else.
    fn check_menu(&self) -> StepResult {
        let items = self.menu_items()?;
        let no_recent = self.window.shared().recent().is_empty();
        let mut problems = Vec::new();
        for id in ActionId::ALL {
            let spec = id.spec();
            let Some(place) = spec.menu else {
                continue;
            };
            let hidden_recent =
                no_recent && place.submenu == Some(stet_domain::actions::RECENT_SUBMENU);
            if hidden_recent || spec.kind == stet_domain::actions::ActionKind::WithString {
                continue;
            }
            let found = items
                .iter()
                .any(|(label, action)| label == spec.label && *action == id.detailed_name());
            if !found {
                problems.push(format!("{id:?} ({}) is not in the menu", spec.label));
            }
        }
        for (label, action) in &items {
            let installed = ActionId::from_name(action).is_some_and(|id| match id.scope() {
                stet_domain::actions::ActionScope::App => {
                    self.app.lookup_action(id.name()).is_some()
                }
                stet_domain::actions::ActionScope::Window => self.window.action(id).is_some(),
            });
            if !installed {
                problems.push(format!(
                    "menu item {label:?} runs {action}, which is not installed"
                ));
            }
        }
        if problems.is_empty() {
            println!("  # menu: {} items, all from the registry", items.len());
            Ok(())
        } else {
            Err(problems.join("\n"))
        }
    }

    /// Every widget key the registry shows for editing actions is bound by GtkTextView.
    fn check_widget_keys(&self) -> StepResult {
        let page = self.page()?;
        let triggers = class_triggers(page.view().upcast_ref());
        let mut missing = Vec::new();
        for id in ActionId::ALL {
            for key in id.keys().widget {
                let wanted = gtk::ShortcutTrigger::parse_string(key)
                    .ok_or_else(|| format!("GTK cannot parse {key}"))?;
                if !triggers.iter().any(|trigger| trigger.equal(&wanted)) {
                    missing.push(format!("{id:?}: {key}"));
                }
            }
        }
        if missing.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "not bound by the text view: {}",
                missing.join(", ")
            ))
        }
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
async fn with_timeout<T>(
    future: impl std::future::Future<Output = T>,
    limit: Duration,
) -> Option<T> {
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
    frame: impl std::future::Future<Output = Result<(), async_channel::RecvError>>,
    timeout: impl std::future::Future<Output = ()>,
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

/// Prints the visible widget tree with window coordinates, for diagnosing layouts.
fn dump_widget(widget: &gtk::Widget, root: &gtk::Widget, depth: usize) {
    if !widget.is_visible() {
        return;
    }
    let bounds = widget.compute_bounds(root);
    let (x, y, width, height) = bounds.map_or((0.0, 0.0, 0.0, 0.0), |b| {
        (b.x(), b.y(), b.width(), b.height())
    });
    println!(
        "  # {:indent$}{} [{}] {x:.0},{y:.0} {width:.0}×{height:.0}",
        "",
        widget.type_().name(),
        widget.css_classes().join(" "),
        indent = depth * 2
    );
    let mut child = widget.first_child();
    while let Some(current) = child {
        dump_widget(&current, root, depth + 1);
        child = current.next_sibling();
    }
}

/// Every button under `widget` labelled `label`, in tree order.
fn buttons_labelled(widget: &gtk::Widget, label: &str) -> Vec<gtk::Button> {
    let mut found = Vec::new();
    if let Some(button) = widget.downcast_ref::<gtk::Button>()
        && button.label().as_deref() == Some(label)
    {
        found.push(button.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        found.extend(buttons_labelled(&current, label));
        child = current.next_sibling();
    }
    found
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

fn number(word: &str) -> Result<usize, String> {
    word.parse()
        .map_err(|_| format!("expected a number, got {word}"))
}

fn color(word: &str) -> Result<Rgb, String> {
    Rgb::parse(word).ok_or_else(|| format!("expected a colour, got {word}"))
}

fn action_id(name: &str) -> Result<ActionId, String> {
    ActionId::from_name(name).ok_or_else(|| format!("no action {name}"))
}

fn equal<T: PartialEq + std::fmt::Debug>(what: &str, actual: T, wanted: T) -> StepResult {
    if actual == wanted {
        Ok(())
    } else {
        Err(format!("{what}: got {actual:?}, wanted {wanted:?}"))
    }
}
