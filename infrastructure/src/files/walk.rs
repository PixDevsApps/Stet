//! The one walk Find in Files, Replace in Files and the project walk build on: hidden files,
//! `.gitignore`, subfolders and the filter list decide which files it visits. Symbolic links
//! are never followed, so a walk can't loop or leave its roots.

use std::path::{Path, PathBuf};

use ignore::{DirEntry, WalkBuilder};

use super::filters::Filters;
use super::{PathError, StartError};

/// Which files a walk visits. Symbolic links are never followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WalkOptions {
    pub(crate) include_hidden: bool,
    /// Skip what `.gitignore`, `.ignore` and git's excludes ignore (inside git repositories).
    pub(crate) respect_gitignore: bool,
    pub(crate) include_subfolders: bool,
}

/// Every file under the root, with subfolders, without hidden files, respecting `.gitignore`.
impl Default for WalkOptions {
    fn default() -> Self {
        Self {
            include_hidden: false,
            respect_gitignore: true,
            include_subfolders: true,
        }
    }
}

impl WalkOptions {
    /// A walk of `root` with these options.
    pub(crate) fn builder(self, root: &Path) -> WalkBuilder {
        let respect = self.respect_gitignore;
        let mut builder = WalkBuilder::new(root);
        builder
            .hidden(!self.include_hidden)
            .ignore(respect)
            .git_ignore(respect)
            .git_global(respect)
            .git_exclude(respect)
            .parents(respect)
            .follow_links(false);
        if !self.include_subfolders {
            builder.max_depth(Some(1));
        }
        builder
    }
}

/// A folder (or file) to walk, with the filter list parsed for it.
pub(crate) struct Root {
    path: PathBuf,
    filters: Filters,
}

impl Root {
    /// Each of `roots` with `filters`, parsed before the job starts so that a bad filter fails
    /// the start.
    pub(crate) fn parse_all(roots: &[PathBuf], filters: &str) -> Result<Vec<Self>, StartError> {
        roots
            .iter()
            .map(|root| {
                Filters::new(root, filters)
                    .map(|filters| Self {
                        path: root.clone(),
                        filters,
                    })
                    .map_err(StartError::Filter)
            })
            .collect()
    }

    /// A walk of this root with `options`; the filters' exclusions prune it.
    pub(crate) fn builder(&self, options: WalkOptions) -> WalkBuilder {
        let mut builder = options.builder(&self.path);
        builder.overrides(self.filters.excludes.clone());
        builder
    }

    /// The file a walk entry is, when the job should visit it: not a folder or another kind of
    /// entry, and passing the filters' inclusions.
    pub(crate) fn file(
        &self,
        entry: Result<DirEntry, ignore::Error>,
    ) -> Result<Option<DirEntry>, PathError> {
        let entry = entry.map_err(PathError::walk)?;
        let wanted = entry.file_type().is_some_and(|kind| kind.is_file())
            && self.filters.includes(entry.path());
        Ok(wanted.then_some(entry))
    }
}
