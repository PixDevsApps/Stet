//! S2: regex search, occurrence counts and replace-all through GtkSourceView's SearchContext.
//!
//! `tools/headless.sh <display> cargo run --release -p stet-spikes --bin s2_search [-- options]`
//! Options: `--runs=N` (default 3), `--lines=N` (use the first N fixture lines),
//! `--undo=dropped,cleared,live,nobrackets,detached,noprimary` (editor state while undoing),
//! `--cases=dense,sparse`, `--no-redo`, `--replace-only` (no counts or bulk swap),
//! `--probes-only`, `--perf-only`, `--tag=NAME` (writes `perf-NAME.md`).
//! Tables are written to `target/spikes/s2/`.

use anyhow::{Context as _, Result, anyhow, bail, ensure};
use gtk4 as gtk;
use gtk4::glib;
use gtk4::glib::translate::{ToGlibPtr, from_glib_full};
use gtk4::prelude::*;
use serde_json::json;
use sourceview5::prelude::*;
use std::cell::Cell;
use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};
use stet_spikes::{
    Report, Row, peak_rss_kib, require_headless, rss_kib, run_main_loop_until, wait_frames,
    write_markdown,
};

const SPIKE: &str = "s2_search";
const SMALL_TIMEOUT: Duration = Duration::from_secs(10);
const DRAIN_TIMEOUT: Duration = Duration::from_secs(600);
const COUNT_TIMEOUT: Duration = Duration::from_secs(300);
const EDIT_LIMIT: Duration = Duration::from_secs(600);
const MAX_MATCHES: usize = 64;
const REPLACE_BAR_MS: f64 = 5000.0;
const RSS_CAP_KIB: u64 = 16 * 1024 * 1024;

fn main() -> Result<()> {
    require_headless()?;
    let args = Args::parse()?;
    gtk::init().context("initializing GTK")?;
    sourceview5::init();
    let report = Report::new(SPIKE);
    let watchdog = Watchdog::start();
    let out_dir = workspace_root().join("target/spikes/s2");
    report.record(
        "versions",
        json!({
            "gtk": format!("{}.{}.{}", gtk::major_version(), gtk::minor_version(), gtk::micro_version()),
            "gtksourceview": format!(
                "{}.{}.{}",
                sourceview5::major_version(),
                sourceview5::minor_version(),
                sourceview5::micro_version()
            ),
        }),
    );

    if args.probes {
        let rows = run_pattern_probes(&report, &watchdog)?;
        write_markdown(
            &out_dir.join("patterns.md"),
            "PCRE2 features through SearchContext",
            &rows,
            &[],
        )?;
        let mut rows = run_template_probes(&report, &watchdog)?;
        rows.extend(check_bindings(&report)?);
        rows.push(investigate_keep_out(&report)?);
        write_markdown(
            &out_dir.join("templates.md"),
            "Replacement templates",
            &rows,
            &[],
        )?;
    }
    if args.perf {
        let rows = run_perf(&report, &watchdog, &args)?;
        let name = match (&args.tag, args.lines) {
            (Some(tag), _) => format!("perf-{tag}.md"),
            (None, Some(lines)) => format!("perf-{lines}.md"),
            (None, None) => "perf.md".to_owned(),
        };
        write_markdown(
            &out_dir.join(name),
            "Search and replace on search-1m.log",
            &rows,
            &[],
        )?;
    }
    report.record("peak_rss_kib", peak_rss_kib()?);
    Ok(())
}

struct Args {
    runs: usize,
    lines: Option<usize>,
    undo: Vec<UndoWith>,
    cases: Vec<String>,
    tag: Option<String>,
    probes: bool,
    perf: bool,
    counts: bool,
    redo: bool,
}

/// What the search context is doing when the replace-all is undone.
#[derive(Clone, Copy)]
enum UndoWith {
    LiveContext,
    ClearedSearch,
    NoContext,
    NoBrackets,
    Detached,
    NoPrimary,
}

impl UndoWith {
    fn parse(name: &str) -> Result<Self> {
        Ok(match name {
            "live" => Self::LiveContext,
            "cleared" => Self::ClearedSearch,
            "dropped" => Self::NoContext,
            "nobrackets" => Self::NoBrackets,
            "detached" => Self::Detached,
            "noprimary" => Self::NoPrimary,
            _ => bail!(
                "unknown undo mode {name} (live, cleared, dropped, nobrackets, detached, noprimary)"
            ),
        })
    }

    fn name(self) -> &'static str {
        match self {
            Self::LiveContext => "live",
            Self::ClearedSearch => "cleared",
            Self::NoContext => "dropped",
            Self::NoBrackets => "nobrackets",
            Self::Detached => "detached",
            Self::NoPrimary => "noprimary",
        }
    }

    fn describe(self) -> &'static str {
        match self {
            Self::LiveContext => "search context alive and highlighting",
            Self::ClearedSearch => "search text set to None first",
            Self::NoContext => "search context disposed first",
            Self::NoBrackets => "search context disposed and highlight-matching-brackets off first",
            Self::Detached => "search context disposed and the buffer detached from the view first",
            Self::NoPrimary => {
                "search context disposed and the buffer removed from the PRIMARY clipboard first"
            }
        }
    }
}

