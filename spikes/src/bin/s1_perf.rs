//! Spike S1: GtkSourceView cost of opening a 100 MB file, typing in a 10 MB file,
//! a 10 MB single-line JSON file, and whole-buffer snapshots; and the M3 follow-up for
//! ADR-014 (`threshold`): typing right after a Rust file of 1–20 MB was loaded the way the
//! app loads it.
//!
//! `tools/headless.sh 11 cargo run --release -p stet-spikes --bin s1_perf -- <group>...`
//! runs every case of the named groups (`open`, `typing`, `long-line`, `snapshot`,
//! `threshold`, `all`, or a case name) three times, each in a fresh child process that the
//! parent kills at a deadline. It prints a Markdown summary per case and writes the raw runs
//! to `.local/s1/`.

use anyhow::{Context, Result, bail};
use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;
use serde_json::{Map, Value, json};
use sourceview5::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};
use stet_spikes::{
    Report, peak_rss_kib, require_headless, rss_kib, run_main_loop_until, wait_frames,
};

const RUNS: usize = 3;
const CHUNK_BYTES: usize = 4 * 1024 * 1024;
const CHUNK_TIMER: Duration = Duration::from_millis(16);
const PHASE_TIMEOUT: Duration = Duration::from_secs(60);
const HEARTBEAT: Duration = Duration::from_millis(5);
const SETTLE_SAMPLE: Duration = Duration::from_millis(50);
const SETTLE_WINDOW: usize = 5;
const SETTLE_MAX_BUSY: f64 = 0.10;
const KEYSTROKES: u32 = 50;
const KEY_INTERVAL: Duration = Duration::from_millis(100);
const SNAPSHOT_REPEATS: usize = 5;
const SNAPSHOT_BIG_BYTES: usize = 50 * 1024 * 1024;
const STALL_KEY: &str = "stall_in_progress_ms";

#[derive(Clone, Copy)]
enum Highlight {
    Off,
    On,
    AfterLoad,
}

#[derive(Clone, Copy)]
enum Load {
    SetText,
    Chunks(Pacing),
}

/// How the chunks after the first are scheduled.
#[derive(Clone, Copy)]
enum Pacing {
    /// One per default-priority idle tick, which runs only after GTK's validation idles.
    Idle,
    /// One per `CHUNK_TIMER` default-priority timeout, which runs ahead of validation.
    Timer,
}

#[derive(Clone, Copy)]
enum Case {
    Open {
        load: Load,
        highlight: Highlight,
    },
    Typing {
        highlight: bool,
    },
    LongLine {
        formatted: bool,
        wrap: bool,
    },
    Snapshot,
    /// Rust text of `kib` KiB loaded with timer chunks and highlighting off, as the app loads
    /// files; highlighting is switched on `delay_ms` after the last chunk and typing starts
    /// right after the last chunk. Without `tags`, the Rust language is set but
    /// `highlight-syntax` stays off, which GtkSourceView documents as tags only.
    Threshold {
        kib: usize,
        lines: Lines,
        delay_ms: u64,
        tags: bool,
    },
}

/// The line shape of a threshold case's text.
#[derive(Clone, Copy)]
enum Lines {
    /// The fixture's own lines, about 55 bytes on average.
    Fixture,
    /// Every word on its own line.
    Words,
    /// Lines broken at a space before this many bytes.
    Width(usize),
}

struct CaseSpec {
    group: &'static str,
    name: &'static str,
    case: Case,
    deadline: Duration,
}

const fn spec(group: &'static str, name: &'static str, case: Case, deadline_s: u64) -> CaseSpec {
    CaseSpec {
        group,
        name,
        case,
        deadline: Duration::from_secs(deadline_s),
    }
}

const fn threshold(kib: usize, lines: Lines, delay_ms: u64) -> Case {
    Case::Threshold {
        kib,
        lines,
        delay_ms,
        tags: true,
    }
}

const fn untagged(kib: usize) -> Case {
    Case::Threshold {
        kib,
        lines: Lines::Fixture,
        delay_ms: 0,
        tags: false,
    }
}

const CASES: [CaseSpec; 30] = [
    spec(
        "open",
        "open-chunked-plain",
        Case::Open {
            load: Load::Chunks(Pacing::Idle),
            highlight: Highlight::Off,
        },
        240,
    ),
    spec(
        "open",
        "open-chunked-rust",
        Case::Open {
            load: Load::Chunks(Pacing::Idle),
            highlight: Highlight::On,
        },
        240,
    ),
    spec(
        "open",
        "open-chunked-rust-after-load",
        Case::Open {
            load: Load::Chunks(Pacing::Idle),
            highlight: Highlight::AfterLoad,
        },
        240,
    ),
    spec(
        "open",
        "open-set-text-plain",
        Case::Open {
            load: Load::SetText,
            highlight: Highlight::Off,
        },
        240,
    ),
    spec(
        "open",
        "open-set-text-rust",
        Case::Open {
            load: Load::SetText,
            highlight: Highlight::On,
        },
        240,
    ),
    spec(
        "open",
        "open-timer-chunks-plain",
        Case::Open {
            load: Load::Chunks(Pacing::Timer),
            highlight: Highlight::Off,
        },
        240,
    ),
    spec(
        "open",
        "open-timer-chunks-rust",
        Case::Open {
            load: Load::Chunks(Pacing::Timer),
            highlight: Highlight::On,
        },
        240,
    ),
    spec(
        "typing",
        "typing-rust",
        Case::Typing { highlight: true },
        180,
    ),
    spec(
        "typing",
        "typing-plain",
        Case::Typing { highlight: false },
        180,
    ),
    spec(
        "long-line",
        "long-line-raw-nowrap",
        Case::LongLine {
            formatted: false,
            wrap: false,
        },
        90,
    ),
    spec(
        "long-line",
        "long-line-raw-wrap",
        Case::LongLine {
            formatted: false,
            wrap: true,
        },
        90,
    ),
    spec(
        "long-line",
        "long-line-formatted-nowrap",
        Case::LongLine {
            formatted: true,
            wrap: false,
        },
        180,
    ),
    spec(
        "long-line",
        "long-line-formatted-wrap",
        Case::LongLine {
            formatted: true,
            wrap: true,
        },
        180,
    ),
    spec("snapshot", "snapshot", Case::Snapshot, 120),
    spec(
        "threshold",
        "threshold-1mb",
        threshold(1024, Lines::Fixture, 0),
        180,
    ),
    spec(
        "threshold",
        "threshold-2mb",
        threshold(2 * 1024, Lines::Fixture, 0),
        180,
    ),
    spec(
        "threshold",
        "threshold-5mb",
        threshold(5 * 1024, Lines::Fixture, 0),
        180,
    ),
    spec(
        "threshold",
        "threshold-10mb",
        threshold(10 * 1024, Lines::Fixture, 0),
        180,
    ),
    spec(
        "threshold",
        "threshold-20mb",
        threshold(20 * 1024, Lines::Fixture, 0),
        240,
    ),
    spec(
        "threshold",
        "threshold-short-lines-2mb",
        threshold(2 * 1024, Lines::Words, 0),
        180,
    ),
    spec(
        "threshold",
        "threshold-10mb-delayed-1s",
        threshold(10 * 1024, Lines::Fixture, 1000),
        180,
    ),
    spec(
        "threshold-more",
        "threshold-3mb",
        threshold(3 * 1024, Lines::Fixture, 0),
        180,
    ),
    spec(
        "threshold-more",
        "threshold-4mb",
        threshold(4 * 1024, Lines::Fixture, 0),
        180,
    ),
    spec(
        "threshold-more",
        "threshold-short-lines-512kb",
        threshold(512, Lines::Words, 0),
        180,
    ),
    spec(
        "threshold-more",
        "threshold-short-lines-1mb",
        threshold(1024, Lines::Words, 0),
        180,
    ),
    spec(
        "threshold-more",
        "threshold-2mb-delayed-1s",
        threshold(2 * 1024, Lines::Fixture, 1000),
        180,
    ),
    spec(
        "threshold-more",
        "threshold-5mb-delayed-1s",
        threshold(5 * 1024, Lines::Fixture, 1000),
        180,
    ),
    spec(
        "threshold-lines",
        "threshold-2mb-width-20",
        threshold(2 * 1024, Lines::Width(20), 0),
        180,
    ),
    spec(
        "threshold-lines",
        "threshold-1mb-width-10",
        threshold(1024, Lines::Width(10), 0),
        180,
    ),
    spec(
        "threshold-untagged",
        "threshold-10mb-language-without-tags",
        untagged(10 * 1024),
        180,
    ),
];

