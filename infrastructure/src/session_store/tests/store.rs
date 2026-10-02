use super::*;
use std::collections::BTreeMap;
use std::fs::Permissions;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use stet_domain::session::{Fingerprint, SCHEMA_VERSION, WindowState};
use stet_domain::text::Eol;

#[test]
fn creates_a_private_layout() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("state/stet");
    let store = open(&dir);
    for sub in ["", "backup", "backup/orphaned"] {
        assert_eq!(mode(&dir.join(sub)), 0o700, "{sub:?}");
    }
    assert_eq!(mode(&dir.join("lock")), 0o600);
    assert_eq!(
        fs::read_to_string(dir.join("lock")).unwrap().trim(),
        std::process::id().to_string()
    );

    let tabs = session_of(vec![untitled("a", 1)]);
    for text in ["first", "second"] {
        store
            .commit(snapshot(tabs.clone(), &[("a", text)], &[]))
            .unwrap();
        store.flush(WAIT).unwrap();
    }
    for file in ["session.json", "session.json.prev", "backup/a.txt"] {
        assert_eq!(mode(&dir.join(file)), 0o600, "{file}");
    }
    assert_eq!(
        file_names(&dir),
        ["lock", "session.json", "session.json.prev"]
    );
    assert_eq!(file_names(&dir.join("backup")), ["a.txt"]);
}

#[test]
fn tightens_existing_permissions() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("stet");
    fs::create_dir_all(dir.join("backup/orphaned")).unwrap();
    fs::write(dir.join("lock"), "").unwrap();
    for path in ["", "backup", "backup/orphaned", "lock"] {
        fs::set_permissions(dir.join(path), Permissions::from_mode(0o755)).unwrap();
    }
    let _store = open(&dir);
    for sub in ["", "backup", "backup/orphaned"] {
        assert_eq!(mode(&dir.join(sub)), 0o700, "{sub:?}");
    }
    assert_eq!(mode(&dir.join("lock")), 0o600);
}

#[test]
fn a_second_store_is_locked_out() {
    let dir = tempfile::tempdir().unwrap();
    let first = open(dir.path());
    match SessionStore::open(dir.path()) {
        Err(StoreError::Locked { state_dir, pid }) => {
            assert_eq!(state_dir, dir.path());
            assert_eq!(pid, Some(std::process::id()));
        }
        other => panic!("expected Locked, got {other:?}"),
    }
    drop(first);
    let _second = open(dir.path());
}

#[test]
fn a_first_run_starts_empty() {
    let dir = tempfile::tempdir().unwrap();
    let restored = open(dir.path()).load().unwrap();
    assert_eq!(
        restored.source,
        ManifestSource::Empty {
            current: ManifestProblem::Missing,
            previous: ManifestProblem::Missing,
        }
    );
    assert_eq!(restored.session, Session::default());
    assert!(restored.orphans.is_empty() && restored.missing_backups.is_empty());
}

