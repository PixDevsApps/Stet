//! Self-test commands for the session: commits and the files the store keeps, restored
//! stubs and a digest of every tab; `stet` processes in the background (`--wait`); child
//! self-tests that share a scratch directory, so a session can be killed, signalled or quit
//! and then restored by a new process; and a private D-Bus bus for activation, whose only
//! service directory holds Stet's own test service file.

use super::registry::{Command, MANY, commands};
use super::script::boolean;
use super::{Harness, StepResult, WAIT_TIMEOUT, build, equal, number, write_file};
use crate::config::APP_ID_ENV;
use crate::convert::{BufferPositions, IterPosition, to_i32};
use crate::runtime::worker;
use crate::window::{Location, SessionPhase, Window};
use gtk4::prelude::*;
use gtk4::{gio, glib};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command as Process, ExitStatus, Stdio};
use std::time::{Duration, Instant};
use stet_domain::session::Session;
use stet_domain::text::CharOffset;

/// A child self-test's scratch directory: given by its parent, and kept when it ends.
pub const ROOT_ENV: &str = "STET_SELFTEST_ROOT";

/// How long a child self-test may run before it counts as hung.
const PROCESS_TIMEOUT: Duration = Duration::from_secs(240);

/// How long `spawn-wait` waits for a background `stet`.
const SPAWN_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) const COMMANDS: &[Command] = commands![
    "quit" (0, 2) => quit,
    "session-commit" (0, 0) => session_commit,
    "session-flush" (0, 0) => async session_flush,
    "wait-backed-up" (1, 1) => async wait_backed_up,
    "tabs-digest" (1, 1) => async tabs_digest,
    "generate-tabs" (3, 3) => async generate_tabs,
    "drop-backup" (2, 2) => drop_backup,
    "spawn-bg" (2, MANY) => spawn_bg,
    "spawn-wait" (2, 2) => async spawn_wait,
    "child-start" (3, 3) => child_start,
    "child-run" (3, 3) => async child_run,
    "child-signal" (2, 2) => child_signal,
    "child-wait" (2, 2) => async child_wait,
    "test-bus-start" (1, 2) => async test_bus_start,
    "test-bus-spawn" (2, MANY) => test_bus_spawn,
    "bus-action" (1, 2) => async bus_action,
    "test-bus-stop" (0, 0) => async test_bus_stop,
    "report-session" (0, 0) => report_session,
    "report-restore" (0, 0) => async report_restore,
    "wait-exists" (1, 1) => async wait_exists,
    "set-window-size" (2, 2) => set_window_size,
    "assert-tabs-digest" (1, 1) => async assert_tabs_digest,
    "assert-restore-time" (1, 2) => async assert_restore_time,
    "assert-session" (1, 1) => assert_session,
    "assert-stub" (2, 2) => assert_stub,
    "assert-backed-up" (1, 1) => assert_backed_up,
    "assert-waiting" (1, 1) => assert_waiting,
    "assert-backups" (2, 2) => assert_backups,
    "assert-orphans" (2, 2) => assert_orphans,
    "assert-session-tabs" (2, 2) => assert_session_tabs,
    "assert-session-contains" (2, 2) => assert_session_contains,
    "assert-session-lacks" (2, 2) => assert_session_lacks,
    "assert-running" (2, 2) => assert_running,
    "assert-bus-owner" (1, 1) => async assert_bus_owner,
    "assert-window-size" (2, 2) => assert_window_size,
];

/// A private session bus for D-Bus activation (`test-bus-start`).
pub struct TestBus {
    daemon: Child,
    address: String,
    /// The id the activated Stet registers.
    app_id: String,
    /// Its state, cache and configuration directories.
    home: PathBuf,
    /// Holds the bus socket; removed with the bus.
    socket_dir: PathBuf,
}

impl Drop for TestBus {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
        let _ = fs::remove_dir_all(&self.socket_dir);
    }
}

impl TestBus {
    fn environment(&self) -> Vec<(&'static str, String)> {
        bus_environment(&self.address, &self.app_id, &self.home)
    }

    /// The object path GApplication exports `app_id` at.
    fn object_path(&self) -> String {
        format!("/{}", self.app_id.replace('.', "/").replace('-', "_"))
    }
}

impl Harness {
    /// `quit`, `quit window` or `quit signal TERM|HUP|INT`: only as the last step; once every
    /// step has run, closes the window the way the user does (the session's last commit and
    /// flush), or sends this process the signal (see `end`).
    fn quit(&self, args: &[String]) -> StepResult {
        match args {
            [] => Ok(()),
            [how] if how == "window" => Ok(()),
            [how, signal] if how == "signal" => signal_number(signal).map(|_| ()),
            _ => Err("use quit, quit window or quit signal TERM|HUP|INT".to_owned()),
        }
    }