impl Args {
    fn parse() -> Result<Self> {
        let mut args = Self {
            runs: 3,
            lines: None,
            undo: vec![
                UndoWith::NoContext,
                UndoWith::ClearedSearch,
                UndoWith::LiveContext,
            ],
            probes: true,
            perf: true,
            counts: true,
            cases: vec!["dense".to_owned(), "sparse".to_owned()],
            redo: true,
            tag: None,
        };
        for arg in std::env::args().skip(1) {
            if arg == "--probes-only" {
                args.perf = false;
            } else if arg == "--perf-only" {
                args.probes = false;
            } else if arg == "--replace-only" {
                args.probes = false;
                args.counts = false;
            } else if arg == "--no-redo" {
                args.redo = false;
            } else if let Some(value) = arg.strip_prefix("--tag=") {
                args.tag = Some(value.to_owned());
            } else if let Some(value) = arg.strip_prefix("--cases=") {
                args.cases = value.split(',').map(str::to_owned).collect();
            } else if let Some(value) = arg.strip_prefix("--runs=") {
                args.runs = value.parse().context("--runs")?;
            } else if let Some(value) = arg.strip_prefix("--undo=") {
                args.undo = value
                    .split(',')
                    .map(UndoWith::parse)
                    .collect::<Result<_>>()?;
            } else if let Some(value) = arg.strip_prefix("--lines=") {
                args.lines = Some(value.parse().context("--lines")?);
            } else {
                bail!("unknown argument {arg}");
            }
        }
        ensure!(args.runs > 0, "--runs must be at least 1");
        Ok(args)
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("spikes crate lives inside the workspace")
        .to_path_buf()
}

/// Exits the process when an armed phase outlives its deadline or RSS passes `RSS_CAP_KIB`,
/// so a synchronous freeze or runaway allocation inside GTK is reported instead of hanging the run.
struct Watchdog {
    armed: Arc<Mutex<Option<(String, Instant)>>>,
}

impl Watchdog {
    fn start() -> Self {
        let armed = Arc::new(Mutex::new(None::<(String, Instant)>));
        let shared = Arc::clone(&armed);
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(200));
                if let Ok(rss) = rss_kib()
                    && rss > RSS_CAP_KIB
                {
                    println!(
                        "{}",
                        json!({ "spike": SPIKE, "key": "rss_cap", "value": rss })
                    );
                    eprintln!("watchdog: RSS {rss} KiB exceeded the {RSS_CAP_KIB} KiB cap");
                    std::process::exit(4);
                }
                let guard = shared.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some((phase, deadline)) = guard.as_ref()
                    && Instant::now() > *deadline
                {
                    println!(
                        "{}",
                        json!({ "spike": SPIKE, "key": phase, "value": "timeout" })
                    );
                    eprintln!("watchdog: {phase} exceeded its time limit");
                    std::process::exit(3);
                }
            }
        });
        Self { armed }
    }

    fn arm(&self, phase: impl Into<String>, limit: Duration) {
        *self.armed.lock().unwrap_or_else(PoisonError::into_inner) =
            Some((phase.into(), Instant::now() + limit));
    }

    fn disarm(&self) {
        *self.armed.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

#[derive(Clone, Copy)]
struct Mode {
    regex: bool,
    case_sensitive: bool,
    word_boundaries: bool,
}

const REGEX: Mode = Mode {
    regex: true,
    case_sensitive: true,
    word_boundaries: false,
};
const REGEX_NOCASE: Mode = Mode {
    case_sensitive: false,
    ..REGEX
};
const REGEX_WORDS: Mode = Mode {
    word_boundaries: true,
    ..REGEX
};
const TEXT: Mode = Mode {
    regex: false,
    ..REGEX
};
const TEXT_WORDS: Mode = Mode {
    word_boundaries: true,
    ..TEXT
};

impl Mode {
    fn settings(self, pattern: Option<&str>) -> sourceview5::SearchSettings {
        let settings = sourceview5::SearchSettings::builder()
            .regex_enabled(self.regex)
            .case_sensitive(self.case_sensitive)
            .at_word_boundaries(self.word_boundaries)
            .build();
        settings.set_search_text(pattern);
        settings
    }

    fn label(self) -> String {
        let mut label = if self.regex { "regex" } else { "text" }.to_owned();
        if !self.case_sensitive {
            label.push_str(", ignore case");
        }
        if self.word_boundaries {
            label.push_str(", whole word");
        }
        label
    }
}

fn escape_controls(text: &str) -> String {
    text.replace('\r', "␍")
        .replace('\n', "⏎")
        .replace('\t', "⇥")
}

fn visible(text: &str) -> String {
    format!("\"{}\"", escape_controls(text))
}

fn visible_list(items: &[impl AsRef<str>]) -> String {
    let items: Vec<String> = items.iter().map(|item| visible(item.as_ref())).collect();
    format!("[{}]", items.join(", "))
}

fn small_buffer(text: &str) -> sourceview5::Buffer {
    let buffer = sourceview5::Buffer::new(None);
    buffer.begin_irreversible_action();
    buffer.set_text(text);
    buffer.end_irreversible_action();
    buffer
}

fn buffer_text(buffer: &sourceview5::Buffer) -> glib::GString {
    let (start, end) = buffer.bounds();
    buffer.text(&start, &end, true)
}

fn scan_done(context: &sourceview5::SearchContext) -> bool {
    context.occurrences_count() >= 0 || context.regex_error().is_some()
}

/// Calls the C function directly: the safe binding drops the replacement count and
/// asserts that a zero return means an error, which panics when nothing matched.
fn replace_all_counted(
    context: &sourceview5::SearchContext,
    replace: &str,
) -> Result<u32, glib::Error> {
    let mut error = std::ptr::null_mut();
    // SAFETY: both pointers are valid for the call; -1 means `replace` is NUL-terminated.
    let replaced = unsafe {
        sourceview5::ffi::gtk_source_search_context_replace_all(
            context.to_glib_none().0,
            replace.to_glib_none().0,
            -1,
            &mut error,
        )
    };
    if error.is_null() {
        Ok(replaced)
    } else {
        // SAFETY: GtkSourceView transferred ownership of a non-null GError.
        Err(unsafe { from_glib_full(error) })
    }
}

enum Expected {
    Matches(&'static [&'static str]),
    Error,
}

struct PatternProbe {
    feature: &'static str,
    pattern: &'static str,
    input: &'static str,
    mode: Mode,
    start: i32,
    expected: Expected,
}

const fn probe(
    feature: &'static str,
    pattern: &'static str,
    input: &'static str,
    mode: Mode,
    matches: &'static [&'static str],
) -> PatternProbe {
    PatternProbe {
        feature,
        pattern,
        input,
        mode,
        start: 0,
        expected: Expected::Matches(matches),
    }
}

/// A regex probe whose forward search starts at a character offset, like Find Next from the caret.
const fn probe_from(
    feature: &'static str,
    pattern: &'static str,
    input: &'static str,
    start: i32,
    matches: &'static [&'static str],
) -> PatternProbe {
    PatternProbe {
        feature,
        pattern,
        input,
        mode: REGEX,
        start,
        expected: Expected::Matches(matches),
    }
}

const fn probe_error(
    feature: &'static str,
    pattern: &'static str,
    input: &'static str,
) -> PatternProbe {
    PatternProbe {
        feature,
        pattern,
        input,
        mode: REGEX,
        start: 0,
        expected: Expected::Error,
    }
}

const USERS: &str = "user=alice id=7\nuser=Bob id=42\nsuperuser=carol id=x\n";
const WORDS: &str = "user users user_x superuser user.";
const BOOST: &str = "<user> user users";
const CASES: &str = "ERROR Error error";

