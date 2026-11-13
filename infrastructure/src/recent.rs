//! The recent-files list older versions kept in `$XDG_STATE_HOME/stet/recent.json`. The
//! session holds the recent files now; the file is only read, to move its list into the
//! session once.

use serde::Deserialize;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use stet_domain::recent::RecentFiles;

#[derive(Deserialize)]
struct RecentJson {
    /// Required, as when the file was written; the version itself is not checked.
    #[serde(rename = "schema_version")]
    _schema_version: u32,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_files_in_order() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("recent.json");
        fs::write(
            &path,
            "{\n  \"schema_version\": 1,\n  \"files\": [\"/a/b.rs\", \"/c d/é.txt\"]\n}\n",
        )
        .unwrap();
        let recent = RecentFiles::new([PathBuf::from("/a/b.rs"), PathBuf::from("/c d/é.txt")]);
        assert_eq!(load(&path), recent);
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