    /// `session-commit`: runs the 7 s backup commit now.
    fn session_commit(&self, _: &[String]) {
        self.window.commit_session(false);
    }

    /// `session-flush`: waits until the store has written every commit.
    async fn session_flush(&self, _: &[String]) -> StepResult {
        self.window.flush_session().await
    }

    /// `tabs-digest <path>`: writes every tab's digest (see `digest`) for
    /// `assert-tabs-digest`.
    async fn tabs_digest(&self, args: &[String]) -> StepResult {
        let digest = self.digest().await?;
        write_file(Path::new(&args[0]), digest.as_bytes())
    }

    /// `assert-tabs-digest <path>`: every tab's digest is the one `tabs-digest` wrote there.
    async fn assert_tabs_digest(&self, args: &[String]) -> StepResult {
        let wanted = fs::read_to_string(&args[0]).map_err(|error| error.to_string())?;
        let digest = self.digest().await?;
        if digest == wanted {
            println!("  # {}", digest.lines().next().unwrap_or_default());
            Ok(())
        } else {
            Err(format!(
                "the tabs differ from {}:\n--- then\n{wanted}--- now\n{digest}",
                args[0]
            ))
        }
    }

    /// `drop-backup <state folder> <tab>`: deletes the backup of tab n (1-based) in the
    /// session in that folder, as a disk problem could.
    fn drop_backup(&self, args: &[String]) -> StepResult {
        let state = Path::new(&args[0]);
        let tab = number(&args[1])?;
        let session = read_session(state)?;
        let record = session
            .tabs
            .get(tab.saturating_sub(1))
            .ok_or_else(|| format!("the session has no tab {tab}"))?;
        let id = record
            .backup
            .as_deref()
            .ok_or_else(|| format!("tab {tab} has no backup"))?;
        fs::remove_file(state.join("backup").join(format!("{id}.txt")))
            .map_err(|error| error.to_string())
    }

    /// `spawn-bg <name> <arguments…>`: starts a `stet` in the background with the harness's
    /// id (for `--wait`), its output in `$TMP/<name>.out` and `.err`.
    fn spawn_bg(&self, args: &[String]) -> StepResult {
        self.spawn_background(&args[0], &args[1..], false)
    }

    /// `test-bus-spawn <name> <arguments…>`: as `spawn-bg`, on the test bus with the activated
    /// id.
    fn test_bus_spawn(&self, args: &[String]) -> StepResult {
        self.spawn_background(&args[0], &args[1..], true)
    }

    /// `spawn-wait <name> <status>`: waits up to a minute for a background `stet` to end with
    /// the status (see `wait_process`).
    async fn spawn_wait(&self, args: &[String]) -> StepResult {
        self.wait_process(&args[0], &args[1], SPAWN_TIMEOUT).await
    }

    /// `child-start <name> <folder> <script>`: starts a child self-test of the script in that
    /// scratch folder.
    fn child_start(&self, args: &[String]) -> StepResult {
        self.start_child(&args[0], Path::new(&args[1]), Path::new(&args[2]))
    }

    /// `child-run <folder> <script> <status>`: runs a child self-test and waits for it to end
    /// with the status.
    async fn child_run(&self, args: &[String]) -> StepResult {
        let name = format!("child{}", self.children.get() + 1);
        self.start_child(&name, Path::new(&args[0]), Path::new(&args[1]))?;
        self.wait_process(&name, &args[2], PROCESS_TIMEOUT).await
    }

    /// `child-signal <name> KILL|TERM|HUP|INT`: sends a background process the signal.
    fn child_signal(&self, args: &[String]) -> StepResult {
        self.signal_process(&args[0], &args[1])
    }

    /// `child-wait <name> <status>`: waits for a child self-test to end with the status.
    async fn child_wait(&self, args: &[String]) -> StepResult {
        self.wait_process(&args[0], &args[1], PROCESS_TIMEOUT).await
    }

    /// `test-bus-start service|none` or `test-bus-start selftest <script>`: starts the private
    /// D-Bus bus (see `start_test_bus`).
    async fn test_bus_start(&self, args: &[String]) -> StepResult {
        self.start_test_bus(&args[0], args.get(1).map(String::as_str))
            .await
    }

    /// `bus-action app.<name> [<parameter>]`: activates an application action of the Stet the
    /// test bus started (see `activate_on_bus`).
    async fn bus_action(&self, args: &[String]) -> StepResult {
        self.activate_on_bus(&args[0], args.get(1).map(String::as_str))
            .await
    }

    /// `test-bus-stop`: stops the test bus; a Stet it started that still runs is stopped
    /// first, and fails the step.
    async fn test_bus_stop(&self, _: &[String]) -> StepResult {
        self.stop_test_bus().await
    }

