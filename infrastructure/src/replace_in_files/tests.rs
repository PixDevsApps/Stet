use super::plan::{Stage, replace_file_with_hook};
use super::*;
use crate::encoding::fixtures::{fixtures, legacy, utf16};
use crate::encoding::{PLACEHOLDER, bom_bytes, decode, detect};
use crate::fs::open_document;
use encoding_rs::{ISO_2022_JP, SHIFT_JIS, UTF_8, UTF_16BE, UTF_16LE, WINDOWS_1252};
use proptest::prelude::*;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::sync::Mutex;
use stet_domain::search::{SearchMode, SearchOptions};
use stet_domain::text::{Eol, normalize_to_lf};

fn query(mode: SearchMode, pattern: &str) -> Query {
    Query::new(pattern, SearchOptions::new(mode).with_match_case(true))
}

fn template(mode: SearchMode, text: &str) -> Template {
    Template::for_mode(mode, text).unwrap()
}

fn matcher(mode: SearchMode, pattern: &str) -> Matcher {
    Matcher::new(&query(mode, pattern)).unwrap()
}

fn options(dir: &Path) -> FileOptions {
    FileOptions {
        max_bytes: MAX_FILE_BYTES,
        save: SaveOptions::new(dir.join("backups")),
        hit_limit: FILE_HIT_LIMIT,
    }
}

fn plan_normal(bytes: &[u8], pattern: &str, replacement: &str) -> Result<Option<Plan>, PlanError> {
    plan(
        bytes,
        &matcher(SearchMode::Normal, pattern),
        &template(SearchMode::Normal, replacement),
        &AtomicBool::new(false),
        FILE_HIT_LIMIT,
    )
}

/// The bytes `text` (with its own line endings, NUL as the placeholder) has in `encoding`,
/// written without Stet's encoder, as an independent oracle.
fn oracle_bytes(
    text: &str,
    encoding: &'static encoding_rs::Encoding,
    bom: bool,
    map_placeholder: bool,
) -> Vec<u8> {
    let text = if map_placeholder {
        text.replace(PLACEHOLDER, "\0")
    } else {
        text.to_owned()
    };
    let body = if encoding == UTF_16LE || encoding == UTF_16BE {
        utf16(&text, encoding == UTF_16LE)
    } else if encoding == UTF_8 {
        text.into_bytes()
    } else {
        legacy(&text, encoding)
    };
    let head: &[u8] = if bom { bom_bytes(encoding) } else { b"" };
    [head, body.as_slice()].concat()
}

/// The first word of `text` (letters or digits, at most four characters).
fn first_word(text: &str) -> Option<String> {
    let start = text.find(|c: char| c.is_alphanumeric())?;
    Some(
        text[start..]
            .chars()
            .take_while(|c| c.is_alphanumeric())
            .take(4)
            .collect(),
    )
}