fn main() -> Result<()> {
    require_headless()?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [flag, name] if flag == "--case" => run_case(name),
        groups => orchestrate(groups),
    }
}

fn workspace_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("spikes crate has no parent directory")
}

fn fixture(name: &str) -> Result<String> {
    let path = workspace_root()?.join("target/fixtures").join(name);
    let bytes = std::fs::read(&path).with_context(|| {
        format!(
            "reading {}; run python3 tools/gen-fixtures.py",
            path.display()
        )
    })?;
    String::from_utf8(bytes).with_context(|| format!("{} is not UTF-8", path.display()))
}

fn ms(duration: Duration) -> f64 {
    (duration.as_secs_f64() * 1_000_000.0).round() / 1000.0
}

fn ratio(numerator: u64, denominator: usize) -> f64 {
    (numerator as f64 / denominator as f64 * 100.0).round() / 100.0
}

// ---------------------------------------------------------------- parent

fn orchestrate(groups: &[String]) -> Result<()> {
    let selected: Vec<&CaseSpec> = CASES
        .iter()
        .filter(|spec| {
            groups.is_empty()
                || groups
                    .iter()
                    .any(|group| group == "all" || group == spec.group || group == spec.name)
        })
        .collect();
    if selected.is_empty() {
        bail!(
            "unknown group; use open, typing, long-line, snapshot, threshold, threshold-more, threshold-lines, threshold-untagged, all or a case name"
        );
    }
    if std::env::var_os("GDK_BACKEND").is_some_and(|backend| backend == "broadway") {
        connect_web_client().context("connecting a Broadway web client")?;
    }
    let exe = std::env::current_exe().context("locating this binary")?;
    let out = workspace_root()?.join(".local/s1");
    std::fs::create_dir_all(&out).with_context(|| format!("creating {}", out.display()))?;
    let stamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_secs();
    let mut results = Map::new();
    for spec in selected {
        let mut runs = Vec::with_capacity(RUNS);
        for run in 1..=RUNS {
            eprintln!("s1: {} run {run}/{RUNS}", spec.name);
            let log = out.join(format!("{}-{stamp}-{run}.stderr", spec.name));
            runs.push(run_child(&exe, spec, &log)?);
        }
        print_summary(spec.name, &runs);
        results.insert(spec.name.to_owned(), Value::Array(runs));
        let path = out.join(format!("results-{stamp}.json"));
        std::fs::write(&path, serde_json::to_string_pretty(&results)?)
            .with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

fn run_child(exe: &Path, spec: &CaseSpec, log: &Path) -> Result<Value> {
    let stderr =
        std::fs::File::create(log).with_context(|| format!("creating {}", log.display()))?;
    let started = Instant::now();
    let mut child = Command::new(exe)
        .args(["--case", spec.name])
        .stdout(Stdio::piped())
        .stderr(stderr)
        .spawn()
        .context("spawning case")?;
    let stdout = child.stdout.take().context("child has no stdout")?;
    let (lines_tx, lines) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if lines_tx.send(line).is_err() {
                break;
            }
        }
    });
    let deadline = started + spec.deadline;
    let mut values = Map::new();
    let mut timed_out = false;
    loop {
        match lines.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(line) => absorb(&mut values, &line),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                timed_out = true;
                if let Err(error) = child.kill() {
                    eprintln!("s1: could not kill {}: {error}", spec.name);
                }
                break;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = child.wait().context("waiting for case")?;
    if reader.join().is_err() {
        bail!("stdout reader panicked");
    }
    for line in lines.try_iter() {
        absorb(&mut values, &line);
    }
    Ok(json!({
        "exit": status.code(),
        "timed_out": timed_out,
        "wall_ms": ms(started.elapsed()),
        "values": values,
    }))
}

fn absorb(values: &mut Map<String, Value>, line: &str) {
    let record = serde_json::from_str::<Value>(line).unwrap_or(Value::Null);
    let (Some(key), Some(value)) = (record["key"].as_str(), record.get("value")) else {
        let unparsed = values
            .entry("unparsed")
            .or_insert_with(|| Value::Array(Vec::new()));
        if let Value::Array(items) = unparsed {
            items.push(Value::String(line.to_owned()));
        }
        return;
    };
    if key == STALL_KEY {
        let previous = values.get(key).and_then(Value::as_f64).unwrap_or(0.0);
        if value.as_f64().is_some_and(|stall| stall > previous) {
            values.insert(key.to_owned(), value.clone());
        }
    } else {
        values.insert(key.to_owned(), value.clone());
    }
}

