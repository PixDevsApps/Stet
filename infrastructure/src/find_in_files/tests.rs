use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use stet_domain::search::{Query, SearchMode, SearchOptions};

use super::*;

fn write(root: &Path, path: &str, contents: impl AsRef<[u8]>) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

/// Runs a search to the end and returns its summary and the files it reported, as paths
/// relative to `root`, sorted.
fn search(root: &Path, request: FifRequest) -> (FifSummary, Vec<(String, Vec<Hit>)>) {
    let found = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&found);
    let handle = start(request, move |file: FileHits| {
        sink.lock().unwrap().push(file);
    })
    .unwrap();
    let summary = handle.wait();
    let mut files: Vec<(String, Vec<Hit>)> = found
        .lock()
        .unwrap()
        .drain(..)
        .map(|file| {
            let path = file
                .path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            (path, file.hits)
        })
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    (summary, files)
}

fn paths(files: &[(String, Vec<Hit>)]) -> Vec<&str> {
    files.iter().map(|(path, _)| path.as_str()).collect()
}

fn request(root: &Path, query: Query) -> FifRequest {
    FifRequest::new(vec![root.to_path_buf()], query)
}

#[test]
fn filters_include_and_exclude() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for file in [
        "a.rs",
        "b.toml",
        "c.min.js",
        "d.js",
        "target/x.rs",
        "src/target/y.rs",
        "src/lib.rs",
        "logs/z.rs",
        "src/logs2/w.rs",
    ] {
        write(root, file, "needle\n");
    }
    let run = |filters: &str| {
        let mut request = request(root, Query::normal("needle"));
        request.filters = filters.to_owned();
        let (summary, files) = search(root, request);
        assert_eq!(summary.files_matched, files.len());
        paths(&files)
            .iter()
            .map(|path| path.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        run("*.rs *.js !*.min.js !target/"),
        ["a.rs", "d.js", "logs/z.rs", "src/lib.rs", "src/logs2/w.rs"]
    );
    assert_eq!(
        run(r"*.rs !\target"),
        [
            "a.rs",
            "logs/z.rs",
            "src/lib.rs",
            "src/logs2/w.rs",
            "src/target/y.rs"
        ]
    );
    assert_eq!(run(r"*.RS !+\target !+\log*"), ["a.rs", "src/lib.rs"]);
    assert_eq!(run("").len(), 9);
    assert_eq!(run("*.*").len(), 9);
    assert_eq!(run("!*.rs"), ["b.toml", "c.min.js", "d.js"]);
}

#[test]
fn hidden_files_and_folders_are_opt_in() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "seen.txt", "needle");
    write(root, ".dotfile", "needle");
    write(root, ".hidden/inside.txt", "needle");
    let (_, files) = search(root, request(root, Query::normal("needle")));
    assert_eq!(paths(&files), ["seen.txt"]);
    let mut with_hidden = request(root, Query::normal("needle"));
    with_hidden.include_hidden = true;
    let (_, files) = search(root, with_hidden);
    assert_eq!(
        paths(&files),
        [".dotfile", ".hidden/inside.txt", "seen.txt"]
    );
}

#[test]
fn gitignore_can_be_respected_or_not() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join(".git")).unwrap();
    write(root, ".gitignore", "ignored.txt\nbuild/\n");
    write(root, "kept.txt", "needle");
    write(root, "ignored.txt", "needle");
    write(root, "build/out.txt", "needle");
    let (_, files) = search(root, request(root, Query::normal("needle")));
    assert_eq!(paths(&files), ["kept.txt"]);
    let mut everything = request(root, Query::normal("needle"));
    everything.respect_gitignore = false;
    let (_, files) = search(root, everything);
    assert_eq!(paths(&files), ["build/out.txt", "ignored.txt", "kept.txt"]);
}

