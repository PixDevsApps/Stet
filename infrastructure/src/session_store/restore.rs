use super::fsutil::remove_file;
use super::layout::Layout;
use super::{ManifestProblem, ManifestSource, Orphan, Restored, StoreError, new_id};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use stet_domain::session::Session;
use stet_domain::text::normalize_to_lf;

/// The regular files in `backup/`; directories and symlinks are left alone.
#[derive(Debug, Default)]
pub(super) struct Scan {
    /// Complete backups by id.
    pub(super) backups: BTreeMap<String, PathBuf>,
    /// Temp files of interrupted writes.
    pub(super) temps: Vec<PathBuf>,
    /// Files with names the store doesn't write.
    pub(super) strays: Vec<PathBuf>,
}

pub(super) fn scan(layout: &Layout) -> Result<Scan, StoreError> {
    let read_error = |error| StoreError::io("read", layout.backups(), error);
    let mut scan = Scan::default();
    for entry in fs::read_dir(layout.backups()).map_err(read_error)? {
        let entry = entry.map_err(read_error)?;
        if !entry.file_type().map_err(read_error)?.is_file() {
            continue;
        }
        let path = entry.path();
        match entry
            .file_name()
            .to_str()
            .and_then(Layout::parse_backup_name)
        {
            Some((id, false)) => {
                scan.backups.insert(id.to_owned(), path);
            }
            Some((_, true)) => scan.temps.push(path),
            None => scan.strays.push(path),
        }
    }
    Ok(scan)
}

/// Reads the session and repairs what a crash can leave behind; see the module docs.
pub(super) fn restore(layout: &Layout) -> Result<Restored, StoreError> {
    let (mut session, source) = read_manifests(layout);
    if let ManifestSource::Previous { current } | ManifestSource::Empty { current, .. } = &source
        && *current != ManifestProblem::Missing
    {
        set_aside_manifest(layout);
    }
    let scan = scan(layout)?;
    for temp in scan.temps.iter().chain([&layout.manifest_temp()]) {
        if let Err(error) = remove_file(temp) {
            tracing::warn!(path = %temp.display(), %error, "could not delete a leftover temp file");
        }
    }

    session.repair(new_id);
    let mut missing_backups = Vec::new();
    for tab in &mut session.tabs {
        if tab
            .backup
            .as_ref()
            .is_some_and(|id| !scan.backups.contains_key(id))
        {
            missing_backups.push(tab.id.clone());
            tab.backup = None;
        }
    }

    let referenced: BTreeSet<&str> = session.backup_ids().collect();
    let unreferenced = scan
        .backups
        .iter()
        .filter(|(id, _)| !referenced.contains(id.as_str()))
        .map(|(_, path)| path);
    for path in unreferenced.chain(&scan.strays) {
        if let Err(error) = move_to_orphans(layout, path) {
            tracing::warn!(path = %path.display(), %error, "could not move an unreferenced backup to orphaned/");
        }
    }
    let orphans = read_orphans(layout)?;

    tracing::info!(
        source = ?source,
        tabs = session.tabs.len(),
        missing_backups = missing_backups.len(),
        orphans = orphans.len(),
        "session loaded"
    );
    Ok(Restored {
        session,
        source,
        missing_backups,
        orphans,
    })
}

/// `session.json`, else `session.json.prev`, else an empty session.
pub(super) fn read_manifests(layout: &Layout) -> (Session, ManifestSource) {
    match read_manifest(&layout.manifest()) {
        Ok(session) => (session, ManifestSource::Current),
        Err(current) => match read_manifest(&layout.previous()) {
            Ok(session) => (session, ManifestSource::Previous { current }),
            Err(previous) => (
                Session::default(),
                ManifestSource::Empty { current, previous },
            ),
        },
    }
}

fn read_manifest(path: &Path) -> Result<Session, ManifestProblem> {
    let bytes = fs::read(path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => ManifestProblem::Missing,
        kind => ManifestProblem::Unreadable(kind),
    })?;
    parse_manifest(&bytes)
}

/// Parses a manifest, which must be a JSON object; a leading BOM is tolerated.
pub(super) fn parse_manifest(bytes: &[u8]) -> Result<Session, ManifestProblem> {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    if let Some(start) = bytes.iter().position(|byte| !byte.is_ascii_whitespace())
        && bytes[start] != b'{'
    {
        let before = &bytes[..start];
        let line_start = before
            .iter()
            .rposition(|&byte| byte == b'\n')
            .map_or(0, |newline| newline + 1);
        return Err(ManifestProblem::Invalid {
            line: before.iter().filter(|&&byte| byte == b'\n').count() + 1,
            column: start - line_start + 1,
        });
    }
    serde_json::from_slice(bytes).map_err(|error| ManifestProblem::Invalid {
        line: error.line(),
        column: error.column(),
    })
}

/// Keeps an unusable `session.json` as `session.json.corrupt`, so the next write doesn't rotate
/// it into `.prev` over the manifest that was just restored.
fn set_aside_manifest(layout: &Layout) {
    if let Err(error) = fs::rename(layout.manifest(), layout.corrupt())
        && error.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!(%error, "could not set aside the unusable session.json");
    }
}

/// Moves `path` into `orphaned/` under its own name, or `<stem>.<n>.<ext>` if that is taken.
fn move_to_orphans(layout: &Layout, path: &Path) -> io::Result<PathBuf> {
    let name = path.file_name().unwrap_or(OsStr::new("orphan"));
    let mut target = layout.orphans().join(name);
    let mut number = 0;
    while fs::symlink_metadata(&target).is_ok() {
        number += 1;
        target = layout.orphans().join(numbered(name, number));
    }
    fs::rename(path, &target)?;
    Ok(target)
}

fn numbered(name: &OsStr, number: u32) -> OsString {
    let name = Path::new(name);
    let mut numbered = name.file_stem().unwrap_or_default().to_owned();
    numbered.push(format!(".{number}"));
    if let Some(extension) = name.extension() {
        numbered.push(".");
        numbered.push(extension);
    }
    numbered
}

/// Every regular file in `orphaned/`, oldest first.
fn read_orphans(layout: &Layout) -> Result<Vec<Orphan>, StoreError> {
    let read_error = |error| StoreError::io("read", layout.orphans(), error);
    let mut orphans = Vec::new();
    for entry in fs::read_dir(layout.orphans()).map_err(read_error)? {
        let entry = entry.map_err(read_error)?;
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        let path = entry.path();
        match fs::read(&path) {
            Ok(bytes) => orphans.push(Orphan {
                modified: entry
                    .metadata()
                    .and_then(|metadata| metadata.modified())
                    .ok(),
                text: buffer_text(bytes),
                path,
            }),
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "could not read an orphaned backup");
            }
        }
    }
    orphans.sort_by(|a, b| {
        a.modified
            .cmp(&b.modified)
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(orphans)
}

/// Makes a file's bytes safe to insert into a buffer: invalid UTF-8 replaced, line endings
/// normalized to LF and NUL shown as U+2400. Text Stet wrote passes through unchanged.
fn buffer_text(bytes: Vec<u8>) -> String {
    let mut text = String::from_utf8(bytes)
        .unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned());
    if let Cow::Owned(normalized) = normalize_to_lf(&text) {
        text = normalized;
    }
    if text.contains('\0') {
        text = text.replace('\0', "\u{2400}");
    }
    text
}