fn print_summary(name: &str, runs: &[Value]) {
    println!("\n### {name}\n");
    let outcomes: Vec<String> = runs
        .iter()
        .map(|run| {
            format!(
                "exit {} / killed at deadline: {} / {} ms",
                run["exit"], run["timed_out"], run["wall_ms"]
            )
        })
        .collect();
    println!("Runs: {}\n", outcomes.join("; "));
    let keys: BTreeSet<&str> = runs
        .iter()
        .filter_map(|run| run["values"].as_object())
        .flat_map(|values| values.keys().map(String::as_str))
        .collect();
    println!("| key | per run | n | median | min | max | p95 |");
    println!("|---|---|---|---|---|---|---|");
    for key in keys {
        let mut numbers = Vec::new();
        let mut per_run = Vec::new();
        for run in runs {
            match &run["values"][key] {
                Value::Array(items) => {
                    let items: Vec<f64> = items.iter().filter_map(Value::as_f64).collect();
                    per_run.push(format!("med {}", fmt(median(&items))));
                    numbers.extend(items);
                }
                Value::Number(number) => {
                    per_run.push(number.to_string());
                    numbers.extend(number.as_f64());
                }
                Value::Null => per_run.push("-".to_owned()),
                other => per_run.push(other.to_string().replace('|', "/")),
            }
        }
        let p95 = if numbers.len() >= 20 {
            fmt(percentile(&numbers, 0.95))
        } else {
            "-".to_owned()
        };
        println!(
            "| {key} | {} | {} | {} | {} | {} | {p95} |",
            per_run.join(" / "),
            numbers.len(),
            fmt(median(&numbers)),
            fmt(numbers.iter().copied().reduce(f64::min)),
            fmt(numbers.iter().copied().reduce(f64::max)),
        );
    }
}

fn fmt(value: Option<f64>) -> String {
    value.map_or_else(|| "-".to_owned(), |value| format!("{value:.2}"))
}

fn sorted(values: &[f64]) -> Vec<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted
}

fn median(values: &[f64]) -> Option<f64> {
    let sorted = sorted(values);
    let middle = sorted.len() / 2;
    match sorted.len() {
        0 => None,
        len if len % 2 == 0 => Some(f64::midpoint(sorted[middle - 1], sorted[middle])),
        _ => Some(sorted[middle]),
    }
}

fn percentile(values: &[f64], fraction: f64) -> Option<f64> {
    let sorted = sorted(values);
    let rank = (fraction * sorted.len() as f64).ceil() as usize;
    sorted.get(rank.saturating_sub(1)).copied()
}

// ---------------------------------------------------------------- Broadway web client

const OP_GRAB_POINTER: u8 = 0;
const OP_UNGRAB_POINTER: u8 = 1;
const OP_NEW_SURFACE: u8 = 2;
const OP_SHOW_SURFACE: u8 = 3;
const OP_HIDE_SURFACE: u8 = 4;
const OP_RAISE_SURFACE: u8 = 5;
const OP_LOWER_SURFACE: u8 = 6;
const OP_DESTROY_SURFACE: u8 = 7;
const OP_MOVE_RESIZE: u8 = 8;
const OP_SET_TRANSIENT_FOR: u8 = 9;
const OP_DISCONNECTED: u8 = 10;
const OP_SET_SHOW_KEYBOARD: u8 = 12;
const OP_UPLOAD_TEXTURE: u8 = 13;
const OP_RELEASE_TEXTURE: u8 = 14;
const OP_SET_NODES: u8 = 15;
const OP_ROUNDTRIP: u8 = 16;
const EVENT_CONFIGURE_NOTIFY: i64 = 11;
const EVENT_SCREEN_SIZE_CHANGED: i64 = 12;
const EVENT_ROUNDTRIP_NOTIFY: i64 = 14;

/// Connects to the Broadway daemon's WebSocket and answers it the way `broadway.js` does,
/// without rendering anything. With no web client connected, GDK rate-limits every Broadway
/// surface to one frame per second, which would swamp every paint measurement.
fn connect_web_client() -> Result<()> {
    let display = std::env::var("BROADWAY_DISPLAY").context("BROADWAY_DISPLAY is not set")?;
    let number: u16 = display
        .trim_start_matches(':')
        .parse()
        .with_context(|| format!("parsing BROADWAY_DISPLAY {display}"))?;
    let mut stream = TcpStream::connect(("127.0.0.1", 8080 + number))?;
    stream.write_all(
        b"GET /socket HTTP/1.1\r\nHost: 127.0.0.1\r\nUpgrade: websocket\r\n\
          Connection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
          Sec-WebSocket-Version: 13\r\nSec-WebSocket-Protocol: broadway\r\n\
          Origin: http://127.0.0.1\r\n\r\n",
    )?;
    let mut response = Vec::new();
    let mut byte = [0];
    while !response.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte)?;
        response.push(byte[0]);
    }
    let response = String::from_utf8_lossy(&response);
    if !response.starts_with("HTTP/1.1 101") {
        bail!("WebSocket upgrade refused: {response}");
    }
    let mut client = WebClient {
        writer: stream.try_clone()?,
        serial: 0,
        surfaces: HashMap::new(),
    };
    client.send(&[EVENT_SCREEN_SIZE_CHANGED, 1920, 1080, 1])?;
    std::thread::spawn(move || {
        if let Err(error) = client.serve(stream) {
            eprintln!("s1: Broadway web client stopped: {error:#}");
        }
    });
    Ok(())
}

struct WebClient {
    writer: TcpStream,
    serial: u32,
    surfaces: HashMap<u16, [i64; 4]>,
}

impl WebClient {
    fn serve(&mut self, mut stream: TcpStream) -> Result<()> {
        let mut message = Vec::new();
        loop {
            let mut head = [0; 2];
            stream.read_exact(&mut head)?;
            let length = match head[1] & 0x7f {
                126 => {
                    let mut length = [0; 2];
                    stream.read_exact(&mut length)?;
                    u64::from(u16::from_be_bytes(length))
                }
                127 => {
                    let mut length = [0; 8];
                    stream.read_exact(&mut length)?;
                    u64::from_be_bytes(length)
                }
                length => u64::from(length),
            };
            let mut mask = [0; 4];
            if head[1] & 0x80 != 0 {
                stream.read_exact(&mut mask)?;
            }
            let start = message.len();
            message.resize(start + usize::try_from(length)?, 0);
            stream.read_exact(&mut message[start..])?;
            for (index, byte) in message[start..].iter_mut().enumerate() {
                *byte ^= mask[index % 4];
            }
            match head[0] & 0x0f {
                8 => return Ok(()),
                0..=2 if head[0] & 0x80 != 0 => {
                    self.handle(&message)?;
                    message.clear();
                }
                0..=2 => {}
                _ => message.truncate(start),
            }
        }
    }