#[test]
fn every_fixture_keeps_its_encoding_bom_and_line_endings() {
    let mut replaced = 0;
    for fixture in fixtures() {
        let name = fixture.name;
        let detection = detect(&fixture.bytes);
        let raw = decode(&fixture.bytes, detection.encoding, detection.bom);
        let lf = normalize_to_lf(&raw.text).into_owned();
        let Some(word) = first_word(&lf) else {
            assert!(
                plan_normal(&fixture.bytes, "x", "y").is_ok_and(|plan| plan.is_none()),
                "{name}"
            );
            continue;
        };
        // An ASCII replacement with a line break, which takes the file's line ending.
        let result = plan_normal(&fixture.bytes, &word, "<\n>");
        if fixture.binary {
            assert!(
                matches!(result, Err(PlanError::Skip(Skip::Binary, _))),
                "{name}"
            );
            continue;
        }
        if fixture.lossy {
            assert!(
                matches!(result, Err(PlanError::Skip(Skip::Inexact { .. }, _))),
                "{name}: {result:?}"
            );
            continue;
        }
        if detection.encoding == ISO_2022_JP {
            // Its escape sequences carry a state across the replacement's edges.
            assert!(
                matches!(result, Err(PlanError::Skip(Skip::BytesNotKept { .. }, _))),
                "{name}: {result:?}"
            );
            continue;
        }
        let plan = result
            .unwrap_or_else(|error| panic!("{name}: {error:?}"))
            .unwrap_or_else(|| panic!("{name}: “{word}” not found"));
        assert!(plan.changed, "{name}");
        assert_eq!(
            plan.replacements,
            lf.matches(word.as_str()).count(),
            "{name}"
        );
        // Every byte but the replacements' is the file's: the oracle encodes the expected
        // text on its own.
        let eol = fixture.eol.as_str();
        let expected_raw = raw.text.replace(word.as_str(), &format!("<{eol}>"));
        assert_eq!(
            plan.bytes,
            oracle_bytes(
                &expected_raw,
                detection.encoding,
                detection.bom,
                raw.map_placeholder()
            ),
            "{name}"
        );
        // Opened again, the file has the same encoding, BOM and line endings.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        fs::write(&path, &plan.bytes).unwrap();
        let reopened = open_document(&path, &crate::fs::SizePolicy::default()).unwrap();
        assert_eq!(reopened.encoding, detection.encoding, "{name}");
        assert_eq!(reopened.bom, detection.bom, "{name}");
        assert_eq!(reopened.eol, fixture.eol, "{name}");
        assert_eq!(reopened.mixed_eol, fixture.mixed_eol, "{name}");
        assert!(!reopened.lossy_roundtrip, "{name}");
        assert_eq!(
            reopened.text,
            lf.replace(word.as_str(), "<\n>"),
            "{name}: the text an open document would have"
        );
        replaced += 1;
    }
    assert!(replaced >= 18, "only {replaced} fixtures were replaced in");
}

#[test]
fn extended_line_breaks_match_crlf_cr_and_lf_and_untouched_lines_keep_theirs() {
    let extended = |bytes: &[u8], pattern: &str, replacement: &str| {
        plan(
            bytes,
            &matcher(SearchMode::Extended, pattern),
            &template(SearchMode::Extended, replacement),
            &AtomicBool::new(false),
            FILE_HIT_LIMIT,
        )
        .unwrap()
        .unwrap()
        .bytes
    };
    // Joining lines in a CRLF file; the other CRLFs stay.
    assert_eq!(
        extended(b"one\r\ntwo\r\nthree\r\n", "one\\r\\ntwo", "1+2"),
        b"1+2\r\nthree\r\n"
    );
    // A CR-only file: \r\n in the query is a line break, and so is \n in the replacement.
    assert_eq!(
        extended(b"a\rb\rc\r", "b\\r\\n", "B\\nB2\\n"),
        b"a\rB\rB2\rc\r"
    );
    // Mixed endings: each line keeps its own, inserted ones take the most frequent one.
    assert_eq!(
        extended(b"k=1\r\nk=2\nk=3\r\nk=4\r", "=", ": "),
        b"k: 1\r\nk: 2\nk: 3\r\nk: 4\r"
    );
    assert_eq!(
        extended(b"x\r\ny\nz\r\n", "y", "y1\\ny2"),
        b"x\r\ny1\r\ny2\nz\r\n"
    );
}

#[test]
fn regex_templates_replace_as_replace_all_does() {
    let text = b"alice@example\r\nbob@test\r\n";
    let plan = plan(
        text,
        &matcher(SearchMode::Regex, r"(\w+)@(\w+)$"),
        &template(SearchMode::Regex, r"\U$2\E at $1"),
        &AtomicBool::new(false),
        FILE_HIT_LIMIT,
    )
    .unwrap()
    .unwrap();
    assert_eq!(plan.bytes, b"EXAMPLE at alice\r\nTEST at bob\r\n");
    assert_eq!(plan.replacements, 2);
    assert_eq!(plan.hits.len(), 2);
    assert_eq!(plan.hits[1].line_number, 2);
    assert_eq!(plan.hits[1].line_text, "TEST at bob");
    assert_eq!(
        plan.hits[1].match_ranges,
        vec![std::ops::Range { start: 0, end: 11 }]
    );
}