#[test]
fn round_trips_forty_tabs_and_keeps_backups_of_unopened_tabs() {
    let dir = tempfile::tempdir().unwrap();
    let mut texts = BTreeMap::new();
    let tabs: Vec<TabRecord> = (0..40)
        .map(|index| {
            let id = new_id();
            let number = index + 1;
            match index % 4 {
                0 => {
                    let text = if index == 0 {
                        "a line of a large scratch buffer\n".repeat(64 * 1024)
                    } else {
                        format!("scratch {index}\nsecond line ✓ 日本語\n")
                    };
                    texts.insert(id.clone(), text);
                    untitled(&id, number)
                }
                1 => {
                    texts.insert(id.clone(), format!("fn main() {{}} // edited {index}\n"));
                    TabRecord {
                        encoding: Some("windows-1252".into()),
                        bom: Some(false),
                        eol: Some(Eol::CrLf),
                        language: Some("rust".into()),
                        caret: 7,
                        selection: Some((2, 7)),
                        first_visible_line: 1,
                        disk_fingerprint: Some(Fingerprint {
                            dev: 57,
                            ino: 1000 + u64::from(index),
                            size: 42,
                            mtime_ns: 1_790_000_000_000_000_000,
                        }),
                        ..file_tab(&id, &format!("/home/user/src/file{index}.rs"), true)
                    }
                }
                2 => TabRecord {
                    pinned: true,
                    read_only: true,
                    caret: 3,
                    ..file_tab(&id, &format!("/etc/file{index}.conf"), false)
                },
                _ => TabRecord {
                    dirty: false,
                    backup: None,
                    ..untitled(&id, number)
                },
            }
        })
        .collect();
    let session = Session {
        schema_version: SCHEMA_VERSION,
        tabs,
        active_tab: Some(17),
        window: WindowState {
            width: 1600,
            height: 1000,
            maximized: true,
            zoom: -2,
            split_position: 0,
        },
        recent_files: vec!["/home/user/src/file1.rs".into(), "/etc/hosts".into()],
        find_history: vec!["fn \\w+".into(), "TODO".into()],
        replace_history: vec!["$1".into()],
        directory_history: vec!["/home/user/src".into()],
        filter_history: vec!["*.rs".into()],
        other_tab: None,
    };
    let backups = texts
        .iter()
        .map(|(id, text)| (id.clone(), Arc::from(text.as_str())))
        .collect();

    let store = open(dir.path());
    store.load().unwrap();
    store
        .commit(Snapshot {
            session: session.clone(),
            backups,
            ..Snapshot::default()
        })
        .unwrap();
    drop(store);

    let store = open(dir.path());
    let restored = store.load().unwrap();
    assert_eq!(restored.source, ManifestSource::Current);
    assert_eq!(restored.session, session);
    assert!(restored.missing_backups.is_empty());
    assert!(restored.orphans.is_empty());
    assert_eq!(file_names(&dir.path().join("backup")).len(), 20);
    for id in restored.session.backup_ids() {
        assert_eq!(store.read_backup(id).unwrap(), texts[id]);
    }

    // Tabs that were restored lazily and never opened keep their backups without the app
    // sending their text again.
    store
        .commit(Snapshot {
            session: restored.session.clone(),
            ..Snapshot::default()
        })
        .unwrap();
    store.close(WAIT).unwrap();
    let store = open(dir.path());
    let again = store.load().unwrap();
    assert_eq!(again.session, session);
    for id in again.session.backup_ids() {
        assert_eq!(store.read_backup(id).unwrap(), texts[id]);
    }
}

#[test]
fn falls_back_to_the_previous_manifest_when_session_json_is_truncated() {
    let dir = tempfile::tempdir().unwrap();
    let first = session_of(vec![untitled("a", 1), untitled("b", 2)]);
    let second = session_of(vec![
        untitled("a", 1),
        file_tab("b", "/tmp/b.txt", false),
        untitled("c", 3),
    ]);
    {
        let store = open(dir.path());
        store
            .commit(snapshot(first.clone(), &[("a", "a1"), ("b", "b1")], &[]))
            .unwrap();
        store.flush(WAIT).unwrap();
        store
            .commit(snapshot(second, &[("a", "a2"), ("c", "c1")], &["b"]))
            .unwrap();
    }
    let manifest = dir.path().join("session.json");
    let bytes = fs::read(&manifest).unwrap();
    fs::write(&manifest, &bytes[..bytes.len() / 2]).unwrap();

    let store = open(dir.path());
    let restored = store.load().unwrap();
    assert!(
        matches!(
            restored.source,
            ManifestSource::Previous {
                current: ManifestProblem::Invalid { .. }
            }
        ),
        "{:?}",
        restored.source
    );
    assert_eq!(tab_ids(&restored.session), ["a", "b"]);
    // b's backup was deleted once the second manifest no longer referenced it; a has the
    // newer, complete text; c, unknown to the first manifest, comes back as an orphan.
    assert_eq!(restored.missing_backups, ["b"]);
    assert_eq!(restored.session.tabs[1].backup, None);
    assert_eq!(store.read_backup("a").unwrap(), "a2");
    let orphans: Vec<&str> = restored.orphans.iter().map(|o| o.text.as_str()).collect();
    assert_eq!(orphans, ["c1"]);
    assert!(!manifest.exists());
    assert_eq!(
        fs::read(dir.path().join("session.json.corrupt")).unwrap(),
        bytes[..bytes.len() / 2]
    );

    // The next write doesn't rotate the corrupt manifest over the good `.prev`.
    store
        .commit(snapshot(restored.session.clone(), &[], &[]))
        .unwrap();
    store.flush(WAIT).unwrap();
    assert_eq!(
        tab_ids(&manifest_on_disk(dir.path(), "session.json.prev")),
        tab_ids(&first)
    );
    assert_eq!(
        tab_ids(&manifest_on_disk(dir.path(), "session.json")),
        ["a", "b"]
    );
}