    fn handle(&mut self, message: &[u8]) -> Result<()> {
        let mut cursor = Cursor {
            data: message,
            pos: 0,
        };
        while cursor.pos < message.len() {
            let op = cursor.u8()?;
            self.serial = cursor.u32()?;
            match op {
                OP_GRAB_POINTER => cursor.skip(3)?,
                OP_UNGRAB_POINTER | OP_DISCONNECTED => {}
                OP_NEW_SURFACE => {
                    let id = cursor.u16()?;
                    let geometry = [
                        i64::from(cursor.i16()?),
                        i64::from(cursor.i16()?),
                        i64::from(cursor.u16()?),
                        i64::from(cursor.u16()?),
                    ];
                    self.surfaces.insert(id, geometry);
                    self.configure(id)?;
                }
                OP_DESTROY_SURFACE => {
                    self.surfaces.remove(&cursor.u16()?);
                }
                OP_SHOW_SURFACE | OP_HIDE_SURFACE | OP_RAISE_SURFACE | OP_LOWER_SURFACE
                | OP_SET_SHOW_KEYBOARD => cursor.skip(2)?,
                OP_MOVE_RESIZE => {
                    let id = cursor.u16()?;
                    let flags = cursor.u8()?;
                    let geometry = self.surfaces.entry(id).or_default();
                    if flags & 1 != 0 {
                        geometry[0] = i64::from(cursor.i16()?);
                        geometry[1] = i64::from(cursor.i16()?);
                    }
                    if flags & 2 != 0 {
                        geometry[2] = i64::from(cursor.u16()?);
                        geometry[3] = i64::from(cursor.u16()?);
                    }
                    self.configure(id)?;
                }
                OP_SET_TRANSIENT_FOR | OP_RELEASE_TEXTURE => cursor.skip(4)?,
                OP_UPLOAD_TEXTURE => {
                    cursor.skip(4)?;
                    let size = cursor.u32()?;
                    cursor.skip(usize::try_from(size)?)?;
                }
                OP_SET_NODES => {
                    cursor.skip(2)?;
                    let words = cursor.u32()?;
                    cursor.skip(usize::try_from(words)? * 4)?;
                }
                OP_ROUNDTRIP => {
                    let id = cursor.u16()?;
                    let tag = cursor.u32()?;
                    self.send(&[EVENT_ROUNDTRIP_NOTIFY, i64::from(id), i64::from(tag)])?;
                }
                other => bail!("unknown Broadway op {other}"),
            }
        }
        Ok(())
    }

    fn configure(&mut self, id: u16) -> Result<()> {
        let [x, y, width, height] = self.surfaces.get(&id).copied().unwrap_or_default();
        self.send(&[EVENT_CONFIGURE_NOTIFY, i64::from(id), x, y, width, height])
    }

    /// Sends `[event, last serial, timestamp, args...]` as big-endian 32-bit words in a masked
    /// binary frame.
    fn send(&mut self, event_and_args: &[i64]) -> Result<()> {
        let (event, args) = event_and_args.split_first().context("empty event")?;
        let words = [*event, i64::from(self.serial), 0]
            .into_iter()
            .chain(args.iter().copied());
        let payload: Vec<u8> = words.flat_map(|word| (word as i32).to_be_bytes()).collect();
        let mask = [0x6e, 0x6f, 0x74, 0x65];
        let mut frame = vec![0x82, 0x80 | u8::try_from(payload.len())?];
        frame.extend(mask);
        frame.extend(
            payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % 4]),
        );
        self.writer.write_all(&frame)?;
        Ok(())
    }
}

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N]> {
        let bytes = self
            .data
            .get(self.pos..self.pos + N)
            .context("truncated Broadway message")?;
        self.pos += N;
        Ok(bytes.try_into()?)
    }

    fn skip(&mut self, count: usize) -> Result<()> {
        if self.pos + count > self.data.len() {
            bail!("truncated Broadway message");
        }
        self.pos += count;
        Ok(())
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.take::<1>()?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take()?))
    }

    fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.take()?))
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take()?))
    }
}

// ---------------------------------------------------------------- child

fn run_case(name: &str) -> Result<()> {
    let spec = CASES
        .iter()
        .find(|spec| spec.name == name)
        .with_context(|| format!("unknown case {name}"))?;
    libadwaita::init().context("initializing libadwaita")?;
    sourceview5::init();
    // The blink animation repaints continuously, and with roundtrips answered at once
    // nothing paces those frames, so it would keep the main thread busy for 10 s.
    gtk::Settings::default()
        .context("no default GtkSettings")?
        .set_gtk_cursor_blink(false);
    let report = Report::new("s1");
    report.record(
        "gtk_version",
        format!(
            "{}.{}.{}",
            gtk::major_version(),
            gtk::minor_version(),
            gtk::micro_version()
        ),
    );
    report.record(
        "gtksourceview_version",
        format!(
            "{}.{}.{}",
            sourceview5::major_version(),
            sourceview5::minor_version(),
            sourceview5::micro_version()
        ),
    );
    if let Some(load) = std::fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|text| text.split_whitespace().next()?.parse::<f64>().ok())
    {
        report.record("loadavg_1m_at_start", load);
    }
    let heartbeat = Heartbeat::start();
    let outcome = match spec.case {
        Case::Open { load, highlight } => open_case(&report, &heartbeat, load, highlight),
        Case::Typing { highlight } => typing_case(&report, &heartbeat, highlight),
        Case::LongLine { formatted, wrap } => long_line_case(&report, &heartbeat, formatted, wrap),
        Case::Snapshot => snapshot_case(&report),
        Case::Threshold {
            kib,
            lines,
            delay_ms,
            tags,
        } => threshold_case(&report, &heartbeat, kib, lines, delay_ms, tags),
    };
    if let Err(error) = &outcome {
        report.record("error", format!("{error:#}"));
    }
    outcome
}

/// Measures main-loop blocking: a 5 ms timer records the longest gap between its runs, and a
/// watchdog thread reports a block still in progress every second, so a stall is visible even
/// when the parent has to kill the process.
struct Heartbeat {
    base: Instant,
    last: Rc<Cell<Instant>>,
    max_gap: Rc<Cell<Duration>>,
    beat: Arc<AtomicU64>,
}

impl Heartbeat {
    fn start() -> Self {
        let base = Instant::now();
        let last = Rc::new(Cell::new(base));
        let max_gap = Rc::new(Cell::new(Duration::ZERO));
        let beat = Arc::new(AtomicU64::new(0));
        glib::timeout_add_local(
            HEARTBEAT,
            glib::clone!(
                #[strong]
                last,
                #[strong]
                max_gap,
                #[strong]
                beat,
                move || {
                    let now = Instant::now();
                    max_gap.set(max_gap.get().max(now - last.get()));
                    last.set(now);
                    beat.store(nanos_since(base, now), Ordering::Relaxed);
                    glib::ControlFlow::Continue
                }
            ),
        );
        let watched = Arc::clone(&beat);
        std::thread::spawn(move || {
            let report = Report::new("s1");
            let mut reported_secs = 0;
            loop {
                std::thread::sleep(Duration::from_millis(100));
                let gap = Duration::from_nanos(
                    nanos_since(base, Instant::now())
                        .saturating_sub(watched.load(Ordering::Relaxed)),
                );
                let secs = gap.as_secs();
                if secs > reported_secs {
                    report.record(STALL_KEY, ms(gap));
                }
                reported_secs = secs;
            }
        });
        Self {
            base,
            last,
            max_gap,
            beat,
        }
    }

