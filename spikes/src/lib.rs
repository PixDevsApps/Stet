//! Shared helpers for the M0 spike binaries in `src/bin/`.
//! Run them headless: `tools/headless.sh <display> cargo run -p stet-spikes --bin <spike>`.

use anyhow::{Context, Result, bail};
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::Cell;
use std::fmt::Write as _;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

const WAKE_INTERVAL: Duration = Duration::from_millis(10);

/// Refuses to run unless GTK targets Broadway, so a spike never opens a window on the live display.
/// `STET_SPIKE_ALLOW_LIVE=1` overrides this for an explicitly authorized live run.
pub fn require_headless() -> Result<()> {
    let broadway = std::env::var_os("GDK_BACKEND").is_some_and(|backend| backend == "broadway");
    let live_allowed = std::env::var_os("STET_SPIKE_ALLOW_LIVE").is_some_and(|value| value == "1");
    if broadway || live_allowed {
        Ok(())
    } else {
        bail!(
            "refusing to open windows on the live display; run through \
             tools/headless.sh <display> <cmd...> or set STET_SPIKE_ALLOW_LIVE=1"
        )
    }
}

/// Prints one JSON object per measurement to stdout.
pub struct Report {
    name: String,
}

impl Report {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    pub fn record(&self, key: &str, value: impl Into<serde_json::Value>) {
        let line = serde_json::json!({ "spike": self.name, "key": key, "value": value.into() });
        println!("{line}");
    }
}

/// One line of a spike result table.
pub struct Row {
    pub measure: String,
    pub result: String,
    pub pass_bar: String,
    pub passed: Option<bool>,
}

impl Row {
    pub fn new(
        measure: impl Into<String>,
        result: impl Into<String>,
        pass_bar: impl Into<String>,
        passed: Option<bool>,
    ) -> Self {
        Self {
            measure: measure.into(),
            result: result.into(),
            pass_bar: pass_bar.into(),
            passed,
        }
    }
}

/// Writes a Markdown section with a result table and caveats, creating parent directories.
pub fn write_markdown(path: &Path, title: &str, rows: &[Row], caveats: &[&str]) -> Result<()> {
    let mut markdown =
        format!("## {title}\n\n| Measure | Result | Pass bar | Verdict |\n|---|---|---|---|\n");
    for row in rows {
        let verdict = match row.passed {
            Some(true) => "pass",
            Some(false) => "FAIL",
            None => "not judged",
        };
        writeln!(
            markdown,
            "| {} | {} | {} | {verdict} |",
            cell(&row.measure),
            cell(&row.result),
            cell(&row.pass_bar)
        )?;
    }
    if !caveats.is_empty() {
        markdown.push_str("\n**Caveats:**\n\n");
        for caveat in caveats {
            writeln!(markdown, "- {caveat}")?;
        }
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(path, markdown).with_context(|| format!("writing {}", path.display()))
}

fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

/// Current resident set size of this process, from `/proc/self/status`.
pub fn rss_kib() -> Result<u64> {
    status_kib("VmRSS")
}

/// Peak resident set size of this process, from `/proc/self/status`.
pub fn peak_rss_kib() -> Result<u64> {
    status_kib("VmHWM")
}

fn status_kib(field: &str) -> Result<u64> {
    let status =
        std::fs::read_to_string("/proc/self/status").context("reading /proc/self/status")?;
    parse_status_kib(&status, field).with_context(|| format!("no {field} in /proc/self/status"))
}

fn parse_status_kib(status: &str, field: &str) -> Option<u64> {
    status.lines().find_map(|line| {
        let value = line.strip_prefix(field)?.strip_prefix(':')?;
        value.trim().strip_suffix("kB")?.trim().parse().ok()
    })
}

/// Iterates the default main context until `done` returns true, and returns the time taken.
/// Fails once `timeout` has passed. Must run on the thread that owns the default context.
pub fn run_main_loop_until(timeout: Duration, done: impl Fn() -> bool) -> Result<Duration> {
    let context = glib::MainContext::default();
    let started = Instant::now();
    let wake = glib::timeout_add_local(WAKE_INTERVAL, || glib::ControlFlow::Continue);
    let result = loop {
        if done() {
            break Ok(started.elapsed());
        }
        if started.elapsed() >= timeout {
            break Err(anyhow::anyhow!("timed out after {timeout:?}"));
        }
        context.iteration(true);
    };
    wake.remove();
    result
}

/// Keeps the widget's frame clock running until it has painted `frames` more times.
/// The widget must be realized. Returns the time taken.
pub fn wait_frames(
    widget: &impl IsA<gtk4::Widget>,
    frames: u32,
    timeout: Duration,
) -> Result<Duration> {
    let clock = widget
        .frame_clock()
        .context("widget has no frame clock; realize it first")?;
    let painted = Rc::new(Cell::new(0));
    let handler = clock.connect_after_paint(glib::clone!(
        #[strong]
        painted,
        move |_| painted.set(painted.get() + 1)
    ));
    clock.begin_updating();
    widget.queue_draw();
    let result = run_main_loop_until(timeout, || painted.get() >= frames);
    clock.end_updating();
    clock.disconnect(handler);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status_fields() {
        let status = "Name:\tstet\nVmHWM:\t  2048 kB\nVmRSS:\t  1024 kB\n";
        assert_eq!(parse_status_kib(status, "VmRSS"), Some(1024));
        assert_eq!(parse_status_kib(status, "VmHWM"), Some(2048));
        assert_eq!(parse_status_kib(status, "VmSwap"), None);
    }

    #[test]
    fn reads_this_process_rss() {
        let rss = rss_kib().unwrap();
        assert!(rss > 0);
        assert!(peak_rss_kib().unwrap() >= rss);
    }

    #[test]
    fn main_loop_runs_until_done_or_timeout() {
        let fired = Rc::new(Cell::new(false));
        glib::timeout_add_local_once(
            Duration::from_millis(20),
            glib::clone!(
                #[strong]
                fired,
                move || fired.set(true)
            ),
        );
        let elapsed = run_main_loop_until(Duration::from_secs(5), || fired.get()).unwrap();
        assert!(elapsed >= Duration::from_millis(20));
        assert!(run_main_loop_until(Duration::from_millis(30), || false).is_err());
    }

    #[test]
    fn markdown_escapes_cells_and_lists_caveats() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/report.md");
        let rows = [Row::new("a|b", "12 ms", "< 16 ms", Some(true))];
        write_markdown(&path, "S1", &rows, &["broadway, not Wayland"]).unwrap();
        let written = std::fs::read_to_string(path).unwrap();
        assert!(written.starts_with("## S1\n"));
        assert!(written.contains("| a\\|b | 12 ms | < 16 ms | pass |"));
        assert!(written.contains("- broadway, not Wayland"));
    }

    #[test]
    fn headless_guard_reads_gdk_backend() {
        let expected = std::env::var_os("GDK_BACKEND").is_some_and(|b| b == "broadway")
            || std::env::var_os("STET_SPIKE_ALLOW_LIVE").is_some_and(|v| v == "1");
        assert_eq!(require_headless().is_ok(), expected);
    }
}