#[test]
fn utf16_and_legacy_encodings_are_written_back_in_their_own_bytes() {
    let le = [b"\xFF\xFE".as_slice(), &utf16("caf\u{e9} \0 bar\r\n", true)].concat();
    let plan = plan_normal(&le, "bar", "b\u{e5}r").unwrap().unwrap();
    assert_eq!(
        plan.bytes,
        [
            b"\xFF\xFE".as_slice(),
            &utf16("caf\u{e9} \0 b\u{e5}r\r\n", true)
        ]
        .concat()
    );
    let be = utf16("UTF-16 without a BOM \0 and text.\nline two.\n", false);
    let plan = plan_normal(&be, "two", "2").unwrap().unwrap();
    assert_eq!(
        plan.bytes,
        utf16("UTF-16 without a BOM \0 and text.\nline 2.\n", false)
    );
    let latin = legacy(
        "Gar\u{e7}on, \u{e7}a co\u{fb}te 5 \u{20ac}.\r\n",
        WINDOWS_1252,
    );
    let plan = plan_normal(&latin, "5 \u{20ac}", "6 \u{20ac}")
        .unwrap()
        .unwrap();
    assert_eq!(
        plan.bytes,
        legacy(
            "Gar\u{e7}on, \u{e7}a co\u{fb}te 6 \u{20ac}.\r\n",
            WINDOWS_1252
        )
    );
    let japanese = legacy("日本語のテキスト。\r\n二行目。\r\n", SHIFT_JIS);
    let plan = plan_normal(&japanese, "二行目", "三行目").unwrap().unwrap();
    assert_eq!(
        plan.bytes,
        legacy("日本語のテキスト。\r\n三行目。\r\n", SHIFT_JIS)
    );
}

#[test]
fn unrepresentable_replacements_skip_the_file() {
    let latin = legacy("Ohm: O, garçon, déjà vu\r\n", WINDOWS_1252);
    assert_eq!(detect(&latin).encoding, WINDOWS_1252);
    let result = plan_normal(&latin, "O", "Ω✓");
    assert_eq!(
        result,
        Err(PlanError::Skip(
            Skip::Unmappable {
                encoding: "windows-1252".into(),
                chars: vec!['Ω', '✓'],
                count: 4,
            },
            2
        ))
    );
    assert!(plan_normal(&latin, "O", "Ô").unwrap().unwrap().changed);
}

#[test]
fn nul_bytes_stay_and_binary_files_are_skipped() {
    // NUL past the sniffed start of a text file stays NUL.
    let mut text = "x".repeat(10_000).into_bytes();
    text.extend_from_slice(b" key\0value\n");
    let plan = plan_normal(&text, "key", "KEY").unwrap().unwrap();
    let mut expected = "x".repeat(10_000).into_bytes();
    expected.extend_from_slice(b" KEY\0value\n");
    assert_eq!(plan.bytes, expected);
    // A placeholder typed into the replacement is written as NUL.
    let plan = plan_normal(&text, "value", "a\u{2400}b").unwrap().unwrap();
    assert!(plan.bytes.ends_with(b"key\0a\0b\n"));
    assert_eq!(
        plan_normal(b"abc\0def\nghi\n", "ghi", "x"),
        Err(PlanError::Skip(Skip::Binary, 1))
    );
    // A binary file without a match is no file to report.
    assert_eq!(plan_normal(b"abc\0def\nghi\n", "absent", "x"), Ok(None));
    // Real U+2400 characters without NUL stay U+2400.
    let plan = plan_normal("a\u{2400}b c\n".as_bytes(), "c", "d")
        .unwrap()
        .unwrap();
    assert_eq!(plan.bytes, "a\u{2400}b d\n".as_bytes());
}

#[test]
fn line_breaks_that_would_join_skip_the_file() {
    let result = plan(
        b"a\rX\nb\r\n",
        &matcher(SearchMode::Normal, "X"),
        &template(SearchMode::Normal, ""),
        &AtomicBool::new(false),
        FILE_HIT_LIMIT,
    );
    assert_eq!(result, Err(PlanError::Skip(Skip::LineBreaksWouldJoin, 1)));
}