    fn reset(&self) {
        let now = Instant::now();
        self.last.set(now);
        self.max_gap.set(Duration::ZERO);
        self.beat
            .store(nanos_since(self.base, now), Ordering::Relaxed);
    }

    /// Longest main-loop block since the last reset, including one still in progress.
    fn max_block(&self) -> Duration {
        self.max_gap.get().max(self.last.get().elapsed())
    }
}

fn nanos_since(base: Instant, now: Instant) -> u64 {
    u64::try_from((now - base).as_nanos()).unwrap_or(u64::MAX)
}

#[derive(Clone, Copy)]
struct Frame {
    start: Instant,
    end: Instant,
}

impl Frame {
    fn work(self) -> Duration {
        self.end - self.start
    }
}

/// Every painted frame of a toplevel, from `before-paint` to `after-paint`.
struct Frames {
    log: Rc<RefCell<Vec<Frame>>>,
}

impl Frames {
    fn attach(widget: &impl IsA<gtk::Widget>) -> Result<Self> {
        let clock = widget
            .frame_clock()
            .context("widget has no frame clock; realize it first")?;
        let log = Rc::new(RefCell::new(Vec::new()));
        let started = Rc::new(Cell::new(None));
        clock.connect_before_paint(glib::clone!(
            #[strong]
            started,
            move |_| started.set(Some(Instant::now()))
        ));
        clock.connect_after_paint(glib::clone!(
            #[strong]
            log,
            move |_| {
                let end = Instant::now();
                let start = started.take().unwrap_or(end);
                log.borrow_mut().push(Frame { start, end });
            }
        ));
        Ok(Self { log })
    }

    /// The first frame whose paint cycle began at or after `since`.
    fn first_since(&self, since: Instant) -> Option<Frame> {
        let log = self.log.borrow();
        log.get(log.partition_point(|frame| frame.start < since))
            .copied()
    }
}

struct Editor {
    window: gtk::Window,
    view: sourceview5::View,
    buffer: sourceview5::Buffer,
    frames: Frames,
}

impl Editor {
    fn new(wrap: gtk::WrapMode) -> Result<Self> {
        let buffer = sourceview5::Buffer::new(None);
        buffer.set_style_scheme(
            sourceview5::StyleSchemeManager::default()
                .scheme("Adwaita")
                .as_ref(),
        );
        let view = sourceview5::View::builder()
            .buffer(&buffer)
            .monospace(true)
            .show_line_numbers(true)
            .highlight_current_line(true)
            .wrap_mode(wrap)
            .build();
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&view)
            .hexpand(true)
            .vexpand(true)
            .build();
        let window = gtk::Window::builder()
            .title("S1")
            .default_width(1000)
            .default_height(700)
            .child(&scrolled)
            .build();
        window.present();
        view.grab_focus();
        wait_frames(&window, 2, Duration::from_secs(10)).context("first window frames")?;
        let frames = Frames::attach(&window)?;
        Ok(Self {
            window,
            view,
            buffer,
            frames,
        })
    }

    fn set_language(&self, id: Option<&str>) -> Result<()> {
        let language = id
            .map(|id| {
                sourceview5::LanguageManager::default()
                    .language(id)
                    .with_context(|| format!("no {id} language"))
            })
            .transpose()?;
        self.buffer.set_highlight_syntax(language.is_some());
        self.buffer.set_language(language.as_ref());
        Ok(())
    }

    fn replace_text(&self, text: &str) {
        self.buffer.begin_irreversible_action();
        self.buffer.set_text(text);
        self.buffer.end_irreversible_action();
        self.buffer.place_cursor(&self.buffer.start_iter());
    }

    fn paint_after(&self, since: Instant, timeout: Duration) -> Result<Frame> {
        run_main_loop_until(timeout, || self.frames.first_since(since).is_some())
            .context("waiting for a painted frame")?;
        self.frames
            .first_since(since)
            .context("painted frame disappeared")
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        self.window.destroy();
    }
}

fn thread_cpu() -> Result<Duration> {
    let stat = std::fs::read_to_string("/proc/thread-self/schedstat")
        .context("reading /proc/thread-self/schedstat")?;
    let nanos = stat
        .split_whitespace()
        .next()
        .context("empty schedstat")?
        .parse()
        .context("parsing schedstat")?;
    Ok(Duration::from_nanos(nanos))
}

/// Iterates the main loop until the main thread has used under 10% of one core for 250 ms,
/// which is when GtkTextView validation and GtkSourceView background highlighting are done.
/// Returns when that quiet window began, or `None` on timeout.
fn wait_settled(timeout: Duration) -> Result<Option<Instant>> {
    let samples = RefCell::new(VecDeque::from([(Instant::now(), thread_cpu()?)]));
    let settled = Cell::new(None);
    let failure = RefCell::new(None);
    let outcome = run_main_loop_until(timeout, || {
        let now = Instant::now();
        let mut samples = samples.borrow_mut();
        if samples
            .back()
            .is_some_and(|(taken, _)| now - *taken < SETTLE_SAMPLE)
        {
            return false;
        }
        match thread_cpu() {
            Ok(cpu) => samples.push_back((now, cpu)),
            Err(error) => {
                failure.replace(Some(error));
                return true;
            }
        }
        while samples.len() > SETTLE_WINDOW + 1 {
            samples.pop_front();
        }
        let (Some(&(first, first_cpu)), Some(&(_, last_cpu))) = (samples.front(), samples.back())
        else {
            return false;
        };
        let quiet = samples.len() == SETTLE_WINDOW + 1
            && (last_cpu - first_cpu).as_secs_f64() < (now - first).as_secs_f64() * SETTLE_MAX_BUSY;
        if quiet {
            settled.set(Some(first));
        }
        quiet
    });
    if let Some(error) = failure.into_inner() {
        return Err(error);
    }
    Ok(outcome.ok().and(settled.get()))
}

fn record_settle(
    report: &Report,
    heartbeat: &Heartbeat,
    started: Instant,
    timeout: Duration,
) -> Result<bool> {
    heartbeat.reset();
    let settled = wait_settled(timeout)?;
    match settled {
        Some(at) => report.record("settled_ms", ms(at - started)),
        None => report.record("settled_ms", format!("not within {timeout:?}")),
    }
    report.record("max_block_settle_ms", ms(heartbeat.max_block()));
    Ok(settled.is_some())
}

fn record_memory(report: &Report, baseline_kib: u64, size: usize) -> Result<()> {
    let end = rss_kib()?;
    let peak = peak_rss_kib()?;
    report.record("rss_baseline_kib", baseline_kib);
    report.record("rss_end_kib", end);
    report.record("rss_peak_kib", peak);
    report.record(
        "ram_ratio_end",
        ratio(end.saturating_sub(baseline_kib) * 1024, size),
    );
    report.record(
        "ram_ratio_peak",
        ratio(peak.saturating_sub(baseline_kib) * 1024, size),
    );
    report.record("rss_end_over_file", ratio(end * 1024, size));
    Ok(())
}

