use super::StoreError;
use super::fsutil::FILE_MODE;
use std::fs::{self, DirBuilder, File, OpenOptions, Permissions};
use std::io::{self, Read};
use std::os::unix::fs::{DirBuilderExt, FileExt as _, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use stet_domain::session::is_valid_id;

const DIR_MODE: u32 = 0o700;
const BACKUP_EXTENSION: &str = ".txt";
const TEMP_SUFFIX: &str = ".tmp";

/// Where the store keeps its files inside the state directory.
#[derive(Debug, Clone)]
pub(super) struct Layout {
    root: PathBuf,
    backups: PathBuf,
    orphans: PathBuf,
}

impl Layout {
    pub(super) fn new(root: PathBuf) -> Self {
        let backups = root.join("backup");
        let orphans = backups.join("orphaned");
        Self {
            root,
            backups,
            orphans,
        }
    }

    pub(super) fn root(&self) -> &Path {
        &self.root
    }

    pub(super) fn backups(&self) -> &Path {
        &self.backups
    }

    pub(super) fn orphans(&self) -> &Path {
        &self.orphans
    }

    pub(super) fn manifest(&self) -> PathBuf {
        self.root.join("session.json")
    }

    pub(super) fn previous(&self) -> PathBuf {
        self.root.join("session.json.prev")
    }

    pub(super) fn corrupt(&self) -> PathBuf {
        self.root.join("session.json.corrupt")
    }

    pub(super) fn manifest_temp(&self) -> PathBuf {
        self.root.join(format!("session.json{TEMP_SUFFIX}"))
    }

    pub(super) fn lock(&self) -> PathBuf {
        self.root.join("lock")
    }

    /// `backup/<id>.txt`; `id` must be valid.
    pub(super) fn backup(&self, id: &str) -> PathBuf {
        self.backups.join(format!("{id}{BACKUP_EXTENSION}"))
    }

    pub(super) fn backup_temp(&self, id: &str) -> PathBuf {
        self.backups
            .join(format!("{id}{BACKUP_EXTENSION}{TEMP_SUFFIX}"))
    }

    /// The backup id of a file name in `backup/`, and whether the file is a temp file.
    pub(super) fn parse_backup_name(name: &str) -> Option<(&str, bool)> {
        let (name, temp) = match name.strip_suffix(TEMP_SUFFIX) {
            Some(name) => (name, true),
            None => (name, false),
        };
        let id = name.strip_suffix(BACKUP_EXTENSION)?;
        is_valid_id(id).then_some((id, temp))
    }

    /// Whether `path` names an entry directly inside `backup/orphaned/`.
    pub(super) fn is_orphan(&self, path: &Path) -> bool {
        path.parent() == Some(self.orphans.as_path()) && path.file_name().is_some()
    }

    /// Creates the directories, private to the user; tightens them if they already exist.
    pub(super) fn create(&self) -> Result<(), StoreError> {
        for dir in [&self.root, &self.backups, &self.orphans] {
            private_dir(dir).map_err(|error| StoreError::io("create", dir, error))?;
        }
        Ok(())
    }

    /// Takes an exclusive lock on `lock`, held until the returned file is closed, and writes the
    /// process id into it for diagnostics.
    pub(super) fn acquire_lock(&self) -> Result<File, StoreError> {
        let path = self.lock();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(FILE_MODE)
            .open(&path)
            .map_err(|error| StoreError::io("open", &path, error))?;
        match fs4::FileExt::try_lock(&file) {
            Ok(()) => {}
            Err(fs4::TryLockError::WouldBlock) => {
                return Err(StoreError::Locked {
                    state_dir: self.root.clone(),
                    pid: read_pid(&file),
                });
            }
            Err(fs4::TryLockError::Error(error)) => {
                return Err(StoreError::io("lock", &path, error));
            }
        }
        let pid = format!("{}\n", std::process::id());
        file.set_permissions(Permissions::from_mode(FILE_MODE))
            .and_then(|()| file.set_len(0))
            .and_then(|()| file.write_all_at(pid.as_bytes(), 0))
            .map_err(|error| StoreError::io("write", &path, error))?;
        Ok(file)
    }
}

fn private_dir(dir: &Path) -> io::Result<()> {
    DirBuilder::new()
        .recursive(true)
        .mode(DIR_MODE)
        .create(dir)?;
    if fs::metadata(dir)?.permissions().mode() & 0o777 != DIR_MODE {
        fs::set_permissions(dir, Permissions::from_mode(DIR_MODE))?;
    }
    Ok(())
}

fn read_pid(mut file: &File) -> Option<u32> {
    let mut text = String::new();
    file.read_to_string(&mut text).ok()?;
    text.trim().parse().ok()
}