#[test]
fn falls_back_to_the_previous_manifest_when_session_json_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    {
        let store = open(dir.path());
        for text in ["one", "two"] {
            store
                .commit(snapshot(
                    session_of(vec![untitled("a", 1)]),
                    &[("a", text)],
                    &[],
                ))
                .unwrap();
            store.flush(WAIT).unwrap();
        }
    }
    fs::remove_file(dir.path().join("session.json")).unwrap();
    let store = open(dir.path());
    let restored = store.load().unwrap();
    assert_eq!(
        restored.source,
        ManifestSource::Previous {
            current: ManifestProblem::Missing
        }
    );
    assert_eq!(tab_ids(&restored.session), ["a"]);
    assert_eq!(store.read_backup("a").unwrap(), "two");
    assert!(!dir.path().join("session.json.corrupt").exists());
}

#[test]
fn without_a_usable_manifest_every_backup_becomes_an_orphan() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("backup")).unwrap();
    fs::write(dir.path().join("session.json"), "not json").unwrap();
    fs::write(dir.path().join("session.json.prev"), "\n  [1, 2]").unwrap();
    fs::write(dir.path().join("backup/a.txt"), "alpha").unwrap();
    fs::write(dir.path().join("backup/b.txt"), "beta").unwrap();

    let store = open(dir.path());
    let restored = store.load().unwrap();
    assert_eq!(
        restored.source,
        ManifestSource::Empty {
            current: ManifestProblem::Invalid { line: 1, column: 1 },
            previous: ManifestProblem::Invalid { line: 2, column: 3 },
        }
    );
    assert!(restored.session.tabs.is_empty());
    let mut orphans: Vec<&str> = restored.orphans.iter().map(|o| o.text.as_str()).collect();
    orphans.sort_unstable();
    assert_eq!(orphans, ["alpha", "beta"]);
    assert!(file_names(&dir.path().join("backup")).is_empty());
}

#[test]
fn parses_manifests_with_a_bom_and_rejects_other_documents() {
    let session = restore::parse_manifest(b"\xef\xbb\xbf{\"tabs\": [{\"id\": \"a\"}]}").unwrap();
    assert_eq!(tab_ids(&session), ["a"]);
    for (document, line, column) in [
        (&b""[..], 1, 0),
        (b"[]", 1, 1),
        (b"null", 1, 1),
        (b"{\"schema_version\": \"1\"}", 1, 22),
        (b"{\n  \"tabs\": [", 2, 11),
    ] {
        assert_eq!(
            restore::parse_manifest(document),
            Err(ManifestProblem::Invalid { line, column }),
            "{}",
            String::from_utf8_lossy(document)
        );
    }
}

