use std::ffi::OsStr;
use std::fs::{self, DirBuilder, File, Metadata, OpenOptions, Permissions};
use std::io::{self, ErrorKind, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, FileExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rustix::fs::{Access, XattrFlags};
use rustix::io::Errno;
use tempfile::Builder;

use super::meta::{Fingerprint, fingerprint_of};
use crate::encoding::Unmappable;

/// How many symbolic links [`safe_save`] follows by hand to a file that doesn't exist yet.
const MAX_LINKS: usize = 40;

/// The temporary file of an atomic save is `.<name>.<RANDOM>.stet-tmp` next to the target.
const TEMP_SUFFIX: &str = ".stet-tmp";
/// The length of the random part, which tempfile makes of ASCII letters and digits.
const TEMP_RANDOM_LEN: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveOptions {
    /// Where an in-place save keeps a copy of the previous content until the new content is
    /// on disk. Created with mode 0700 when missing; the copy is removed after success.
    pub backup_dir: PathBuf,
}

impl SaveOptions {
    pub fn new(backup_dir: impl Into<PathBuf>) -> Self {
        Self {
            backup_dir: backup_dir.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SaveMethod {
    /// A new file was written next to the target and renamed over it.
    AtomicRename,
    /// The target was overwritten after a backup copy, because it has other hard links, its
    /// directory isn't writable, a rename would lose its owner or ACL, or the rename was
    /// refused (a bind-mounted file, a sticky directory).
    InPlace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveOutcome {
    /// Of the saved file, to recognize this save when the file monitor reports it.
    pub fingerprint: Fingerprint,
    pub method: SaveMethod,
    /// The file written: the path with its symbolic links resolved.
    pub target: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error(transparent)]
    Unmappable(#[from] Unmappable),
    /// The file or its directory isn't writable. The UI shows the read-only banner, which
    /// suggests `SUDO_EDITOR="stet --wait" sudoedit`.
    #[error("permission denied writing {}", .path.display())]
    PermissionDenied { path: PathBuf },
    #[error("{} is on a read-only file system", .path.display())]
    ReadOnlyFilesystem { path: PathBuf },
    #[error("{} is not a regular file", .path.display())]
    NotRegularFile { path: PathBuf },
    #[error("not enough disk space to save {}", .path.display())]
    NoSpace { path: PathBuf },
    /// Nothing was written.
    #[error("could not back up {} before overwriting it: {source}", .path.display())]
    Backup {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// Overwriting in place failed partway; the previous content is in `backup`.
    #[error(
        "saving {} failed partway; the previous content is in {}: {source}",
        .path.display(),
        .backup.display()
    )]
    Interrupted {
        path: PathBuf,
        backup: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not save {}: {source}", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

impl SaveError {
    fn io(path: &Path, source: io::Error) -> Self {
        let path = path.to_path_buf();
        match source.kind() {
            ErrorKind::PermissionDenied => Self::PermissionDenied { path },
            ErrorKind::ReadOnlyFilesystem => Self::ReadOnlyFilesystem { path },
            ErrorKind::StorageFull | ErrorKind::QuotaExceeded => Self::NoSpace { path },
            ErrorKind::IsADirectory => Self::NotRegularFile { path },
            _ => Self::Io { path, source },
        }
    }
}

/// Points where the tests stop a save to check what a crash would leave behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    /// The new content is complete and synced in the temporary file; the target is untouched.
    TempSynced,
    /// The backup copy is synced; the target is untouched.
    BackupSynced,
}

/// Writes `bytes` to the file `path` names without ever leaving it truncated.
///
/// Symbolic links are followed, so a link stays a link and its target gets the content.
/// Normally a temporary file in the target's directory is written, synced, given the
/// target's mode, owner and extended attributes, and renamed over the target, and then the
/// directory is synced: a crash leaves either the old or the new file. When the target has
/// several hard links, its directory isn't writable, its owner or ACL can't be carried over,
/// or the rename is refused, it is overwritten in place instead, after a synced copy to
/// [`SaveOptions::backup_dir`]. A target the user may not write fails with
/// [`SaveError::PermissionDenied`], even when its directory is writable.
pub fn safe_save(
    path: &Path,
    bytes: &[u8],
    options: &SaveOptions,
) -> Result<SaveOutcome, SaveError> {
    save_with_hook(path, bytes, options, &mut |_| {})
}

pub(crate) fn save_with_hook(
    path: &Path,
    bytes: &[u8],
    options: &SaveOptions,
    hook: &mut dyn FnMut(Stage),
) -> Result<SaveOutcome, SaveError> {
    let target = resolve_target(path).map_err(|error| SaveError::io(path, error))?;
    let existing = match fs::metadata(&target) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(SaveError::io(&target, error)),
    };
    if let Some(metadata) = &existing {
        if !metadata.is_file() {
            return Err(SaveError::NotRegularFile { path: target });
        }
        rustix::fs::access(&target, Access::WRITE_OK)
            .map_err(|errno| SaveError::io(&target, errno.into()))?;
    }
    let dir = target.parent().unwrap_or(Path::new("/"));
    let dir_writable = rustix::fs::access(dir, Access::WRITE_OK | Access::EXEC_OK);
    let Some(metadata) = existing else {
        dir_writable.map_err(|errno| SaveError::io(&target, errno.into()))?;
        return atomic(&target, dir, bytes, None, hook).map_err(|error| match error {
            Atomic::Failed(error) => error,
            Atomic::Fallback(reason) => SaveError::Io {
                path: target.clone(),
                source: io::Error::other(reason),
            },
        });
    };
    if metadata.nlink() > 1 {
        tracing::debug!("the file has other hard links; overwriting in place");
        return in_place(&target, bytes, options, hook);
    }
    if dir_writable.is_err() {
        tracing::debug!("the directory isn't writable; overwriting in place");
        return in_place(&target, bytes, options, hook);
    }
    match atomic(&target, dir, bytes, Some(&metadata), hook) {
        Ok(outcome) => Ok(outcome),
        Err(Atomic::Fallback(reason)) => {
            tracing::debug!(reason, "overwriting in place");
            in_place(&target, bytes, options, hook)
        }
        Err(Atomic::Failed(error)) => Err(error),
    }
}

/// The file `path` names with every symbolic link resolved, also when it doesn't exist
/// yet, as for a new file or a dangling link.
fn resolve_target(path: &Path) -> io::Result<PathBuf> {
    match fs::canonicalize(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        result => return result,
    }
    let mut current = std::path::absolute(path)?;
    for _ in 0..MAX_LINKS {
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let link = fs::read_link(&current)?;
                current = current.parent().unwrap_or(Path::new("/")).join(link);
            }
            Ok(_) => return fs::canonicalize(&current),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let name = current
                    .file_name()
                    .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "no file name"))?;
                let parent = current.parent().unwrap_or(Path::new("/"));
                return Ok(fs::canonicalize(parent)?.join(name));
            }
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        ErrorKind::InvalidInput,
        "too many levels of symbolic links",
    ))
}

