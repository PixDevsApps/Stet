//! Self-test commands for M4's search: the find bar's options and fields, the results panel,
//! Find in Files, the widget parity table, and the performance probes (docs/TESTING.md).

use super::script::{Step, boolean};
use super::{Harness, StepResult, WAIT_TIMEOUT, equal, number, write_file};
use crate::page_search::OWN_MATCH_TAG;
use crate::window::Field;
use crate::worker;
use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;
use sourceview5::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};
use stet_domain::actions::ActionId;
use stet_domain::search::{History, SearchHistory, SearchMode, translate};
use stet_infrastructure::search::{Matcher, parity};

thread_local! {
    static CHECKPOINTS: RefCell<HashMap<String, u64>> = RefCell::new(HashMap::new());
    /// The main-loop heartbeat from `fif-start` to `wait-fif`, and what it measured.
    static FIF_HEARTBEAT: RefCell<Option<Heartbeat>> = const { RefCell::new(None) };
    static FIF_STALL: Cell<Option<Duration>> = const { Cell::new(None) };
}

/// Measures the longest stretch the main loop went without running a 2 ms timeout: how long
/// the GTK thread was blocked.
struct Heartbeat {
    longest: Rc<Cell<Duration>>,
    source: glib::SourceId,
}

impl Heartbeat {
    fn start() -> Self {
        let longest = Rc::new(Cell::new(Duration::ZERO));
        let last = Cell::new(Instant::now());
        let max = Rc::clone(&longest);
        let source = glib::timeout_add_local(Duration::from_millis(2), move || {
            let now = Instant::now();
            let gap = now - last.get();
            if gap > max.get() {
                max.set(gap);
            }
            last.set(now);
            glib::ControlFlow::Continue
        });
        Self { longest, source }
    }

    /// The longest gap so far; the next measurement starts from zero.
    fn take(&self) -> Duration {
        self.longest.replace(Duration::ZERO)
    }

    fn stop(self) -> Duration {
        self.source.remove();
        self.longest.get()
    }
}

fn ms(duration: Duration) -> String {
    format!("{:.1} ms", duration.as_secs_f64() * 1e3)
}

/// `VmRSS` and `VmHWM` of this process, in MiB.
fn memory() -> (f64, f64) {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |name: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|rest| {
                rest.trim()
                    .trim_end_matches("kB")
                    .trim()
                    .parse::<f64>()
                    .ok()
            })
            .map_or(0.0, |kib| kib / 1024.0)
    };
    (field("VmRSS:"), field("VmHWM:"))
}