const PATTERN_PROBES: &[PatternProbe] = &[
    probe(
        "Lookbehind",
        r"(?<=user=)\w+",
        USERS,
        REGEX,
        &["alice", "Bob", "carol"],
    ),
    probe(
        "Negative lookbehind",
        r"(?<!super)user=\w+",
        USERS,
        REGEX,
        &["user=alice", "user=Bob"],
    ),
    probe(
        "Bounded variable-length lookbehind",
        r"(?<=\w{2,4}=)\d+",
        USERS,
        REGEX,
        &["7", "42"],
    ),
    probe(
        "Lookbehind across a line break",
        r"(?<=7\n)user",
        USERS,
        REGEX,
        &["user"],
    ),
    probe("Lookahead", r"\w+(?==\d)", USERS, REGEX, &["id", "id"]),
    probe(
        r"\K resets the match start",
        r"user=\K\w+",
        USERS,
        REGEX,
        &["alice", "Bob", "carol"],
    ),
    probe(
        r"\R matches LF, CRLF and CR",
        r"\R",
        "a\nb\r\nc\rd",
        REGEX,
        &["\n", "\r\n", "\r"],
    ),
    probe(r"\R inside a pattern", r"a\Rb", "a\nb", REGEX, &["a\nb"]),
    probe(
        "Named groups (?<n>...)",
        r"(?<key>\w+)=(?<val>\d+)",
        USERS,
        REGEX,
        &["id=7", "id=42"],
    ),
    probe(
        "Named groups (?P<n>...)",
        r"(?P<key>\w+)=\d+",
        USERS,
        REGEX,
        &["id=7", "id=42"],
    ),
    probe(
        r"Backreference \1 in the pattern",
        r"(\w)\1",
        "aa bc dd",
        REGEX,
        &["aa", "dd"],
    ),
    probe(
        r"Named backreference \k<n>",
        r"(?<ch>\w)\k<ch>",
        "aa bc dd",
        REGEX,
        &["aa", "dd"],
    ),
    probe(
        "Named backreference (?P=n)",
        r"(?P<ch>\w)(?P=ch)",
        "aa bc dd",
        REGEX,
        &["aa", "dd"],
    ),
    probe(
        r"\n spanning two lines",
        r"foo\nbar",
        "foo\nbar\nfoo\nbaz\n",
        REGEX,
        &["foo\nbar"],
    ),
    probe(
        r"\n spanning three lines",
        r"a\nb\nc",
        "a\nb\nc\n",
        REGEX,
        &["a\nb\nc"],
    ),
    probe(
        "Real newline in a plain-text search",
        "foo\nbar",
        "foo\nbar\nfoo\nbaz\n",
        TEXT,
        &["foo\nbar"],
    ),
    probe(
        "^ at every line start",
        r"^bar",
        "foo\nbar\nbar",
        REGEX,
        &["bar", "bar"],
    ),
    probe(
        "$ at every line end",
        r"foo$",
        "foo\nfoo bar\nfoo",
        REGEX,
        &["foo", "foo"],
    ),
    probe("$ before CRLF", r"c$", "abc\r\nabc", REGEX, &["c", "c"]),
    probe(
        ". does not match a newline",
        r"foo.bar",
        "foo\nbar",
        REGEX,
        &[],
    ),
    probe(
        "(?s) lets . match a newline",
        r"(?s)foo.bar",
        "foo\nbar",
        REGEX,
        &["foo\nbar"],
    ),
    probe(
        r"\s matches a newline",
        r"foo\sbar",
        "foo\nbar",
        REGEX,
        &["foo\nbar"],
    ),
    probe(
        "Negated class matches a newline",
        r"foo[^x]bar",
        "foo\nbar",
        REGEX,
        &["foo\nbar"],
    ),
    probe(r"\A only at buffer start", r"\Aa", "a\na\na", REGEX, &["a"]),
    probe(r"\z only at buffer end", r"a\z", "a\na\na", REGEX, &["a"]),
    probe(
        "Empty match ^ (one per line)",
        "^",
        "a\nb\nc",
        REGEX,
        &["", "", ""],
    ),
    probe(
        "Empty match $ (one per line)",
        "$",
        "a\nb\nc",
        REGEX,
        &["", "", ""],
    ),
    probe(
        "case-sensitive off",
        "error",
        CASES,
        REGEX_NOCASE,
        &["ERROR", "Error", "error"],
    ),
    probe("case-sensitive on", "error", CASES, REGEX, &["error"]),
    probe(
        "Inline (?i)",
        "(?i)error",
        CASES,
        REGEX,
        &["ERROR", "Error", "error"],
    ),
    probe(
        "case-sensitive off, non-ASCII",
        "café",
        "CAFÉ Café café",
        REGEX_NOCASE,
        &["CAFÉ", "Café", "café"],
    ),
    probe(
        r"\w is Unicode-aware",
        r"\w+",
        "café 漢字",
        REGEX,
        &["café", "漢字"],
    ),
    probe(
        r"\b word boundary",
        r"\buser\b",
        WORDS,
        REGEX,
        &["user", "user"],
    ),
    probe(
        "at-word-boundaries, text",
        "user",
        WORDS,
        TEXT_WORDS,
        &["user", "user"],
    ),
    probe(
        "at-word-boundaries, regex",
        r"use\w",
        WORDS,
        REGEX_WORDS,
        &["user", "user"],
    ),
    probe(
        r"Boost \< \> word anchors",
        r"\<user\>",
        BOOST,
        REGEX,
        &["user", "user"],
    ),
    probe(
        r"\< \> translated to PCRE2",
        r"\b(?=\w)user\b(?<=\w)",
        BOOST,
        REGEX,
        &["user", "user"],
    ),
    probe("Possessive quantifier", r"\d++1", "111", REGEX, &[]),
    probe("Atomic group", r"(?>\d+)1", "111", REGEX, &[]),
    probe(
        r"Unicode property \p{Lu}",
        r"\p{Lu}+",
        "abcDEF ÅÄÖ",
        REGEX,
        &["DEF", "ÅÄÖ"],
    ),
    probe(r"\x{hhhh} escape", r"\x{263A}", "smile ☺", REGEX, &["☺"]),
    probe(r"\xhh and \t escapes", r"\x41\t", "A\tB A", REGEX, &["A\t"]),
    probe(r"\Q...\E quoting", r"\Qa.b\E", "a.b axb", REGEX, &["a.b"]),
    probe(
        r"\h horizontal space",
        r"\h",
        "a b\tc\nd",
        REGEX,
        &[" ", "\t"],
    ),
    probe(r"\N non-newline", r"a\Nb", "axb a\nb", REGEX, &["axb"]),
    probe(
        "Recursion (?R)",
        r"\((?:[^()]|(?R))*\)",
        "(a(b)c) (d",
        REGEX,
        &["(a(b)c)"],
    ),
    probe(
        "(?x) extended syntax",
        r"(?x) u s e r",
        "user",
        REGEX,
        &["user"],
    ),
    probe_error("Invalid pattern reports an error", "(unclosed", "x"),
    probe_error("Unknown alphanumeric escape", r"\i", "x"),
    probe_from(
        "Lookbehind sees text before the start iter",
        r"(?<=user=)\w+",
        "user=alice",
        5,
        &["alice"],
    ),
    probe_from(
        r"\b sees the character before the start iter",
        r"\bser\w*",
        "users serx",
        1,
        &["serx"],
    ),
    probe_from(
        "^ sees the character before the start iter",
        r"^ser\w*",
        "users\nserx",
        1,
        &["serx"],
    ),
];

struct Found {
    count: i32,
    matches: Vec<String>,
    error: Option<String>,
}

fn search_small(pattern: &str, input: &str, mode: Mode, start: i32) -> Result<Found> {
    let buffer = small_buffer(input);
    let settings = mode.settings(Some(pattern));
    let context = sourceview5::SearchContext::new(&buffer, Some(&settings));
    run_main_loop_until(SMALL_TIMEOUT, || scan_done(&context))?;
    if let Some(error) = context.regex_error() {
        return Ok(Found {
            count: context.occurrences_count(),
            matches: Vec::new(),
            error: Some(error.message().to_owned()),
        });
    }
    let mut matches = Vec::new();
    let mut iter = buffer.iter_at_offset(start);
    while matches.len() < MAX_MATCHES {
        let Some((start, end, wrapped)) = context.forward(&iter) else {
            break;
        };
        if wrapped {
            break;
        }
        matches.push(buffer.text(&start, &end, true).to_string());
        iter = end;
        if start == end {
            if iter.is_end() {
                break;
            }
            iter.forward_char();
        }
    }
    Ok(Found {
        count: context.occurrences_count(),
        matches,
        error: None,
    })
}