#[test]
fn orphans_stay_until_adopted() {
    let dir = tempfile::tempdir().unwrap();
    let backup = dir.path().join("backup");
    let orphaned = backup.join("orphaned");
    fs::create_dir_all(&orphaned).unwrap();
    fs::write(backup.join("lost.txt"), "lost text").unwrap();
    fs::write(orphaned.join("lost.txt"), "older lost text").unwrap();
    fs::write(backup.join("notes.md"), b"a\r\nb\0c\xff").unwrap();
    fs::write(backup.join("x.txt.tmp"), "half a wri").unwrap();
    let outside = dir.path().join("outside.txt");
    fs::write(&outside, "not an orphan").unwrap();

    let store = open(dir.path());
    let restored = store.load().unwrap();
    let mut texts: Vec<&str> = restored.orphans.iter().map(|o| o.text.as_str()).collect();
    texts.sort_unstable();
    assert_eq!(
        texts,
        ["a\nb\u{2400}c\u{fffd}", "lost text", "older lost text"]
    );
    assert!(file_names(&backup).is_empty());
    assert_eq!(
        file_names(&orphaned),
        ["lost.1.txt", "lost.txt", "notes.md"]
    );
    assert!(
        restored
            .orphans
            .iter()
            .all(|orphan| orphan.modified.is_some())
    );

    // Not adopted yet, so the next load returns them again.
    let again = store.load().unwrap();
    assert_eq!(again.orphans.len(), 3);

    let (adopted, kept): (Vec<&Orphan>, Vec<&Orphan>) = again
        .orphans
        .iter()
        .partition(|orphan| orphan.text != "older lost text");
    let tabs: Vec<TabRecord> = (1..=adopted.len() as u32)
        .map(|number| untitled(&format!("adopted{number}"), number))
        .collect();
    let backups = tabs
        .iter()
        .zip(&adopted)
        .map(|(tab, orphan)| (tab.id.clone(), Arc::from(orphan.text.as_str())))
        .collect();
    let mut adopted_orphans: Vec<PathBuf> =
        adopted.iter().map(|orphan| orphan.path.clone()).collect();
    adopted_orphans.push(outside.clone());
    store
        .commit(Snapshot {
            session: session_of(tabs),
            backups,
            removed: Vec::new(),
            adopted_orphans,
        })
        .unwrap();
    store.flush(WAIT).unwrap();

    assert_eq!(
        file_names(&orphaned),
        [kept[0].path.file_name().unwrap().to_str().unwrap()]
    );
    assert_eq!(fs::read_to_string(&outside).unwrap(), "not an orphan");
    let last = store.load().unwrap();
    assert_eq!(last.orphans.len(), 1);
    assert_eq!(last.session.tabs.len(), 2);
    for (index, id) in ["adopted1", "adopted2"].into_iter().enumerate() {
        assert_eq!(store.read_backup(id).unwrap(), adopted[index].text);
    }
}

#[test]
fn flush_waits_for_every_queued_commit() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    for index in 0..50 {
        let mut session = session_of(vec![untitled("a", 1)]);
        session.find_history = vec![index.to_string()];
        store
            .commit(snapshot(session, &[("a", &format!("text {index}"))], &[]))
            .unwrap();
    }
    store.flush(WAIT).unwrap();
    assert_eq!(
        manifest_on_disk(dir.path(), "session.json").find_history,
        ["49"]
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("backup/a.txt")).unwrap(),
        "text 49"
    );
    let status = store.status();
    assert!(status.error.is_none());
    assert!((1..=50).contains(&status.commits), "{status:?}");
}

#[test]
fn flush_times_out_while_the_writer_is_busy() {
    let dir = tempfile::tempdir().unwrap();
    let (hook, gate) = gated(Phase::BackupTempWritten);
    let store = open_hooked(dir.path(), hook);
    store
        .commit(snapshot(
            session_of(vec![untitled("a", 1)]),
            &[("a", "text")],
            &[],
        ))
        .unwrap();
    gate.entered.recv_timeout(WAIT).unwrap();
    assert!(matches!(
        store.flush(Duration::from_millis(50)),
        Err(StoreError::Timeout)
    ));
    gate.open.send(()).unwrap();
    store.flush(WAIT).unwrap();
    assert_eq!(store.read_backup("a").unwrap(), "text");
}

