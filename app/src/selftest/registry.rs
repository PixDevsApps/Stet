//! The self-test commands, each declared once: its name, how many arguments it takes and the
//! [`Harness`] method that runs it, in its area's `COMMANDS` table. The script parser checks
//! names and argument counts against the tables before the app starts; a step runs by calling
//! its command's handler, and `wait-until` polls an assertion's handler the same way.

use super::{Harness, StepResult};
use std::future::Future;
use std::pin::Pin;

/// No upper limit on the number of arguments.
pub(super) const MANY: usize = usize::MAX;

/// A step that is running.
pub(super) type Running<'a> = Pin<Box<dyn Future<Output = StepResult> + 'a>>;

/// A command a script can use.
pub(super) struct Command {
    pub(super) name: &'static str,
    /// The fewest arguments it takes.
    pub(super) min: usize,
    /// The most arguments it takes, or [`MANY`].
    pub(super) max: usize,
    /// Runs it with arguments whose count the parser checked.
    pub(super) run: for<'a> fn(&'a Harness, &'a [String]) -> Running<'a>,
}

/// What a handler returns: `()` when it cannot fail, else a [`StepResult`].
pub(super) trait Outcome {
    fn into_result(self) -> StepResult;
}

impl Outcome for () {
    fn into_result(self) -> StepResult {
        Ok(())
    }
}

impl Outcome for StepResult {
    fn into_result(self) -> StepResult {
        self
    }
}

/// An area's table: `"name" (min, max) => handler` for a [`Harness`] method that returns at
/// once, `"name" (min, max) => async handler` for one that awaits. A handler takes
/// `(&self, args: &[String])` and returns `()` or a [`StepResult`].
macro_rules! commands {
    ($($name:literal ($min:expr, $max:expr) => $first:ident $($second:ident)?),* $(,)?) => {
        &[$($crate::selftest::registry::Command {
            name: $name,
            min: $min,
            max: $max,
            run: $crate::selftest::registry::commands!(@run $first $($second)?),
        }),*]
    };
    (@run async $handler:ident) => {
        |harness, args| {
            Box::pin(async move {
                $crate::selftest::registry::Outcome::into_result(
                    $crate::selftest::Harness::$handler(harness, args).await,
                )
            })
        }
    };
    (@run $handler:ident) => {
        |harness, args| {
            Box::pin(std::future::ready(
                $crate::selftest::registry::Outcome::into_result(
                    $crate::selftest::Harness::$handler(harness, args),
                ),
            ))
        }
    };
}
pub(super) use commands;

/// Every area's table.
const AREAS: &[&[Command]] = &[
    super::files::COMMANDS,
    super::documents::COMMANDS,
    super::editing::COMMANDS,
    super::actions::COMMANDS,
    super::dialogs::COMMANDS,
    super::theme::COMMANDS,
    super::timing::COMMANDS,
    super::screenshot::COMMANDS,
    super::search::COMMANDS,
    super::focus::COMMANDS,
    super::session::COMMANDS,
    super::tools::COMMANDS,
    super::column::COMMANDS,
    super::nav::COMMANDS,
    super::views::COMMANDS,
];

/// Every command.
pub(super) fn all() -> impl Iterator<Item = &'static Command> {
    AREAS.iter().copied().flatten()
}

/// The command called `name`.
pub(super) fn find(name: &str) -> Option<&'static Command> {
    all().find(|command| command.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    #[test]
    fn every_command_is_declared_once() {
        let mut seen = HashSet::new();
        for command in all() {
            assert!(
                seen.insert(command.name),
                "{} is declared twice",
                command.name
            );
            assert!(command.min <= command.max, "{}", command.name);
            assert!(
                command
                    .name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{}",
                command.name
            );
        }
    }

    fn scripts(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                scripts(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "stet-test") {
                out.push(path);
            }
        }
    }

    /// Parses every script in the repository with the real parser, without running it.
    #[test]
    fn every_script_uses_registered_commands() {
        const VARS: &[(&str, &str)] = &[("TMP", "/tmp/run"), ("DIR", "/repo/tests")];
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/selftest");
        let mut paths = Vec::new();
        scripts(&dir, &mut paths);
        assert!(!paths.is_empty(), "no scripts in {}", dir.display());
        for path in paths {
            let text = std::fs::read_to_string(&path).unwrap();
            let steps = super::super::script::parse(&text, VARS)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            for step in steps {
                let polled = (step.command == "wait-until").then(|| step.args[0].as_str());
                for name in [Some(step.command.as_str()), polled].into_iter().flatten() {
                    assert!(
                        find(name).is_some(),
                        "{}: line {}: {name} is not registered",
                        path.display(),
                        step.line
                    );
                }
            }
        }
    }
}