enum Atomic {
    /// Keeping the original's owner or ACL, or replacing it at all, needs an in-place save.
    Fallback(&'static str),
    Failed(SaveError),
}

fn atomic(
    target: &Path,
    dir: &Path,
    bytes: &[u8],
    original: Option<&Metadata>,
    hook: &mut dyn FnMut(Stage),
) -> Result<SaveOutcome, Atomic> {
    let failed = |error: io::Error| Atomic::Failed(SaveError::io(target, error));
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let mode = if original.is_some() { 0o600 } else { 0o666 };
    let mut temp = Builder::new()
        .prefix(&format!(".{name}."))
        .rand_bytes(TEMP_RANDOM_LEN)
        .suffix(TEMP_SUFFIX)
        .permissions(Permissions::from_mode(mode))
        .tempfile_in(dir)
        .map_err(failed)?;
    temp.write_all(bytes).map_err(failed)?;
    if let Some(original) = original {
        keep_owner(temp.as_file(), original).map_err(|error| match error.kind() {
            ErrorKind::PermissionDenied => Atomic::Fallback("only root can keep another owner"),
            _ => failed(error),
        })?;
        temp.as_file()
            .set_permissions(Permissions::from_mode(original.mode() & 0o7777))
            .map_err(failed)?;
        copy_xattrs(target, temp.as_file())?;
    }
    temp.as_file().sync_all().map_err(failed)?;
    hook(Stage::TempSynced);
    let file = temp
        .persist(target)
        .map_err(|error| match error.error.kind() {
            ErrorKind::PermissionDenied | ErrorKind::ResourceBusy | ErrorKind::Unsupported => {
                Atomic::Fallback("the file can't be renamed over")
            }
            _ => failed(error.error),
        })?;
    sync_dir(dir);
    let metadata = file.metadata().map_err(failed)?;
    Ok(SaveOutcome {
        fingerprint: fingerprint_of(&metadata),
        method: SaveMethod::AtomicRename,
        target: target.to_path_buf(),
    })
}

/// Gives `file` the owner and group of `original` where they differ. Changing the group
/// works when the user belongs to it; changing the owner needs root.
fn keep_owner(file: &File, original: &Metadata) -> io::Result<()> {
    let current = file.metadata()?;
    let uid = (current.uid() != original.uid()).then_some(original.uid());
    let gid = (current.gid() != original.gid()).then_some(original.gid());
    if uid.is_none() && gid.is_none() {
        return Ok(());
    }
    std::os::unix::fs::fchown(file, uid, gid)
}

/// Copies extended attributes, which a rename would drop. Attributes that need privileges
/// (`security.*`, `trusted.*`) are skipped; an ACL (`system.*`) that can't be copied, or
/// attributes that can't be read, make the save fall back to overwriting in place.
fn copy_xattrs(from: &Path, to: &File) -> Result<(), Atomic> {
    const FALLBACK: Atomic = Atomic::Fallback("the extended attributes can't be copied");
    let names = match read_xattr(|buffer| rustix::fs::listxattr(from, buffer)) {
        Ok(names) => names,
        Err(Errno::OPNOTSUPP | Errno::NOSYS) => return Ok(()),
        Err(_) => return Err(FALLBACK),
    };
    for name in names
        .split(|&byte| byte == 0)
        .filter(|name| !name.is_empty())
    {
        let name = OsStr::from_bytes(name);
        let value = match read_xattr(|buffer| rustix::fs::getxattr(from, name, buffer)) {
            Ok(value) => value,
            Err(Errno::NODATA) => continue,
            Err(_) => return Err(FALLBACK),
        };
        if rustix::fs::fsetxattr(to, name, &value, XattrFlags::empty()).is_err()
            && name.as_bytes().starts_with(b"system.")
        {
            return Err(FALLBACK);
        }
    }
    Ok(())
}

/// Calls a size-then-fill xattr function until the value fits.
fn read_xattr(
    mut call: impl FnMut(&mut [u8]) -> rustix::io::Result<usize>,
) -> rustix::io::Result<Vec<u8>> {
    loop {
        let size = call(&mut [])?;
        let mut buffer = vec![0; size];
        match call(&mut buffer) {
            Ok(len) => {
                buffer.truncate(len);
                return Ok(buffer);
            }
            Err(Errno::RANGE) => continue,
            Err(errno) => return Err(errno),
        }
    }
}

fn in_place(
    target: &Path,
    bytes: &[u8],
    options: &SaveOptions,
    hook: &mut dyn FnMut(Stage),
) -> Result<SaveOutcome, SaveError> {
    let file = OpenOptions::new()
        .write(true)
        .open(target)
        .map_err(|error| SaveError::io(target, error))?;
    let backup = backup_copy(target, &options.backup_dir).map_err(|source| SaveError::Backup {
        path: target.to_path_buf(),
        source,
    })?;
    hook(Stage::BackupSynced);
    let interrupted = |source: io::Error| SaveError::Interrupted {
        path: target.to_path_buf(),
        backup: backup.clone(),
        source,
    };
    file.write_all_at(bytes, 0).map_err(interrupted)?;
    file.set_len(bytes.len() as u64).map_err(interrupted)?;
    file.sync_all().map_err(interrupted)?;
    let metadata = file.metadata().map_err(interrupted)?;
    if let Err(error) = fs::remove_file(&backup) {
        tracing::warn!(%error, "could not remove the backup copy after saving");
    }
    Ok(SaveOutcome {
        fingerprint: fingerprint_of(&metadata),
        method: SaveMethod::InPlace,
        target: target.to_path_buf(),
    })
}

/// Copies the target into `backup_dir` as a private file and syncs it.
fn backup_copy(target: &Path, backup_dir: &Path) -> io::Result<PathBuf> {
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(backup_dir)?;
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let mut backup = Builder::new()
        .prefix(&format!("{name}."))
        .suffix(".bak")
        .tempfile_in(backup_dir)?;
    io::copy(&mut File::open(target)?, backup.as_file_mut())?;
    backup.as_file().sync_all()?;
    sync_dir(backup_dir);
    let (_, path) = backup.keep().map_err(|error| error.error)?;
    Ok(path)
}

/// Removes the temporary files that interrupted saves of `target` left next to it
/// (`.<name>.<RANDOM>.stet-tmp`, see [`safe_save`]) when they were last modified at least
/// `min_age` ago, so that a save another process is running keeps its file. Other files,
/// including the temporary files of other targets, are never touched. Returns how many were
/// removed; a file that can't be inspected or removed is skipped.
pub fn remove_stale_temps(target: &Path, min_age: Duration) -> usize {
    let (Some(dir), Some(name)) = (target.parent(), target.file_name()) else {
        return 0;
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let prefix = [b".", name.as_bytes(), b"."].concat();
    let now = SystemTime::now();
    let mut removed = 0;
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(random) = file_name
            .as_bytes()
            .strip_prefix(prefix.as_slice())
            .and_then(|rest| rest.strip_suffix(TEMP_SUFFIX.as_bytes()))
        else {
            continue;
        };
        if random.len() != TEMP_RANDOM_LEN || !random.iter().all(u8::is_ascii_alphanumeric) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let old_enough = metadata
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= min_age);
        if metadata.is_file() && old_enough && fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Makes a rename or a new directory entry durable. A failure is logged, not returned: the
/// content itself is already synced.
fn sync_dir(dir: &Path) {
    if let Err(error) = File::open(dir).and_then(|dir| dir.sync_all()) {
        tracing::warn!(%error, "could not sync the directory");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::fingerprint;
    use std::os::unix::fs::symlink;
    use std::panic::AssertUnwindSafe;

    struct Setup {
        dir: tempfile::TempDir,
        options: SaveOptions,
    }

    impl Setup {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let options = SaveOptions::new(dir.path().join("backups"));
            Self { dir, options }
        }

        fn path(&self, name: &str) -> PathBuf {
            self.dir.path().join(name)
        }

        fn file(&self, name: &str, content: &str, mode: u32) -> PathBuf {
            let path = self.path(name);
            fs::write(&path, content).unwrap();
            fs::set_permissions(&path, Permissions::from_mode(mode)).unwrap();
            path
        }

        fn save(&self, path: &Path, content: &str) -> Result<SaveOutcome, SaveError> {
            safe_save(path, content.as_bytes(), &self.options)
        }

        fn leftovers(&self) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(self.dir.path())
                .unwrap()
                .chain(fs::read_dir(&self.options.backup_dir).into_iter().flatten())
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with(".stet-tmp") || name.ends_with(".bak"))
                .collect();
            names.sort();
            names
        }
    }

