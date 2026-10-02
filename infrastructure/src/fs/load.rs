use std::fs::OpenOptions;
use std::io::{self, ErrorKind, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use rustix::fs::OFlags;

use super::meta::FileMeta;
use super::size::{Refusal, SizeClass, SizePolicy, human_size, mem_available};

/// A file's bytes, not decoded yet.
#[derive(Debug)]
pub struct Loaded {
    pub bytes: Vec<u8>,
    /// Taken before reading, so a change during the read shows up as a changed fingerprint.
    pub meta: FileMeta,
    /// Never [`SizeClass::TooLarge`]; such files are refused.
    pub class: SizeClass,
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("{} does not exist", .path.display())]
    NotFound { path: PathBuf },
    #[error("permission denied reading {}", .path.display())]
    PermissionDenied { path: PathBuf },
    #[error("{} is a directory", .path.display())]
    IsDirectory { path: PathBuf },
    #[error("{} is not a regular file", .path.display())]
    NotRegularFile { path: PathBuf },
    #[error("{} ({}) is too large to open: {refusal}", .path.display(), human_size(*.size))]
    TooLarge {
        path: PathBuf,
        size: u64,
        refusal: Refusal,
    },
    #[error("could not read {}: {source}", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

impl LoadError {
    fn io(path: &Path, source: io::Error) -> Self {
        let path = path.to_path_buf();
        match source.kind() {
            ErrorKind::NotFound => Self::NotFound { path },
            ErrorKind::PermissionDenied => Self::PermissionDenied { path },
            ErrorKind::IsADirectory => Self::IsDirectory { path },
            _ => Self::Io { path, source },
        }
    }
}

/// Reads a whole regular file, refusing it by `policy` before reading when it is too large
/// or too little memory is free. Doesn't decode. The file is opened non-blocking, so a named
/// pipe is refused instead of waiting for a writer.
pub fn load(path: &Path, policy: &SizePolicy) -> Result<Loaded, LoadError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(OFlags::NONBLOCK.bits() as i32)
        .open(path)
        .map_err(|error| LoadError::io(path, error))?;
    let metadata = file
        .metadata()
        .map_err(|error| LoadError::io(path, error))?;
    if metadata.is_dir() {
        return Err(LoadError::IsDirectory {
            path: path.to_path_buf(),
        });
    }
    if !metadata.is_file() {
        return Err(LoadError::NotRegularFile {
            path: path.to_path_buf(),
        });
    }
    let available = mem_available();
    let too_large = |size: u64, refusal: Refusal| LoadError::TooLarge {
        path: path.to_path_buf(),
        size,
        refusal,
    };
    if let Some(refusal) = policy.refusal(metadata.len(), available) {
        return Err(too_large(metadata.len(), refusal));
    }
    let meta = FileMeta::new(path, &metadata).map_err(|error| LoadError::io(path, error))?;
    let mut bytes = Vec::new();
    let expected = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    if bytes.try_reserve_exact(expected).is_err() {
        let refusal = Refusal::Memory {
            needed: metadata.len(),
            available: available.unwrap_or_default(),
        };
        return Err(too_large(metadata.len(), refusal));
    }
    (&file)
        .take(policy.refuse_from)
        .read_to_end(&mut bytes)
        .map_err(|error| LoadError::io(path, error))?;
    let size = bytes.len() as u64;
    if let Some(refusal) = policy.refusal(size, available) {
        return Err(too_large(size, refusal));
    }
    Ok(Loaded {
        bytes,
        meta,
        class: policy.classify(size, available),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn small_policy() -> SizePolicy {
        SizePolicy {
            large_from: 1024,
            refuse_from: 4096,
            ram_factor: 4,
        }
    }

    #[test]
    fn loads_bytes_and_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file.txt");
        fs::write(&path, b"caf\xe9\r\n").unwrap();
        let loaded = load(&path, &SizePolicy::default()).unwrap();
        assert_eq!(loaded.bytes, b"caf\xe9\r\n");
        assert_eq!(loaded.meta.size, 6);
        assert_eq!(loaded.class, SizeClass::Normal);
    }

    #[test]
    fn classifies_by_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large.txt");
        fs::write(&path, vec![b'x'; 2048]).unwrap();
        assert_eq!(
            load(&path, &small_policy()).unwrap().class,
            SizeClass::Large
        );
    }

    #[test]
    fn refuses_files_over_the_cap_with_a_clear_message() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("huge.log");
        fs::write(&path, vec![b'x'; 4096]).unwrap();
        let error = load(&path, &small_policy()).unwrap_err();
        assert!(matches!(
            error,
            LoadError::TooLarge {
                size: 4096,
                refusal: Refusal::Size { limit: 4096 },
                ..
            }
        ));
        assert!(error.to_string().ends_with(
            "huge.log (4.0 KiB) is too large to open: Stet opens files smaller than 4.0 KiB"
        ));
    }

    #[test]
    fn refuses_what_is_not_a_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let policy = SizePolicy::default();
        assert!(matches!(
            load(dir.path(), &policy),
            Err(LoadError::IsDirectory { .. })
        ));
        assert!(matches!(
            load(Path::new("/dev/null"), &policy),
            Err(LoadError::NotRegularFile { .. })
        ));
        assert!(matches!(
            load(&dir.path().join("missing"), &policy),
            Err(LoadError::NotFound { .. })
        ));
    }

    #[test]
    fn a_named_pipe_is_refused_without_waiting_for_a_writer() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("pipe");
        let mode = rustix::fs::Mode::from_raw_mode(0o600);
        rustix::fs::mkfifoat(rustix::fs::CWD, &fifo, mode).unwrap();
        assert!(matches!(
            load(&fifo, &SizePolicy::default()),
            Err(LoadError::NotRegularFile { .. })
        ));
    }

    #[test]
    fn reports_unreadable_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.txt");
        fs::write(&path, "x").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::File::open(&path).is_ok() {
            return;
        }
        assert!(matches!(
            load(&path, &SizePolicy::default()),
            Err(LoadError::PermissionDenied { .. })
        ));
    }
}