fn run_pattern_probes(report: &Report, watchdog: &Watchdog) -> Result<Vec<Row>> {
    let mut rows = Vec::new();
    for probe in PATTERN_PROBES {
        watchdog.arm(format!("pattern: {}", probe.feature), SMALL_TIMEOUT * 2);
        let found = search_small(probe.pattern, probe.input, probe.mode, probe.start)?;
        watchdog.disarm();
        let (wanted, passed) = match probe.expected {
            Expected::Matches(expected) => (
                format!("{} {}", expected.len(), visible_list(expected)),
                found.error.is_none()
                    && found.matches == expected
                    && (probe.start > 0 || usize::try_from(found.count) == Ok(expected.len())),
            ),
            Expected::Error => ("regex error".to_owned(), found.error.is_some()),
        };
        let result = match &found.error {
            Some(error) => format!("error: {error}"),
            None if probe.start > 0 => format!(
                "forward() found {} (whole-buffer count {})",
                visible_list(&found.matches),
                found.count
            ),
            None if usize::try_from(found.count) == Ok(found.matches.len()) => {
                format!("{} {}", found.count, visible_list(&found.matches))
            }
            None => format!(
                "count {}, forward() found {}",
                found.count,
                visible_list(&found.matches)
            ),
        };
        report.record(
            "pattern",
            json!({
                "feature": probe.feature,
                "pattern": probe.pattern,
                "input": probe.input,
                "mode": probe.mode.label(),
                "start_offset": probe.start,
                "count": found.count,
                "matches": found.matches,
                "error": found.error,
                "as_expected": passed,
            }),
        );
        let from = if probe.start > 0 {
            format!(", from offset {}", probe.start)
        } else {
            String::new()
        };
        rows.push(Row::new(
            format!(
                "{}: `{}` ({}) on {}{from}",
                probe.feature,
                escape_controls(probe.pattern),
                probe.mode.label(),
                visible(probe.input)
            ),
            result,
            wanted,
            Some(passed),
        ));
    }
    Ok(rows)
}