    fn mode_of(path: &Path) -> u32 {
        fs::metadata(path).unwrap().mode() & 0o7777
    }

    /// Permission tests mean nothing for root, who may write anything.
    fn denied(path: &Path) -> bool {
        rustix::fs::access(path, Access::WRITE_OK).is_err()
    }

    #[test]
    fn replaces_the_file_atomically_and_keeps_its_mode() {
        let setup = Setup::new();
        let path = setup.file("notes.txt", "old", 0o640);
        let before = fingerprint(&path).unwrap();
        let outcome = setup.save(&path, "new content").unwrap();
        assert_eq!(outcome.method, SaveMethod::AtomicRename);
        assert_eq!(fs::read_to_string(&path).unwrap(), "new content");
        assert_eq!(mode_of(&path), 0o640);
        assert_ne!(outcome.fingerprint.ino, before.ino);
        assert_eq!(outcome.fingerprint, fingerprint(&path).unwrap());
        assert_eq!(outcome.target, fs::canonicalize(&path).unwrap());
        assert!(setup.leftovers().is_empty());
    }

    #[test]
    fn keeps_an_executable_bit() {
        let setup = Setup::new();
        let path = setup.file("script.sh", "#!/bin/sh\n", 0o755);
        setup.save(&path, "#!/bin/sh\necho hi\n").unwrap();
        assert_eq!(mode_of(&path), 0o755);
    }

