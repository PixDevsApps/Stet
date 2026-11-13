//! The generated GtkSourceView style scheme in Stet's cache (`$XDG_CACHE_HOME/stet/styles`,
//! ADR-004): exactly one `stet-omarchy-<fingerprint>.xml` is kept there.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::xdg;

/// The start of every generated scheme id, and so of every file name this module manages.
pub const SCHEME_PREFIX: &str = "stet-omarchy-";

/// Writes `<id>.xml` through a temporary file renamed into place, and removes the other
/// generated schemes, so exactly one is in the search path.
pub fn write_scheme(dir: &Path, id: &str, xml: &str) -> io::Result<PathBuf> {
    xdg::ensure_private_dir(dir)?;
    let path = dir.join(format!("{id}.xml"));
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    temp.write_all(xml.as_bytes())?;
    temp.persist(&path).map_err(|error| error.error)?;
    for entry in fs::read_dir(dir)?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(SCHEME_PREFIX) && name.ends_with(".xml") && entry.path() != path {
            let _ = fs::remove_file(entry.path());
        }
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn keeps_only_the_newest_scheme() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("styles");
        let first = write_scheme(&dir, "stet-omarchy-1", "<one/>").unwrap();
        assert_eq!(first, dir.join("stet-omarchy-1.xml"));
        assert_eq!(fs::read_to_string(&first).unwrap(), "<one/>");
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        fs::write(dir.join("other.xml"), "").unwrap();
        fs::write(dir.join("stet-omarchy-notes.txt"), "").unwrap();
        let second = write_scheme(&dir, "stet-omarchy-2", "<two/>").unwrap();
        assert_eq!(fs::read_to_string(&second).unwrap(), "<two/>");
        assert_eq!(
            names(&dir),
            ["other.xml", "stet-omarchy-2.xml", "stet-omarchy-notes.txt"]
        );
    }

    #[test]
    fn rewriting_the_same_scheme_replaces_it() {
        let root = tempfile::tempdir().unwrap();
        write_scheme(root.path(), "stet-omarchy-1", "<old/>").unwrap();
        let path = write_scheme(root.path(), "stet-omarchy-1", "<new/>").unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "<new/>");
        assert_eq!(names(root.path()), ["stet-omarchy-1.xml"]);
    }
}