#[test]
fn queued_snapshots_are_folded_with_the_newest_text_winning() {
    let dir = tempfile::tempdir().unwrap();
    let (hook, gate) = gated(Phase::BackupTempWritten);
    let store = open_hooked(dir.path(), hook);
    let both = || session_of(vec![untitled("a", 1), untitled("b", 2)]);
    store
        .commit(snapshot(both(), &[("a", "a1"), ("b", "b1")], &[]))
        .unwrap();
    gate.entered.recv_timeout(WAIT).unwrap();
    store.commit(snapshot(both(), &[("a", "a2")], &[])).unwrap();
    store
        .commit(snapshot(
            session_of(vec![untitled("a", 1)]),
            &[("a", "a3")],
            &["b"],
        ))
        .unwrap();
    store
        .commit(snapshot(both(), &[("a", "a4"), ("b", "b2")], &[]))
        .unwrap();
    gate.open.send(()).unwrap();
    store.flush(WAIT).unwrap();

    let status = store.status();
    assert_eq!(status.commits, 2);
    let last = status.last_commit.unwrap();
    assert_eq!((last.snapshots, last.backups), (3, 2));
    assert_eq!(last.backup_bytes, 4);
    assert_eq!(store.read_backup("a").unwrap(), "a4");
    assert_eq!(store.read_backup("b").unwrap(), "b2");
    assert_eq!(
        tab_ids(&manifest_on_disk(dir.path(), "session.json")),
        ["a", "b"]
    );
}

#[test]
fn a_failed_write_is_retried_and_leaves_the_last_manifest_alone() {
    let dir = tempfile::tempdir().unwrap();
    let failing = Arc::new(AtomicBool::new(false));
    let store = open_hooked(dir.path(), {
        let failing = Arc::clone(&failing);
        move |phase| {
            if phase == Phase::BackupTempWritten && failing.load(Ordering::SeqCst) {
                Err(io::ErrorKind::StorageFull.into())
            } else {
                Ok(())
            }
        }
    });
    store
        .commit(snapshot(
            session_of(vec![untitled("a", 1)]),
            &[("a", "a1")],
            &[],
        ))
        .unwrap();
    store.flush(WAIT).unwrap();

    failing.store(true, Ordering::SeqCst);
    store
        .commit(snapshot(
            session_of(vec![untitled("a", 1), untitled("b", 2)]),
            &[("a", "a2"), ("b", "b1")],
            &[],
        ))
        .unwrap();
    assert!(matches!(store.flush(WAIT), Err(StoreError::Io { .. })));
    assert!(matches!(store.status().error, Some(StoreError::Io { .. })));
    assert_eq!(
        tab_ids(&manifest_on_disk(dir.path(), "session.json")),
        ["a"]
    );
    assert_eq!(file_names(&dir.path().join("backup")), ["a.txt"]);
    assert_eq!(store.read_backup("a").unwrap(), "a1");

    failing.store(false, Ordering::SeqCst);
    store.flush(WAIT).unwrap();
    assert!(store.status().error.is_none());
    assert_eq!(
        tab_ids(&manifest_on_disk(dir.path(), "session.json")),
        ["a", "b"]
    );
    assert_eq!(store.read_backup("a").unwrap(), "a2");
    assert_eq!(store.read_backup("b").unwrap(), "b1");
}

#[test]
fn referenced_backups_are_kept_and_dangling_references_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let one = session_of(vec![untitled("a", 1)]);
    store
        .commit(snapshot(one.clone(), &[("a", "a1")], &[]))
        .unwrap();
    store.flush(WAIT).unwrap();
    store.commit(snapshot(one, &[], &["a"])).unwrap();
    store.flush(WAIT).unwrap();
    assert_eq!(store.read_backup("a").unwrap(), "a1");

    store
        .commit(snapshot(
            session_of(vec![untitled("a", 1), untitled("never-sent", 2)]),
            &[],
            &[],
        ))
        .unwrap();
    store.flush(WAIT).unwrap();
    let manifest = manifest_on_disk(dir.path(), "session.json");
    assert_eq!(manifest.tabs[0].backup.as_deref(), Some("a"));
    assert_eq!(manifest.tabs[1].backup, None);
}

