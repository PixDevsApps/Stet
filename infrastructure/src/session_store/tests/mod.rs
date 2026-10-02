//! Tests against real files in temporary directories.

mod crash;
mod store;

use super::layout::Layout;
use super::writer::{BackupWriter, Phase};
use super::*;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::panic;
use std::sync::Mutex;
use stet_domain::session::TabRecord;

const WAIT: Duration = Duration::from_secs(30);

/// Opens a store, waiting out a transient `Locked`. The flock belongs to the open file, and a
/// process that another test is spawning holds a copy of every descriptor until it execs, so
/// a store dropped just before can still look locked for a moment.
fn open(dir: &Path) -> SessionStore {
    let started = std::time::Instant::now();
    loop {
        match SessionStore::open(dir) {
            Ok(store) => return store,
            Err(StoreError::Locked { .. }) if started.elapsed() < WAIT => {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(error) => panic!("{error}"),
        }
    }
}

/// A store whose writer calls `hook` at each [`Phase`].
fn open_hooked(
    dir: &Path,
    hook: impl Fn(Phase) -> io::Result<()> + Send + Sync + 'static,
) -> SessionStore {
    let writer = BackupWriter::new(Layout::new(dir.to_owned()))
        .unwrap()
        .with_hook(Arc::new(hook));
    SessionStore::start(writer).unwrap()
}

/// Ends the writer thread on the spot, as a crash would: no further file operation runs.
fn crash() -> ! {
    panic::resume_unwind(Box::new("simulated crash"))
}

/// Blocks the writer the first time it reaches a phase, until the test opens the gate.
struct Gate {
    entered: mpsc::Receiver<()>,
    open: mpsc::Sender<()>,
}

fn gated(
    phase: Phase,
) -> (
    impl Fn(Phase) -> io::Result<()> + Send + Sync + 'static,
    Gate,
) {
    let (entered_tx, entered) = mpsc::channel();
    let (open, open_rx) = mpsc::channel::<()>();
    let once = Mutex::new(Some((entered_tx, open_rx)));
    let hook = move |at| {
        let waiting = if at == phase {
            once.lock().unwrap().take()
        } else {
            None
        };
        if let Some((entered, open)) = waiting {
            entered.send(()).unwrap();
            let _ = open.recv();
        }
        Ok(())
    };
    (hook, Gate { entered, open })
}

fn untitled(id: &str, number: u32) -> TabRecord {
    TabRecord {
        id: id.into(),
        display_name: format!("new {number}"),
        untitled_number: Some(number),
        dirty: true,
        backup: Some(id.into()),
        ..TabRecord::default()
    }
}

fn file_tab(id: &str, path: &str, dirty: bool) -> TabRecord {
    TabRecord {
        id: id.into(),
        path: Some(path.into()),
        display_name: path.rsplit('/').next().unwrap().into(),
        dirty,
        backup: dirty.then(|| id.to_owned()),
        ..TabRecord::default()
    }
}

fn session_of(tabs: Vec<TabRecord>) -> Session {
    Session {
        active_tab: (!tabs.is_empty()).then_some(0),
        tabs,
        ..Session::default()
    }
}

fn snapshot(session: Session, backups: &[(&str, &str)], removed: &[&str]) -> Snapshot {
    Snapshot {
        session,
        backups: backups
            .iter()
            .map(|&(id, text)| (id.to_owned(), Arc::from(text)))
            .collect(),
        removed: removed.iter().map(|&id| id.to_owned()).collect(),
        adopted_orphans: Vec::new(),
    }
}

fn tab_ids(session: &Session) -> Vec<&str> {
    session.tabs.iter().map(|tab| tab.id.as_str()).collect()
}

/// The regular files in `dir`, sorted.
fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(Result::unwrap)
        .filter(|entry| entry.file_type().unwrap().is_file())
        .map(|entry| entry.file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// A manifest as it is on disk, parsed without the store.
fn manifest_on_disk(dir: &Path, name: &str) -> Session {
    serde_json::from_slice(&fs::read(dir.join(name)).unwrap()).unwrap()
}