    #[test]
    fn a_new_file_gets_the_umask_default_mode() {
        let setup = Setup::new();
        let reference = setup.path("reference.txt");
        fs::write(&reference, "").unwrap();
        let path = setup.path("new.txt");
        let outcome = setup.save(&path, "fresh").unwrap();
        assert_eq!(outcome.method, SaveMethod::AtomicRename);
        assert_eq!(fs::read_to_string(&path).unwrap(), "fresh");
        assert_eq!(mode_of(&path), mode_of(&reference));
    }

    #[test]
    fn saving_through_a_symlink_keeps_the_link() {
        let setup = Setup::new();
        let target = setup.file("target.txt", "old", 0o644);
        let link = setup.path("link.txt");
        symlink("target.txt", &link).unwrap();
        let chain = setup.path("chain.txt");
        symlink("link.txt", &chain).unwrap();

        let outcome = setup.save(&chain, "new").unwrap();
        assert_eq!(outcome.method, SaveMethod::AtomicRename);
        assert_eq!(outcome.target, fs::canonicalize(&target).unwrap());
        assert_eq!(fs::read_link(&chain).unwrap(), Path::new("link.txt"));
        assert_eq!(fs::read_link(&link).unwrap(), Path::new("target.txt"));
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
        assert_eq!(fs::read_to_string(&chain).unwrap(), "new");
    }

