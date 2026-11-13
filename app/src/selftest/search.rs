//! Self-test commands for search: the find bar's options and fields, the results panel,
//! Find in Files, the widget parity table, and the performance probes (docs/TESTING.md).

use super::registry::{Command, commands};
use super::script::boolean;
use super::{Harness, StepResult, WAIT_TIMEOUT, equal, number, write_file};
use crate::convert::{BufferPositions, IterPosition, to_i32, to_u32};
use crate::editor::search::OWN_MATCH_TAG;
use crate::runtime::worker;
use crate::window::{Field, SearchCommand};
use gtk4 as gtk;
use gtk4::glib;
use gtk4::glib::translate::IntoGlib;
use gtk4::prelude::*;
use sourceview5::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};
use stet_domain::search::{History, SearchHistory, SearchMode, translate};
use stet_infrastructure::search::{Matcher, parity};

pub(super) const COMMANDS: &[Command] = commands![
    "find" (1, 1) => async find,
    "find-case" (1, 1) => find_case,
    "key" (2, 2) => key,
    "find-mode" (1, 1) => find_mode,
    "find-option" (2, 2) => find_option,
    "replace-text" (1, 1) => replace_text,
    "fif-folder" (1, 1) => fif_folder,
    "fif-filters" (1, 1) => fif_filters,
    "select" (4, 4) => select,
    "fif-start" (0, 0) => async fif_start,
    "wait-fif" (0, 1) => async wait_fif,
    "fif-stop" (0, 0) => fif_stop,
    "report-fif" (0, 0) => report_fif,
    "results-activate" (1, 1) => async results_activate,
    "set-history" (2, 2) => set_history,
    "pick-history" (2, 2) => async pick_history,
    "generate-tree" (3, 3) => async generate_tree,
    "wait-idle-low" (0, 1) => async wait_idle_low,
    "text-checkpoint" (1, 1) => text_checkpoint,
    "bench-replace-all" (0, 1) => async bench_replace_all,
    "bench-undo" (0, 0) => async bench_undo,
    "assert-widget-parity" (0, 0) => async assert_widget_parity,
    "assert-clipboard-contains" (1, 1) => async assert_clipboard_contains,
    "assert-current-match" (1, 1) => assert_current_match,
    "assert-match-count" (1, 1) => assert_match_count,
    "assert-find-open" (1, 1) => assert_find_open,
    "assert-search-highlight" (1, 1) => assert_search_highlight,
    "assert-find-message" (1, 1) => assert_find_message,
    "assert-replace-hint" (1, 1) => assert_replace_hint,
    "assert-field" (2, 2) => assert_field,
    "assert-find-error" (1, 2) => assert_find_error,
    "assert-routed" (1, 1) => assert_routed,
    "assert-own-matches" (1, 1) => assert_own_matches,
    "assert-own-tags" (1, 1) => assert_own_tags,
    "assert-smart-highlight" (1, 1) => assert_smart_highlight,
    "assert-bar-mode" (1, 1) => assert_bar_mode,
    "assert-can-undo" (1, 1) => assert_can_undo,
    "assert-can-redo" (1, 1) => assert_can_redo,
    "assert-results-heading" (1, 1) => assert_results_heading,
    "assert-results-contains" (1, 1) => assert_results_contains,
    "assert-results-status" (1, 1) => assert_results_status,
    "assert-results-shown" (1, 1) => assert_results_shown,
    "assert-results-focused" (1, 1) => assert_results_focused,
    "assert-results-searches" (1, 1) => assert_results_searches,
    "assert-fif-running" (1, 1) => assert_fif_running,
    "assert-fif" (2, 2) => assert_fif,
    "assert-history" (2, 2) => assert_history,
    "assert-replace-stats" (2, 2) => assert_replace_stats,
    "assert-text-checkpoint" (1, 1) => assert_text_checkpoint,
];

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
    /// `find <text>`: opens the find bar unless it is open (keeping its mode), types the text
    /// and waits until it is compiled and searched, so the steps after it see the new query.
    async fn find(&self, args: &[String]) -> StepResult {
        let window = &self.window;
        if !window.find_open() {
            window.show_find();
        }
        self.field("find")?.set_text(&args[0]);
        let started = Instant::now();
        while window.find_compiled_text() != args[0] {
            if started.elapsed() > WAIT_TIMEOUT {
                return Err("the find field was never searched".to_owned());
            }
            glib::timeout_future(Duration::from_millis(5)).await;
        }
        self.wait_idle().await
    }

    /// `find-case true|false`: sets Match case.
    fn find_case(&self, args: &[String]) -> StepResult {
        self.window.set_find_option("case", boolean(&args[0])?);
        Ok(())
    }

    /// `key find|replace|directory|filters|results <key>`: sends the key (`Return`,
    /// `<Shift>Return`, `Escape`, `Up`) to that field's, or the results panel's, key handler.
    fn key(&self, args: &[String]) -> StepResult {
        let (target, key) = (args[0].as_str(), args[1].as_str());
        let controller = self.window.key_controller(target).ok_or_else(|| {
            format!("no key target {target}; use find, replace, directory, filters or results")
        })?;
        let (keyval, modifiers) =
            gtk::accelerator_parse(key).ok_or_else(|| format!("bad key {key}"))?;
        let values: [&dyn ToValue; 3] = [&keyval.into_glib(), &0u32, &modifiers];
        controller.emit_by_name::<bool>("key-pressed", &values);
        Ok(())
    }

    /// `find-mode normal|extended|regex`: the search mode.
    fn find_mode(&self, args: &[String]) -> StepResult {
        let mode = match args[0].as_str() {
            "normal" => SearchMode::Normal,
            "extended" => SearchMode::Extended,
            "regex" => SearchMode::Regex,
            other => return Err(format!("no mode {other}; use normal, extended or regex")),
        };
        self.window.set_find_search_mode(mode);
        Ok(())
    }

    /// `find-option <option> true|false`: a search option: `case`, `word`, `selection`,
    /// `wrap`, `backward`, `dot-newline`, `subfolders`, `hidden` or `gitignore`.
    fn find_option(&self, args: &[String]) -> StepResult {
        let value = boolean(&args[1])?;
        match args[0].as_str() {
            name @ ("case" | "word" | "selection" | "wrap" | "backward" | "dot-newline"
            | "subfolders" | "hidden" | "gitignore") => {
                self.window.set_find_option(name, value);
                Ok(())
            }
            other => Err(format!("no option {other}")),
        }
    }

    /// `replace-text <text>`: the replace field's text.
    fn replace_text(&self, args: &[String]) -> StepResult {
        self.field("replace")?.set_text(&args[0]);
        Ok(())
    }

    /// `fif-folder <folder>`: Find in Files' folder field.
    fn fif_folder(&self, args: &[String]) -> StepResult {
        self.field("directory")?.set_text(&args[0]);
        Ok(())
    }

    /// `fif-filters <filters>`: Find in Files' filters field.
    fn fif_filters(&self, args: &[String]) -> StepResult {
        self.field("filters")?.set_text(&args[0]);
        Ok(())
    }

    /// `select <line> <column> <line> <column>`: selects between two positions (1-based);
    /// the caret ends at the second.
    fn select(&self, args: &[String]) -> StepResult {
        let page = self.page()?;
        let buffer = page.buffer();
        let iter = |line: &str, column: &str| -> Result<gtk::TextIter, String> {
            let (line, column) = (number(line)?, number(column)?);
            let mut iter = line
                .checked_sub(1)
                .and_then(|line| buffer.iter_at_line_number(line))
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

    /// `fif-start`: starts Find in Files with the bar's files row (opening it if needed) and
    /// waits until the search runs, has ended, or failed to start with a message in the bar;
    /// a heartbeat measures the main loop until `wait-fif`.
    async fn fif_start(&self, _: &[String]) -> StepResult {
        if let Some(old) = FIF_HEARTBEAT.with(|beat| beat.replace(Some(Heartbeat::start()))) {
            old.stop();
        }
        let window = &self.window;
        if window.find_bar_mode() != crate::window::BarMode::Files || !window.find_open() {
            window.run_search(SearchCommand::FindInFiles);
        }
        window.clear_find_message();
        window.click_find_in_files();
        let started = Instant::now();
        while !window.find_in_files_running()
            && window.find_in_files_stats().is_none()
            && !window.find_message().1
        {
            if started.elapsed() > WAIT_TIMEOUT {
                return Err(format!(
                    "Find in Files did not start: {}",
                    window.find_message().0
                ));
            }
            glib::timeout_future(Duration::from_millis(1)).await;
        }
        Ok(())
    }

    /// `wait-fif [<seconds>]`: waits until Find in Files has ended (15 s unless given).
    async fn wait_fif(&self, args: &[String]) -> StepResult {
        let limit = match args.first() {
            Some(seconds) => Duration::from_secs(number(seconds)? as u64),
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

    /// `fif-stop`: Stop.
    fn fif_stop(&self, _: &[String]) {
        self.window.run_search(SearchCommand::StopFindInFiles);
    }

    /// `results-activate <row>`: selects a row of the results panel (0 is the newest search's
    /// heading), activates it as Enter does and waits for its work.
    async fn results_activate(&self, args: &[String]) -> StepResult {
        let position = to_u32(number(&args[0])?);
        if self.window.activate_result(position) {
            self.wait_idle().await
        } else {
            Err(format!("the results panel has no row {position}"))
        }
    }

    /// `set-history find|replace|directory|filters <entries>`: replaces a history through the
    /// session API; the entries are joined by `|`.
    fn set_history(&self, args: &[String]) -> StepResult {
        let field = field(&args[0])?;
        let mut history = self.window.search_history();
        let list = History::from_entries(
            args[1]
                .split('|')
                .filter(|entry| !entry.is_empty())
                .map(str::to_owned),
            stet_domain::search::HISTORY_LIMIT,
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

    /// `pick-history find|replace <index>`: picks an entry from that field's drop-down and
    /// waits for its work.
    async fn pick_history(&self, args: &[String]) -> StepResult {
        let name = match args[0].as_str() {
            name @ ("find" | "replace") => name,
            other => return Err(format!("no history drop-down {other}")),
        };
        let index = number(&args[1])?;
        if self.window.pick_history(name, to_i32(index)) == Some(true) {
            self.wait_idle().await
        } else {
            Err(format!("the drop-down has no entry {index}"))
        }
    }

    /// `wait-idle-low [<seconds>]`: waits until the main loop runs a low-priority idle (60 s
    /// unless given).
    async fn wait_idle_low(&self, args: &[String]) -> StepResult {
        let seconds = args.first().map_or(Ok(60), |seconds| number(seconds))?;
        wait_low_idle(Duration::from_secs(seconds as u64)).await
    }

    /// `text-checkpoint <name>`: remembers a hash of the current tab's text.
    fn text_checkpoint(&self, args: &[String]) -> StepResult {
        let hash = text_hash(&self.page()?.text());
        CHECKPOINTS.with(|checkpoints| {
            checkpoints.borrow_mut().insert(args[0].clone(), hash);
        });
        Ok(())
    }

    /// `assert-clipboard-contains <text>`: the clipboard's text contains the text.
    async fn assert_clipboard_contains(&self, args: &[String]) -> StepResult {
        let clipboard = self.window.window().clipboard();
        match clipboard.read_text_future().await {
            Ok(Some(text)) if text.contains(args[0].as_str()) => Ok(()),
            Ok(text) => Err(format!("the clipboard holds {text:?}, not {:?}", args[0])),
            Err(error) => Err(format!("could not read the clipboard: {error}")),
        }
    }

    /// `assert-current-match <text>`: the text of the current tab's current match.
    fn assert_current_match(&self, args: &[String]) -> StepResult {
        let page = self.page()?;
        let buffer = page.buffer();
        let text = page
            .current_match()
            .map(|(start, end)| buffer.text(&start, &end, true).to_string())
            .unwrap_or_default();
        equal("current match", text, args[0].clone())
    }

    /// `assert-match-count <text>`: the find bar's match count, as shown.
    fn assert_match_count(&self, args: &[String]) -> StepResult {
        equal("match count", self.window.find_count(), args[0].clone())
    }

    /// `assert-find-open true|false`: whether the find bar is open.
    fn assert_find_open(&self, args: &[String]) -> StepResult {
        equal("find bar open", self.window.find_open(), boolean(&args[0])?)
    }

    /// `assert-search-highlight true|false`: whether GtkSourceView highlights every match in
    /// the current tab.
    fn assert_search_highlight(&self, args: &[String]) -> StepResult {
        let page = self.page()?;
        equal(
            "highlight all",
            page.search()
                .existing_context()
                .is_some_and(|context| context.is_highlight()),
            boolean(&args[0])?,
        )
    }

    /// `assert-find-message <text>`: the find bar's message.
    fn assert_find_message(&self, args: &[String]) -> StepResult {
        equal(
            "find bar message",
            self.window.find_message().0,
            args[0].clone(),
        )
    }

    /// `assert-replace-hint <text>|none`: the replace field's warning, or none.
    fn assert_replace_hint(&self, args: &[String]) -> StepResult {
        let entry = self.field("replace")?;
        let tooltip = entry.tooltip_text().map(|text| text.to_string());
        let styled = entry.has_css_class("warning") || entry.has_css_class("error");
        match (args[0].as_str(), tooltip) {
            ("none", None) if !styled => Ok(()),
            ("none", tooltip) => Err(format!("the replace field says {tooltip:?}")),
            (wanted, Some(text)) if styled && text.contains(wanted) => Ok(()),
            (wanted, tooltip) => Err(format!(
                "the replace field says {tooltip:?} (styled {styled}), not {wanted:?}"
            )),
        }
    }

    /// `assert-field find|replace|directory|filters <text>`: a field's text.
    fn assert_field(&self, args: &[String]) -> StepResult {
        let text = match args[0].as_str() {
            name @ ("find" | "replace" | "directory" | "filters") => self.field(name)?.text(),
            other => return Err(format!("no field {other}")),
        };
        equal("field", text.to_string(), args[1].clone())
    }

    /// `assert-find-error none` or `assert-find-error <offset> <text>`: the pattern error, at
    /// that character offset of the find field and with that text.
    fn assert_find_error(&self, args: &[String]) -> StepResult {
        let error = self.window.find_pattern_error();
        match (args, error) {
            ([none], None) if none == "none" => Ok(()),
            ([none], Some(error)) if none == "none" => Err(format!("pattern error {error}")),
            ([offset, text], Some(error)) => {
                let marked = self.field("find")?.has_css_class("error");
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

    /// `assert-routed true|false`: whether the find field's query goes to our matcher instead
    /// of GtkSourceView's.
    fn assert_routed(&self, args: &[String]) -> StepResult {
        let compiled = self
            .window
            .find_compiled()
            .ok_or_else(|| "the find field holds no valid query".to_owned())?;
        equal("routed to our matcher", compiled.own(), boolean(&args[0])?)
    }

    /// `assert-own-matches <n>`: how many matches our matcher found in the current tab.
    fn assert_own_matches(&self, args: &[String]) -> StepResult {
        let page = self.page()?;
        let compiled = self
            .window
            .find_compiled()
            .ok_or_else(|| "the find field holds no valid query".to_owned())?;
        let count = page
            .search()
            .own_matches(compiled.id, page.revision())
            .map(|(matches, _)| matches.len())
            .ok_or_else(|| "our matcher has no matches for this text yet".to_owned())?;
        equal("our matcher's matches", count, number(&args[0])?)
    }

    /// `assert-own-tags <n>`: how many runs of the current tab's text carry our matcher's
    /// highlight tag.
    fn assert_own_tags(&self, args: &[String]) -> StepResult {
        let page = self.page()?;
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
        equal("highlighted matches", ranges, number(&args[0])?)
    }

    /// `assert-smart-highlight <n>`: how many occurrences smart highlighting marks.
    fn assert_smart_highlight(&self, args: &[String]) -> StepResult {
        let page = self.page()?;
        let search = page.search();
        let count = search
            .smart_context()
            .map_or(0, |context| context.occurrences_count());
        let wanted = args[0]
            .parse::<i32>()
            .map_err(|_| format!("expected a number, got {}", args[0]))?;
        equal("smart highlights", count, wanted)
            .map_err(|error| format!("{error} (pattern {:?})", search.smart_pattern()))
    }

    /// `assert-bar-mode find|replace|files|mark`: the find bar's row.
    fn assert_bar_mode(&self, args: &[String]) -> StepResult {
        let mode = match self.window.find_bar_mode() {
            crate::window::BarMode::Find => "find",
            crate::window::BarMode::Replace => "replace",
            crate::window::BarMode::Files => "files",
            crate::window::BarMode::Mark => "mark",
        };
        equal("bar mode", mode.to_owned(), args[0].clone())
    }

    /// `assert-can-undo true|false`: whether the current tab can undo.
    fn assert_can_undo(&self, args: &[String]) -> StepResult {
        let page = self.page()?;
        equal("can undo", page.buffer().can_undo(), boolean(&args[0])?)
    }

    /// `assert-can-redo true|false`: whether the current tab can redo.
    fn assert_can_redo(&self, args: &[String]) -> StepResult {
        let page = self.page()?;
        equal("can redo", page.buffer().can_redo(), boolean(&args[0])?)
    }

    /// `assert-results-heading <text>`: the newest heading in the results panel.
    fn assert_results_heading(&self, args: &[String]) -> StepResult {
        equal(
            "newest results heading",
            self.window.results_heading().unwrap_or_default(),
            args[0].clone(),
        )
    }

    /// `assert-results-contains <text>`: the results panel has the text.
    fn assert_results_contains(&self, args: &[String]) -> StepResult {
        let text = self.window.results_text();
        if text.contains(args[0].as_str()) {
            Ok(())
        } else {
            Err(format!("results {text:?} lack {:?}", args[0]))
        }
    }

    /// `assert-results-status <text>`: the results panel's status has the text.
    fn assert_results_status(&self, args: &[String]) -> StepResult {
        let status = self.window.results_status();
        if status.contains(args[0].as_str()) {
            Ok(())
        } else {
            Err(format!("results status {status:?} lacks {:?}", args[0]))
        }
    }

    /// `assert-results-shown true|false`: whether the results panel shows.
    fn assert_results_shown(&self, args: &[String]) -> StepResult {
        let wanted = boolean(&args[0])?;
        equal("results shown", self.window.results_shown(), wanted)
    }

    /// `assert-results-focused true|false`: whether the results panel has the focus.
    fn assert_results_focused(&self, args: &[String]) -> StepResult {
        let wanted = boolean(&args[0])?;
        equal("results focused", self.window.results_focused(), wanted)
    }

    /// `assert-results-searches <n>`: how many searches the results panel lists.
    fn assert_results_searches(&self, args: &[String]) -> StepResult {
        let wanted = number(&args[0])?;
        equal(
            "searches in the results panel",
            self.window.results_search_count() as usize,
            wanted,
        )
    }

    /// `assert-fif-running true|false`: whether Find in Files runs.
    fn assert_fif_running(&self, args: &[String]) -> StepResult {
        let wanted = boolean(&args[0])?;
        equal(
            "Find in Files running",
            self.window.find_in_files_running(),
            wanted,
        )
    }

    /// `assert-fif <field> <value>`: a figure of the last Find in Files: `searched`,
    /// `matched`, `hits`, `cancelled`, `truncated`, `errors`, or `searched-below`.
    fn assert_fif(&self, args: &[String]) -> StepResult {
        let (name, value) = (args[0].as_str(), args[1].as_str());
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

    /// `assert-history find|replace|directory|filters <entries>`: a history, its entries
    /// joined by `|`.
    fn assert_history(&self, args: &[String]) -> StepResult {
        let field = field(&args[0])?;
        let history = self.window.search_history();
        equal(
            "history",
            history_list(&history, field).entries().join("|"),
            args[1].clone(),
        )
    }

    /// `assert-replace-stats <count> true|false`: how many matches the last Replace All
    /// replaced, and whether as one bulk edit.
    fn assert_replace_stats(&self, args: &[String]) -> StepResult {
        let stats = self
            .window
            .replace_stats()
            .ok_or_else(|| "no Replace All has finished".to_owned())?;
        let count = number(&args[0])?;
        equal(
            "replaced matches and bulk edit",
            (stats.count, stats.bulk),
            (count, boolean(&args[1])?),
        )
    }

    /// `assert-text-checkpoint <name>`: the current tab's text hashes as at that
    /// `text-checkpoint`.
    fn assert_text_checkpoint(&self, args: &[String]) -> StepResult {
        let page = self.page()?;
        let wanted = CHECKPOINTS
            .with(|checkpoints| checkpoints.borrow().get(args[0].as_str()).copied())
            .ok_or_else(|| format!("no checkpoint {}", args[0]))?;
        equal("text hash", text_hash(&page.text()), wanted)
    }

    /// `report-fif`: prints the last Find in Files' timings and summary.
    fn report_fif(&self, _: &[String]) -> StepResult {
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

    /// `generate-tree <folder> <files> <lines>`: writes that many text files of that many
    /// lines, spread over 20 folders, on a worker.
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

    /// `bench-replace-all [<count>]`: Replace All with the bar's fields, reporting the cost of
    /// every phase, how long the main loop was blocked and the memory; with a count, that many
    /// matches must have been replaced.
    async fn bench_replace_all(&self, args: &[String]) -> StepResult {
        let window = &self.window;
        window.reset_replace_stats();
        let (rss_before, _) = memory();
        let heartbeat = Heartbeat::start();
        let started = Instant::now();
        self.window.run_search(SearchCommand::ReplaceAll);
        let limit = Duration::from_secs(600);
        while window.replace_stats().is_none() {
            if started.elapsed() > limit {
                heartbeat.stop();
                return Err(format!(
                    "no result after {limit:?}: {}",
                    window.find_message().0
                ));
            }
            glib::timeout_future(Duration::from_millis(2)).await;
        }
        let seen = started.elapsed();
        let stall = heartbeat.take();
        let (rss_after, peak) = memory();
        let stats = window.replace_stats().expect("checked above");
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

    /// `bench-undo`: Undo, timed: the call itself, and until the main loop is idle again.
    async fn bench_undo(&self, _: &[String]) -> StepResult {
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

    /// `assert-widget-parity`: the parity table against the real GtkSourceView: every row the
    /// translation leaves to the widget must give the same spans and count as our matcher.
    async fn assert_widget_parity(&self, _: &[String]) -> StepResult {
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
                if wrapped || widget.len() > 10_000 || end.char_offset() <= iter.char_offset() {
                    break;
                }
                widget.push((start.char_offset(), end.char_offset()));
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
            if widget != ours || usize::try_from(count).ok() != Some(ours.len()) {
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