#[test]
fn equal_replacements_count_but_write_nothing() {
    let plan = plan_normal(b"same same\n", "same", "same")
        .unwrap()
        .unwrap();
    assert_eq!(plan.replacements, 2);
    assert!(!plan.changed);
    assert!(plan.bytes.is_empty());
    assert_eq!(plan_normal(b"nothing here\n", "absent", "x"), Ok(None));
}

#[test]
fn files_are_saved_safely_through_links_with_their_mode() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target.txt");
    fs::write(&target, "old value\r\n").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
    let link = dir.path().join("link.txt");
    symlink("target.txt", &link).unwrap();
    let outcome = replace_file(
        &link,
        &matcher(SearchMode::Normal, "old"),
        &template(SearchMode::Normal, "new"),
        &options(dir.path()),
        &AtomicBool::new(false),
    );
    assert!(
        matches!(
            outcome,
            FileOutcome::Replaced {
                replacements: 1,
                written: true,
                ..
            }
        ),
        "{outcome:?}"
    );
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_link(&link).unwrap(), Path::new("target.txt"));
    assert_eq!(fs::read(&target).unwrap(), b"new value\r\n");
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o640
    );
}

#[test]
fn read_only_files_are_skipped_with_their_matches() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("locked.txt");
    fs::write(&path, "a a a\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
    if rustix::fs::access(&path, rustix::fs::Access::WRITE_OK).is_ok() {
        return; // Running as root.
    }
    let outcome = replace_file(
        &path,
        &matcher(SearchMode::Normal, "a"),
        &template(SearchMode::Normal, "b"),
        &options(dir.path()),
        &AtomicBool::new(false),
    );
    assert_eq!(
        outcome,
        FileOutcome::Skipped {
            reason: Skip::ReadOnly,
            matches: Some(3)
        }
    );
    assert_eq!(fs::read(&path).unwrap(), b"a a a\n");
}

#[test]
fn a_file_changed_between_reading_and_writing_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("busy.txt");
    fs::write(&path, "before\n").unwrap();
    let outcome = replace_file_with_hook(
        &path,
        &matcher(SearchMode::Normal, "before"),
        &template(SearchMode::Normal, "after"),
        &options(dir.path()),
        &AtomicBool::new(false),
        &mut |stage| {
            assert_eq!(stage, Stage::Planned);
            fs::write(&path, "someone else's text\n").unwrap();
        },
    );
    assert_eq!(
        outcome,
        FileOutcome::Skipped {
            reason: Skip::ChangedMeanwhile,
            matches: Some(1)
        }
    );
    assert_eq!(fs::read(&path).unwrap(), b"someone else's text\n");
}

#[test]
fn large_files_are_skipped_before_reading() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.log");
    fs::write(&path, "x".repeat(2048)).unwrap();
    let outcome = replace_file(
        &path,
        &matcher(SearchMode::Normal, "x"),
        &template(SearchMode::Normal, "y"),
        &FileOptions {
            max_bytes: 1024,
            ..options(dir.path())
        },
        &AtomicBool::new(false),
    );
    assert_eq!(
        outcome,
        FileOutcome::Skipped {
            reason: Skip::TooLarge {
                size: 2048,
                limit: 1024
            },
            matches: None
        }
    );
}

fn tree(root: &Path) {
    let write = |relative: &str, text: &str| {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    };
    fs::create_dir_all(root.join(".git")).unwrap();
    write(".gitignore", "ignored/\n");
    write("a.txt", "TODO one\r\nTODO two\r\n");
    write("sub/b.txt", "nothing\n");
    write("sub/c.md", "TODO in markdown\n");
    write("sub/skip-me.txt", "TODO skipped by the filter\n");
    write("ignored/d.txt", "TODO ignored\n");
    write(".hidden/e.txt", "TODO hidden\n");
    write("open.txt", "TODO open\n");
    write("bin.dat", "TODO\0binary\n");
}

fn run_to_end(request: RifRequest) -> (Vec<RifEvent>, RifSummary) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let handle = start(request, move |event| sink.lock().unwrap().push(event)).unwrap();
    let summary = handle.wait();
    let mut events = Arc::try_unwrap(events).unwrap().into_inner().unwrap();
    events.sort_by_key(|event| format!("{event:?}"));
    (events, summary)
}

