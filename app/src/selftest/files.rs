//! Self-test commands for files on disk: writing and changing them before or while the app
//! runs, and checking what the app left there.

use super::registry::{Command, commands};
use super::script::hex_bytes;
use super::{Harness, StepResult, equal, generate, number, write_file};
use crate::runtime::worker;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub(super) const COMMANDS: &[Command] = commands![
    "launch-file" (2, 2) => launch_file,
    "write" (2, 2) => write,
    "write-hex" (2, 2) => write_hex,
    "write-generated" (3, 3) => async write_generated,
    "chmod" (2, 2) => chmod,
    "mkdir" (1, 1) => mkdir,
    "rename" (2, 2) => rename,
    "remove" (1, 1) => remove,
    "symlink" (2, 2) => symlink,
    "truncate" (2, 2) => truncate,
    "set-mtime" (2, 2) => set_mtime,
    "assert-file" (2, 2) => assert_file,
    "assert-file-hex" (2, 2) => assert_file_hex,
    "assert-file-contains" (2, 2) => assert_file_contains,
    "assert-mode" (2, 2) => assert_mode,
    "assert-missing" (1, 1) => assert_missing,
    "assert-exists" (1, 1) => assert_exists,
    "assert-symlink" (2, 2) => assert_symlink,
    "assert-files-like" (3, 3) => assert_files_like,
];

impl Harness {
    /// `launch-file <path> <text>`: only before every other step; the file is written and put
    /// on the app's first command line before the app starts (see `run`), so the step itself
    /// does nothing.
    fn launch_file(&self, _: &[String]) {}

    /// `write <path> <text>`: writes the text to the file, creating its folders.
    fn write(&self, args: &[String]) -> StepResult {
        write_file(Path::new(&args[0]), args[1].as_bytes())
    }

    /// `write-hex <path> <hex bytes>`: writes the bytes to the file, creating its folders.
    fn write_hex(&self, args: &[String]) -> StepResult {
        write_file(Path::new(&args[0]), &hex_bytes(&args[1])?)
    }

    /// `write-generated <path> <kind> <bytes>`: writes at least that many bytes of generated
    /// text of that kind (see `generate`), on a worker.
    async fn write_generated(&self, args: &[String]) -> StepResult {
        let path = PathBuf::from(&args[0]);
        let kind = args[1].clone();
        let bytes = number(&args[2])?;
        worker::run(move || {
            let text = generate::generate(&kind, bytes)?;
            write_file(&path, text.as_bytes())
        })
        .await
        .ok_or("the worker failed")?
    }

    /// `chmod <path> <octal mode>`: sets the file's permissions.
    fn chmod(&self, args: &[String]) -> StepResult {
        let mode = u32::from_str_radix(&args[1], 8).map_err(|error| error.to_string())?;
        fs::set_permissions(&args[0], fs::Permissions::from_mode(mode))
            .map_err(|error| error.to_string())
    }

    /// `mkdir <path>`: creates the folder and its parents.
    fn mkdir(&self, args: &[String]) -> StepResult {
        fs::create_dir_all(&args[0]).map_err(|error| error.to_string())
    }

    /// `rename <from> <to>`: renames a file or folder.
    fn rename(&self, args: &[String]) -> StepResult {
        fs::rename(&args[0], &args[1]).map_err(|error| error.to_string())
    }

    /// `remove <path>`: removes a folder with its contents, or a file.
    fn remove(&self, args: &[String]) -> StepResult {
        fs::remove_dir_all(&args[0])
            .or_else(|_| fs::remove_file(&args[0]))
            .map_err(|error| error.to_string())
    }

    /// `symlink <target> <link>`: makes a symbolic link.
    fn symlink(&self, args: &[String]) -> StepResult {
        std::os::unix::fs::symlink(&args[0], &args[1]).map_err(|error| error.to_string())
    }

    /// `truncate <path> <bytes>`: a sparse file of that size, for size checks that never read
    /// it.
    fn truncate(&self, args: &[String]) -> StepResult {
        let file = fs::File::create(&args[0]).map_err(|error| error.to_string())?;
        file.set_len(number(&args[1])? as u64)
            .map_err(|error| error.to_string())
    }

    /// `set-mtime <path> <seconds>`: sets the file's modification time that many seconds ago.
    fn set_mtime(&self, args: &[String]) -> StepResult {
        let ago = Duration::from_secs(number(&args[1])? as u64);
        let file = fs::File::options()
            .write(true)
            .open(&args[0])
            .map_err(|error| error.to_string())?;
        file.set_modified(SystemTime::now() - ago)
            .map_err(|error| error.to_string())
    }

    /// `assert-file <path> <text>`: the file holds exactly that text.
    fn assert_file(&self, args: &[String]) -> StepResult {
        let text = fs::read_to_string(&args[0]).map_err(|error| error.to_string())?;
        equal("file contents", text, args[1].clone())
    }

    /// `assert-file-hex <path> <hex bytes>`: the file holds exactly those bytes.
    fn assert_file_hex(&self, args: &[String]) -> StepResult {
        let bytes = fs::read(&args[0]).map_err(|error| error.to_string())?;
        equal("file bytes", bytes, hex_bytes(&args[1])?)
    }

    /// `assert-file-contains <path> <text>`: the file's text contains the text.
    fn assert_file_contains(&self, args: &[String]) -> StepResult {
        match fs::read_to_string(&args[0]) {
            Ok(text) if text.contains(args[1].as_str()) => Ok(()),
            Ok(text) => Err(format!("{} lacks {:?}:\n{text}", args[0], args[1])),
            Err(error) => Err(format!("{}: {error}", args[0])),
        }
    }

    /// `assert-mode <path> <octal mode>`: the file's permission bits.
    fn assert_mode(&self, args: &[String]) -> StepResult {
        let mode = fs::metadata(&args[0])
            .map_err(|error| error.to_string())?
            .permissions()
            .mode()
            & 0o7777;
        equal(
            "mode",
            format!("{mode:o}"),
            args[1].trim_start_matches('0').to_owned(),
        )
    }

    /// `assert-missing <path>`: nothing is there.
    fn assert_missing(&self, args: &[String]) -> StepResult {
        if Path::new(&args[0]).exists() {
            Err(format!("{} exists", args[0]))
        } else {
            Ok(())
        }
    }

    /// `assert-exists <path>`: something is there.
    fn assert_exists(&self, args: &[String]) -> StepResult {
        if Path::new(&args[0]).exists() {
            Ok(())
        } else {
            Err(format!("{} does not exist", args[0]))
        }
    }

    /// `assert-symlink <link> <target>`: the link points at the target.
    fn assert_symlink(&self, args: &[String]) -> StepResult {
        let target = fs::read_link(&args[0]).map_err(|error| error.to_string())?;
        equal("link target", target, PathBuf::from(&args[1]))
    }

    /// `assert-files-like <folder> <text> <count>`: that many entries of the folder have the
    /// text in their names.
    fn assert_files_like(&self, args: &[String]) -> StepResult {
        let count = fs::read_dir(&args[0])
            .map_err(|error| error.to_string())?
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .contains(args[1].as_str())
            })
            .count();
        equal("matching files", count, number(&args[2])?)
    }
}
