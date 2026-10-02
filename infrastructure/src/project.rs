//! A project's files, for quick open (M8): the project root of a folder, and a walk that lists
//! the files someone would open there (no hidden files, nothing `.gitignore` ignores, nothing
//! that looks binary by its extension), streamed in batches and capped.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use ignore::{WalkBuilder, WalkState};

/// Files go to the sink in batches of at most this many, or whatever arrived in
/// [`BATCH_INTERVAL`].
pub const BATCH_FILES: usize = 4096;
pub const BATCH_INTERVAL: Duration = Duration::from_millis(16);

/// Extensions of files that are not text, which quick open leaves out.
const BINARY_EXTENSIONS: &[&str] = &[
    "7z", "a", "apk", "avi", "bin", "blend", "bmp", "bz2", "class", "dat", "db", "deb", "dll",
    "dmg", "doc", "docx", "dylib", "ear", "eot", "exe", "fbx", "flac", "gif", "glb", "gz", "ico",
    "iso", "jar", "jpeg", "jpg", "lib", "m4a", "mkv", "mov", "mp3", "mp4", "o", "obj", "odp",
    "ods", "odt", "ogg", "opus", "otf", "pdf", "png", "ppt", "pptx", "psd", "pyc", "pyo", "rar",
    "rlib", "rmeta", "rpm", "so", "sqlite", "sqlite3", "tar", "tgz", "tif", "tiff", "ttf", "war",
    "wasm", "wav", "webm", "webp", "woff", "woff2", "xcf", "xls", "xlsx", "xz", "zip", "zst",
];

/// The folder a document belongs to as a project: the nearest folder from `dir` up that holds
/// `.git` (a folder, or a file in a worktree), else `dir` itself.
pub fn project_root(dir: &Path) -> PathBuf {
    dir.ancestors()
        .find(|folder| folder.join(".git").exists())
        .unwrap_or(dir)
        .to_path_buf()
}

/// Whether `path`'s extension says it is not text.
pub fn looks_binary(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            BINARY_EXTENSIONS
                .binary_search(&extension.to_ascii_lowercase().as_str())
                .is_ok()
        })
}

/// A file the walk found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectFile {
    pub path: PathBuf,
    /// The path relative to the root, with `/` between folders.
    pub relative: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WalkSummary {
    pub files: usize,
    /// The cap was reached and the walk stopped there.
    pub truncated: bool,
    pub cancelled: bool,
    /// When the first batch went to the sink.
    pub first_batch: Option<Duration>,
    pub elapsed: Duration,
}