fn line_chunks(text: &str, size: usize) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < bytes.len() {
        let target = (start + size).min(bytes.len());
        let end = bytes[target..]
            .iter()
            .position(|&byte| byte == b'\n')
            .map_or(bytes.len(), |newline| target + newline + 1);
        chunks.push(start..end);
        start = end;
    }
    chunks
}

fn append(buffer: &sourceview5::Buffer, text: &str) {
    buffer.begin_irreversible_action();
    buffer.insert(&mut buffer.end_iter(), text);
    buffer.end_irreversible_action();
}

struct ChunkedLoad {
    first_insert: Duration,
    first_frame: Frame,
    all_frame: Frame,
    max_insert: Duration,
    chunks: usize,
}

/// Inserts the first ~4 MB chunk, waits for it to paint, then inserts the rest one chunk at a
/// time, as the plan's loader would.
fn load_chunked(editor: &Editor, text: String, pacing: Pacing) -> Result<ChunkedLoad> {
    let chunks = line_chunks(&text, CHUNK_BYTES);
    let count = chunks.len();
    let Some(first) = chunks.first().cloned() else {
        bail!("nothing to insert");
    };
    let started = Instant::now();
    append(&editor.buffer, &text[first]);
    editor.buffer.place_cursor(&editor.buffer.start_iter());
    let first_insert = started.elapsed();
    let first_frame = editor
        .paint_after(started, PHASE_TIMEOUT)
        .context("first chunk")?;
    let next = Rc::new(Cell::new(1));
    let max_insert = Rc::new(Cell::new(first_insert));
    let done = Rc::new(Cell::new((count == 1).then(Instant::now)));
    if count > 1 {
        let buffer = editor.buffer.clone();
        let step = glib::clone!(
            #[strong]
            next,
            #[strong]
            max_insert,
            #[strong]
            done,
            move || {
                let index = next.get();
                let inserting = Instant::now();
                append(&buffer, &text[chunks[index].clone()]);
                max_insert.set(max_insert.get().max(inserting.elapsed()));
                next.set(index + 1);
                if index + 1 < chunks.len() {
                    glib::ControlFlow::Continue
                } else {
                    done.set(Some(Instant::now()));
                    glib::ControlFlow::Break
                }
            }
        );
        match pacing {
            Pacing::Idle => glib::idle_add_local_full(glib::Priority::DEFAULT_IDLE, step),
            Pacing::Timer => glib::timeout_add_local(CHUNK_TIMER, step),
        };
    }
    run_main_loop_until(PHASE_TIMEOUT.saturating_sub(started.elapsed()), || {
        done.get().is_some()
    })
    .with_context(|| {
        format!(
            "only {} of {count} chunks inserted within {PHASE_TIMEOUT:?}",
            next.get()
        )
    })?;
    let finished = done.get().context("load finished without a time")?;
    let all_frame = editor
        .paint_after(finished, PHASE_TIMEOUT)
        .context("last chunk")?;
    Ok(ChunkedLoad {
        first_insert,
        first_frame,
        all_frame,
        max_insert: max_insert.get(),
        chunks: count,
    })
}

fn record_chunked(report: &Report, load: &ChunkedLoad) {
    report.record("chunks", load.chunks);
    report.record("first_chunk_insert_ms", ms(load.first_insert));
    report.record("max_chunk_insert_ms", ms(load.max_insert));
    report.record("first_frame_work_ms", ms(load.first_frame.work()));
}

fn open_case(
    report: &Report,
    heartbeat: &Heartbeat,
    load: Load,
    highlight: Highlight,
) -> Result<()> {
    let path = workspace_root()?.join("target/fixtures/big-1m-lines.txt");
    let editor = Editor::new(gtk::WrapMode::None)?;
    editor.view.set_editable(false);
    editor.set_language(matches!(highlight, Highlight::On).then_some("rust"))?;
    let baseline = rss_kib()?;

    let started = Instant::now();
    let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let read = started.elapsed();
    let decoding = Instant::now();
    let text = String::from_utf8(bytes).context("fixture is not UTF-8")?;
    let decode = decoding.elapsed();
    let size = text.len();
    report.record("bytes", size);
    report.record("read_ms", ms(read));
    report.record("decode_ms", ms(decode));

    heartbeat.reset();
    let inserting = Instant::now();
    let (first_frame, all_frame) = match load {
        Load::Chunks(pacing) => {
            let load = load_chunked(&editor, text, pacing)?;
            record_chunked(report, &load);
            (load.first_frame, load.all_frame)
        }
        Load::SetText => {
            editor.replace_text(&text);
            report.record("set_text_ms", ms(inserting.elapsed()));
            drop(text);
            let frame = editor.paint_after(inserting, PHASE_TIMEOUT)?;
            report.record("first_frame_work_ms", ms(frame.work()));
            (frame, frame)
        }
    };
    report.record("first_screen_ms", ms(first_frame.end - started));
    report.record("insert_to_first_paint_ms", ms(first_frame.end - inserting));
    report.record("all_inserted_painted_ms", ms(all_frame.end - started));
    report.record("max_block_load_ms", ms(heartbeat.max_block()));
    report.record("rss_loaded_kib", rss_kib()?);

    if matches!(highlight, Highlight::AfterLoad) {
        heartbeat.reset();
        let enabling = Instant::now();
        editor.set_language(Some("rust"))?;
        report.record("language_call_ms", ms(enabling.elapsed()));
        let frame = editor.paint_after(enabling, PHASE_TIMEOUT)?;
        report.record("language_to_paint_ms", ms(frame.end - enabling));
        report.record("max_block_language_ms", ms(heartbeat.max_block()));
    }
    report.record("buffer_lines", editor.buffer.line_count());
    report.record("buffer_chars", editor.buffer.char_count());

    record_settle(report, heartbeat, started, PHASE_TIMEOUT)?;
    record_memory(report, baseline, size)
}

#[derive(Clone, Copy)]
struct Keystroke {
    target: Instant,
    dispatched: Instant,
    inserted: Duration,
}

/// Types one character every `KEY_INTERVAL` at the cursor the way GtkTextView commits text
/// (user action, interactive insert, scroll the cursor on screen) and pairs each keystroke with
/// the first frame painted after it.
fn type_keys(editor: &Editor) -> Result<Vec<(Keystroke, Frame)>> {
    let strokes = Rc::new(RefCell::new(Vec::new()));
    let scheduled = Instant::now();
    for index in 1..=KEYSTROKES {
        let delay = KEY_INTERVAL * index;
        let target = scheduled + delay;
        let buffer = editor.buffer.clone();
        let view = editor.view.clone();
        let strokes = Rc::clone(&strokes);
        glib::timeout_add_local_once(delay, move || {
            let dispatched = Instant::now();
            buffer.begin_user_action();
            buffer.insert_interactive_at_cursor("x", true);
            buffer.end_user_action();
            view.scroll_mark_onscreen(&buffer.get_insert());
            strokes.borrow_mut().push(Keystroke {
                target,
                dispatched,
                inserted: dispatched.elapsed(),
            });
        });
    }
    let expected = usize::try_from(KEYSTROKES)?;
    run_main_loop_until(KEY_INTERVAL * KEYSTROKES + PHASE_TIMEOUT, || {
        let strokes = strokes.borrow();
        strokes.len() == expected
            && strokes
                .last()
                .is_some_and(|last| editor.frames.first_since(last.dispatched).is_some())
    })
    .context("typing")?;
    strokes
        .borrow()
        .iter()
        .map(|stroke| {
            editor
                .frames
                .first_since(stroke.dispatched)
                .map(|frame| (*stroke, frame))
                .context("keystroke was never painted")
        })
        .collect()
}