/// `ignore` lets whitelisted paths override the hidden-file and `.gitignore` rules; filters
/// must never bring those files back (found in M4's self-test with the default `*.*`).
#[test]
fn filters_never_bring_back_hidden_or_ignored_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join(".git")).unwrap();
    write(root, ".gitignore", "ignored.rs\n");
    write(root, "kept.rs", "needle");
    write(root, "ignored.rs", "needle");
    write(root, ".hidden/inside.rs", "needle");
    write(root, ".dot.rs", "needle");
    for filters in ["*.*", "*.rs", "*", "kept.rs ignored.rs .dot.rs", "!*.txt"] {
        let mut filtered = request(root, Query::normal("needle"));
        filtered.filters = filters.to_owned();
        let (summary, files) = search(root, filtered);
        assert_eq!(paths(&files), ["kept.rs"], "{filters}");
        assert_eq!(summary.files_searched, 1, "{filters}");
    }
}

#[test]
fn subfolders_can_be_left_out() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "top.txt", "needle");
    write(root, "sub/deep.txt", "needle");
    let mut shallow = request(root, Query::normal("needle"));
    shallow.include_subfolders = false;
    let (_, files) = search(root, shallow);
    assert_eq!(paths(&files), ["top.txt"]);
    let (_, files) = search(root, request(root, Query::normal("needle")));
    assert_eq!(paths(&files), ["sub/deep.txt", "top.txt"]);
}

#[test]
fn binary_files_are_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "text.txt", "needle\n");
    write(root, "early.bin", b"\0\x01\x02needle\n");
    let mut late = b"needle at the start\n".to_vec();
    late.extend(std::iter::repeat_n(b'x', 100_000));
    late.extend(b"\0 needle\n");
    write(root, "late.bin", &late);
    for query in [Query::normal("needle"), Query::regex(r"needle\s")] {
        let (summary, files) = search(root, request(root, query));
        assert_eq!(paths(&files), ["text.txt"]);
        assert_eq!(summary.files_searched, 3);
    }
}

#[test]
fn byte_order_marks_are_decoded() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut utf16 = vec![0xFF, 0xFE];
    for unit in "first line\r\nthe needle here\r\n".encode_utf16() {
        utf16.extend(unit.to_le_bytes());
    }
    write(root, "utf16.txt", &utf16);
    write(root, "utf8-bom.txt", "\u{FEFF}needle first\n");
    let (_, files) = search(root, request(root, Query::normal("needle")));
    assert_eq!(paths(&files), ["utf16.txt", "utf8-bom.txt"]);
    let utf16_hit = &files[0].1[0];
    assert_eq!(utf16_hit.line_number, 2);
    assert_eq!(utf16_hit.line_text, "the needle here");
    assert_eq!(
        &utf16_hit.line_text[utf16_hit.match_ranges[0].clone()],
        "needle"
    );
    assert_eq!(files[1].1[0].line_text, "needle first");
    assert_eq!(
        files[1].1[0].match_ranges.as_slice(),
        std::slice::from_ref(&(0..6))
    );
}

#[test]
fn only_utf8_and_utf16_with_a_bom_are_read_as_text() {
    let encoding = |label: &str| Encoding::for_label(label.as_bytes()).unwrap();
    assert!(reads_as_text(encoding("utf-8"), false));
    assert!(reads_as_text(encoding("utf-8"), true));
    assert!(reads_as_text(encoding("utf-16le"), true));
    assert!(reads_as_text(encoding("utf-16be"), true));
    assert!(!reads_as_text(encoding("utf-16le"), false));
    assert!(!reads_as_text(encoding("windows-1252"), false));
    assert!(!reads_as_text(encoding("shift_jis"), false));
    assert!(!reads_as_text(encoding("gb18030"), false));
}

