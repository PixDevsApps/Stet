//! The recent-files list on disk: `$XDG_STATE_HOME/stet/recent.json`, in a 0700 directory with
//! mode 0600, written atomically.

use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use stet_domain::recent::RecentFiles;

const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct RecentJson {
    schema_version: u32,
    #[serde(default)]
    files: Vec<String>,
}

/// Reads the list. A missing file is an empty list; a corrupt one is logged and ignored.
pub fn load(path: &Path) -> RecentFiles {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            if error.kind() != io::ErrorKind::NotFound {
                tracing::warn!(path = %path.display(), %error, "could not read the recent files");
            }
            return RecentFiles::default();
        }
    };
    match serde_json::from_str::<RecentJson>(&text) {
        Ok(json) => RecentFiles::new(json.files.into_iter().map(PathBuf::from)),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "ignoring a corrupt recent-files list");
            RecentFiles::default()
        }
    }
}

/// Writes the list. Paths that are not valid UTF-8 are left out.
pub fn save(path: &Path, recent: &RecentFiles) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no parent directory"))?;
    crate::xdg::ensure_private_dir(dir)?;
    let json = RecentJson {
        schema_version: SCHEMA_VERSION,
        files: recent
            .paths()
            .iter()
            .filter_map(|path| path.to_str().map(str::to_owned))
            .collect(),
    };
    let mut bytes = serde_json::to_vec_pretty(&json).map_err(io::Error::other)?;
    bytes.push(b'\n');
    let temp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let written = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temp);
    }
    written?;
    File::open(dir)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn round_trips_with_private_permissions() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("state/stet/recent.json");
        let recent = RecentFiles::new([PathBuf::from("/a/b.rs"), PathBuf::from("/c d/é.txt")]);
        save(&path, &recent).unwrap();
        assert_eq!(load(&path), recent);
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"schema_version\": 1"), "{text}");
    }

    #[test]
    fn missing_and_corrupt_files_are_empty() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recent.json");
        assert!(load(&path).is_empty());
        fs::write(&path, "{ not json").unwrap();
        assert!(load(&path).is_empty());
        fs::write(&path, "{\"schema_version\": 1}").unwrap();
        assert!(load(&path).is_empty());
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recent.json");
        fs::write(
            &path,
            "{\"schema_version\": 2, \"files\": [\"/x\"], \"pinned\": []}",
        )
        .unwrap();
        assert_eq!(load(&path).paths(), [PathBuf::from("/x")]);
    }
}