#[derive(Clone, Copy)]
enum Want {
    Text(&'static str),
    Error,
    Unjudged,
}

struct TemplateProbe {
    template: &'static str,
    pattern: &'static str,
    input: &'static str,
    mode: Mode,
    wanted: Want,
}

const fn template(
    template: &'static str,
    pattern: &'static str,
    input: &'static str,
    mode: Mode,
    wanted: Want,
) -> TemplateProbe {
    TemplateProbe {
        template,
        pattern,
        input,
        mode,
        wanted,
    }
}

const PAIR: &str = "user=alice user=Bob";
const PAIR_PATTERN: &str = r"user=(\w+)";
const TEN_GROUPS: &str = r"(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)";

const TEMPLATE_PROBES: &[TemplateProbe] = &[
    template(
        r"<\0>",
        PAIR_PATTERN,
        PAIR,
        REGEX,
        Want::Text("<user=alice> <user=Bob>"),
    ),
    template(
        r"<\1>",
        PAIR_PATTERN,
        PAIR,
        REGEX,
        Want::Text("<alice> <Bob>"),
    ),
    template(
        r"<\g<1>>",
        PAIR_PATTERN,
        PAIR,
        REGEX,
        Want::Text("<alice> <Bob>"),
    ),
    template(
        r"<\g<0>>",
        PAIR_PATTERN,
        PAIR,
        REGEX,
        Want::Text("<user=alice> <user=Bob>"),
    ),
    template(
        r"<$1>",
        PAIR_PATTERN,
        PAIR,
        REGEX,
        Want::Text("<alice> <Bob>"),
    ),
    template(
        r"<${1}>",
        PAIR_PATTERN,
        PAIR,
        REGEX,
        Want::Text("<alice> <Bob>"),
    ),
    template(
        r"<$0>",
        PAIR_PATTERN,
        PAIR,
        REGEX,
        Want::Text("<user=alice> <user=Bob>"),
    ),
    template(
        r"<$&>",
        PAIR_PATTERN,
        PAIR,
        REGEX,
        Want::Text("<user=alice> <user=Bob>"),
    ),
    template(
        r"<\U\1\E>",
        PAIR_PATTERN,
        PAIR,
        REGEX,
        Want::Text("<ALICE> <BOB>"),
    ),
    template(
        r"<\U\1>",
        PAIR_PATTERN,
        PAIR,
        REGEX,
        Want::Text("<ALICE> <BOB>"),
    ),
    template(
        r"<\U$1\E>",
        PAIR_PATTERN,
        PAIR,
        REGEX,
        Want::Text("<ALICE> <BOB>"),
    ),
    template(
        r"<\U\g<1>\E>",
        PAIR_PATTERN,
        PAIR,
        REGEX,
        Want::Text("<ALICE> <BOB>"),
    ),
    template(
        r"<\L\1\E>",
        PAIR_PATTERN,
        PAIR,
        REGEX,
        Want::Text("<alice> <bob>"),
    ),
    template(
        r"<\u\1>",
        PAIR_PATTERN,
        PAIR,
        REGEX,
        Want::Text("<Alice> <Bob>"),
    ),
    template(
        r"<\l\1>",
        PAIR_PATTERN,
        PAIR,
        REGEX,
        Want::Text("<alice> <bob>"),
    ),
    template(
        r"<\U\1\E-\1>",
        PAIR_PATTERN,
        PAIR,
        REGEX,
        Want::Text("<ALICE-alice> <BOB-Bob>"),
    ),
    template(r"<\n>", PAIR_PATTERN, PAIR, REGEX, Want::Text("<\n> <\n>")),
    template(r"<\t>", PAIR_PATTERN, PAIR, REGEX, Want::Text("<\t> <\t>")),
    template(r"<\\>", PAIR_PATTERN, PAIR, REGEX, Want::Text(r"<\> <\>")),
    template(r"<\x41>", PAIR_PATTERN, PAIR, REGEX, Want::Text("<A> <A>")),
    template(r"<&>", PAIR_PATTERN, PAIR, REGEX, Want::Text("<&> <&>")),
    template(r"<\2>", PAIR_PATTERN, PAIR, REGEX, Want::Text("<> <>")),
    template(r"<\", PAIR_PATTERN, PAIR, REGEX, Want::Error),
    template(
        r"<\g<name>>",
        r"user=(?<name>\w+)",
        PAIR,
        REGEX,
        Want::Text("<alice> <Bob>"),
    ),
    template(
        r"<$+{name}>",
        r"user=(?<name>\w+)",
        PAIR,
        REGEX,
        Want::Text("<alice> <Bob>"),
    ),
    template(r"<\10>", TEN_GROUPS, "abcdefghij", REGEX, Want::Text("<j>")),
    template(
        r"<\g<10>>",
        TEN_GROUPS,
        "abcdefghij",
        REGEX,
        Want::Text("<j>"),
    ),
    template(r"<$10>", TEN_GROUPS, "abcdefghij", REGEX, Want::Text("<j>")),
    template(r"\U\1", r"(\w+)", "café", REGEX, Want::Text("CAFÉ")),
    template("> ", "^", "a\nb\nc", REGEX, Want::Text("> a\n> b\n> c")),
    template(";", "$", "a\nb\nc", REGEX, Want::Text("a;\nb;\nc;")),
    template("", r"^\R", "a\n\nb\n\n\nc", REGEX, Want::Text("a\nb\nc")),
    template(
        r"\U\0$1",
        "alice",
        PAIR,
        TEXT,
        Want::Text(r"user=\U\0$1 user=Bob"),
    ),
    template("<\n>", PAIR_PATTERN, PAIR, REGEX, Want::Text("<\n> <\n>")),
    template(
        r"<\\1>",
        PAIR_PATTERN,
        PAIR,
        REGEX,
        Want::Text(r"<\1> <\1>"),
    ),
    template(r"<\\\\>", PAIR_PATTERN, PAIR, REGEX, Want::Unjudged),
    template(r"<\q>", PAIR_PATTERN, PAIR, REGEX, Want::Unjudged),
    template(r"<\e>", PAIR_PATTERN, PAIR, REGEX, Want::Unjudged),
    template(r"<$>", PAIR_PATTERN, PAIR, REGEX, Want::Text("<$> <$>")),
    template(
        r"<\g<1>0>",
        PAIR_PATTERN,
        PAIR,
        REGEX,
        Want::Text("<alice0> <Bob0>"),
    ),
    template(r"<\>", PAIR_PATTERN, PAIR, REGEX, Want::Unjudged),
    template(r"<\\\1>", PAIR_PATTERN, PAIR, REGEX, Want::Unjudged),
    template(r"<\\U\1>", PAIR_PATTERN, PAIR, REGEX, Want::Unjudged),
    template(r"<\r\v\f\a>", PAIR_PATTERN, PAIR, REGEX, Want::Unjudged),
    template(r"<\b>", PAIR_PATTERN, PAIR, REGEX, Want::Unjudged),
    template(r"<\x{263A}>", PAIR_PATTERN, PAIR, REGEX, Want::Unjudged),
    template(
        r"<\1\n>",
        PAIR_PATTERN,
        PAIR,
        REGEX,
        Want::Text("<alice\n> <Bob\n>"),
    ),
    template(
        r"<\1\\>",
        PAIR_PATTERN,
        PAIR,
        REGEX,
        Want::Text(r"<alice\> <Bob\>"),
    ),
    template(
        r"<\1\x41>",
        PAIR_PATTERN,
        PAIR,
        REGEX,
        Want::Text("<aliceA> <BobA>"),
    ),
    template(r"<\Uab\E>", PAIR_PATTERN, PAIR, REGEX, Want::Unjudged),
    template("X", r"user=\K\w+", PAIR, REGEX, Want::Text("user=X user=X")),
    template(
        r"> \1",
        "^(.*)$",
        "a\n\nc",
        REGEX,
        Want::Text("> a\n> \n> c"),
    ),
];

fn replace_small(
    pattern: &str,
    input: &str,
    mode: Mode,
    replace: &str,
) -> Result<Result<(u32, String), String>> {
    let buffer = small_buffer(input);
    let settings = mode.settings(Some(pattern));
    let context = sourceview5::SearchContext::new(&buffer, Some(&settings));
    run_main_loop_until(SMALL_TIMEOUT, || scan_done(&context))?;
    if let Some(error) = context.regex_error() {
        bail!("pattern {pattern:?} failed to compile: {}", error.message());
    }
    Ok(match replace_all_counted(&context, replace) {
        Ok(count) => Ok((count, buffer_text(&buffer).to_string())),
        Err(error) => Err(error.message().to_owned()),
    })
}

fn run_template_probes(report: &Report, watchdog: &Watchdog) -> Result<Vec<Row>> {
    let mut rows = Vec::new();
    for probe in TEMPLATE_PROBES {
        watchdog.arm(format!("template: {}", probe.template), SMALL_TIMEOUT * 2);
        let outcome = replace_small(probe.pattern, probe.input, probe.mode, probe.template)?;
        watchdog.disarm();
        let passed = match (&outcome, probe.wanted) {
            (_, Want::Unjudged) => None,
            (Ok((_, text)), Want::Text(wanted)) => Some(text == wanted),
            (Err(_), Want::Error) => Some(true),
            _ => Some(false),
        };
        let result = match &outcome {
            Ok((count, text)) => format!("{} ({count} replaced)", visible(text)),
            Err(error) => format!("error: {error}"),
        };
        report.record(
            "template",
            json!({
                "template": probe.template,
                "pattern": probe.pattern,
                "input": probe.input,
                "mode": probe.mode.label(),
                "replaced": outcome.as_ref().ok().map(|(count, _)| count),
                "output": outcome.as_ref().ok().map(|(_, text)| text),
                "error": outcome.as_ref().err(),
                "as_wanted": passed,
            }),
        );
        rows.push(Row::new(
            format!(
                "`{}` for `{}` ({}) on {}",
                escape_controls(probe.template),
                escape_controls(probe.pattern),
                probe.mode.label(),
                visible(probe.input)
            ),
            result,
            match probe.wanted {
                Want::Text(wanted) => visible(wanted),
                Want::Error => "an error".to_owned(),
                Want::Unjudged => "informative".to_owned(),
            },
            passed,
        ));
    }
    Ok(rows)
}

/// Records how the safe `SearchContext::replace_all` binding and `utils_unescape_search_text` behave.
fn check_bindings(report: &Report) -> Result<Vec<Row>> {
    let buffer = small_buffer("abc");
    let settings = REGEX.settings(Some("zzz"));
    let context = sourceview5::SearchContext::new(&buffer, Some(&settings));
    run_main_loop_until(SMALL_TIMEOUT, || scan_done(&context))?;
    let raw = replace_all_counted(&context, "x").map_err(|error| anyhow!("{error}"))?;
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let wrapped = std::panic::catch_unwind(AssertUnwindSafe(|| context.replace_all("x")));
    std::panic::set_hook(hook);
    let wrapper_ok = matches!(wrapped, Ok(Ok(())));
    let wrapper = match wrapped {
        Ok(Ok(())) => "Ok(())".to_owned(),
        Ok(Err(error)) => format!("Err({})", error.message()),
        Err(panic) => {
            let message = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_default();
            format!("panicked: {}", message.lines().next().unwrap_or_default())
        }
    };
    report.record(
        "binding_replace_all_zero_matches",
        json!({ "ffi_count": raw, "safe_wrapper": wrapper }),
    );

    let escaped = r"a\tb\nc\x41\u263A\\";
    let unescaped = sourceview5::utils_unescape_search_text(escaped).to_string();
    report.record(
        "utils_unescape_search_text",
        json!({ "input": escaped, "output": unescaped }),
    );
    Ok(vec![
        Row::new(
            "Safe `SearchContext::replace_all` binding with zero matches",
            format!("C returns {raw}; wrapper {wrapper}"),
            "Ok, with a count",
            Some(wrapper_ok),
        ),
        Row::new(
            format!("`utils_unescape_search_text` on `{escaped}`"),
            visible(&unescaped),
            "informative",
            None,
        ),
    ])
}

/// `\K` counts matches but `forward()` finds none; records what each API reports instead.
fn investigate_keep_out(report: &Report) -> Result<Row> {
    let buffer = small_buffer(PAIR);
    let table = buffer.tag_table();
    let mut before = Vec::new();
    table.foreach(|tag| before.push(tag.clone()));
    let settings = REGEX.settings(Some(r"user=\K\w+"));
    let context = sourceview5::SearchContext::new(&buffer, Some(&settings));
    run_main_loop_until(SMALL_TIMEOUT, || scan_done(&context))?;
    let mut found_tags = Vec::new();
    table.foreach(|tag| {
        if !before.contains(tag) {
            found_tags.push(tag.clone());
        }
    });
    let highlighted: Vec<(i32, i32)> = found_tags
        .iter()
        .flat_map(|tag| tag_ranges(&buffer, tag))
        .collect();
    let offsets = |found: Option<(gtk::TextIter, gtk::TextIter, bool)>| {
        found.map(|(start, end, _)| (start.offset(), end.offset()))
    };
    let forward = offsets(context.forward(&buffer.start_iter()));
    let backward = offsets(context.backward(&buffer.end_iter()));
    let position = |start: i32, end: i32| {
        context.occurrence_position(&buffer.iter_at_offset(start), &buffer.iter_at_offset(end))
    };
    let alice = position(5, 10);
    let whole = position(0, 10);
    report.record(
        "keep_out",
        json!({
            "count": context.occurrences_count(),
            "highlighted_ranges": highlighted,
            "forward_from_start": forward,
            "backward_from_end": backward,
            "occurrence_position_alice": alice,
            "occurrence_position_user_alice": whole,
        }),
    );
    Ok(Row::new(
        format!("`user=\\K\\w+` on {} through each API", visible(PAIR)),
        format!(
            "count {}; highlighted char ranges {highlighted:?}; forward {forward:?}; backward {backward:?}; \
             occurrence_position(5..10) {alice}, (0..10) {whole}",
            context.occurrences_count()
        ),
        "matches [5..10) and [16..19)",
        Some(forward == Some((5, 10)) && highlighted == [(5, 10), (16, 19)]),
    ))
}

fn tag_ranges(buffer: &sourceview5::Buffer, tag: &gtk::TextTag) -> Vec<(i32, i32)> {
    let mut ranges = Vec::new();
    let mut iter = buffer.start_iter();
    let mut open = iter.starts_tag(Some(tag)).then_some(0);
    while iter.forward_to_tag_toggle(Some(tag)) {
        if iter.starts_tag(Some(tag)) {
            open = Some(iter.offset());
        } else if let Some(start) = open.take() {
            ranges.push((start, iter.offset()));
        }
    }
    ranges
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

struct Stats {
    median: f64,
    min: f64,
    max: f64,
}

impl Stats {
    fn of(samples: &[f64]) -> Self {
        let mut sorted = samples.to_vec();
        sorted.sort_by(f64::total_cmp);
        let mid = sorted.len() / 2;
        let median = if sorted.len() % 2 == 1 {
            sorted[mid]
        } else {
            (sorted[mid - 1] + sorted[mid]) / 2.0
        };
        Self {
            median,
            min: sorted[0],
            max: sorted[sorted.len() - 1],
        }
    }

    fn describe(&self, runs: usize) -> String {
        format!(
            "median {:.0} ms (min {:.0}, max {:.0}; n={runs})",
            self.median, self.min, self.max
        )
    }

    fn json(&self) -> serde_json::Value {
        json!({ "median_ms": self.median, "min_ms": self.min, "max_ms": self.max })
    }
}

fn digest(text: &str) -> String {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Waits until a low-priority idle callback runs, meaning GTK's own idle work
/// (layout validation, search scans) has drained.
fn drain_main_loop() -> Result<Duration> {
    let idle = Rc::new(Cell::new(false));
    let flag = Rc::clone(&idle);
    glib::idle_add_local_full(glib::Priority::LOW, move || {
        flag.set(true);
        glib::ControlFlow::Break
    });
    run_main_loop_until(DRAIN_TIMEOUT, || idle.get())
}

/// Resets VmHWM so `peak_rss_kib` reports the peak since this call.
fn reset_peak_rss() {
    if let Err(error) = std::fs::write("/proc/self/clear_refs", "5") {
        eprintln!("could not reset peak RSS: {error}");
    }
}

fn truncate_lines(text: &mut String, lines: usize) {
    if let Some((index, _)) = text.match_indices('\n').nth(lines.saturating_sub(1)) {
        text.truncate(index + 1);
    }
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Uppercases the ASCII word run that starts at `at`; returns its length.
fn upper_word(out: &mut String, text: &str, at: usize) -> usize {
    let len = text.as_bytes()[at..]
        .iter()
        .take_while(|&&byte| is_word_byte(byte))
        .count();
    out.push_str(&text[at..at + len].to_ascii_uppercase());
    len
}

/// Expected output of `user=(\w+)` → `user=\U\1\E`, computed without a regex engine.
fn expect_user_upper(text: &str) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut count = 0;
    let mut copied = 0;
    for (index, needle) in text.match_indices("user=") {
        if index < copied {
            continue;
        }
        let word = index + needle.len();
        if !text.as_bytes().get(word).copied().is_some_and(is_word_byte) {
            continue;
        }
        out.push_str(&text[copied..word]);
        copied = word + upper_word(&mut out, text, word);
        count += 1;
    }
    out.push_str(&text[copied..]);
    (out, count)
}

/// Expected output of `(ERROR .* user=)(\w+)` → `\1\U\2\E`, line by line.
fn expect_error_user_upper(text: &str) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut count = 0;
    for line in text.split_inclusive('\n') {
        let target = line.find("ERROR ").and_then(|error| {
            let rest = &line[error + "ERROR ".len()..];
            let user = rest.rfind(" user=")?;
            let word = error + "ERROR ".len() + user + " user=".len();
            line.as_bytes()
                .get(word)
                .copied()
                .is_some_and(is_word_byte)
                .then_some(word)
        });
        match target {
            Some(word) => {
                out.push_str(&line[..word]);
                let len = upper_word(&mut out, line, word);
                out.push_str(&line[word + len..]);
                count += 1;
            }
            None => out.push_str(line),
        }
    }
    (out, count)
}

struct CountCase {
    name: &'static str,
    pattern: &'static str,
    mode: Mode,
}

const COUNT_CASES: &[CountCase] = &[
    CountCase {
        name: "timestamp",
        pattern: r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z",
        mode: REGEX,
    },
    CountCase {
        name: "error_user",
        pattern: r"ERROR .* user=(\w+)",
        mode: REGEX,
    },
    CountCase {
        name: "literal_user_alice",
        pattern: "user=alice",
        mode: TEXT,
    },
];

struct ReplaceCase {
    name: &'static str,
    pattern: &'static str,
    template: &'static str,
    expect: fn(&str) -> (String, usize),
}

const REPLACE_CASES: &[ReplaceCase] = &[
    ReplaceCase {
        name: "dense",
        pattern: r"user=(\w+)",
        template: r"user=\U\1\E",
        expect: expect_user_upper,
    },
    ReplaceCase {
        name: "sparse",
        pattern: r"(ERROR .* user=)(\w+)",
        template: r"\1\U\2\E",
        expect: expect_error_user_upper,
    },
];

struct Bench<'a> {
    report: &'a Report,
    watchdog: &'a Watchdog,
    runs: usize,
    redo: bool,
    buffer: sourceview5::Buffer,
    view: sourceview5::View,
    original: String,
    original_digest: String,
    rows: Vec<Row>,
}

fn run_perf(report: &Report, watchdog: &Watchdog, args: &Args) -> Result<Vec<Row>> {
    let path = workspace_root().join("target/fixtures/search-1m.log");
    let mut original = std::fs::read_to_string(&path).with_context(|| {
        format!(
            "reading {} (run python3 tools/gen-fixtures.py)",
            path.display()
        )
    })?;
    if let Some(lines) = args.lines {
        truncate_lines(&mut original, lines);
    }
    let lines = original.bytes().filter(|&byte| byte == b'\n').count();
    report.record(
        "fixture",
        json!({ "path": path.display().to_string(), "bytes": original.len(), "lines": lines }),
    );

    let rss_empty = rss_kib()?;
    let buffer = sourceview5::Buffer::new(None);
    watchdog.arm("load", EDIT_LIMIT + DRAIN_TIMEOUT);
    let started = Instant::now();
    buffer.begin_irreversible_action();
    buffer.set_text(&original);
    buffer.end_irreversible_action();
    let load = started.elapsed();
    let view = sourceview5::View::with_buffer(&buffer);
    let scrolled = gtk::ScrolledWindow::builder().child(&view).build();
    let window = gtk::Window::builder()
        .default_width(1000)
        .default_height(700)
        .child(&scrolled)
        .build();
    window.present();
    let first_frames = wait_frames(&view, 2, Duration::from_secs(120))?;
    let settle = drain_main_loop()?;
    watchdog.disarm();
    let rss_loaded = rss_kib()?;
    report.record(
        "load",
        json!({
            "set_text_ms": ms(load),
            "first_frames_ms": ms(first_frames),
            "drain_ms": ms(settle),
            "rss_before_kib": rss_empty,
            "rss_loaded_kib": rss_loaded,
            "max_undo_levels": buffer.max_undo_levels(),
            "can_undo": buffer.can_undo(),
        }),
    );

    let original_digest = digest(&original);
    let mut bench = Bench {
        report,
        watchdog,
        runs: args.runs,
        redo: args.redo,
        buffer,
        view,
        original,
        original_digest,
        rows: Vec::new(),
    };
    bench.rows.push(Row::new(
        format!("Load {} bytes / {lines} lines (`set_text`)", bench.original.len()),
        format!(
            "{:.0} ms; first frames {:.0} ms; idle work drained after {:.0} ms more; RSS {} → {} MiB",
            ms(load),
            ms(first_frames),
            ms(settle),
            rss_empty / 1024,
            rss_loaded / 1024
        ),
        "informative",
        None,
    ));
    if args.counts {
        for case in COUNT_CASES {
            for highlight in [false, true] {
                bench.count(case, highlight)?;
            }
        }
    }
    for case in REPLACE_CASES {
        if args.cases.iter().any(|name| name == case.name) {
            for &undo_with in &args.undo {
                bench.replace(case, undo_with)?;
            }
        }
    }
    if args.counts {
        bench.bulk_swap()?;
    }
    window.destroy();
    Ok(bench.rows)
}

impl Bench<'_> {
    fn count(&mut self, case: &CountCase, highlight: bool) -> Result<()> {
        let label = format!(
            "{}.{}",
            case.name,
            if highlight {
                "highlight"
            } else {
                "no_highlight"
            }
        );
        let mut times = Vec::new();
        let mut counts = Vec::new();
        let mut teardowns = Vec::new();
        for run in 1..=self.runs {
            let settings = case.mode.settings(None);
            let context = sourceview5::SearchContext::builder()
                .buffer(&self.buffer)
                .settings(&settings)
                .highlight(highlight)
                .build();
            drain_main_loop()?;
            self.watchdog.arm(format!("count {label}"), COUNT_TIMEOUT);
            let started = Instant::now();
            settings.set_search_text(Some(case.pattern));
            run_main_loop_until(COUNT_TIMEOUT, || scan_done(&context))?;
            let elapsed = started.elapsed();
            self.watchdog.disarm();
            if let Some(error) = context.regex_error() {
                bail!("{label}: {}", error.message());
            }
            let count = context.occurrences_count();
            wait_frames(&self.view, 1, Duration::from_secs(30))?;
            let rss = rss_kib()?;
            self.watchdog
                .arm(format!("teardown {label}"), COUNT_TIMEOUT);
            let started = Instant::now();
            drop(context);
            drop(settings);
            let teardown = started.elapsed();
            self.watchdog.disarm();
            self.report.record(
                &format!("count.{label}.run{run}"),
                json!({
                    "ms": ms(elapsed),
                    "count": count,
                    "teardown_ms": ms(teardown),
                    "rss_kib": rss,
                }),
            );
            times.push(ms(elapsed));
            counts.push(count);
            teardowns.push(ms(teardown));
        }
        let stats = Stats::of(&times);
        let consistent = counts.windows(2).all(|pair| pair[0] == pair[1]);
        self.report.record(
            &format!("count.{label}"),
            json!({ "stats": stats.json(), "counts": counts, "teardown": Stats::of(&teardowns).json() }),
        );
        self.rows.push(Row::new(
            format!(
                "Count `{}` ({}), highlight {}",
                case.pattern,
                case.mode.label(),
                if highlight { "on" } else { "off" }
            ),
            format!(
                "{}; {} matches{}; context teardown {:.0} ms median",
                stats.describe(self.runs),
                counts[0],
                if consistent {
                    ""
                } else {
                    " (counts differ between runs)"
                },
                Stats::of(&teardowns).median
            ),
            "no bar (measured)",
            None,
        ));
        Ok(())
    }

    fn wait_for_scan(&self, context: &sourceview5::SearchContext, phase: &str) -> Result<Duration> {
        self.watchdog.arm(phase, COUNT_TIMEOUT);
        let waited = run_main_loop_until(COUNT_TIMEOUT, || scan_done(context))?;
        self.watchdog.disarm();
        Ok(waited)
    }

    fn replace(&mut self, case: &ReplaceCase, undo_with: UndoWith) -> Result<()> {
        let (expected, expected_count) = (case.expect)(&self.original);
        let expected_digest = digest(&expected);
        let label = format!("{}.{}", case.name, undo_with.name());
        let mut replace_times = Vec::new();
        let mut rescan_times = Vec::new();
        let mut undo_times = Vec::new();
        let mut replaced_counts = Vec::new();
        let mut all_correct = true;
        let mut one_step = true;
        reset_peak_rss();
        for run in 1..=self.runs {
            let settings = REGEX.settings(Some(case.pattern));
            let context = sourceview5::SearchContext::builder()
                .buffer(&self.buffer)
                .settings(&settings)
                .highlight(true)
                .build();
            drain_main_loop()?;
            let scan = self.wait_for_scan(&context, "scan before replace")?;
            let rss_before = rss_kib()?;
            self.watchdog
                .arm(format!("replace_all {label}"), EDIT_LIMIT);
            let started = Instant::now();
            let replaced = replace_all_counted(&context, case.template)
                .map_err(|error| anyhow!("replace_all {label}: {}", error.message()))?;
            let replace_time = started.elapsed();
            self.watchdog.disarm();
            let rss_replaced = rss_kib()?;
            let after = buffer_text(&self.buffer);
            let after_digest = digest(&after);
            let correct =
                after.as_str() == expected && usize::try_from(replaced) == Ok(expected_count);
            drop(after);
            let could_undo = self.buffer.can_undo();
            let rescan = self.wait_for_scan(&context, "rescan after replace")?;

            let context = match undo_with {
                UndoWith::LiveContext => Some(context),
                UndoWith::ClearedSearch => {
                    settings.set_search_text(None);
                    Some(context)
                }
                UndoWith::NoContext
                | UndoWith::NoBrackets
                | UndoWith::Detached
                | UndoWith::NoPrimary => None,
            };
            let primary = self.view.primary_clipboard();
            if matches!(undo_with, UndoWith::NoPrimary) {
                self.buffer.remove_selection_clipboard(&primary);
            }
            if matches!(undo_with, UndoWith::NoBrackets) {
                self.buffer.set_highlight_matching_brackets(false);
            }
            if matches!(undo_with, UndoWith::Detached) {
                self.view.set_buffer(Some(&sourceview5::Buffer::new(None)));
            }
            drain_main_loop()?;
            self.watchdog.arm(format!("undo {label}"), EDIT_LIMIT);
            let started = Instant::now();
            self.buffer.undo();
            let undo_time = started.elapsed();
            self.watchdog.disarm();
            let restored_text = buffer_text(&self.buffer);
            let restored_digest = digest(&restored_text);
            let restored = restored_text.as_str() == self.original;
            drop(restored_text);
            let can_undo_after = self.buffer.can_undo();
            let can_redo_after = self.buffer.can_redo();
            let rss_undone = rss_kib()?;
            let single_step = could_undo && restored && !can_undo_after && can_redo_after;

            let mut redo = serde_json::Value::Null;
            if run == 1 && self.redo {
                self.watchdog.arm(format!("redo {label}"), EDIT_LIMIT * 2);
                let started = Instant::now();
                self.buffer.redo();
                let redo_time = started.elapsed();
                let redone = buffer_text(&self.buffer).as_str() == expected;
                let started = Instant::now();
                self.buffer.undo();
                let undo_again_time = started.elapsed();
                let undone_again = buffer_text(&self.buffer).as_str() == self.original;
                self.watchdog.disarm();
                one_step &= redone && undone_again;
                redo = json!({
                    "redo_ms": ms(redo_time),
                    "matches_expected": redone,
                    "undo_again_ms": ms(undo_again_time),
                    "undo_again_restores": undone_again,
                });
            }
            self.buffer.set_highlight_matching_brackets(true);
            if matches!(undo_with, UndoWith::NoPrimary) {
                self.buffer.add_selection_clipboard(&primary);
            }
            if matches!(undo_with, UndoWith::Detached) {
                self.view.set_buffer(Some(&self.buffer));
            }
            drop(context);
            drop(settings);

            self.report.record(
                &format!("replace.{label}.run{run}"),
                json!({
                    "scan_before_ms": ms(scan),
                    "replace_ms": ms(replace_time),
                    "replaced": replaced,
                    "expected_replacements": expected_count,
                    "output_digest": after_digest,
                    "expected_digest": expected_digest,
                    "output_correct": correct,
                    "rescan_ms": ms(rescan),
                    "undo_ms": ms(undo_time),
                    "restored_digest": restored_digest,
                    "original_digest": self.original_digest,
                    "restored": restored,
                    "can_undo_before": could_undo,
                    "can_undo_after_one_undo": can_undo_after,
                    "can_redo_after_one_undo": can_redo_after,
                    "redo": redo,
                    "rss_kib": { "before": rss_before, "after_replace": rss_replaced, "after_undo": rss_undone },
                }),
            );
            replace_times.push(ms(replace_time));
            rescan_times.push(ms(rescan));
            undo_times.push(ms(undo_time));
            replaced_counts.push(replaced);
            all_correct &= correct;
            one_step &= single_step;
        }
        let peak = peak_rss_kib()?;

        let replace_stats = Stats::of(&replace_times);
        let rescan_stats = Stats::of(&rescan_times);
        let undo_stats = Stats::of(&undo_times);
        self.report.record(
            &format!("replace.{label}"),
            json!({
                "replace": replace_stats.json(),
                "rescan": rescan_stats.json(),
                "undo": undo_stats.json(),
                "replaced": replaced_counts,
                "all_correct": all_correct,
                "one_undo_step": one_step,
                "peak_rss_kib": peak,
            }),
        );
        self.rows.push(Row::new(
            format!(
                "Replace all `{}` → `{}` ({} expected replacements; run set: {})",
                case.pattern,
                case.template,
                expected_count,
                undo_with.name()
            ),
            format!(
                "{}; replaced {:?}; output equals independently computed text: {}; \
                 count available again {:.0} ms later (median)",
                replace_stats.describe(self.runs),
                replaced_counts,
                if all_correct { "yes" } else { "NO" },
                rescan_stats.median
            ),
            "≤ 5 s",
            Some(replace_stats.median <= REPLACE_BAR_MS && all_correct),
        ));
        self.rows.push(Row::new(
            format!("Undo of that replace-all, {}", undo_with.describe()),
            format!(
                "{}; one undo() restores the original (digest {}), leaves nothing to undo, \
                 and redo re-applies it: {}; peak RSS {} MiB",
                undo_stats.describe(self.runs),
                self.original_digest,
                if one_step { "yes" } else { "NO" },
                peak / 1024
            ),
            "exactly one undo step",
            Some(one_step),
        ));
        Ok(())
    }

    /// The ADR-003 bulk path: replace the whole text once inside one user action.
    fn bulk_swap(&mut self) -> Result<()> {
        let (expected, _) = expect_user_upper(&self.original);
        let mut swap_times = Vec::new();
        let mut undo_times = Vec::new();
        let mut ok = true;
        for run in 1..=self.runs {
            drain_main_loop()?;
            self.watchdog.arm("bulk swap", EDIT_LIMIT);
            let started = Instant::now();
            self.buffer.begin_user_action();
            let (mut start, mut end) = self.buffer.bounds();
            self.buffer.delete(&mut start, &mut end);
            self.buffer.insert(&mut start, &expected);
            self.buffer.end_user_action();
            let swap_time = started.elapsed();
            let correct = buffer_text(&self.buffer).as_str() == expected;
            let started = Instant::now();
            self.buffer.undo();
            let undo_time = started.elapsed();
            let restored = buffer_text(&self.buffer).as_str() == self.original;
            let single_step = restored && !self.buffer.can_undo();
            self.watchdog.disarm();
            self.report.record(
                &format!("bulk_swap.run{run}"),
                json!({
                    "swap_ms": ms(swap_time),
                    "undo_ms": ms(undo_time),
                    "correct": correct,
                    "restored": restored,
                    "one_undo_step": single_step,
                }),
            );
            swap_times.push(ms(swap_time));
            undo_times.push(ms(undo_time));
            ok &= correct && single_step;
        }
        let swap_stats = Stats::of(&swap_times);
        let undo_stats = Stats::of(&undo_times);
        self.report.record(
            "bulk_swap",
            json!({ "swap": swap_stats.json(), "undo": undo_stats.json(), "ok": ok }),
        );
        self.rows.push(Row::new(
            "Bulk path (ADR-003): delete all + insert precomputed dense result in one user action",
            format!(
                "swap {}; undo {}; correct and one undo step: {}",
                swap_stats.describe(self.runs),
                undo_stats.describe(self.runs),
                if ok { "yes" } else { "NO" }
            ),
            "comparison only",
            None,
        ));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expected_transforms_match_hand_results() {
        let text = "a ERROR x user=bob\nb INFO user=eve\nsuperuser=x_1 user=\n";
        let (dense, dense_count) = expect_user_upper(text);
        assert_eq!(
            dense,
            "a ERROR x user=BOB\nb INFO user=EVE\nsuperuser=X_1 user=\n"
        );
        assert_eq!(dense_count, 3);
        let (sparse, sparse_count) = expect_error_user_upper(text);
        assert_eq!(
            sparse,
            "a ERROR x user=BOB\nb INFO user=eve\nsuperuser=x_1 user=\n"
        );
        assert_eq!(sparse_count, 1);
    }

    #[test]
    fn stats_take_the_middle_sample() {
        let stats = Stats::of(&[30.0, 10.0, 20.0]);
        assert_eq!((stats.median, stats.min, stats.max), (20.0, 10.0, 30.0));
        assert_eq!(Stats::of(&[1.0, 3.0]).median, 2.0);
    }

    #[test]
    fn truncates_to_whole_lines() {
        let mut text = "a\nb\nc\n".to_owned();
        truncate_lines(&mut text, 2);
        assert_eq!(text, "a\nb\n");
    }
}