    #[test]
    fn stale_temporary_files_of_the_target_are_removed() {
        let setup = Setup::new();
        let path = setup.file("notes.txt", "x", 0o644);
        let hour_ago = SystemTime::now() - Duration::from_secs(3600);
        let make = |name: &str, modified: SystemTime| {
            let file = File::create(setup.path(name)).unwrap();
            file.set_modified(modified).unwrap();
        };
        make(".notes.txt.Ab3xYz.stet-tmp", hour_ago);
        make(".notes.txt.Qq9Qq9.stet-tmp", SystemTime::now());
        make(".notes.txt.bak-up.stet-tmp", hour_ago);
        make(".notes.txt.old.Ab3xYz.stet-tmp", hour_ago);
        make(".notes.txt.Ab3xYz.tmp", hour_ago);
        make(".other.txt.Ab3xYz.stet-tmp", hour_ago);
        make("notes.txt.Ab3xYz.stet-tmp", hour_ago);

        assert_eq!(remove_stale_temps(&path, Duration::from_secs(60)), 1);
        let mut left: Vec<String> = fs::read_dir(setup.dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                ".notes.txt.Ab3xYz.tmp",
                ".notes.txt.Qq9Qq9.stet-tmp",
                ".notes.txt.bak-up.stet-tmp",
                ".notes.txt.old.Ab3xYz.stet-tmp",
                ".other.txt.Ab3xYz.stet-tmp",
                "notes.txt",
                "notes.txt.Ab3xYz.stet-tmp",
            ]
        );
        assert_eq!(remove_stale_temps(&path, Duration::ZERO), 1);
        assert_eq!(
            remove_stale_temps(&setup.path("missing/x"), Duration::ZERO),
            0
        );
    }

    #[test]
    fn the_temporary_file_of_a_crashed_save_is_what_the_cleanup_removes() {
        let setup = Setup::new();
        let path = setup.file("notes.txt", "original", 0o644);
        let mut kept = None;
        let crash = std::panic::catch_unwind(AssertUnwindSafe(|| {
            save_with_hook(&path, b"new", &setup.options, &mut |_| {
                // A real crash leaves the file behind; unwinding would delete it.
                let name = setup.leftovers().pop().unwrap();
                fs::copy(setup.path(&name), setup.path("copy")).unwrap();
                kept = Some(name);
                std::panic::panic_any("simulated crash");
            })
        }));
        assert!(crash.is_err());
        let name = kept.unwrap();
        fs::rename(setup.path("copy"), setup.path(&name)).unwrap();
        assert_eq!(setup.leftovers(), [name]);

        setup.save(&path, "after").unwrap();
        assert_eq!(remove_stale_temps(&path, Duration::ZERO), 1);
        assert!(setup.leftovers().is_empty());
        assert_eq!(fs::read_to_string(&path).unwrap(), "after");
    }

    #[test]
    fn the_temporary_file_goes_next_to_the_link_target() {
        let setup = Setup::new();
        fs::create_dir(setup.path("real")).unwrap();
        fs::create_dir(setup.path("links")).unwrap();
        let target = setup.file("real/file.txt", "old", 0o644);
        let link = setup.path("links/file.txt");
        symlink("../real/file.txt", &link).unwrap();
        fs::set_permissions(setup.path("links"), Permissions::from_mode(0o555)).unwrap();

        let outcome = setup.save(&link, "new");
        fs::set_permissions(setup.path("links"), Permissions::from_mode(0o755)).unwrap();
        assert_eq!(outcome.unwrap().method, SaveMethod::AtomicRename);
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn a_dangling_link_creates_its_target() {
        let setup = Setup::new();
        let link = setup.path("dangling.txt");
        symlink("created.txt", &link).unwrap();
        setup.save(&link, "made").unwrap();
        assert_eq!(
            fs::read_to_string(setup.path("created.txt")).unwrap(),
            "made"
        );
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn hard_links_stay_in_sync_through_an_in_place_save() {
        let setup = Setup::new();
        let path = setup.file("first.txt", "old", 0o644);
        let other = setup.path("second.txt");
        fs::hard_link(&path, &other).unwrap();
        let before = fingerprint(&path).unwrap();

        let outcome = setup.save(&path, "new and longer").unwrap();
        assert_eq!(outcome.method, SaveMethod::InPlace);
        assert_eq!(fs::read_to_string(&other).unwrap(), "new and longer");
        assert_eq!(outcome.fingerprint.ino, before.ino);
        assert_eq!(outcome.fingerprint, fingerprint(&other).unwrap());

        setup.save(&other, "short").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "short");
        assert!(setup.leftovers().is_empty());
    }

    #[test]
    fn a_read_only_directory_falls_back_to_in_place() {
        let setup = Setup::new();
        fs::create_dir(setup.path("locked")).unwrap();
        let path = setup.file("locked/file.txt", "old", 0o644);
        fs::set_permissions(setup.path("locked"), Permissions::from_mode(0o555)).unwrap();
        let skip = !denied(&setup.path("locked"));

        let outcome = setup.save(&path, "new");
        let new_file = setup.save(&setup.path("locked/other.txt"), "x");
        fs::set_permissions(setup.path("locked"), Permissions::from_mode(0o755)).unwrap();
        if skip {
            return;
        }
        assert_eq!(outcome.unwrap().method, SaveMethod::InPlace);
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        assert!(matches!(new_file, Err(SaveError::PermissionDenied { .. })));
        assert!(setup.leftovers().is_empty());
    }

    #[test]
    fn a_read_only_file_is_permission_denied_even_in_a_writable_directory() {
        let setup = Setup::new();
        let path = setup.file("read-only.txt", "keep me", 0o444);
        if !denied(&path) {
            return;
        }
        let error = setup.save(&path, "overwrite").unwrap_err();
        assert!(matches!(error, SaveError::PermissionDenied { .. }));
        assert!(error.to_string().starts_with("permission denied writing "));
        assert_eq!(fs::read_to_string(&path).unwrap(), "keep me");
        assert!(setup.leftovers().is_empty());
    }

    #[test]
    fn refuses_what_is_not_a_regular_file() {
        let setup = Setup::new();
        assert!(matches!(
            setup.save(setup.dir.path(), "x"),
            Err(SaveError::NotRegularFile { .. })
        ));
        assert!(matches!(
            setup.save(Path::new("/dev/null"), "x"),
            Err(SaveError::NotRegularFile { .. })
        ));
        assert!(matches!(
            setup.save(&setup.path("missing-dir/file.txt"), "x"),
            Err(SaveError::Io { .. })
        ));
    }

    #[test]
    fn keeps_a_group_the_user_belongs_to() {
        let setup = Setup::new();
        let path = setup.file("shared.txt", "old", 0o664);
        let own = fs::metadata(&path).unwrap().gid();
        let Some(group) = supplementary_groups().into_iter().find(|&gid| gid != own) else {
            return;
        };
        std::os::unix::fs::chown(&path, None, Some(group)).unwrap();
        let outcome = setup.save(&path, "new").unwrap();
        assert_eq!(outcome.method, SaveMethod::AtomicRename);
        assert_eq!(fs::metadata(&path).unwrap().gid(), group);
        assert_eq!(mode_of(&path), 0o664);
    }

    fn supplementary_groups() -> Vec<u32> {
        fs::read_to_string("/proc/self/status")
            .unwrap_or_default()
            .lines()
            .find_map(|line| line.strip_prefix("Groups:"))
            .map(|groups| {
                groups
                    .split_whitespace()
                    .filter_map(|gid| gid.parse().ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn keeps_user_extended_attributes() {
        let setup = Setup::new();
        let path = setup.file("tagged.txt", "old", 0o644);
        match rustix::fs::setxattr(&path, "user.stet.test", b"kept", XattrFlags::empty()) {
            Ok(()) => {}
            Err(Errno::OPNOTSUPP | Errno::PERM) => return,
            Err(errno) => panic!("setxattr: {errno}"),
        }
        let outcome = setup.save(&path, "new").unwrap();
        assert_eq!(outcome.method, SaveMethod::AtomicRename);
        let value = read_xattr(|buffer| rustix::fs::getxattr(&path, "user.stet.test", buffer));
        assert_eq!(value.unwrap(), b"kept");
    }

    #[test]
    fn an_in_place_save_backs_up_first() {
        let setup = Setup::new();
        let path = setup.file("linked.txt", "original", 0o644);
        fs::hard_link(&path, setup.path("alias.txt")).unwrap();
        let mut backup_seen = None;
        let outcome = save_with_hook(&path, b"replacement", &setup.options, &mut |stage| {
            assert_eq!(stage, Stage::BackupSynced);
            assert_eq!(fs::read_to_string(&path).unwrap(), "original");
            let backups: Vec<_> = fs::read_dir(&setup.options.backup_dir)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect();
            assert_eq!(backups.len(), 1);
            assert_eq!(mode_of(&backups[0]), 0o600);
            backup_seen = Some(fs::read_to_string(&backups[0]).unwrap());
        })
        .unwrap();
        assert_eq!(outcome.method, SaveMethod::InPlace);
        assert_eq!(backup_seen.as_deref(), Some("original"));
        assert_eq!(mode_of(&setup.options.backup_dir), 0o700);
        assert!(setup.leftovers().is_empty());
    }

    /// A crash is simulated in-process, by panicking out of the save at the last point before
    /// the rename: forking a child here could hold another test's freshly written script
    /// open and make its exec fail with ETXTBSY (see `omarchy::colors` tests).
    #[test]
    fn a_crash_between_write_and_rename_leaves_the_original_intact() {
        const CRASH: &str = "simulated crash";
        let setup = Setup::new();
        let path = setup.file("notes.txt", "original content", 0o644);
        let mut on_disk = None;
        let crash = std::panic::catch_unwind(AssertUnwindSafe(|| {
            save_with_hook(&path, b"new content", &setup.options, &mut |stage| {
                assert_eq!(stage, Stage::TempSynced);
                let temps = setup.leftovers();
                let temp_content = temps
                    .iter()
                    .map(|name| fs::read_to_string(setup.path(name)).unwrap())
                    .collect::<Vec<_>>();
                on_disk = Some((fs::read_to_string(&path).unwrap(), temps, temp_content));
                std::panic::panic_any(CRASH);
            })
        }));
        let payload = crash.expect_err("the save should have stopped before the rename");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&CRASH));

        let (target, temps, temp_content) = on_disk.unwrap();
        assert_eq!(target, "original content");
        assert_eq!(temps.len(), 1, "{temps:?}");
        assert!(temps[0].starts_with(".notes.txt."));
        assert_eq!(temp_content, ["new content"]);
        assert_eq!(fs::read_to_string(&path).unwrap(), "original content");

        setup.save(&path, "after the crash").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "after the crash");
    }
}