fn event_path(event: &RifEvent) -> &Path {
    match event {
        RifEvent::Replaced { path, .. }
        | RifEvent::Skipped { path, .. }
        | RifEvent::Failed { path, .. }
        | RifEvent::OpenDocument { path } => path,
    }
}

#[test]
fn a_run_walks_like_find_in_files_and_hands_back_open_documents() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("tree");
    tree(&root);
    let mut request = RifRequest::new(
        vec![root.clone()],
        query(SearchMode::Normal, "TODO"),
        template(SearchMode::Normal, "DONE"),
        dir.path().join("backups"),
    );
    request.filters = "*.txt *.md *.dat !skip-*".to_owned();
    request.open_documents = vec![root.join("open.txt")];
    let (events, summary) = run_to_end(request);
    let names: Vec<String> = events
        .iter()
        .map(|event| {
            event_path(event)
                .strip_prefix(&root)
                .or_else(|_| event_path(event).strip_prefix(fs::canonicalize(&root).unwrap()))
                .unwrap()
                .display()
                .to_string()
        })
        .collect();
    assert_eq!(events.len(), 4, "{events:?}");
    assert!(names.contains(&"open.txt".to_owned()), "{names:?}");
    assert!(
        events
            .iter()
            .any(|event| matches!(event, RifEvent::OpenDocument { .. }))
    );
    assert!(events.iter().any(|event| matches!(
        event,
        RifEvent::Skipped {
            reason: Skip::Binary,
            ..
        }
    )));
    assert_eq!(
        fs::read(root.join("a.txt")).unwrap(),
        b"DONE one\r\nDONE two\r\n"
    );
    assert_eq!(
        fs::read(root.join("sub/c.md")).unwrap(),
        b"DONE in markdown\n"
    );
    assert_eq!(fs::read(root.join("open.txt")).unwrap(), b"TODO open\n");
    for untouched in ["sub/skip-me.txt", "ignored/d.txt", ".hidden/e.txt"] {
        assert!(
            fs::read_to_string(root.join(untouched))
                .unwrap()
                .starts_with("TODO"),
            "{untouched}"
        );
    }
    assert_eq!(summary.files_changed, 2);
    assert_eq!(summary.replacements, 3);
    assert_eq!(summary.skipped, 1);
    assert_eq!(summary.open_documents, 1);
    assert!(!summary.cancelled);
}

#[test]
fn a_cancelled_run_writes_each_file_whole_or_not_at_all() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("many");
    for index in 0..400 {
        let path = root.join(format!("d{}/f{index}.txt", index % 8));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "alpha beta\r\nalpha gamma\r\n").unwrap();
    }
    let request = RifRequest::new(
        vec![root.clone()],
        query(SearchMode::Normal, "alpha"),
        template(SearchMode::Normal, "omega"),
        dir.path().join("backups"),
    );
    let reported = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&reported);
    let cancel: Arc<Mutex<Option<Arc<Shared>>>> = Arc::new(Mutex::new(None));
    let handle = start(request, {
        let cancel = Arc::clone(&cancel);
        move |event| {
            sink.lock().unwrap().push(event);
            if let Some(shared) = cancel.lock().unwrap().as_ref() {
                shared.cancelled.store(true, Ordering::SeqCst);
                shared.stop.store(true, Ordering::SeqCst);
            }
        }
    })
    .unwrap();
    cancel.lock().unwrap().replace(Arc::clone(&handle.shared));
    handle.cancel();
    let summary = handle.wait();
    assert!(summary.cancelled);
    let reported = reported.lock().unwrap();
    let mut changed = 0;
    for entry in walkdir(&root) {
        let bytes = fs::read(&entry).unwrap();
        if bytes == b"omega beta\r\nomega gamma\r\n" {
            changed += 1;
            assert!(
                reported.iter().any(|event| event_path(event) == entry),
                "{} changed but was not reported",
                entry.display()
            );
        } else {
            assert_eq!(bytes, b"alpha beta\r\nalpha gamma\r\n");
        }
    }
    assert_eq!(changed, summary.files_changed);
    assert!(changed < 400, "the run was not stopped");
}

