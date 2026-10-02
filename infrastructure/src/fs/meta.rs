use std::fs::{self, Metadata};
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use rustix::fs::Access;

/// What Stet compares to notice that a file changed on disk: the session store's type, so a
/// baseline taken at load or save is the one a session keeps (ADR-006). Recorded right after
/// Stet's own saves so that they don't count as outside changes; compare with
/// [`Fingerprint::matches`], which ignores the device number.
pub use stet_domain::session::Fingerprint;

/// The fingerprint of `metadata`, from `fs::metadata` or from the handle a load read.
pub fn fingerprint_of(metadata: &Metadata) -> Fingerprint {
    Fingerprint {
        dev: metadata.dev(),
        ino: metadata.ino(),
        size: metadata.size(),
        mtime_ns: mtime_ns(metadata),
    }
}

/// The fingerprint of the file `path` names, following symbolic links.
pub fn fingerprint(path: &Path) -> io::Result<Fingerprint> {
    Ok(fingerprint_of(&fs::metadata(path)?))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMeta {
    pub size: u64,
    pub mtime_ns: i64,
    pub dev: u64,
    pub ino: u64,
    /// `st_mode`: the file type and permission bits.
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub nlink: u64,
    /// The current user may write the file, by `access(2)`. False on a read-only file system.
    pub writable: bool,
    /// The path given is a symbolic link.
    pub is_symlink: bool,
    /// The path with every symbolic link resolved: the file that saves write to.
    pub canonical: PathBuf,
}

impl FileMeta {
    /// Combines `metadata` of the file `path` resolves to with what only the path knows.
    pub(crate) fn new(path: &Path, metadata: &Metadata) -> io::Result<Self> {
        let is_symlink = fs::symlink_metadata(path)?.file_type().is_symlink();
        let canonical = fs::canonicalize(path)?;
        Ok(Self {
            size: metadata.size(),
            mtime_ns: mtime_ns(metadata),
            dev: metadata.dev(),
            ino: metadata.ino(),
            mode: metadata.mode(),
            uid: metadata.uid(),
            gid: metadata.gid(),
            nlink: metadata.nlink(),
            writable: rustix::fs::access(&canonical, Access::WRITE_OK).is_ok(),
            is_symlink,
            canonical,
        })
    }

    pub fn fingerprint(&self) -> Fingerprint {
        Fingerprint {
            dev: self.dev,
            ino: self.ino,
            size: self.size,
            mtime_ns: self.mtime_ns,
        }
    }
}

/// Metadata of the file `path` names, following symbolic links.
pub fn file_meta(path: &Path) -> io::Result<FileMeta> {
    FileMeta::new(path, &fs::metadata(path)?)
}

fn mtime_ns(metadata: &Metadata) -> i64 {
    metadata
        .mtime()
        .saturating_mul(1_000_000_000)
        .saturating_add(metadata.mtime_nsec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn meta_follows_links_and_remembers_them() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        fs::write(&target, "twelve bytes").unwrap();
        let link = dir.path().join("link.txt");
        symlink("target.txt", &link).unwrap();

        let meta = file_meta(&link).unwrap();
        assert!(meta.is_symlink);
        assert_eq!(meta.canonical, fs::canonicalize(&target).unwrap());
        assert_eq!(meta.size, 12);
        assert_eq!(meta.nlink, 1);
        assert!(meta.writable);
        assert_eq!(meta.fingerprint(), fingerprint(&target).unwrap());
        assert!(!file_meta(&target).unwrap().is_symlink);
    }

    #[test]
    fn writable_reflects_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("read-only.txt");
        fs::write(&file, "x").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o444)).unwrap();
        if rustix::fs::access(&file, Access::WRITE_OK).is_ok() {
            return;
        }
        let meta = file_meta(&file).unwrap();
        assert!(!meta.writable);
        assert_eq!(meta.mode & 0o777, 0o444);
    }

    #[test]
    fn fingerprints_change_with_the_content() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file.txt");
        fs::write(&file, "one").unwrap();
        let before = fingerprint(&file).unwrap();
        fs::write(&file, "three").unwrap();
        let after = fingerprint(&file).unwrap();
        assert_eq!((before.dev, before.ino), (after.dev, after.ino));
        assert_ne!(before, after);
        assert!(!before.matches(&after));
        assert!(after.matches(&fingerprint(&file).unwrap()));
        assert!(fingerprint(&dir.path().join("missing")).is_err());
    }
}