#[test]
fn ids_never_escape_the_backup_directory() {
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside.txt");
    fs::write(&outside, "keep me").unwrap();
    fs::write(
        dir.path().join("session.json"),
        r#"{"tabs": [{"id": "t", "backup": "../outside"}, {"id": "t"}, {"id": "../x"}]}"#,
    )
    .unwrap();

    let store = open(dir.path());
    let restored = store.load().unwrap();
    assert_eq!(restored.missing_backups, ["t"]);
    assert_eq!(restored.session.tabs[0].backup, None);
    let ids = tab_ids(&restored.session);
    assert_eq!(ids[0], "t");
    assert!(ids[1..].iter().all(|id| is_valid_id(id) && *id != "t"));
    assert_ne!(ids[1], ids[2]);
    assert!(store.backup_path("../outside").is_none());
    assert!(matches!(
        store.read_backup("../outside"),
        Err(StoreError::InvalidId(_))
    ));

    store
        .commit(Snapshot {
            session: restored.session,
            backups: vec![("../evil".into(), Arc::from("x"))],
            removed: vec!["../outside".into()],
            adopted_orphans: vec![
                outside.clone(),
                dir.path().join("backup/orphaned/../../lock"),
            ],
        })
        .unwrap();
    store.flush(WAIT).unwrap();
    assert_eq!(fs::read_to_string(&outside).unwrap(), "keep me");
    assert!(dir.path().join("lock").exists());
    assert!(!dir.path().join("evil.txt").exists());
    assert!(file_names(&dir.path().join("backup")).is_empty());
}

#[test]
fn forget_drafts_deletes_every_backup_and_orphan() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("backup/orphaned")).unwrap();
    fs::write(dir.path().join("backup/orphaned/old.txt"), "old draft").unwrap();
    let store = open(dir.path());
    assert_eq!(store.load().unwrap().orphans.len(), 1);

    let session = Session {
        find_history: vec!["needle".into()],
        active_tab: Some(1),
        ..session_of(vec![
            untitled("u", 1),
            file_tab("f", "/tmp/f.txt", true),
            file_tab("g", "/tmp/g.txt", false),
        ])
    };
    for _ in 0..2 {
        store
            .commit(snapshot(
                session.clone(),
                &[("u", "untitled draft"), ("f", "unsaved edit")],
                &[],
            ))
            .unwrap();
        store.flush(WAIT).unwrap();
    }
    assert!(dir.path().join("session.json.prev").exists());

    store.forget_drafts().unwrap();
    assert!(file_names(&dir.path().join("backup")).is_empty());
    assert!(file_names(&dir.path().join("backup/orphaned")).is_empty());
    assert_eq!(file_names(dir.path()), ["lock", "session.json"]);
    let manifest = manifest_on_disk(dir.path(), "session.json");
    assert_eq!(tab_ids(&manifest), ["f", "g"]);
    assert!(
        manifest
            .tabs
            .iter()
            .all(|tab| !tab.dirty && tab.backup.is_none())
    );
    assert_eq!(manifest.active_tab, Some(0));
    assert_eq!(manifest.find_history, ["needle"]);

    drop(store);
    let restored = open(dir.path()).load().unwrap();
    assert_eq!(restored.session, manifest);
    assert!(restored.orphans.is_empty() && restored.missing_backups.is_empty());
}

/// Printable ASCII in lines of varying length from a fixed xorshift sequence: text a
/// compressing filesystem such as btrfs with zstd can't shrink much.
fn noise_text(len: usize) -> String {
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            if state.is_multiple_of(80) {
                '\n'
            } else {
                char::from(b' ' + (state % 95) as u8)
            }
        })
        .collect()
}