fn typing_case(report: &Report, heartbeat: &Heartbeat, highlight: bool) -> Result<()> {
    let text = fixture("medium-10mb.rs")?;
    let editor = Editor::new(gtk::WrapMode::None)?;
    editor.set_language(highlight.then_some("rust"))?;
    let started = Instant::now();
    editor.replace_text(&text);
    let frame = editor.paint_after(started, PHASE_TIMEOUT)?;
    report.record("bytes", text.len());
    report.record("load_to_first_paint_ms", ms(frame.end - started));
    record_settle(report, heartbeat, started, PHASE_TIMEOUT)?;

    for position in ["end", "middle", "line_20", "top"] {
        let buffer = &editor.buffer;
        let line_end = |line: i32| -> Result<gtk::TextIter> {
            let mut iter = buffer.iter_at_line(line).context("no such line")?;
            if !iter.ends_line() {
                iter.forward_to_line_end();
            }
            Ok(iter)
        };
        let iter = match position {
            "end" => buffer.end_iter(),
            "middle" => line_end(buffer.line_count() / 2)?,
            "line_20" => line_end(19)?,
            _ => buffer.start_iter(),
        };
        buffer.place_cursor(&iter);
        editor
            .view
            .scroll_to_mark(&buffer.get_insert(), 0.0, true, 0.0, 0.5);
        editor.view.queue_draw();
        editor.paint_after(Instant::now(), PHASE_TIMEOUT)?;
        wait_settled(PHASE_TIMEOUT)?;
        record_typing(report, heartbeat, &editor, position)?;
    }

    for position in ["line_20_right_after_load", "top_right_after_load"] {
        let reloading = Instant::now();
        editor.replace_text(&text);
        if position.starts_with("line_20") {
            let mut iter = editor.buffer.iter_at_line(19).context("no line 20")?;
            if !iter.ends_line() {
                iter.forward_to_line_end();
            }
            editor.buffer.place_cursor(&iter);
        }
        editor.paint_after(reloading, PHASE_TIMEOUT)?;
        record_typing(report, heartbeat, &editor, position)?;
    }
    Ok(())
}

fn record_typing(
    report: &Report,
    heartbeat: &Heartbeat,
    editor: &Editor,
    position: &str,
) -> Result<()> {
    heartbeat.reset();
    let typing = Instant::now();
    let strokes = type_keys(editor)?;
    let longest_frame_gap = {
        let log = editor.frames.log.borrow();
        let during = &log[log.partition_point(|frame| frame.start < typing)..];
        during
            .windows(2)
            .map(|pair| pair[1].end - pair[0].end)
            .max()
            .unwrap_or_default()
    };
    report.record(
        &format!("{position}_longest_gap_between_frames_ms"),
        ms(longest_frame_gap),
    );
    let series = |measure: fn(&(Keystroke, Frame)) -> Duration| -> Vec<f64> {
        strokes.iter().map(|stroke| ms(measure(stroke))).collect()
    };
    report.record(
        &format!("{position}_insert_to_paint_ms"),
        series(|(stroke, frame)| frame.end - stroke.dispatched),
    );
    report.record(
        &format!("{position}_scheduled_to_paint_ms"),
        series(|(stroke, frame)| frame.end - stroke.target),
    );
    report.record(
        &format!("{position}_insert_call_ms"),
        series(|(stroke, _)| stroke.inserted),
    );
    report.record(
        &format!("{position}_frame_work_ms"),
        series(|(_, frame)| frame.work()),
    );
    report.record(
        &format!("{position}_input_delay_ms"),
        series(|(stroke, _)| stroke.dispatched - stroke.target),
    );
    report.record(
        &format!("{position}_max_block_ms"),
        ms(heartbeat.max_block()),
    );
    Ok(())
}

struct Formatted {
    text: String,
    parse: Duration,
    pretty: Duration,
}

fn pretty_print(text: &str) -> Result<Formatted> {
    let parsing = Instant::now();
    let value: Value = serde_json::from_str(text).context("parsing JSON")?;
    let parse = parsing.elapsed();
    let printing = Instant::now();
    let text = serde_json::to_string_pretty(&value).context("printing JSON")?;
    Ok(Formatted {
        text,
        parse,
        pretty: printing.elapsed(),
    })
}

fn long_line_case(
    report: &Report,
    heartbeat: &Heartbeat,
    formatted: bool,
    wrap: bool,
) -> Result<()> {
    let text = fixture("single-line-10mb.json")?;
    let size = text.len();
    report.record("bytes", size);
    let editor = Editor::new(if wrap {
        gtk::WrapMode::WordChar
    } else {
        gtk::WrapMode::None
    })?;
    let baseline = rss_kib()?;

    let mut max_block = Duration::ZERO;
    let started = if formatted {
        editor.set_language(Some("json"))?;
        heartbeat.reset();
        let started = Instant::now();
        let (sender, results) = mpsc::channel();
        std::thread::spawn(move || sender.send(pretty_print(&text)));
        let received = RefCell::new(None);
        run_main_loop_until(PHASE_TIMEOUT, || {
            if let Ok(result) = results.try_recv() {
                received.replace(Some(result));
            }
            received.borrow().is_some()
        })
        .context("formatting")?;
        let output = received.into_inner().context("formatter sent nothing")??;
        report.record("parse_ms", ms(output.parse));
        report.record("pretty_ms", ms(output.pretty));
        report.record("format_wall_ms", ms(started.elapsed()));
        report.record("formatted_bytes", output.text.len());
        report.record("max_block_format_ms", ms(heartbeat.max_block()));
        max_block = heartbeat.max_block();

        heartbeat.reset();
        let load = load_chunked(&editor, output.text, Pacing::Timer)?;
        record_chunked(report, &load);
        report.record("first_screen_ms", ms(load.first_frame.end - started));
        report.record("all_inserted_painted_ms", ms(load.all_frame.end - started));
        started
    } else {
        editor.set_language(None)?;
        heartbeat.reset();
        let started = Instant::now();
        append(&editor.buffer, &text);
        report.record("insert_call_ms", ms(started.elapsed()));
        editor.buffer.place_cursor(&editor.buffer.start_iter());
        report.record("insert_and_place_cursor_ms", ms(started.elapsed()));
        drop(text);
        let frame = editor.paint_after(started, PHASE_TIMEOUT)?;
        report.record("first_screen_ms", ms(frame.end - started));
        report.record("first_frame_work_ms", ms(frame.work()));
        started
    };
    max_block = max_block.max(heartbeat.max_block());
    report.record("max_block_load_ms", ms(heartbeat.max_block()));
    report.record("buffer_lines", editor.buffer.line_count());
    report.record("rss_loaded_kib", rss_kib()?);

    record_settle(report, heartbeat, started, PHASE_TIMEOUT)?;
    max_block = max_block.max(heartbeat.max_block());
    report.record("max_block_overall_ms", ms(max_block));
    record_memory(report, baseline, size)
}