#[test]
fn line_breaks_in_queries_match_any_line_ending() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "crlf.txt", "zero\r\none\r\ntwo\r\n");
    write(root, "lf.txt", "zero\none\ntwo\n");
    write(root, "cr.txt", "zero\rone\rtwo\r");
    let queries = [
        Query::extended(r"one\r\ntwo"),
        Query::extended(r"one\ntwo"),
        Query::regex(r"one\r?\ntwo"),
        Query::regex(r"one\Rtwo"),
        Query::normal("one\ntwo"),
    ];
    for query in queries {
        let (_, files) = search(root, request(root, query.clone()));
        assert_eq!(paths(&files), ["cr.txt", "crlf.txt", "lf.txt"], "{query:?}");
        for (path, hits) in &files[1..] {
            assert_eq!(hits[0].line_number, 2, "{query:?} in {path}");
            assert_eq!(hits[0].line_text, "one", "{query:?} in {path}");
        }
        assert_eq!(files[0].1[0].line_number, 1, "lines are counted by LF");
    }
}

#[test]
fn hits_carry_lines_and_ranges() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "a.txt", "one fish\ntwo fish fish\nred\n");
    let options = SearchOptions::new(SearchMode::Regex).with_match_case(true);
    let (summary, files) = search(root, request(root, Query::new(r"\bfish\b", options)));
    assert_eq!(summary.hits, 3);
    assert_eq!(summary.files_matched, 1);
    let hits = &files[0].1;
    assert_eq!(
        (hits[0].line_number, hits[0].match_ranges.clone()),
        (1, std::slice::from_ref(&(4..8)).to_vec())
    );
    assert_eq!(
        (hits[1].line_number, hits[1].match_ranges.clone()),
        (2, vec![4..8, 9..13])
    );
    let whole = SearchOptions::new(SearchMode::Normal).with_whole_word(true);
    let (summary, _) = search(root, request(root, Query::new("fish", whole)));
    assert_eq!(summary.hits, 3);
}

#[test]
fn the_hit_cap_truncates() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for file in ["a.txt", "b.txt", "c.txt"] {
        write(root, file, "hit\n".repeat(100));
    }
    let mut capped = request(root, Query::normal("hit"));
    capped.max_hits = 150;
    let (summary, files) = search(root, capped);
    assert!(summary.truncated);
    assert!(!summary.cancelled);
    assert_eq!(summary.hits, 150);
    let reported: usize = files
        .iter()
        .flat_map(|(_, hits)| hits)
        .map(|hit| hit.match_ranges.len())
        .sum();
    assert_eq!(reported, 150);
}

#[test]
fn open_documents_are_searched_from_memory() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "a.txt", "needle on disk\n");
    write(root, "b.txt", "needle\n");
    write(root, "c.log", "nothing\n");
    let canonical = |name: &str| fs::canonicalize(root.join(name)).unwrap();
    let mut filtered = request(root, Query::normal("needle"));
    filtered.filters = "*.txt".to_owned();
    filtered.open_documents = vec![
        OpenDocument {
            path: canonical("a.txt"),
            text: Arc::from("first\nneedle in memory\nneedle again\n"),
        },
        OpenDocument {
            path: canonical("c.log"),
            text: Arc::from("needle\n"),
        },
        OpenDocument {
            path: root.join("gone.txt"),
            text: Arc::from("needle\n"),
        },
    ];
    let (summary, files) = search(root, filtered.clone());
    assert_eq!(paths(&files), ["a.txt", "b.txt"]);
    let lines: Vec<(u64, &str, usize)> = files[0]
        .1
        .iter()
        .map(|hit| (hit.line_number, hit.line_text.as_str(), hit.column()))
        .collect();
    assert_eq!(lines, [(2, "needle in memory", 0), (3, "needle again", 0)]);
    assert_eq!(summary.hits, 3);
    assert_eq!(summary.files_searched, 2);

    // The buffer's NUL placeholder and an emptied document.
    filtered.query = Query::extended(r"x\0");
    filtered.open_documents[0].text = Arc::from("x\u{2400}\n");
    let (_, files) = search(root, filtered.clone());
    assert_eq!(paths(&files), ["a.txt"]);
    filtered.query = Query::normal("needle");
    filtered.open_documents[0].text = Arc::from("");
    let (_, files) = search(root, filtered.clone());
    assert_eq!(paths(&files), ["b.txt"]);

    // Open documents count against the hit cap.
    filtered.open_documents[0].text = Arc::from("needle needle\nneedle\n");
    filtered.max_hits = 2;
    filtered.filters = "a.txt".to_owned();
    let (summary, files) = search(root, filtered);
    assert!(summary.truncated);
    assert_eq!(summary.hits, 2);
    assert_eq!(files[0].1.len(), 1);
    assert_eq!(files[0].1[0].match_ranges.len(), 2);
}

