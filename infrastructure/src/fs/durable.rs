//! Durable writes of the files Stet keeps for itself, and the steps they share with the
//! document save ([`super::safe_save`]) and the session store.
//!
//! **The rule.** A file is never rewritten where it lies. [`write_atomic`] writes the new
//! content to a temporary file in the target's directory, syncs it, renames it over the
//! target and then syncs the directory: a crash leaves the old content or the new, never a
//! mix, and once the directory is synced the new name survives a power loss.
//!
//! **Reporting.** Every step up to and including the rename returns its error, and the
//! temporary file is removed. A failed directory sync after the rename is logged, not
//! returned: the new content is already in place and is what every reader sees, so returning
//! an error would report a failed write for a file that was written, and retrying could only
//! write the same thing again. The document save follows the same rule. [`sync_dir`] itself
//! returns the error, for writers whose next step depends on it: the session store syncs
//! `backup/` before it writes a manifest that references the new backups.

use std::ffi::OsStr;
use std::fs::{self, DirBuilder, File, OpenOptions, Permissions};
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use tempfile::{Builder, NamedTempFile};

/// Readable and writable by the owner only.
pub(crate) const PRIVATE_FILE_MODE: u32 = 0o600;
const PRIVATE_DIR_MODE: u32 = 0o700;

/// The temporary file of an atomic write is `.<name>.<RANDOM>.stet-tmp` next to the target.
const TEMP_SUFFIX: &str = ".stet-tmp";
/// The length of the random part, which tempfile makes of ASCII letters and digits.
const TEMP_RANDOM_LEN: usize = 6;

/// The permissions a newly written file gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileMode {
    /// 0666 less the process umask, as for a file a shell redirection creates.
    Umask,
    /// Exactly these permission bits, whatever the umask.
    Exact(u32),
}

impl FileMode {
    pub(crate) const PRIVATE: Self = Self::Exact(PRIVATE_FILE_MODE);

    fn create_bits(self) -> u32 {
        match self {
            Self::Umask => 0o666,
            Self::Exact(mode) => mode,
        }
    }

    /// Gives `file` the exact bits, which the umask may have narrowed at creation.
    fn apply(self, file: &File) -> io::Result<()> {
        match self {
            Self::Umask => Ok(()),
            Self::Exact(mode) => file.set_permissions(Permissions::from_mode(mode)),
        }
    }
}

/// How [`write_atomic`] treats the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WriteOptions {
    mode: FileMode,
    replace: bool,
}

impl WriteOptions {
    /// Replace the target if it exists.
    pub(crate) fn replace(mode: FileMode) -> Self {
        Self {
            mode,
            replace: true,
        }
    }

    /// Never replace an existing target: the write then fails with
    /// [`io::ErrorKind::AlreadyExists`] and the target is untouched.
    pub(crate) fn create_new(mode: FileMode) -> Self {
        Self {
            mode,
            replace: false,
        }
    }
}

/// A step of [`write_atomic`] failed; nothing was renamed over the target.
#[derive(Debug, thiserror::Error)]
#[error("could not {action} {}: {source}", .path.display())]
pub(crate) struct DurableError {
    action: &'static str,
    path: PathBuf,
    #[source]
    source: io::Error,
}

impl DurableError {
    fn new(action: &'static str, path: &Path, source: io::Error) -> Self {
        Self {
            action,
            path: path.to_path_buf(),
            source,
        }
    }

    pub(crate) fn kind(&self) -> io::ErrorKind {
        self.source.kind()
    }
}

impl From<DurableError> for io::Error {
    fn from(error: DurableError) -> Self {
        Self::new(error.kind(), error)
    }
}

/// Writes `bytes` to `path` atomically and durably; see the module docs.
pub(crate) fn write_atomic(
    path: &Path,
    bytes: &[u8],
    options: WriteOptions,
) -> Result<(), DurableError> {
    let fail = |action, error| DurableError::new(action, path, error);
    if !options.replace && fs::symlink_metadata(path).is_ok() {
        return Err(fail("create", io::ErrorKind::AlreadyExists.into()));
    }
    let mut temp = temp_beside(path, options.mode).map_err(|error| fail("write", error))?;
    temp.write_all(bytes)
        .and_then(|()| temp.as_file().sync_all())
        .map_err(|error| fail("write", error))?;
    let persisted = if options.replace {
        temp.persist(path)
    } else {
        temp.persist_noclobber(path)
    };
    persisted.map_err(|error| fail("replace", error.error))?;
    sync_dir_or_warn(parent_dir(path));
    Ok(())
}

/// A new, empty temporary file in `target`'s directory, named `.<name>.<RANDOM>.stet-tmp`
/// (see [`is_temp_of`]) and created with `mode`. It is removed when dropped unless persisted.
pub(crate) fn temp_beside(target: &Path, mode: FileMode) -> io::Result<NamedTempFile> {
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let temp = Builder::new()
        .prefix(&format!(".{name}."))
        .rand_bytes(TEMP_RANDOM_LEN)
        .suffix(TEMP_SUFFIX)
        .permissions(Permissions::from_mode(mode.create_bits()))
        .tempfile_in(parent_dir(target))?;
    mode.apply(temp.as_file())?;
    Ok(temp)
}

