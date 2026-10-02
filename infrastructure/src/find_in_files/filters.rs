//! Find in Files filters, as `ignore` override globs.
//!
//! The list is separated by spaces (or `;`). Accepted forms:
//! - `*.rs`, `Makefile`, `foo.*`: include matching file names, at any depth; with none, every
//!   file is included. `*.*` means every file, as on Windows, even without an extension.
//! - `!*.min.js`: exclude matching file names.
//! - `!\dir` or `!/dir`: exclude the folder `dir` directly under the searched folder.
//! - `!+\dir` or `!+/dir`: exclude folders named `dir` at any depth. Globs work: `!+\log*`.
//! - `!dir/`: the same, in `.gitignore` spelling.
//!
//! Matching ignores case, as on Windows. Exclusions win over inclusions.
//!
//! Inclusions only choose among the files the walk visits. `ignore` lets a whitelisted path
//! override the hidden-file and `.gitignore` rules, so an inclusion such as `*.*` given to the
//! walk would bring hidden and ignored files back; only the exclusions are walk overrides, and
//! inclusions are checked file by file.

use std::path::Path;

use ignore::overrides::{Override, OverrideBuilder};

/// Override globs for `filters`: inclusions first, then exclusions, because the last glob that
/// matches decides.
pub(crate) fn globs(filters: &str) -> Vec<String> {
    let mut include = Vec::new();
    let mut exclude = Vec::new();
    for token in filters
        .split([' ', '\t', ';'])
        .filter(|token| !token.is_empty())
    {
        let token = token.replace('\\', "/");
        match token.strip_prefix('!') {
            Some(rest) => {
                if let Some(folder) = rest.strip_prefix("+/") {
                    exclude.push(format!("!{}/", folder.trim_end_matches('/')));
                } else if let Some(folder) = rest.strip_prefix('/') {
                    exclude.push(format!("!/{}/", folder.trim_end_matches('/')));
                } else if !rest.is_empty() {
                    exclude.push(format!("!{rest}"));
                }
            }
            None if token == "*.*" => include.push("*".to_owned()),
            None => include.push(token),
        }
    }
    include.extend(exclude);
    include
}

/// A search's filters under one root.
#[derive(Clone)]
pub(crate) struct Filters {
    /// The exclusions, as overrides for the walk: they never re-include anything.
    pub(crate) excludes: Override,
    includes: Override,
}

impl Filters {
    pub(crate) fn new(root: &Path, filters: &str) -> Result<Self, ignore::Error> {
        let (excludes, includes): (Vec<String>, Vec<String>) = globs(filters)
            .into_iter()
            .partition(|glob| glob.starts_with('!'));
        Ok(Self {
            excludes: overrides(root, &excludes)?,
            includes: overrides(root, &includes)?,
        })
    }

    /// Whether the file at `path` passes the inclusions.
    pub(crate) fn includes(&self, path: &Path) -> bool {
        !self.includes.matched(path, false).is_ignore()
    }
}

fn overrides(root: &Path, globs: &[String]) -> Result<Override, ignore::Error> {
    let mut builder = OverrideBuilder::new(root);
    builder.case_insensitive(true)?;
    for glob in globs {
        builder.add(glob)?;
    }
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_filter_lists() {
        assert_eq!(globs(""), Vec::<String>::new());
        assert_eq!(globs("*.rs *.toml"), ["*.rs", "*.toml"]);
        assert_eq!(globs("!*.min.js *.js;*.ts"), ["*.js", "*.ts", "!*.min.js"]);
        assert_eq!(
            globs(r"*.* !\tests !+\log* !target/"),
            ["*", "!/tests/", "!log*/", "!target/"]
        );
        assert_eq!(globs("!/build/ !"), ["!/build/"]);
    }

    #[test]
    fn filters_decide_what_is_searched() {
        let root = Path::new("/project");
        let filters = Filters::new(root, r"*.rs *.TOML !*.min.rs !\tests !+\gen*").unwrap();
        let skipped_file = |path: &str| {
            let path = root.join(path);
            filters.excludes.matched(&path, false).is_ignore() || !filters.includes(&path)
        };
        let skipped_folder =
            |path: &str| filters.excludes.matched(root.join(path), true).is_ignore();
        assert!(!skipped_file("src/main.rs"));
        assert!(!skipped_file("Cargo.toml"));
        assert!(skipped_file("README.md"));
        assert!(skipped_file("src/app.min.rs"));
        assert!(!skipped_folder("src"));
        assert!(skipped_folder("tests"));
        assert!(!skipped_folder("src/tests"));
        assert!(skipped_folder("src/generated"));
        let everything = Filters::new(root, "*.*").unwrap();
        assert!(everything.includes(&root.join("Makefile")));
        assert!(
            !everything
                .excludes
                .matched(root.join(".hidden"), true)
                .is_whitelist()
        );
    }
}
