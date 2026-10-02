//! The recent-files list: most recent first, without duplicates, capped.

use std::path::{Path, PathBuf};

/// The most paths the list keeps.
pub const RECENT_LIMIT: usize = 20;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecentFiles {
    paths: Vec<PathBuf>,
}

impl RecentFiles {
    /// Keeps the first occurrence of each path and at most [`RECENT_LIMIT`] paths.
    pub fn new(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        let mut recent = Self::default();
        for path in paths {
            if recent.paths.len() == RECENT_LIMIT {
                break;
            }
            if !recent.paths.contains(&path) {
                recent.paths.push(path);
            }
        }
        recent
    }

    /// Moves `path` to the front. Returns whether the list changed.
    pub fn add(&mut self, path: PathBuf) -> bool {
        if self.paths.first() == Some(&path) {
            return false;
        }
        self.paths.retain(|existing| existing != &path);
        self.paths.insert(0, path);
        self.paths.truncate(RECENT_LIMIT);
        true
    }

    pub fn remove(&mut self, path: &Path) -> bool {
        let before = self.paths.len();
        self.paths.retain(|existing| existing != path);
        self.paths.len() != before
    }

    pub fn clear(&mut self) -> bool {
        let changed = !self.paths.is_empty();
        self.paths.clear();
        changed
    }

    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn add_moves_to_the_front() {
        let mut recent = RecentFiles::new(paths(&["/a", "/b", "/c"]));
        assert!(recent.add("/c".into()));
        assert_eq!(recent.paths(), paths(&["/c", "/a", "/b"]));
        assert!(!recent.add("/c".into()));
        assert!(recent.add("/d".into()));
        assert_eq!(recent.paths(), paths(&["/d", "/c", "/a", "/b"]));
    }

    #[test]
    fn remove_and_clear_report_changes() {
        let mut recent = RecentFiles::new(paths(&["/a", "/b"]));
        assert!(recent.remove(Path::new("/a")));
        assert!(!recent.remove(Path::new("/a")));
        assert!(recent.clear());
        assert!(!recent.clear());
        assert!(recent.is_empty());
    }

    #[test]
    fn new_drops_duplicates_and_caps() {
        let recent = RecentFiles::new(paths(&["/a", "/a", "/b"]));
        assert_eq!(recent.paths(), paths(&["/a", "/b"]));
        let many = RecentFiles::new((0..50).map(|n| PathBuf::from(format!("/{n}"))));
        assert_eq!(many.paths().len(), RECENT_LIMIT);
        assert_eq!(many.paths()[0], Path::new("/0"));
    }

    proptest! {
        #[test]
        fn stays_unique_and_capped(ops in proptest::collection::vec(0u8..40, 0..200)) {
            let mut recent = RecentFiles::default();
            for op in ops {
                let path = PathBuf::from(format!("/{}", op % 30));
                if op >= 35 {
                    recent.remove(&path);
                } else {
                    recent.add(path.clone());
                    prop_assert_eq!(recent.paths().first(), Some(&path));
                }
                let mut unique = recent.paths().to_vec();
                unique.sort();
                unique.dedup();
                prop_assert_eq!(unique.len(), recent.paths().len());
                prop_assert!(recent.paths().len() <= RECENT_LIMIT);
            }
        }
    }
}
