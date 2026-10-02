use super::*;
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, UNIX_EPOCH};

/// Writes a session of tabs a and c, then starts a second write (a changed, b new, c closed)
/// and crashes at the first `phase` of it.
fn crash_during_second_write(phase: Phase) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let armed = Arc::new(AtomicBool::new(false));
    let store = open_hooked(dir.path(), {
        let armed = Arc::clone(&armed);
        move |at| {
            if at == phase && armed.load(Ordering::SeqCst) {
                crash();
            }
            Ok(())
        }
    });
    store
        .commit(snapshot(
            session_of(vec![untitled("a", 1), untitled("c", 3)]),
            &[("a", "a1"), ("c", "c1")],
            &[],
        ))
        .unwrap();
    store.flush(WAIT).unwrap();
    armed.store(true, Ordering::SeqCst);
    store
        .commit(snapshot(
            session_of(vec![untitled("a", 1), untitled("b", 2)]),
            &[("a", "a2"), ("b", "b1")],
            &["c"],
        ))
        .unwrap();
    assert!(matches!(store.flush(WAIT), Err(StoreError::WriterStopped)));
    drop(store);
    dir
}

/// What a load finds after a crash at `phase`.
struct Expected {
    phase: Phase,
    source: ManifestSource,
    tabs: &'static [&'static str],
    backups: &'static [(&'static str, &'static str)],
    orphans: &'static [&'static str],
}

#[test]
fn a_crash_at_any_phase_leaves_a_consistent_session() {
    let before_manifest = |phase, backups, orphans| Expected {
        phase,
        source: ManifestSource::Current,
        tabs: &["a", "c"],
        backups,
        orphans,
    };
    let cases = [
        before_manifest(Phase::BackupTempWritten, &[("a", "a1"), ("c", "c1")], &[]),
        before_manifest(Phase::BackupRenamed, &[("a", "a2"), ("c", "c1")], &[]),
        before_manifest(Phase::BackupsSynced, &[("a", "a2"), ("c", "c1")], &["b1"]),
        before_manifest(
            Phase::ManifestTempWritten,
            &[("a", "a2"), ("c", "c1")],
            &["b1"],
        ),
        Expected {
            source: ManifestSource::Previous {
                current: ManifestProblem::Missing,
            },
            ..before_manifest(Phase::ManifestRetired, &[("a", "a2"), ("c", "c1")], &["b1"])
        },
        Expected {
            phase: Phase::ManifestRenamed,
            source: ManifestSource::Current,
            tabs: &["a", "b"],
            backups: &[("a", "a2"), ("b", "b1")],
            orphans: &["c1"],
        },
    ];
    for Expected {
        phase,
        source,
        tabs,
        backups,
        orphans,
    } in cases
    {
        let dir = crash_during_second_write(phase);
        let store = open(dir.path());
        let restored = store.load().unwrap();
        assert_eq!(restored.source, source, "{phase:?}");
        assert_eq!(tab_ids(&restored.session), tabs, "{phase:?}");
        assert!(restored.missing_backups.is_empty(), "{phase:?}");
        for &(id, text) in backups {
            assert_eq!(store.read_backup(id).unwrap(), text, "{phase:?}: {id}");
        }
        let mut orphan_texts: Vec<&str> =
            restored.orphans.iter().map(|o| o.text.as_str()).collect();
        orphan_texts.sort_unstable();
        assert_eq!(orphan_texts, orphans, "{phase:?}");
        assert!(
            file_names(&dir.path().join("backup"))
                .iter()
                .all(|name| name.ends_with(".txt")),
            "{phase:?}: temp files left"
        );
        assert!(!dir.path().join("session.json.tmp").exists(), "{phase:?}");
    }
}

/// Set for the coordinator process; names the state directory of the kill rounds.
const KILL_DIR: &str = "STET_SESSION_STORE_KILL_DIR";
/// Set for the processes the coordinator kills.
const KILL_VICTIM: &str = "STET_SESSION_STORE_KILL_VICTIM";

/// A text that shows whether it was written completely: a header naming the id and a
/// generation, filler, and a trailer repeating both.
fn marked_text(id: &str, generation: u128, len: usize) -> String {
    let filler = "the quick brown fox jumps over the lazy dog ✓\n";
    format!(
        "{id} {generation}\n{}end {id} {generation}\n",
        filler.repeat(len / filler.len() + 1)
    )
}

fn is_complete(id: &str, text: &str) -> bool {
    let Some((header, _)) = text.split_once('\n') else {
        return false;
    };
    header
        .strip_prefix(id)
        .and_then(|rest| rest.strip_prefix(' '))
        .is_some_and(|generation| text.ends_with(&format!("\nend {id} {generation}\n")))
}