#[test]
fn bad_queries_and_filters_fail_to_start() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    match start(request(root, Query::regex("(abc")), |_| {}) {
        Err(FifError::Pattern(error)) => assert_eq!(error.offset, 4),
        other => panic!("{:?}", other.map(|_| ())),
    }
    let mut bad_filter = request(root, Query::normal("x"));
    bad_filter.filters = "a[".to_owned();
    assert!(matches!(
        start(bad_filter, |_| {}),
        Err(FifError::Filter(_))
    ));
}

/// Cancels line-by-line and whole-file searches of a tree that takes far longer than 50 ms.
#[test]
fn cancel_stops_within_50_ms() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let line = format!("{}\n", "lorem ipsum dolor sit amet ".repeat(8));
    let contents = line.repeat(1_200);
    for i in 0..600 {
        write(root, &format!("d{}/f{i}.txt", i % 20), &contents);
    }
    for query in [
        Query::regex(r"(\w+ +){3}\d{7}"),
        Query::regex(r"(\w+\s+){3}\d{7}"),
    ] {
        let delivered = Arc::new(AtomicUsize::new(0));
        let sink_delivered = Arc::clone(&delivered);
        let handle = start(request(root, query.clone()), move |_| {
            sink_delivered.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(30));
        assert!(
            !handle.is_finished(),
            "{query:?} finished before it could be cancelled"
        );
        let cancelled = Instant::now();
        handle.cancel();
        let summary = handle.wait();
        let latency = cancelled.elapsed();
        assert!(summary.cancelled);
        assert!(
            latency <= Duration::from_millis(50),
            "{query:?} took {latency:?} to stop"
        );
        println!(
            "{query:?}: stopped {latency:?} after cancel, {} files searched",
            summary.files_searched
        );
    }
}

#[test]
#[ignore = "reads ~/Projects: cargo test --release -p stet-infrastructure -- --ignored --nocapture"]
fn find_in_files_over_the_projects_folder() {
    let home = std::env::home_dir().unwrap();
    let root = home.join("Projects");
    let queries = [
        (Query::normal("fn main"), true),
        (Query::regex(r"TODO|FIXME"), true),
        (Query::regex(r"\bimpl\s+\w+\s+for\b"), true),
        (Query::normal("fn main"), false),
    ];
    for (query, defaults) in queries {
        let first = Arc::new(OnceLock::new());
        let files = Arc::new(AtomicUsize::new(0));
        let (sink_first, sink_files) = (Arc::clone(&first), Arc::clone(&files));
        let mut request = FifRequest::new(vec![root.clone()], query.clone());
        request.include_hidden = !defaults;
        request.respect_gitignore = defaults;
        let started = Instant::now();
        let handle = start(request, move |_| {
            sink_first.get_or_init(|| started.elapsed());
            sink_files.fetch_add(1, Ordering::Relaxed);
        })
        .unwrap();
        let summary = handle.wait();
        let scope = if defaults {
            "respecting .gitignore, no hidden files"
        } else {
            "everything, hidden files too"
        };
        println!(
            "{:?} ({:?}, {scope}): first result after {:?}, done in {:?}; {} files searched, {} matched, {} hits, truncated {}, {} errors",
            query.pattern,
            query.options.mode,
            first.get(),
            summary.elapsed,
            summary.files_searched,
            summary.files_matched,
            summary.hits,
            summary.truncated,
            summary.errors.len(),
        );
        assert_eq!(files.load(Ordering::Relaxed), summary.files_matched);
    }
}