    /// `report-restore`: prints the restore's timings from the process start (session read,
    /// tabs in the strip, first frame), once the first frame is painted.
    async fn report_restore(&self, _: &[String]) -> StepResult {
        self.wait_first_frame().await?;
        self.print_restore()
    }

    /// `assert-restore-time <release ms> [<debug ms>]`: from the process start to the first
    /// frame, which shows the restored window with its active tab's text, within the limit; a
    /// debug build takes the second limit, when given.
    async fn assert_restore_time(&self, args: &[String]) -> StepResult {
        self.wait_first_frame().await?;
        self.check_restore_time(args)
    }

    /// `set-window-size <width> <height>`: the size the window keeps in the session, as a
    /// resize by the user sets it (its default size).
    fn set_window_size(&self, args: &[String]) -> StepResult {
        let (width, height) = (number(&args[0])?, number(&args[1])?);
        self.window
            .window()
            .set_default_size(to_i32(width), to_i32(height));
        Ok(())
    }

    /// `wait-exists <path>`: waits until the path exists; a child self-test leaves a file when
    /// it is ready, and may take a while to start.
    async fn wait_exists(&self, args: &[String]) -> StepResult {
        let started = Instant::now();
        while !Path::new(&args[0]).exists() {
            if started.elapsed() > PROCESS_TIMEOUT {
                return Err(format!("{} never appeared", args[0]));
            }
            glib::timeout_future(Duration::from_millis(20)).await;
        }
        Ok(())
    }

    /// `assert-session on|off|pending`: the session's phase.
    fn assert_session(&self, args: &[String]) -> StepResult {
        let phase = match self.window.session_phase() {
            SessionPhase::On => "on",
            SessionPhase::Off => "off",
            SessionPhase::Pending => "pending",
        };
        equal("session", phase.to_owned(), args[0].clone())
    }

    /// `assert-stub <n> true|false`: whether tab n is a restored stub, its text not loaded.
    fn assert_stub(&self, args: &[String]) -> StepResult {
        let index = number(&args[0])?;
        let pages = self.window.pages();
        let page = pages
            .get(index.saturating_sub(1))
            .ok_or_else(|| format!("there is no tab {index}"))?;
        equal("stub", Window::is_stub(page), boolean(&args[1])?)
    }

    /// `assert-backed-up true|false`: whether every document's current text is backed up.
    fn assert_backed_up(&self, args: &[String]) -> StepResult {
        let wanted = boolean(&args[0])?;
        equal(
            "every backup current",
            self.window.backups_current(),
            wanted,
        )
    }

    /// `assert-waiting <n>`: how many `--wait` command lines wait.
    fn assert_waiting(&self, args: &[String]) -> StepResult {
        let wanted = number(&args[0])?;
        equal("--wait command lines", self.window.waiting(), wanted)
    }

    /// `assert-backups <state folder> <n>`: how many backups the folder holds.
    fn assert_backups(&self, args: &[String]) -> StepResult {
        let wanted = number(&args[1])?;
        equal(
            "backups",
            count_files(&Path::new(&args[0]).join("backup"))?,
            wanted,
        )
    }

    /// `assert-orphans <state folder> <n>`: how many orphaned backups the folder holds.
    fn assert_orphans(&self, args: &[String]) -> StepResult {
        let wanted = number(&args[1])?;
        equal(
            "orphans",
            count_files(&Path::new(&args[0]).join("backup/orphaned"))?,
            wanted,
        )
    }

    /// `assert-session-tabs <state folder> <n>`: how many tabs that folder's `session.json`
    /// lists.
    fn assert_session_tabs(&self, args: &[String]) -> StepResult {
        let wanted = number(&args[1])?;
        equal(
            "tabs in session.json",
            read_session(Path::new(&args[0]))?.tabs.len(),
            wanted,
        )
    }

    /// `assert-session-contains <state folder> <text>`: that folder's `session.json` has the
    /// text.
    fn assert_session_contains(&self, args: &[String]) -> StepResult {
        session_has(&args[0], &args[1], true)
    }

    /// `assert-session-lacks <state folder> <text>`: that folder's `session.json` lacks the
    /// text.
    fn assert_session_lacks(&self, args: &[String]) -> StepResult {
        session_has(&args[0], &args[1], false)
    }

    /// `assert-running <name> true|false`: whether a background process still runs.
    fn assert_running(&self, args: &[String]) -> StepResult {
        let wanted = boolean(&args[1])?;
        let mut processes = self.processes.borrow_mut();
        let child = processes
            .get_mut(args[0].as_str())
            .ok_or_else(|| format!("no process {}", args[0]))?;
        let running = child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_none();
        equal(&format!("{} running", args[0]), running, wanted)
    }