/// Lists the files under `root`, at most `cap`, in no fixed order, handing them to `sink` in
/// batches from the calling thread while a parallel walk runs on others. Hidden files and
/// folders, what `.gitignore`, `.ignore` and git's excludes ignore, and binary-looking files
/// are left out; symbolic links are not followed. Setting `cancel` stops the walk soon after.
pub fn walk_project(
    root: &Path,
    cap: usize,
    cancel: &AtomicBool,
    mut sink: impl FnMut(Vec<ProjectFile>),
) -> WalkSummary {
    let started = Instant::now();
    let found = AtomicUsize::new(0);
    let truncated = AtomicBool::new(false);
    let (sender, receiver) = crossbeam_channel::unbounded::<ProjectFile>();
    let mut summary = WalkSummary::default();
    std::thread::scope(|scope| {
        let walk = scope.spawn(|| {
            WalkBuilder::new(root)
                .hidden(true)
                .ignore(true)
                .git_ignore(true)
                .git_global(true)
                .git_exclude(true)
                .parents(true)
                .follow_links(false)
                .build_parallel()
                .run(|| {
                    let sender = sender.clone();
                    let (found, truncated) = (&found, &truncated);
                    Box::new(move |entry| {
                        if cancel.load(Ordering::Relaxed) || truncated.load(Ordering::Relaxed) {
                            return WalkState::Quit;
                        }
                        let Ok(entry) = entry else {
                            return WalkState::Continue;
                        };
                        if !entry.file_type().is_some_and(|kind| kind.is_file())
                            || looks_binary(entry.path())
                        {
                            return WalkState::Continue;
                        }
                        if found.fetch_add(1, Ordering::Relaxed) >= cap {
                            truncated.store(true, Ordering::Relaxed);
                            return WalkState::Quit;
                        }
                        let path = entry.into_path();
                        let relative = path
                            .strip_prefix(root)
                            .unwrap_or(&path)
                            .to_string_lossy()
                            .into_owned();
                        let _ = sender.send(ProjectFile { path, relative });
                        WalkState::Continue
                    })
                });
            drop(sender);
        });
        let mut batch = Vec::with_capacity(BATCH_FILES);
        let mut since = Instant::now();
        loop {
            let received = receiver.recv_timeout(BATCH_INTERVAL);
            let disconnected = matches!(
                received,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected)
            );
            if let Ok(file) = received {
                batch.push(file);
            }
            let due = batch.len() >= BATCH_FILES || since.elapsed() >= BATCH_INTERVAL;
            if !batch.is_empty() && (due || disconnected) {
                summary.files += batch.len();
                summary.first_batch.get_or_insert_with(|| started.elapsed());
                sink(std::mem::replace(
                    &mut batch,
                    Vec::with_capacity(BATCH_FILES),
                ));
                since = Instant::now();
            }
            if disconnected {
                break;
            }
        }
        let _ = walk.join();
    });
    summary.truncated = truncated.load(Ordering::Relaxed);
    summary.cancelled = cancel.load(Ordering::Relaxed);
    summary.elapsed = started.elapsed();
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(root: &Path, relative: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "x").unwrap();
    }

    fn listed(root: &Path, cap: usize) -> (Vec<String>, WalkSummary) {
        let mut files = Vec::new();
        let summary = walk_project(root, cap, &AtomicBool::new(false), |batch| {
            files.extend(batch.into_iter().map(|file| file.relative));
        });
        files.sort();
        (files, summary)
    }

    #[test]
    fn the_extension_list_is_sorted_for_binary_search() {
        assert!(BINARY_EXTENSIONS.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(looks_binary(Path::new("a/photo.PNG")));
        assert!(!looks_binary(Path::new("a/main.rs")));
        assert!(!looks_binary(Path::new("Makefile")));
    }

    #[test]
    fn the_root_is_the_nearest_git_folder() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(repo.join("src/deep")).unwrap();
        assert_eq!(project_root(&repo.join("src/deep")), repo);
        assert_eq!(project_root(&repo), repo);
        let worktree = dir.path().join("worktree");
        fs::create_dir_all(worktree.join("sub")).unwrap();
        fs::write(worktree.join(".git"), "gitdir: elsewhere\n").unwrap();
        assert_eq!(project_root(&worktree.join("sub")), worktree);
        let plain = dir.path().join("plain/folder");
        fs::create_dir_all(&plain).unwrap();
        assert_eq!(project_root(&plain), plain);
    }

    #[test]
    fn lists_text_files_without_hidden_ignored_or_binary_ones() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::write(root.join(".gitignore"), "target/\n*.log\n").unwrap();
        for file in [
            "src/main.rs",
            "src/window/mod.rs",
            "README.md",
            "target/debug/build.rs",
            "run.log",
            ".hidden/secret.txt",
            ".env",
            "assets/icon.png",
        ] {
            write(root, file);
        }
        let (files, summary) = listed(root, 100);
        assert_eq!(files, ["README.md", "src/main.rs", "src/window/mod.rs"]);
        assert_eq!(summary.files, 3);
        assert!(!summary.truncated && !summary.cancelled);
        assert!(summary.first_batch.is_some());
    }

    #[test]
    fn the_cap_cuts_the_walk() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..50 {
            write(dir.path(), &format!("f{index:02}.txt"));
        }
        let (files, summary) = listed(dir.path(), 20);
        assert_eq!(files.len(), 20);
        assert_eq!(summary.files, 20);
        assert!(summary.truncated);
    }

    #[test]
    fn a_cancelled_walk_stops() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..200 {
            write(dir.path(), &format!("d{}/f{index}.txt", index % 10));
        }
        let cancel = AtomicBool::new(true);
        let mut count = 0;
        let summary = walk_project(dir.path(), 1000, &cancel, |batch| count += batch.len());
        assert!(summary.cancelled);
        assert!(count < 200, "{count}");
    }
}