fn walkdir(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(walkdir(&path));
        } else {
            files.push(path);
        }
    }
    files
}

fn encoded(text: &str, which: usize) -> (Vec<u8>, &'static encoding_rs::Encoding, bool) {
    match which {
        0 => (text.as_bytes().to_vec(), UTF_8, false),
        1 => (
            [b"\xEF\xBB\xBF".as_slice(), text.as_bytes()].concat(),
            UTF_8,
            true,
        ),
        2 => (
            [b"\xFF\xFE".as_slice(), &utf16(text, true)].concat(),
            UTF_16LE,
            true,
        ),
        3 => (
            [b"\xFE\xFF".as_slice(), &utf16(text, false)].concat(),
            UTF_16BE,
            true,
        ),
        _ => (legacy(text, WINDOWS_1252), WINDOWS_1252, false),
    }
}

proptest! {
    #[test]
    fn replacing_changes_only_the_matches(
        parts in proptest::collection::vec(
            prop_oneof![
                Just("ab"), Just("é"), Just("x"), Just(" "), Just("\n"), Just("\r\n"), Just("\r"),
            ],
            1..40,
        ),
        which in 0..5usize,
        word in prop_oneof![Just("ab"), Just("x"), Just("é"), Just("b\nx")],
        replacement in "[ZÆ\n]{0,3}",
    ) {
        let raw = parts.concat();
        let (bytes, _, with_bom) = encoded(&raw, which);
        // The file is read as the editor reads it: detected, which a short legacy text may
        // read as another legacy encoding, and all-ASCII text as UTF-8.
        let detection = detect(&bytes);
        let (encoding, bom) = (detection.encoding, detection.bom);
        prop_assert_eq!(bom, with_bom);
        let decoded = decode(&bytes, encoding, bom);
        let query = Query::extended(word.replace('\n', "\\n"));
        let template = Template::for_mode(SearchMode::Extended, &replacement.replace('\n', "\\n")).unwrap();
        let matcher = Matcher::new(&query).unwrap();
        let lf = normalize_to_lf(&decoded.text).into_owned();
        let expected_lf = matcher
            .replace_all(&lf, &template, None, &AtomicBool::new(false))
            .unwrap()
            .apply(&lf);
        match plan(&bytes, &matcher, &template, &AtomicBool::new(false), 10) {
            Ok(Some(plan)) if plan.changed => {
                let check = decode(&plan.bytes, encoding, bom);
                prop_assert!(!check.had_errors);
                let normalized = normalize_to_lf(&check.text);
                prop_assert_eq!(normalized.as_ref(), expected_lf.as_str());
                prop_assert_eq!(detect(&plan.bytes).bom, bom);
                if bom {
                    prop_assert!(plan.bytes.starts_with(bom_bytes(encoding)));
                }
            }
            Ok(_) => prop_assert_eq!(expected_lf, lf),
            Err(PlanError::Skip(Skip::LineBreaksWouldJoin, _)) => {}
            Err(PlanError::Skip(Skip::Inexact { .. }, _)) => {
                prop_assert!(decoded.had_errors || decoded.lossy_roundtrip);
            }
            // Æ may be missing from the legacy encoding the text was read as.
            Err(PlanError::Skip(Skip::Unmappable { chars, .. }, _)) => {
                prop_assert_eq!(chars, vec!['Æ']);
            }
            Err(error) => prop_assert!(false, "{error:?}"),
        }
    }
}

#[test]
fn lines_report_the_replaced_text() {
    let plan = plan_normal(b"one\r\ntwo two\r\n", "two", "2\n2")
        .unwrap()
        .unwrap();
    assert_eq!(plan.bytes, b"one\r\n2\r\n2 2\r\n2\r\n");
    let lines: Vec<(u64, &str)> = plan
        .hits
        .iter()
        .map(|hit| (hit.line_number, hit.line_text.as_str()))
        .collect();
    assert_eq!(lines, [(2, "2"), (3, "2 2")]);
    assert_eq!(Eol::CrLf.as_str(), "\r\n");
}