    /// `assert-bus-owner true|false`: whether the activated id has an owner on the test bus,
    /// asked on a worker.
    async fn assert_bus_owner(&self, args: &[String]) -> StepResult {
        match (boolean(&args[0]), self.bus_owned().await) {
            (Ok(wanted), Ok(owned)) => equal("activated Stet on the bus", owned, wanted),
            (Err(error), _) | (_, Err(error)) => Err(error),
        }
    }

    /// `assert-window-size <width> <height>`: the size the window keeps in the session.
    fn assert_window_size(&self, args: &[String]) -> StepResult {
        let width = number(&args[0])?;
        let height = number(&args[1])?;
        equal(
            "window size",
            self.window.window().default_size(),
            (to_i32(width), to_i32(height)),
        )
    }

    /// `wait-backed-up <limit ms>`: waits for the next backup commit after this step began
    /// that leaves every document's current text backed up and on disk, without forcing one;
    /// fails if that took longer than the limit.
    async fn wait_backed_up(&self, args: &[String]) -> StepResult {
        let limit_ms = number(&args[0])?;
        let started = Instant::now();
        let before = self.window.session_stats().commits;
        let limit = Duration::from_millis(limit_ms as u64);
        loop {
            let stats = self.window.session_stats();
            if stats.commits > before && self.window.backups_current() {
                let committed = stats.committed_at.unwrap_or_else(Instant::now) - started;
                self.window.flush_session().await?;
                let on_disk = started.elapsed();
                println!(
                    "  # backed up {:.2} s after the step began, on disk after {:.2} s (limit \
                     {:.1} s for the commit)",
                    committed.as_secs_f64(),
                    on_disk.as_secs_f64(),
                    limit.as_secs_f64()
                );
                return if committed <= limit {
                    Ok(())
                } else {
                    Err(format!(
                        "the backup came after {committed:?}, over {limit:?}"
                    ))
                };
            }
            if started.elapsed() > limit + WAIT_TIMEOUT {
                return Err(format!("no backup commit after {:?}", started.elapsed()));
            }
            glib::timeout_future(Duration::from_millis(10)).await;
        }
    }

    /// Every tab, after loading the stubs: name, path, dirty, a hash of the text, its length,
    /// the selection (anchor>caret), language, encoding, BOM, line ending and the encoding
    /// chosen with Reinterpret; and which tab is active.
    async fn digest(&self) -> Result<String, String> {
        self.window.load_stubs().await;
        self.wait_idle().await?;
        let pages = self.window.pages();
        let current = self.window.current_page();
        let active = pages
            .iter()
            .position(|page| Some(page) == current.as_ref())
            .map_or_else(|| "none".to_owned(), |index| (index + 1).to_string());
        let mut out = format!("{} tabs, tab {active} active\n", pages.len());
        for (index, page) in pages.iter().enumerate() {
            let buffer = page.buffer();
            let text = page.text();
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            text.hash(&mut hasher);
            let anchor = buffer
                .iter_at_mark(&buffer.selection_bound())
                .char_offset()
                .get();
            let caret = buffer
                .iter_at_mark(&buffer.get_insert())
                .char_offset()
                .get();
            let state = page.state();
            let language = page
                .language()
                .map_or_else(|| "none".to_owned(), |language| language.id().to_string());
            let _ = writeln!(
                out,
                "{}\t{}\t{}\tdirty={}\ttext={:016x}/{}\tselection={anchor}>{caret}\t{language}\t{}{}\t{}\tchosen={}",
                index + 1,
                page.name(),
                state
                    .path
                    .as_deref()
                    .map_or_else(|| "-".to_owned(), |path| path.display().to_string()),
                page.is_dirty(),
                hasher.finish(),
                text.chars().count(),
                state.save.format.encoding.name(),
                if state.save.format.bom { "+bom" } else { "" },
                state.save.format.eol.label(),
                state.chosen.map_or_else(
                    || "-".to_owned(),
                    |choice| choice.encoding.name().to_owned()
                ),
            );
        }
        Ok(out)
    }