/// Whether `file_name` is a temporary file [`temp_beside`] makes for a target named `target`.
pub(crate) fn is_temp_of(file_name: &OsStr, target: &OsStr) -> bool {
    let prefix = [b".", target.as_bytes(), b"."].concat();
    file_name
        .as_bytes()
        .strip_prefix(prefix.as_slice())
        .and_then(|rest| rest.strip_suffix(TEMP_SUFFIX.as_bytes()))
        .is_some_and(|random| {
            random.len() == TEMP_RANDOM_LEN && random.iter().all(u8::is_ascii_alphanumeric)
        })
}

/// Creates or truncates `path` with `mode`, writes `bytes` and syncs the file. Not atomic by
/// itself: for a temporary file with a fixed name that the caller renames into place.
pub(crate) fn write_synced(path: &Path, bytes: &[u8], mode: FileMode) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(mode.create_bits())
        .open(path)?;
    mode.apply(&file)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Syncs a directory, so renames and new entries in it survive a power loss.
pub(crate) fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

/// [`sync_dir`] as the last step of a write that has already replaced its file: a failure
/// is logged, not returned (see the module docs).
pub(crate) fn sync_dir_or_warn(dir: &Path) {
    if let Err(error) = sync_dir(dir) {
        tracing::warn!(dir = %dir.display(), %error, "could not sync the directory");
    }
}

/// Creates `dir` and its missing parents with mode 0700, and makes `dir` itself mode 0700
/// also when it existed with wider permissions.
pub(crate) fn ensure_private_dir(dir: &Path) -> io::Result<()> {
    DirBuilder::new()
        .recursive(true)
        .mode(PRIVATE_DIR_MODE)
        .create(dir)?;
    if fs::metadata(dir)?.permissions().mode() & 0o777 != PRIVATE_DIR_MODE {
        fs::set_permissions(dir, Permissions::from_mode(PRIVATE_DIR_MODE))?;
    }
    Ok(())
}

/// The directory `path` is in; `.` for a bare file name.
fn parent_dir(path: &Path) -> &Path {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode_of(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o7777
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn replaces_the_target_and_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file");
        fs::write(&path, "old").unwrap();
        write_atomic(&path, b"new", WriteOptions::replace(FileMode::PRIVATE)).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(names(dir.path()), ["file"]);
    }

    #[test]
    fn modes_are_exact_or_follow_the_umask() {
        let dir = tempfile::tempdir().unwrap();
        let reference = dir.path().join("reference");
        fs::write(&reference, "").unwrap();
        for (mode, expected) in [
            (FileMode::PRIVATE, 0o600),
            (FileMode::Exact(0o640), 0o640),
            (FileMode::Umask, mode_of(&reference)),
        ] {
            let path = dir.path().join(format!("{expected:o}"));
            write_atomic(&path, b"x", WriteOptions::replace(mode)).unwrap();
            assert_eq!(mode_of(&path), expected, "{mode:?}");
            let fixed = dir.path().join(format!("{expected:o}.tmp"));
            write_synced(&fixed, b"x", mode).unwrap();
            assert_eq!(mode_of(&fixed), expected, "{mode:?}");
        }
    }

    #[test]
    fn create_new_never_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let options = WriteOptions::create_new(FileMode::Umask);
        write_atomic(&path, b"first", options).unwrap();
        let error = write_atomic(&path, b"second", options).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&path).unwrap(), "first");
        assert_eq!(names(dir.path()), ["config.toml"]);
    }

    #[test]
    fn a_failed_write_names_the_target() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing/file");
        let error =
            write_atomic(&path, b"x", WriteOptions::replace(FileMode::PRIVATE)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(error.to_string().contains(&path.display().to_string()));
        let error = io::Error::from(error);
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn temporary_files_are_recognized_by_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("notes.txt");
        let temp = temp_beside(&target, FileMode::PRIVATE).unwrap();
        let name = temp.path().file_name().unwrap();
        assert!(is_temp_of(name, OsStr::new("notes.txt")));
        assert!(!is_temp_of(name, OsStr::new("notes")));
        for other in [
            ".notes.txt.abc.stet-tmp",
            ".notes.txt.abcde!.stet-tmp",
            "notes.txt.abcdef.stet-tmp",
            ".notes.txt.abcdef.tmp",
        ] {
            assert!(
                !is_temp_of(OsStr::new(other), OsStr::new("notes.txt")),
                "{other}"
            );
        }
    }

    #[test]
    fn private_dirs_are_0700() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("a/b");
        ensure_private_dir(&dir).unwrap();
        assert_eq!(mode_of(&dir), 0o700);
        fs::set_permissions(&dir, Permissions::from_mode(0o755)).unwrap();
        ensure_private_dir(&dir).unwrap();
        assert_eq!(mode_of(&dir), 0o700);
        sync_dir(&dir).unwrap();
        assert!(sync_dir(&root.path().join("missing")).is_err());
    }
}