#[test]
fn writes_a_ten_mebibyte_backup_quickly() {
    // On the disk that holds the build, not on tmpfs, so the fsyncs cost what they will.
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target");
    let dir = tempfile::Builder::new()
        .prefix("session-store-")
        .tempdir_in(&target)
        .or_else(|_| tempfile::tempdir())
        .unwrap();
    let text = noise_text(10 * 1024 * 1024);
    let store = open(dir.path());
    let started = Instant::now();
    store
        .commit(snapshot(
            session_of(vec![untitled("big", 1)]),
            &[("big", &text)],
            &[],
        ))
        .unwrap();
    store.flush(WAIT).unwrap();
    let elapsed = started.elapsed();
    let stats = store.status().last_commit.unwrap();
    eprintln!(
        "{} byte backup in {}: backup {:?}, manifest {:?}, write {:?}, commit to flushed {:?}",
        text.len(),
        dir.path().display(),
        stats.backup_time,
        stats.manifest_time,
        stats.total_time,
        elapsed
    );
    assert_eq!(stats.backup_bytes, text.len() as u64);
    assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
    assert_eq!(store.read_backup("big").unwrap(), text);
}

#[test]
fn dropping_the_store_writes_what_is_queued() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    store
        .commit(snapshot(
            session_of(vec![untitled("a", 1)]),
            &[("a", "queued")],
            &[],
        ))
        .unwrap();
    drop(store);
    let store = open(dir.path());
    assert_eq!(tab_ids(&store.load().unwrap().session), ["a"]);
    assert_eq!(store.read_backup("a").unwrap(), "queued");
}

#[test]
fn close_keeps_the_lock_until_a_slow_writer_is_done() {
    let dir = tempfile::tempdir().unwrap();
    let (hook, gate) = gated(Phase::BackupTempWritten);
    let store = open_hooked(dir.path(), hook);
    store
        .commit(snapshot(
            session_of(vec![untitled("a", 1)]),
            &[("a", "slow")],
            &[],
        ))
        .unwrap();
    gate.entered.recv_timeout(WAIT).unwrap();
    assert!(matches!(
        store.close(Duration::from_millis(50)),
        Err(StoreError::Timeout)
    ));
    assert!(matches!(
        SessionStore::open(dir.path()),
        Err(StoreError::Locked { .. })
    ));

    gate.open.send(()).unwrap();
    let store = open(dir.path());
    assert_eq!(store.read_backup("a").unwrap(), "slow");
    assert_eq!(tab_ids(&store.load().unwrap().session), ["a"]);
}

#[test]
fn a_stopped_writer_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_hooked(dir.path(), |_| crash());
    store
        .commit(snapshot(
            session_of(vec![untitled("a", 1)]),
            &[("a", "text")],
            &[],
        ))
        .unwrap();
    assert!(matches!(store.flush(WAIT), Err(StoreError::WriterStopped)));
    // A blocking request returns only once the writer's queue is gone, so later sends fail.
    assert!(matches!(store.load(), Err(StoreError::WriterStopped)));
    assert!(matches!(
        store.commit(Snapshot::default()),
        Err(StoreError::WriterStopped)
    ));
}

#[test]
fn new_ids_are_valid_unique_and_ordered() {
    let ids: Vec<String> = (0..1000).map(|_| new_id()).collect();
    assert!(ids.iter().all(|id| is_valid_id(id)));
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(sorted, ids);
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len());
}

#[test]
fn fingerprints_follow_file_changes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file.txt");
    let now = |path: &Path| fingerprint(&fs::metadata(path).unwrap());
    fs::write(&path, "one").unwrap();
    let saved = now(&path);
    assert_eq!(saved.size, 3);
    assert!(saved.matches(&now(&path)));

    fs::write(&path, "three").unwrap();
    let edited = now(&path);
    assert!(!saved.matches(&edited));

    // Another editor's safe save: same size, a new inode.
    let replacement = dir.path().join("file.txt.new");
    fs::write(&replacement, "THREE").unwrap();
    fs::rename(&replacement, &path).unwrap();
    assert!(!edited.matches(&now(&path)));
}