/// Frames painted per second while the frame clock is asked to update continuously.
fn continuous_frame_rate(editor: &Editor) -> Result<f64> {
    let span = Duration::from_secs(2);
    let clock = editor
        .window
        .frame_clock()
        .context("window has no frame clock")?;
    let since = Instant::now();
    clock.begin_updating();
    let measured = run_main_loop_until(span * 2, || since.elapsed() >= span);
    clock.end_updating();
    let elapsed = measured?;
    let log = editor.frames.log.borrow();
    let painted = log.len() - log.partition_point(|frame| frame.start < since);
    Ok(painted as f64 / elapsed.as_secs_f64())
}

fn snapshot_case(report: &Report) -> Result<()> {
    let editor = Editor::new(gtk::WrapMode::None)?;
    report.record("continuous_frame_rate_hz", continuous_frame_rate(&editor)?);

    let medium = fixture("medium-10mb.rs")?;
    let mut big = fixture("big-1m-lines.txt")?;
    let cut = big.as_bytes()[SNAPSHOT_BIG_BYTES..]
        .iter()
        .position(|&byte| byte == b'\n')
        .map_or(big.len(), |newline| SNAPSHOT_BIG_BYTES + newline + 1);
    big.truncate(cut);

    for (label, text) in [("10mb", medium), ("50mb", big)] {
        let loading = Instant::now();
        editor.replace_text(&text);
        report.record(&format!("set_text_{label}_ms"), ms(loading.elapsed()));
        editor.paint_after(loading, PHASE_TIMEOUT)?;
        let mut taken = Vec::with_capacity(SNAPSHOT_REPEATS);
        let mut copied = Vec::with_capacity(SNAPSHOT_REPEATS);
        for _ in 0..SNAPSHOT_REPEATS {
            let (start, end) = editor.buffer.bounds();
            let taking = Instant::now();
            let snapshot = editor.buffer.text(&start, &end, true);
            taken.push(ms(taking.elapsed()));
            let copying = Instant::now();
            let owned = snapshot.as_str().to_owned();
            copied.push(ms(copying.elapsed()));
            if owned != text {
                bail!("{label} snapshot differs from the inserted text");
            }
        }
        report.record(&format!("snapshot_{label}_bytes"), text.len());
        report.record(&format!("snapshot_{label}_ms"), taken);
        report.record(&format!("snapshot_{label}_to_string_ms"), copied);
    }
    Ok(())
}

/// The 10 MB Rust fixture cut, or repeated, to at least `bytes`, ending at a line end.
fn rust_text(bytes: usize) -> Result<String> {
    let base = fixture("medium-10mb.rs")?;
    let mut text = String::with_capacity(bytes + base.len());
    while text.len() < bytes {
        text.push_str(&base);
    }
    let cut = text.as_bytes()[bytes..]
        .iter()
        .position(|&byte| byte == b'\n')
        .map_or(text.len(), |newline| bytes + newline + 1);
    text.truncate(cut);
    Ok(text)
}

/// Breaks each line at its last space before `width` bytes, until no line is longer or has
/// no space left to break at.
fn reflow(text: &str, width: usize) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let mut rest = line;
        while rest.len() > width {
            match rest[..width].rfind(' ') {
                Some(space) if space > 0 => {
                    out.push_str(&rest[..space]);
                    out.push('\n');
                    rest = &rest[space + 1..];
                }
                _ => break,
            }
        }
        out.push_str(rest);
    }
    out
}

fn threshold_case(
    report: &Report,
    heartbeat: &Heartbeat,
    kib: usize,
    lines: Lines,
    delay_ms: u64,
    tags: bool,
) -> Result<()> {
    let mut text = rust_text(kib * 1024)?;
    match lines {
        Lines::Fixture => {}
        Lines::Words => text = text.replace(' ', "\n"),
        Lines::Width(width) => text = reflow(&text, width),
    }
    report.record("bytes", text.len());
    report.record("lines", line_count(&text));
    let editor = Editor::new(gtk::WrapMode::None)?;
    editor.view.set_editable(false);
    editor.set_language(None)?;

    heartbeat.reset();
    let started = Instant::now();
    let load = load_chunked(&editor, text, Pacing::Timer)?;
    record_chunked(report, &load);
    report.record("first_screen_ms", ms(load.first_frame.end - started));
    report.record("all_inserted_painted_ms", ms(load.all_frame.end - started));
    report.record("max_block_load_ms", ms(heartbeat.max_block()));

    editor.view.set_editable(true);
    let mut iter = editor.buffer.iter_at_line(19).context("no line 20")?;
    if !iter.ends_line() {
        iter.forward_to_line_end();
    }
    editor.buffer.place_cursor(&iter);
    let enabled = Rc::new(Cell::new(None));
    if !tags {
        editor.set_language(Some("rust"))?;
        editor.buffer.set_highlight_syntax(false);
        enabled.set(Some(Instant::now()));
    } else if delay_ms == 0 {
        editor.set_language(Some("rust"))?;
        enabled.set(Some(Instant::now()));
    } else {
        let buffer = editor.buffer.clone();
        let language = sourceview5::LanguageManager::default()
            .language("rust")
            .context("no rust language")?;
        glib::timeout_add_local_once(
            Duration::from_millis(delay_ms),
            glib::clone!(
                #[strong]
                enabled,
                move || {
                    buffer.set_highlight_syntax(true);
                    buffer.set_language(Some(&language));
                    enabled.set(Some(Instant::now()));
                }
            ),
        );
    }
    record_typing(report, heartbeat, &editor, "after_load")?;
    let enabled = enabled
        .get()
        .context("highlighting was never switched on")?;
    heartbeat.reset();
    match wait_settled(PHASE_TIMEOUT)? {
        Some(at) => report.record(
            "highlight_settled_ms",
            ms(at.saturating_duration_since(enabled)),
        ),
        None => report.record(
            "highlight_settled_ms",
            format!("not within {PHASE_TIMEOUT:?}"),
        ),
    }
    Ok(())
}

fn line_count(text: &str) -> usize {
    text.bytes().filter(|&byte| byte == b'\n').count() + 1
}