    /// `generate-tabs <folder> <files> <untitled>`: opens that many generated files and edits
    /// each, and opens that many untitled documents with text, some with a selection made
    /// backwards.
    async fn generate_tabs(&self, args: &[String]) -> StepResult {
        let dir = Path::new(&args[0]);
        let (files, untitled) = (number(&args[1])?, number(&args[2])?);
        fs::create_dir_all(dir).map_err(|error| error.to_string())?;
        let mut locations = Vec::with_capacity(files);
        for index in 0..files {
            let path = dir.join(format!("file{index:02}.txt"));
            write_file(
                &path,
                format!("file {index}\nsecond line {index}\n").as_bytes(),
            )?;
            locations.push(Location {
                path,
                position: None,
                create: false,
            });
        }
        let pages = self.window.open_locations(locations);
        self.wait_idle().await?;
        for (index, page) in pages.iter().enumerate() {
            let buffer = page.buffer();
            buffer.begin_user_action();
            buffer.insert(&mut buffer.end_iter(), &format!("edited {index}\n"));
            buffer.end_user_action();
            if index % 3 == 0 {
                buffer.select_range(
                    &buffer.iter_at_char(CharOffset::new(2)),
                    &buffer.iter_at_char(CharOffset::new(8)),
                );
            } else {
                buffer.place_cursor(&buffer.iter_at_char(CharOffset::new(3)));
            }
        }
        for index in 0..untitled {
            let page = self.window.new_untitled();
            let buffer = page.buffer();
            buffer.insert_interactive_at_cursor(
                &format!("untitled {index}\nmore text {index}\n"),
                true,
            );
            if index % 2 == 1 {
                buffer.select_range(
                    &buffer.iter_at_char(CharOffset::ZERO),
                    &buffer.iter_at_char(CharOffset::new(5)),
                );
            }
        }
        Ok(())
    }

    /// A `stet` in the background, with the harness's id (or on the test bus with the
    /// activated id), its output in `$TMP/<name>.out` and `.err`.
    fn spawn_background(&self, name: &str, args: &[String], on_test_bus: bool) -> StepResult {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        let output = |extension: &str| {
            File::create(self.root.join(format!("{name}.{extension}")))
                .map_err(|error| error.to_string())
        };
        let mut command = Process::new(exe);
        command
            .args(args)
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .stdout(output("out")?)
            .stderr(output("err")?);
        if on_test_bus {
            let bus = self.bus.borrow();
            let bus = bus.as_ref().ok_or("run test-bus-start first")?;
            command.envs(bus.environment());
        } else {
            command.env(APP_ID_ENV, &self.app_id);
        }
        let child = command.spawn().map_err(|error| error.to_string())?;
        self.processes.borrow_mut().insert(name.to_owned(), child);
        Ok(())
    }

    /// A child self-test running `script` in `root`, with an id of its own: the parent can
    /// kill or signal it, and a later child restores what it left in `root`.
    fn start_child(&self, name: &str, root: &Path, script: &Path) -> StepResult {
        let number = self.children.get() + 1;
        self.children.set(number);
        fs::create_dir_all(root).map_err(|error| error.to_string())?;
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        let output = |extension: &str| {
            File::create(self.root.join(format!("{name}.{extension}")))
                .map_err(|error| error.to_string())
        };
        let child = Process::new(exe)
            .arg("--self-test")
            .arg(script)
            .env(ROOT_ENV, root)
            .env(APP_ID_ENV, format!("{}.Child{number}", self.app_id))
            .stdin(Stdio::null())
            .stdout(output("tap")?)
            .stderr(output("err")?)
            .spawn()
            .map_err(|error| error.to_string())?;
        self.processes.borrow_mut().insert(name.to_owned(), child);
        Ok(())
    }

    /// Waits for a background process to end with `wanted`: an exit status, `killed`
    /// (SIGKILL), `signal` (any signal) or `any`. A failed child self-test's report is shown.
    async fn wait_process(&self, name: &str, wanted: &str, timeout: Duration) -> StepResult {
        let started = Instant::now();
        let status = loop {
            let status = {
                let mut processes = self.processes.borrow_mut();
                let child = processes
                    .get_mut(name)
                    .ok_or_else(|| format!("no process {name}"))?;
                child.try_wait().map_err(|error| error.to_string())?
            };
            if let Some(status) = status {
                self.processes.borrow_mut().remove(name);
                break status;
            }
            if started.elapsed() > timeout {
                return Err(format!("{name} is still running after {timeout:?}"));
            }
            glib::timeout_future(Duration::from_millis(20)).await;
        };
        println!(
            "  # {name} ended after {:.2} s: {status}",
            started.elapsed().as_secs_f64()
        );
        if status_matches(status, wanted) {
            // A child self-test's measurements and summary, for the parent's log.
            if let Ok(text) = fs::read_to_string(self.root.join(format!("{name}.tap"))) {
                for line in text
                    .lines()
                    .filter(|line| line.starts_with("  # ") || line.starts_with("# "))
                {
                    println!("  #   {}", line.trim_start_matches([' ', '#']));
                }
            }
            return Ok(());
        }
        let mut message = format!("{name} ended with {status}, wanted {wanted}");
        for extension in ["tap", "err"] {
            if let Ok(text) = fs::read_to_string(self.root.join(format!("{name}.{extension}"))) {
                for line in text
                    .lines()
                    .filter(|line| line.starts_with("not ok") || line.starts_with("  #"))
                    .take(40)
                {
                    message.push('\n');
                    message.push_str(line);
                }
                if extension == "err" {
                    for line in text.lines().rev().take(5) {
                        message.push('\n');
                        message.push_str(line);
                    }
                }
            }
        }
        Err(message)
    }