fn text_hash(text: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn field(name: &str) -> Result<Field, String> {
    match name {
        "find" => Ok(Field::Find),
        "replace" => Ok(Field::Replace),
        "directory" => Ok(Field::Directory),
        "filters" => Ok(Field::Filters),
        other => Err(format!(
            "no history {other}; use find, replace, directory or filters"
        )),
    }
}

fn history_list(history: &SearchHistory, field: Field) -> &History {
    match field {
        Field::Find => &history.find,
        Field::Replace => &history.replace,
        Field::Directory => &history.directories,
        Field::Filters => &history.filters,
    }
}

impl Harness {
    /// Runs a search command; `None` when `step` is not one.
    pub(super) async fn search_step(&self, step: &Step) -> Option<StepResult> {
        let args = &step.args;
        let arg = |index: usize| args[index].as_str();
        let find = &self.window.inner.find;
        Some(match step.command.as_str() {
            // Opens the bar unless it is open (keeping its mode), and waits until the text is
            // compiled, so the steps after it see the new query.
            "find" => {
                if !find.is_open() {
                    self.window.show_find();
                }
                find.entry.set_text(arg(0));
                let started = Instant::now();
                while find.compiled_text() != arg(0) {
                    if started.elapsed() > WAIT_TIMEOUT {
                        return Some(Err("the find field was never searched".to_owned()));
                    }
                    glib::timeout_future(Duration::from_millis(5)).await;
                }
                self.wait_idle().await
            }
            "assert-clipboard-contains" => {
                let clipboard = self.window.window().clipboard();
                match clipboard.read_text_future().await {
                    Ok(Some(text)) if text.contains(arg(0)) => Ok(()),
                    Ok(text) => Err(format!("the clipboard holds {text:?}, not {:?}", arg(0))),
                    Err(error) => Err(format!("could not read the clipboard: {error}")),
                }
            }
            "find-mode" => {
                let mode = match arg(0) {
                    "normal" => SearchMode::Normal,
                    "extended" => SearchMode::Extended,
                    "regex" => SearchMode::Regex,
                    other => {
                        return Some(Err(format!(
                            "no mode {other}; use normal, extended or regex"
                        )));
                    }
                };
                find.set_search_mode(mode);
                Ok(())
            }
            "find-option" => self.set_option(arg(0), arg(1)),
            "replace-text" => {
                find.replace_entry.set_text(arg(0));
                Ok(())
            }
            "fif-folder" => {
                find.directory.set_text(arg(0));
                Ok(())
            }
            "fif-filters" => {
                find.filters.set_text(arg(0));
                Ok(())
            }
            "select" => self.select(args),
            "fif-start" => {
                if let Some(old) = FIF_HEARTBEAT.with(|beat| beat.replace(Some(Heartbeat::start())))
                {
                    old.stop();
                }
                self.fif_start().await
            }
            "wait-fif" => {
                let limit = match args.first().map(|seconds| number(seconds)) {
                    Some(Ok(seconds)) => Duration::from_secs(seconds as u64),
                    Some(Err(error)) => return Some(Err(error)),
                    None => WAIT_TIMEOUT,
                };
                let started = Instant::now();
                let waited = loop {
                    if !self.window.find_in_files_running() {
                        break Ok(());
                    }
                    if started.elapsed() > limit {
                        break Err(format!("Find in Files still runs after {limit:?}"));
                    }
                    glib::timeout_future(Duration::from_millis(5)).await;
                };
                let stall = FIF_HEARTBEAT.with(|beat| beat.take().map(Heartbeat::stop));
                FIF_STALL.with(|cell| cell.set(stall));
                waited
            }
            "fif-stop" => {
                self.window.run_search_action(ActionId::StopFindInFiles);
                Ok(())
            }
            "report-fif" => self.report_fif(),
            "results-activate" => {
                let results = &self.window.inner.results;
                let position = match number(arg(0)) {
                    Ok(position) => position as u32,
                    Err(error) => return Some(Err(error)),
                };
                if results.select_position(position) && results.activate_selected() {
                    self.wait_idle().await
                } else {
                    Err(format!("the results panel has no row {position}"))
                }
            }
            "set-history" => self.set_history(arg(0), arg(1)),
            "pick-history" => {
                let button = match arg(0) {
                    "find" => &find.find_history,
                    "replace" => &find.replace_history,
                    other => return Some(Err(format!("no history drop-down {other}"))),
                };
                match number(arg(1)) {
                    Ok(index) if button.pick(index as i32) => self.wait_idle().await,
                    Ok(index) => Err(format!("the drop-down has no entry {index}")),
                    Err(error) => Err(error),
                }
            }
            "generate-tree" => self.generate_tree(args).await,
            "wait-idle-low" => {
                let seconds = args.first().map_or(Ok(60), |seconds| number(seconds));
                match seconds {
                    Ok(seconds) => wait_low_idle(Duration::from_secs(seconds as u64)).await,
                    Err(error) => Err(error),
                }
            }
            "text-checkpoint" => self.page().map(|page| {
                let hash = text_hash(&page.text());
                CHECKPOINTS.with(|checkpoints| {
                    checkpoints.borrow_mut().insert(arg(0).to_owned(), hash);
                });
            }),
            "bench-replace-all" => self.bench_replace_all(args).await,
            "bench-undo" => self.bench_undo().await,
            "assert-widget-parity" => self.widget_parity().await,
            _ => return None,
        })
    }

    /// Checks a search assertion; `None` when `assertion` is not one.
    pub(super) fn search_check(&self, assertion: &str, args: &[String]) -> Option<StepResult> {
        let arg = |index: usize| args[index].as_str();
        let find = &self.window.inner.find;
        let results = &self.window.inner.results;
        Some(match assertion {
            // Whether GtkSourceView highlights every match in the current tab.
            "assert-search-highlight" => match (self.page(), boolean(arg(0))) {
                (Ok(page), Ok(wanted)) => equal(
                    "highlight all",
                    page.search()
                        .existing_context()
                        .is_some_and(|context| context.is_highlight()),
                    wanted,
                ),
                (Err(error), _) | (_, Err(error)) => Err(error),
            },
            "assert-find-message" => equal(
                "find bar message",
                find.message.label().to_string(),
                arg(0).to_owned(),
            ),
            "assert-replace-hint" => {
                let entry = &find.replace_entry;
                let tooltip = entry.tooltip_text().map(|text| text.to_string());
                let styled = entry.has_css_class("warning") || entry.has_css_class("error");
                match (arg(0), tooltip) {
                    ("none", None) if !styled => Ok(()),
                    ("none", tooltip) => Err(format!("the replace field says {tooltip:?}")),
                    (wanted, Some(text)) if styled && text.contains(wanted) => Ok(()),
                    (wanted, tooltip) => Err(format!(
                        "the replace field says {tooltip:?} (styled {styled}), not {wanted:?}"
                    )),
                }
            }
            "assert-field" => {
                let text = match arg(0) {
                    "find" => find.entry.text(),
                    "replace" => find.replace_entry.text(),
                    "directory" => find.directory.text(),
                    "filters" => find.filters.text(),
                    other => return Some(Err(format!("no field {other}"))),
                };
                equal("field", text.to_string(), arg(1).to_owned())
            }
            "assert-find-error" => self.check_error(args),
            "assert-routed" => match (find.compiled(), boolean(arg(0))) {
                (Some(compiled), Ok(wanted)) => {
                    equal("routed to our matcher", compiled.own(), wanted)
                }
                (None, _) => Err("the find field holds no valid query".to_owned()),
                (_, Err(error)) => Err(error),
            },
            "assert-own-matches" => self.check_own_matches(arg(0)),
            "assert-own-tags" => self.page().and_then(|page| {
                let buffer = page.buffer();
                let tag = buffer
                    .tag_table()
                    .lookup(OWN_MATCH_TAG)
                    .ok_or_else(|| "no own-match tag".to_owned())?;
                let mut iter = buffer.start_iter();
                let mut ranges = 0;
                loop {
                    if iter.starts_tag(Some(&tag)) {
                        ranges += 1;
                    }
                    if !iter.forward_to_tag_toggle(Some(&tag)) {
                        break;
                    }
                }
                equal("highlighted matches", ranges, number(arg(0))?)
            }),
            "assert-smart-highlight" => self.page().and_then(|page| {
                let search = page.search();
                let count = search
                    .smart_context()
                    .map_or(0, |context| context.occurrences_count());
                let wanted = arg(0)
                    .parse::<i32>()
                    .map_err(|_| format!("expected a number, got {}", arg(0)))?;
                equal("smart highlights", count, wanted)
                    .map_err(|error| format!("{error} (pattern {:?})", search.smart_pattern()))
            }),
            "assert-bar-mode" => {
                let mode = match find.bar_mode() {
                    crate::window::BarMode::Find => "find",
                    crate::window::BarMode::Replace => "replace",
                    crate::window::BarMode::Files => "files",
                    crate::window::BarMode::Mark => "mark",
                };
                equal("bar mode", mode.to_owned(), arg(0).to_owned())
            }
            "assert-can-undo" => self
                .page()
                .and_then(|page| equal("can undo", page.buffer().can_undo(), boolean(arg(0))?)),
            "assert-can-redo" => self
                .page()
                .and_then(|page| equal("can redo", page.buffer().can_redo(), boolean(arg(0))?)),
            "assert-results-heading" => equal(
                "newest results heading",
                results
                    .latest()
                    .map(|search| search.heading())
                    .unwrap_or_default(),
                arg(0).to_owned(),
            ),
            "assert-results-contains" => {
                let text = results.text();
                if text.contains(arg(0)) {
                    Ok(())
                } else {
                    Err(format!("results {text:?} lack {:?}", arg(0)))
                }
            }
            "assert-results-status" => {
                let status = results.status.label().to_string();
                if status.contains(arg(0)) {
                    Ok(())
                } else {
                    Err(format!("results status {status:?} lacks {:?}", arg(0)))
                }
            }
            "assert-results-shown" => boolean(arg(0))
                .and_then(|wanted| equal("results shown", results.is_shown(), wanted)),
            "assert-results-focused" => boolean(arg(0))
                .and_then(|wanted| equal("results focused", self.window.results_focused(), wanted)),
            "assert-results-searches" => number(arg(0)).and_then(|wanted| {
                equal(
                    "searches in the results panel",
                    results.search_count() as usize,
                    wanted,
                )
            }),
            "assert-fif-running" => boolean(arg(0)).and_then(|wanted| {
                equal(
                    "Find in Files running",
                    self.window.find_in_files_running(),
                    wanted,
                )
            }),
            "assert-fif" => self.check_fif(arg(0), arg(1)),
            "assert-history" => field(arg(0)).and_then(|field| {
                let history = self.window.search_history();
                equal(
                    "history",
                    history_list(&history, field).entries().join("|"),
                    arg(1).to_owned(),
                )
            }),
            "assert-replace-stats" => match *find.stats.borrow() {
                None => Err("no Replace All has finished".to_owned()),
                Some(stats) => number(arg(0)).and_then(|count| {
                    equal(
                        "replaced matches and bulk edit",
                        (stats.count, stats.bulk),
                        (count, boolean(arg(1))?),
                    )
                }),
            },
            "assert-text-checkpoint" => self.page().and_then(|page| {
                let wanted = CHECKPOINTS
                    .with(|checkpoints| checkpoints.borrow().get(arg(0)).copied())
                    .ok_or_else(|| format!("no checkpoint {}", arg(0)))?;
                equal("text hash", text_hash(&page.text()), wanted)
            }),
            _ => return None,
        })
    }

    fn set_option(&self, name: &str, value: &str) -> StepResult {
        let value = boolean(value)?;
        let find = &self.window.inner.find;
        let button: gtk::Widget = match name {
            "case" => find.case.clone().upcast(),
            "word" => find.word.clone().upcast(),
            "selection" => find.in_selection.clone().upcast(),
            "wrap" => find.wrap.clone().upcast(),
            "backward" => find.backward.clone().upcast(),
            "dot-newline" => find.dot_newline.clone().upcast(),
            "subfolders" => find.subfolders.clone().upcast(),
            "hidden" => find.hidden.clone().upcast(),
            "gitignore" => find.gitignore.clone().upcast(),
            other => return Err(format!("no option {other}")),
        };
        if let Some(toggle) = button.downcast_ref::<gtk::ToggleButton>() {
            toggle.set_active(value);
        } else if let Some(check) = button.downcast_ref::<gtk::CheckButton>() {
            check.set_active(value);
        }
        Ok(())
    }

    /// `select l1 c1 l2 c2`: selects from line l1, column c1 to l2, c2 (1-based); the caret
    /// ends at the second position.
    fn select(&self, args: &[String]) -> StepResult {
        let page = self.page()?;
        let buffer = page.buffer();
        let iter = |line: &str, column: &str| -> Result<gtk::TextIter, String> {
            let (line, column) = (number(line)?, number(column)?);
            let mut iter = buffer
                .iter_at_line(line as i32 - 1)
                .ok_or_else(|| format!("there is no line {line}"))?;
            for _ in 1..column {
                if iter.ends_line() {
                    break;
                }
                iter.forward_char();
            }
            Ok(iter)
        };
        let start = iter(&args[0], &args[1])?;
        let end = iter(&args[2], &args[3])?;
        buffer.select_range(&end, &start);
        Ok(())
    }

    fn check_error(&self, args: &[String]) -> StepResult {
        let error = self.window.inner.find.pattern_error();
        match (args, error) {
            ([none], None) if none == "none" => Ok(()),
            ([none], Some(error)) if none == "none" => Err(format!("pattern error {error}")),
            ([offset, text], Some(error)) => {
                let marked = self.window.inner.find.entry.has_css_class("error");
                equal("error style on the find field", marked, true)?;
                equal("error offset", error.offset, number(offset)?)?;
                if error.message.contains(text.as_str()) {
                    Ok(())
                } else {
                    Err(format!("error {:?} lacks {text:?}", error.message))
                }
            }
            (_, None) => Err("no pattern error".to_owned()),
            _ => Err("use assert-find-error none, or an offset and a text".to_owned()),
        }
    }

    fn check_own_matches(&self, wanted: &str) -> StepResult {
        let page = self.page()?;
        let compiled = self
            .window
            .inner
            .find
            .compiled()
            .ok_or_else(|| "the find field holds no valid query".to_owned())?;
        let count = page
            .search()
            .own_matches(compiled.id, page.revision())
            .map(|(matches, _)| matches.len())
            .ok_or_else(|| "our matcher has no matches for this text yet".to_owned())?;
        equal("our matcher's matches", count, number(wanted)?)
    }

    fn check_fif(&self, name: &str, value: &str) -> StepResult {
        let stats = self
            .window
            .find_in_files_stats()
            .ok_or_else(|| "no Find in Files has finished".to_owned())?;
        let summary = &stats.summary;
        match name {
            "searched" => equal("files searched", summary.files_searched, number(value)?),
            "matched" => equal("files matched", summary.files_matched, number(value)?),
            "hits" => equal("hits", summary.hits, number(value)?),
            "cancelled" => equal("cancelled", summary.cancelled, boolean(value)?),
            "truncated" => equal("truncated", summary.truncated, boolean(value)?),
            "errors" => equal("errors", summary.errors.len(), number(value)?),
            "searched-below" => {
                if summary.files_searched < number(value)? {
                    Ok(())
                } else {
                    Err(format!(
                        "{} files were searched, not fewer than {value}",
                        summary.files_searched
                    ))
                }
            }
            other => Err(format!("no Find in Files field {other}")),
        }
    }

    fn report_fif(&self) -> StepResult {
        let stats = self
            .window
            .find_in_files_stats()
            .ok_or_else(|| "no Find in Files has finished".to_owned())?;
        let summary = &stats.summary;
        let stopped = match (stats.search_ended_after_stop, stats.stopped_after) {
            (Some(ended), Some(done)) => format!(
                ", search ended {} and panel done {} after Stop",
                ms(ended),
                ms(done)
            ),
            _ => String::new(),
        };
        let stall = FIF_STALL.with(Cell::take);
        println!(
            "  # find in files: first file found {}, first result shown {}, done {}{stopped}; {} files searched, {} matched, {} hits, truncated {}, cancelled {}, {} errors; slowest drain {}, longest main-loop stall {}",
            stats.first_found.map_or_else(|| "none".to_owned(), ms),
            stats.first_result.map_or_else(|| "none".to_owned(), ms),
            ms(stats.total),
            summary.files_searched,
            summary.files_matched,
            summary.hits,
            summary.truncated,
            summary.cancelled,
            summary.errors.len(),
            ms(stats.slowest_drain),
            stall.map_or_else(|| "not measured".to_owned(), ms),
        );
        Ok(())
    }

    /// Starts Find in Files with the bar's files row (opening it if needed) and waits until
    /// the search runs, has ended, or failed to start with a message in the bar.
    async fn fif_start(&self) -> StepResult {
        let window = &self.window;
        let find = &window.inner.find;
        if find.bar_mode() != crate::window::BarMode::Files || !find.is_open() {
            window.run_search_action(ActionId::FindInFiles);
        }
        find.set_message("", crate::window::Tone::Info);
        find.find_in_files.emit_clicked();
        let started = Instant::now();
        while !window.find_in_files_running()
            && window.find_in_files_stats().is_none()
            && !find.message.has_css_class("error")
        {
            if started.elapsed() > WAIT_TIMEOUT {
                return Err(format!(
                    "Find in Files did not start: {}",
                    find.message.label()
                ));
            }
            glib::timeout_future(Duration::from_millis(1)).await;
        }
        Ok(())
    }

    fn set_history(&self, name: &str, entries: &str) -> StepResult {
        let field = field(name)?;
        let mut history = self.window.search_history();
        let list = History::from_entries(
            entries
                .split('|')
                .filter(|entry| !entry.is_empty())
                .map(str::to_owned),
            stet_domain::search::history::HISTORY_LIMIT,
        );
        match field {
            Field::Find => history.find = list,
            Field::Replace => history.replace = list,
            Field::Directory => history.directories = list,
            Field::Filters => history.filters = list,
        }
        self.window.set_search_history(history);
        Ok(())
    }

    /// `generate-tree <dir> <files> <lines>`: text files spread over 20 folders.
    async fn generate_tree(&self, args: &[String]) -> StepResult {
        let dir = PathBuf::from(&args[0]);
        let files = number(&args[1])?;
        let lines = number(&args[2])?;
        worker::run(move || -> StepResult {
            let line =
                "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor\n";
            let contents = line.repeat(lines);
            for index in 0..files {
                write_file(
                    &dir.join(format!("d{}/f{index}.txt", index % 20)),
                    contents.as_bytes(),
                )?;
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|| Err("the worker failed".to_owned()))
    }

    /// Replace All with the bar's fields, reporting the cost of every phase and how long the
    /// main loop was blocked.
    async fn bench_replace_all(&self, args: &[String]) -> StepResult {
        let find = &self.window.inner.find;
        find.stats.replace(None);
        let (rss_before, _) = memory();
        let heartbeat = Heartbeat::start();
        let started = Instant::now();
        self.window.run_search_action(ActionId::ReplaceAll);
        let limit = Duration::from_secs(600);
        while find.stats.borrow().is_none() {
            if started.elapsed() > limit {
                heartbeat.stop();
                return Err(format!(
                    "no result after {limit:?}: {}",
                    find.message.label()
                ));
            }
            glib::timeout_future(Duration::from_millis(2)).await;
        }
        let seen = started.elapsed();
        let stall = heartbeat.take();
        let (rss_after, peak) = memory();
        let stats = find.stats.borrow().expect("checked above");
        // GtkSourceView then rescans and GtkTextView revalidates in idle callbacks.
        let settled = wait_low_idle(limit).await;
        let idle = started.elapsed();
        let stall_after = heartbeat.stop();
        settled?;
        println!(
            "  # replace all: {} matches, {} edits{}; snapshot {}, search on a worker {}, apply {}, total {} (seen by the script after {}); longest main-loop stall until applied {}; idle again after {}, longest stall in between {}; RSS {:.0} -> {:.0} MiB (peak {:.0})",
            stats.count,
            stats.edits,
            if stats.bulk { " as one bulk edit" } else { "" },
            ms(stats.snapshot),
            ms(stats.search),
            ms(stats.apply),
            ms(stats.total),
            ms(seen),
            ms(stall),
            ms(idle),
            ms(stall_after),
            rss_before,
            rss_after,
            peak,
        );
        if let Some(wanted) = args.first() {
            equal("replaced matches", stats.count, number(wanted)?)?;
        }
        Ok(())
    }

    /// Undo, timed: the call itself, and until the main loop is idle again.
    async fn bench_undo(&self) -> StepResult {
        let page = self.page()?;
        let buffer = page.buffer();
        if !buffer.can_undo() {
            return Err("nothing to undo".to_owned());
        }
        let (rss_before, _) = memory();
        let heartbeat = Heartbeat::start();
        let started = Instant::now();
        buffer.undo();
        let call = started.elapsed();
        glib::timeout_future(Duration::from_millis(10)).await;
        let stall = heartbeat.take();
        wait_low_idle(Duration::from_secs(600)).await?;
        let settled = started.elapsed();
        let stall_after = heartbeat.stop();
        let (rss_after, peak) = memory();
        println!(
            "  # undo: undo() {}, longest main-loop stall {}; idle again after {}, longest stall in between {}; RSS {:.0} -> {:.0} MiB (peak {:.0}); can undo {}",
            ms(call),
            ms(stall),
            ms(settled),
            ms(stall_after),
            rss_before,
            rss_after,
            peak,
            buffer.can_undo(),
        );
        Ok(())
    }

    /// The M4 parity table against the real GtkSourceView: every row the translation leaves
    /// to the widget must give the same spans and count as our matcher.
    async fn widget_parity(&self) -> StepResult {
        let rows = parity::rows();
        let mut failures = Vec::new();
        let (mut checked, mut routed, mut errors, mut untranslated) = (0, 0, 0, 0);
        for row in &rows {
            if row.routed {
                routed += 1;
                continue;
            }
            let query = row.query();
            let translated = match translate(&query) {
                Ok(translated) => translated,
                Err(_) if row.error_at.is_some() => {
                    untranslated += 1;
                    continue;
                }
                Err(error) => {
                    failures.push(format!("{}: translation failed: {error}", row.name));
                    continue;
                }
            };
            let buffer = sourceview5::Buffer::new(None);
            buffer.set_text(row.input);
            let settings = sourceview5::SearchSettings::new();
            settings.set_regex_enabled(true);
            settings.set_case_sensitive(translated.case_sensitive);
            settings.set_at_word_boundaries(translated.at_word_boundaries);
            settings.set_wrap_around(false);
            settings.set_search_text(Some(&translated.pattern));
            let context = sourceview5::SearchContext::new(&buffer, Some(&settings));
            context.set_highlight(false);
            let started = Instant::now();
            while context.occurrences_count() == -1 && context.regex_error().is_none() {
                if started.elapsed() > Duration::from_secs(5) {
                    break;
                }
                glib::timeout_future(Duration::from_millis(1)).await;
            }
            if row.error_at.is_some() {
                errors += 1;
                if context.regex_error().is_none() {
                    failures.push(format!("{}: the widget accepted the pattern", row.name));
                }
                continue;
            }
            if let Some(error) = context.regex_error() {
                failures.push(format!(
                    "{}: the widget rejected the pattern: {error}",
                    row.name
                ));
                continue;
            }
            let mut widget = Vec::new();
            let mut iter = buffer.start_iter();
            while let Some((start, end, wrapped)) = context.forward(&iter) {
                if wrapped || widget.len() > 10_000 || end.offset() <= iter.offset() {
                    break;
                }
                widget.push((start.offset() as usize, end.offset() as usize));
                iter = end;
            }
            let ours: Vec<(usize, usize)> = match Matcher::new(&query) {
                Ok(matcher) => matcher
                    .find_all(row.input, None, usize::MAX, &AtomicBool::new(false))
                    .map(|found| found.matches.iter().map(|m| (m.start, m.end)).collect())
                    .unwrap_or_default(),
                Err(error) => {
                    failures.push(format!("{}: our matcher rejected it: {error}", row.name));
                    continue;
                }
            };
            let count = context.occurrences_count();
            if widget != ours || count != ours.len() as i32 {
                failures.push(format!(
                    "{}: GtkSourceView found {widget:?} (count {count}), our matcher {ours:?}",
                    row.name
                ));
            }
            checked += 1;
        }
        println!(
            "  # widget parity: {} rows; {checked} compared with GtkSourceView (spans and count), \
             {errors} invalid patterns rejected by both, {untranslated} rejected before reaching it, \
             {routed} routed to our matcher",
            rows.len()
        );
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("\n"))
        }
    }
}

/// Waits until the main loop runs a low-priority idle: GtkTextView's line validation and
/// GtkSourceView's scans have drained.
async fn wait_low_idle(limit: Duration) -> StepResult {
    let (sender, receiver) = async_channel::bounded(1);
    glib::idle_add_local_full(glib::Priority::LOW, move || {
        let _ = sender.try_send(());
        glib::ControlFlow::Break
    });
    let started = Instant::now();
    loop {
        if receiver.try_recv().is_ok() {
            return Ok(());
        }
        if started.elapsed() > limit {
            return Err(format!("the main loop was still busy after {limit:?}"));
        }
        glib::timeout_future(Duration::from_millis(20)).await;
    }
}