#[test]
fn debug_output_never_shows_text() {
    let mut session = session_of(vec![untitled("a", 1)]);
    session.find_history = vec!["secret needle".into()];
    let snapshot = snapshot(session, &[("a", "secret text")], &[]);
    let orphan = Orphan {
        path: "/state/backup/orphaned/a.txt".into(),
        text: "secret text".into(),
        modified: None,
    };
    for debug in [format!("{snapshot:?}"), format!("{orphan:?}")] {
        assert!(!debug.contains("secret"), "{debug}");
    }
}

#[test]
fn the_store_can_be_shared_between_threads() {
    fn shareable<T: Send + Sync>() {}
    shareable::<SessionStore>();
    shareable::<Snapshot>();
    shareable::<Restored>();
    shareable::<StoreError>();
}

const IDS: [&str; 4] = ["a", "b", "c", "d"];

/// The snapshots of a well-behaved app: in each step each of four tabs is closed (0), kept (1)
/// or edited (2); keeping a closed tab leaves it closed. Also returns the texts left open.
fn app_snapshots(steps: &[[(u8, String); 4]]) -> (Vec<Snapshot>, BTreeMap<String, String>) {
    let mut open = BTreeMap::new();
    let mut snapshots = Vec::new();
    for step in steps {
        let mut next = Snapshot::default();
        for (id, (action, text)) in IDS.into_iter().zip(step) {
            match action {
                2 => {
                    open.insert(id.to_owned(), text.clone());
                    next.backups.push((id.to_owned(), Arc::from(text.as_str())));
                }
                1 if open.contains_key(id) => {}
                _ => {
                    if open.remove(id).is_some() {
                        next.removed.push(id.to_owned());
                    }
                }
            }
        }
        next.session = session_of(
            (1..)
                .zip(open.keys())
                .map(|(number, id)| untitled(id, number))
                .collect(),
        );
        snapshots.push(next);
    }
    (snapshots, open)
}

/// The backups in `dir`, by id.
fn backup_texts(dir: &Path) -> BTreeMap<String, String> {
    let backup = dir.join("backup");
    file_names(&backup)
        .into_iter()
        .map(|name| {
            let text = fs::read_to_string(backup.join(&name)).unwrap();
            (name.trim_end_matches(".txt").to_owned(), text)
        })
        .collect()
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(64))]

    #[test]
    fn folded_commits_leave_what_one_by_one_commits_leave(
        steps in proptest::collection::vec(
            proptest::array::uniform4((0..3_u8, "[a-z]{0,6}")),
            1..8,
        ),
    ) {
        let (snapshots, expected) = app_snapshots(&steps);

        let one_by_one = tempfile::tempdir().unwrap();
        let store = open(one_by_one.path());
        for snapshot in snapshots.clone() {
            store.commit(snapshot).unwrap();
            store.flush(WAIT).unwrap();
        }
        drop(store);

        let folded = tempfile::tempdir().unwrap();
        let (hook, gate) = gated(Phase::ManifestTempWritten);
        let store = open_hooked(folded.path(), hook);
        let mut snapshots = snapshots.into_iter();
        store.commit(snapshots.next().unwrap()).unwrap();
        gate.entered.recv_timeout(WAIT).unwrap();
        for snapshot in snapshots {
            store.commit(snapshot).unwrap();
        }
        gate.open.send(()).unwrap();
        store.flush(WAIT).unwrap();
        proptest::prop_assert!(store.status().commits <= 2);
        drop(store);

        for dir in [one_by_one.path(), folded.path()] {
            proptest::prop_assert_eq!(&backup_texts(dir), &expected);
            let manifest = manifest_on_disk(dir, "session.json");
            let referenced: Vec<&str> = manifest.backup_ids().collect();
            let open: Vec<&str> = expected.keys().map(String::as_str).collect();
            proptest::prop_assert_eq!(referenced, open);
        }
    }
}