/// This test binary, set up to run only the test `name`, with `variable` set to `dir`.
fn this_test(name: &str, variable: &str, dir: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", name, "--test-threads=1", "--nocapture"])
        .env(variable, dir)
        .stdin(Stdio::null());
    command
}

/// SIGKILLs a process that commits in a loop, at many different moments. After each kill the
/// manifest must reference only complete backups, and every orphan must be complete too.
///
/// The rounds run in a coordinator process: each round forks, and a fork made here would
/// also copy the other tests' descriptors, holding their locks, until it execs.
#[test]
fn kill_9_never_leaves_a_dangling_reference() {
    let _spawning = crate::test_support::spawn_lock();
    let dir = tempfile::tempdir().unwrap();
    let output = this_test(
        "session_store::tests::crash::kill_9_coordinator",
        KILL_DIR,
        dir.path(),
    )
    .output()
    .unwrap();
    let log = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{log}");
    let summary = log.lines().find(|line| line.starts_with("kill -9:"));
    eprintln!("{}", summary.expect("the coordinator reports its rounds"));
}

/// Runs the rounds of `kill_9_never_leaves_a_dangling_reference`; as a plain test it returns
/// at once.
#[test]
fn kill_9_coordinator() {
    const ROUNDS: u64 = 24;
    let Some(dir) = std::env::var_os(KILL_DIR).map(PathBuf::from) else {
        return;
    };
    let (mut interrupted, mut fallbacks, mut orphans) = (0, 0, 0);
    for round in 0..ROUNDS {
        let mut victim = this_test(
            "session_store::tests::crash::kill_9_victim",
            KILL_VICTIM,
            &dir,
        )
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
        std::thread::sleep(Duration::from_millis(20 + round * 37 % 120));
        victim.kill().unwrap();
        let status = victim.wait().unwrap();
        assert_eq!(
            status.signal(),
            Some(9),
            "round {round}: the victim ended on its own: {status}"
        );

        let backup = dir.join("backup");
        if file_names(&backup)
            .iter()
            .any(|name| name.ends_with(".tmp"))
            || dir.join("session.json.tmp").exists()
        {
            interrupted += 1;
        }
        let store = open(&dir);
        let restored = store.load().unwrap();
        match &restored.source {
            ManifestSource::Current => {}
            ManifestSource::Previous {
                current: ManifestProblem::Missing,
            } => fallbacks += 1,
            ManifestSource::Empty {
                current: ManifestProblem::Missing,
                previous: ManifestProblem::Missing,
            } if round == 0 => {}
            other => panic!("round {round}: unusable manifest: {other:?}"),
        }
        assert!(
            restored.missing_backups.is_empty(),
            "round {round}: missing backups {:?}",
            restored.missing_backups
        );
        for id in restored.session.backup_ids() {
            let text = store.read_backup(id).unwrap();
            assert!(
                is_complete(id, &text),
                "round {round}: backup {id} is incomplete"
            );
        }
        for orphan in &restored.orphans {
            let name = orphan.path.file_name().unwrap().to_str().unwrap();
            let id = name.split('.').next().unwrap();
            assert!(
                is_complete(id, &orphan.text),
                "round {round}: orphan {name} is incomplete"
            );
        }
        assert!(
            file_names(&backup)
                .iter()
                .all(|name| name.ends_with(".txt"))
        );
        orphans = restored.orphans.len();
    }
    eprintln!(
        "kill -9: {ROUNDS} rounds; {interrupted} left temp files, {fallbacks} fell back to \
         session.json.prev, {orphans} orphans at the end"
    );
}

/// Killed by `kill_9_coordinator`: commits snapshots of six tabs, some dirty and some just
/// saved, until it dies. As a plain test it returns at once.
#[test]
fn kill_9_victim() {
    let Some(dir) = std::env::var_os(KILL_VICTIM) else {
        return;
    };
    let store = open(Path::new(&dir));
    store.load().unwrap();
    let epoch = UNIX_EPOCH.elapsed().unwrap().as_nanos();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut step = 0_usize;
    while Instant::now() < deadline {
        let mut next = Snapshot::default();
        for index in 0..6 {
            let id = format!("tab{index}");
            let mut tab = untitled(&id, index as u32 + 1);
            if (step + index).is_multiple_of(5) {
                tab.dirty = false;
                tab.backup = None;
                next.removed.push(id);
            } else {
                let len = 100_000 * (index + 1) + step % 7 * 997;
                let text = marked_text(&id, epoch + step as u128, len);
                next.backups.push((id, Arc::from(text)));
            }
            next.session.tabs.push(tab);
        }
        store.commit(next).unwrap();
        if step.is_multiple_of(3) {
            store.flush(WAIT).unwrap();
        }
        step += 1;
    }
}