    fn signal_process(&self, name: &str, signal: &str) -> StepResult {
        let mut processes = self.processes.borrow_mut();
        let child = processes
            .get_mut(name)
            .ok_or_else(|| format!("no process {name}"))?;
        if signal == "KILL" {
            return child.kill().map_err(|error| error.to_string());
        }
        signal_number(signal)?;
        let status = Process::new("kill")
            .arg(format!("-{signal}"))
            .arg(child.id().to_string())
            .status()
            .map_err(|error| error.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("kill -{signal} failed: {status}"))
        }
    }

    /// Starts a private `dbus-daemon` whose only service directory is `$TMP/testbus/services`.
    /// That holds one test service file for the id `<harness id>.Activated`, or none: with
    /// `service` it runs this executable with `--gapplication-service`; with `selftest` it
    /// runs this executable with `--self-test <script>` instead, in `$TMP/activated`, so that
    /// the activated Stet can drive its own window (GTK exports no window actions over D-Bus
    /// under Broadway); with `none` there is nothing to activate. Either Stet gets state,
    /// cache and configuration directories of its own, and its output goes to
    /// `$TMP/testbus/activated.out` and `daemon.log`.
    async fn start_test_bus(&self, mode: &str, script: Option<&str>) -> StepResult {
        if self.bus.borrow().is_some() {
            return Err("the test bus runs already".to_owned());
        }
        let service = match (mode, script) {
            ("service", None) => Some(vec!["--gapplication-service".to_owned()]),
            ("selftest", Some(script)) => Some(vec!["--self-test".to_owned(), script.to_owned()]),
            ("none", None) => None,
            _ => return Err("use test-bus-start service, none or selftest <script>".to_owned()),
        };
        let dir = self.root.join("testbus");
        let services = dir.join("services");
        let home = self.root.join("activated");
        let _ = fs::remove_dir_all(&services);
        fs::create_dir_all(&services).map_err(|error| error.to_string())?;
        // A socket path must be short (108 bytes), so it goes in a directory of its own.
        let socket_dir = std::env::temp_dir().join(format!("stet-bus-{}", std::process::id()));
        fs::create_dir_all(&socket_dir).map_err(|error| error.to_string())?;
        let socket = socket_dir.join("socket");
        let _ = fs::remove_file(&socket);
        let address = format!("unix:path={}", socket.display());
        let app_id = format!("{}.Activated", self.app_id);
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        if let Some(arguments) = service {
            let mut environment: Vec<String> = bus_environment(&address, &app_id, &home)
                .into_iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect();
            if mode == "selftest" {
                environment.push(format!("{ROOT_ENV}={}", home.display()));
            }
            write_file(
                &services.join(format!("{app_id}.service")),
                format!(
                    "[D-BUS Service]\nName={app_id}\nExec=/usr/bin/env {} {} {}\n",
                    environment.join(" "),
                    exe.display(),
                    arguments.join(" ")
                )
                .as_bytes(),
            )?;
        }
        let config = dir.join("bus.conf");
        write_file(
            &config,
            format!(
                "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\"\n \
                 \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
                 <!-- Stet's activation self-test: its one service directory holds only Stet's \
                 own test service file. -->\n\
                 <busconfig>\n  <type>session</type>\n  <keep_umask/>\n  <listen>{address}</listen>\n  \
                 <auth>EXTERNAL</auth>\n  <servicedir>{}</servicedir>\n  \
                 <policy context=\"default\">\n    <allow send_destination=\"*\" eavesdrop=\"true\"/>\n    \
                 <allow eavesdrop=\"true\"/>\n    <allow own=\"*\"/>\n  </policy>\n</busconfig>\n",
                services.display()
            )
            .as_bytes(),
        )?;
        let log = File::create(dir.join("daemon.log")).map_err(|error| error.to_string())?;
        // An activated service inherits the daemon's output.
        let out = File::create(dir.join("activated.out")).map_err(|error| error.to_string())?;
        let daemon = Process::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .arg("--nofork")
            .arg("--nopidfile")
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(log)
            .spawn()
            .map_err(|error| format!("dbus-daemon: {error}"))?;
        let bus = TestBus {
            daemon,
            address,
            app_id,
            home,
            socket_dir,
        };
        let started = Instant::now();
        while !socket.exists() {
            if started.elapsed() > WAIT_TIMEOUT {
                return Err("the test bus did not start".to_owned());
            }
            glib::timeout_future(Duration::from_millis(10)).await;
        }
        self.bus.replace(Some(bus));
        Ok(())
    }

    /// Activates the application action `action` (`app.<name>`) on the Stet that the test
    /// bus started, through `org.gtk.Actions`, as another program could. (GTK exports window
    /// actions too, but only on Wayland and X11, not under Broadway.)
    async fn activate_on_bus(&self, action: &str, parameter: Option<&str>) -> StepResult {
        let name = action
            .strip_prefix("app.")
            .ok_or_else(|| format!("{action}: use app.<name>"))?
            .to_owned();
        let (address, app_id, path) = {
            let bus = self.bus.borrow();
            let bus = bus.as_ref().ok_or("run test-bus-start first")?;
            (bus.address.clone(), bus.app_id.clone(), bus.object_path())
        };
        let parameter = parameter.map(str::to_owned);
        worker::run(move || -> StepResult {
            let connection = connect(&address)?;
            let parameters: Vec<glib::Variant> =
                parameter.iter().map(ToVariant::to_variant).collect();
            let platform: HashMap<String, glib::Variant> = HashMap::new();
            connection
                .call_sync(
                    Some(&app_id),
                    &path,
                    "org.gtk.Actions",
                    "Activate",
                    Some(&(name, parameters, platform).to_variant()),
                    None,
                    gio::DBusCallFlags::NONE,
                    5000,
                    gio::Cancellable::NONE,
                )
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
        .await
        .ok_or("the worker failed")?
    }

    /// Whether the activated id has an owner on the test bus.
    async fn bus_owned(&self) -> Result<bool, String> {
        let (address, app_id) = {
            let bus = self.bus.borrow();
            let bus = bus.as_ref().ok_or("run test-bus-start first")?;
            (bus.address.clone(), bus.app_id.clone())
        };
        worker::run(move || connect(&address).map(|connection| has_owner(&connection, &app_id)))
            .await
            .ok_or("the worker failed")?
    }

    /// Stops the test bus; a Stet it started that still runs is stopped first.
    async fn stop_test_bus(&self) -> StepResult {
        let Some(bus) = self.bus.take() else {
            return Ok(());
        };
        let address = bus.address.clone();
        let app_id = bus.app_id.clone();
        let leftover = worker::run(move || {
            let connection = connect(&address).ok()?;
            let reply = connection
                .call_sync(
                    Some("org.freedesktop.DBus"),
                    "/org/freedesktop/DBus",
                    "org.freedesktop.DBus",
                    "GetConnectionUnixProcessID",
                    Some(&(app_id,).to_variant()),
                    None,
                    gio::DBusCallFlags::NONE,
                    2000,
                    gio::Cancellable::NONE,
                )
                .ok()?;
            reply.child_value(0).get::<u32>()
        })
        .await
        .flatten();
        if let Some(pid) = leftover {
            println!("  # stopping the activated Stet (process {pid}) that still runs");
            let _ = Process::new("kill").arg(pid.to_string()).status();
        }
        drop(bus);
        match leftover {
            Some(pid) => Err(format!(
                "the activated Stet (process {pid}) was still running"
            )),
            None => Ok(()),
        }
    }

    /// Stops what the script left running: background processes, child self-tests and the
    /// test bus.
    pub(super) async fn stop_processes(&self) {
        let mut processes = std::mem::take(&mut *self.processes.borrow_mut());
        for (name, child) in &mut processes {
            if matches!(child.try_wait(), Ok(None)) {
                println!("  # stopping {name}, which still runs");
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        let _ = self.stop_test_bus().await;
    }

    /// `report-session`: prints the last commit's texts, bytes and GTK-thread time and the
    /// store's write times.
    fn report_session(&self, _: &[String]) -> StepResult {
        let stats = self.window.session_stats();
        let ms = |duration: Duration| duration.as_secs_f64() * 1e3;
        println!(
            "  # last commit: {} texts, {} bytes, taken in {:.1} ms on the GTK thread (longest \
             {:.1} ms); the whole commit built in {:.1} ms; {} commits sent",
            stats.texts,
            stats.bytes,
            ms(stats.text_time),
            ms(stats.longest_text),
            ms(stats.build_time),
            stats.commits
        );
        let status = self.window.store_status().ok_or("the session is off")?;
        if let Some(commit) = status.last_commit {
            println!(
                "  # store: {} writes; the last folded {} snapshots: {} backups, {} bytes; \
                 backups {:.1} ms, manifest {:.1} ms, total {:.1} ms",
                status.commits,
                commit.snapshots,
                commit.backups,
                commit.backup_bytes,
                ms(commit.backup_time),
                ms(commit.manifest_time),
                ms(commit.total_time)
            );
        }
        match status.error {
            Some(error) => Err(format!("the last write failed: {error}")),
            None => Ok(()),
        }
    }

    /// The steps start as soon as the window is presented; its first frame comes a little
    /// later.
    async fn wait_first_frame(&self) -> StepResult {
        let started = Instant::now();
        while self.state.first_frame().is_none() {
            if started.elapsed() > WAIT_TIMEOUT {
                return Err("no first frame was painted".to_owned());
            }
            glib::timeout_future(Duration::from_millis(2)).await;
        }
        Ok(())
    }

    fn print_restore(&self) -> StepResult {
        let stats = self
            .window
            .restore_stats()
            .ok_or("no session was restored")?;
        let since = |at: Instant| (at - self.started).as_secs_f64() * 1e3;
        let frame = self
            .state
            .first_frame()
            .map_or_else(|| "-".to_owned(), |at| format!("{:.1} ms", since(at)));
        println!(
            "  # restore: {} tabs, {} orphans; from the process start: session read {:.1} ms, \
             tabs in the strip {:.1} ms, first frame {frame}",
            stats.tabs,
            stats.orphans,
            since(stats.read),
            since(stats.built)
        );
        Ok(())
    }

    /// From the process start to the first frame, within the limit in `args` for this build.
    fn check_restore_time(&self, args: &[String]) -> StepResult {
        let first = self
            .state
            .first_frame()
            .ok_or("no first frame was recorded")?;
        let elapsed = first - self.started;
        let debug = cfg!(debug_assertions) && args.len() > 1;
        let limit = Duration::from_millis(number(&args[usize::from(debug)])? as u64);
        println!(
            "  # restored window's first frame {:.1} ms after the process started ({} build, \
             limit {limit:?})",
            elapsed.as_secs_f64() * 1e3,
            build()
        );
        if self
            .window
            .current_page()
            .is_some_and(|page| Window::is_stub(&page))
        {
            return Err("the active tab was still a stub".to_owned());
        }
        if elapsed <= limit {
            Ok(())
        } else {
            Err(format!("the first frame took {elapsed:?}, over {limit:?}"))
        }
    }
}

/// The environment of a `stet` on the test bus: the bus, the activated id, and directories
/// of its own under `home`, so it never touches the user's session.
fn bus_environment(address: &str, app_id: &str, home: &Path) -> Vec<(&'static str, String)> {
    let dir = |name: &str| home.join(name).to_string_lossy().into_owned();
    vec![
        ("DBUS_SESSION_BUS_ADDRESS", address.to_owned()),
        (APP_ID_ENV, app_id.to_owned()),
        ("XDG_STATE_HOME", dir("state")),
        ("XDG_CACHE_HOME", dir("cache")),
        ("XDG_CONFIG_HOME", dir("config")),
    ]
}

fn connect(address: &str) -> Result<gio::DBusConnection, String> {
    gio::DBusConnection::for_address_sync(
        address,
        gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
            | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
        None,
        gio::Cancellable::NONE,
    )
    .map_err(|error| error.to_string())
}

fn has_owner(connection: &gio::DBusConnection, name: &str) -> bool {
    connection
        .call_sync(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "NameHasOwner",
            Some(&(name,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            2000,
            gio::Cancellable::NONE,
        )
        .ok()
        .and_then(|reply| reply.child_value(0).get::<bool>())
        .unwrap_or(false)
}

fn signal_number(name: &str) -> Result<i32, String> {
    match name {
        "TERM" => Ok(15),
        "HUP" => Ok(1),
        "INT" => Ok(2),
        "KILL" => Ok(9),
        other => Err(format!("no signal {other}; use TERM, HUP, INT or KILL")),
    }
}

fn status_matches(status: ExitStatus, wanted: &str) -> bool {
    match wanted {
        "any" => true,
        "killed" => status.signal() == Some(9),
        "signal" => status.signal().is_some(),
        code => code
            .parse::<i32>()
            .is_ok_and(|code| status.code() == Some(code)),
    }
}

fn count_files(dir: &Path) -> Result<usize, String> {
    Ok(fs::read_dir(dir)
        .map_err(|error| format!("{}: {error}", dir.display()))?
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .count())
}

fn read_session(state: &Path) -> Result<Session, String> {
    let path = state.join("session.json");
    let text = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))
}

/// Whether the `session.json` in `state` has `text`, as `wanted`.
fn session_has(state: &str, text: &str, wanted: bool) -> StepResult {
    let session = fs::read_to_string(Path::new(state).join("session.json"))
        .map_err(|error| error.to_string())?;
    if session.contains(text) == wanted {
        Ok(())
    } else if wanted {
        Err(format!("session.json lacks {text:?}:\n{session}"))
    } else {
        Err(format!("session.json has {text:?}:\n{session}"))
    }
}
