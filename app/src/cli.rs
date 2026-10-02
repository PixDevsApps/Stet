//! The command line. It is parsed twice: locally in `main`, so a typo never reaches a running
//! editor, and again in the primary instance for forwarded command lines, where errors go back
//! to the calling terminal (ADR-011). Neither path calls `process::exit`.
//!
//! `--gapplication-service` is GApplication's own option for D-Bus activation (the service
//! file runs `stet --gapplication-service`); it is accepted here, hidden, so that the local
//! parse lets it through to GApplication.

use clap::{Arg, ArgAction, Command, value_parser};
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cli {
    pub files: Vec<OsString>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub self_test: Option<PathBuf>,
    /// Block until every file given is closed (`GIT_EDITOR="stet --wait"`).
    pub wait: bool,
    /// Start without restoring the session and without keeping this one.
    pub no_session: bool,
    /// Run as the D-Bus-activated service.
    pub service: bool,
}

pub fn command() -> Command {
    Command::new("stet")
        .about("A fast, keyboard-first text and code editor for Omarchy")
        .version(env!("CARGO_PKG_VERSION"))
        .arg(
            Arg::new("files")
                .value_name("FILE[:LINE[:COL]]")
                .num_args(0..)
                .action(ArgAction::Append)
                .value_parser(value_parser!(OsString))
                .help(
                    "Files to open. A :LINE or :LINE:COL suffix is used when the path \
                     itself does not exist",
                ),
        )
        .arg(
            Arg::new("line")
                .short('n')
                .value_name("LINE")
                .value_parser(value_parser!(u32).range(1..))
                .help("Go to LINE in the files given"),
        )
        .arg(
            Arg::new("column")
                .short('c')
                .value_name("COL")
                .value_parser(value_parser!(u32).range(1..))
                .help("Go to column COL in the files given"),
        )
        .arg(
            Arg::new("wait")
                .long("wait")
                .short('w')
                .action(ArgAction::SetTrue)
                .help(
                    "Wait until the files given are closed; exit with status 1 if one was \
                     closed with unsaved changes (for GIT_EDITOR and SUDO_EDITOR)",
                ),
        )
        .arg(
            Arg::new("no-session")
                .long("no-session")
                .action(ArgAction::SetTrue)
                .help(
                    "When this starts Stet: don't restore the last session and don't keep \
                     this one",
                ),
        )
        .arg(
            Arg::new("gapplication-service")
                .long("gapplication-service")
                .action(ArgAction::SetTrue)
                .hide(true),
        )
        .arg(
            Arg::new("self-test")
                .long("self-test")
                .value_name("SCRIPT")
                .value_parser(value_parser!(PathBuf))
                .help("Run a self-test script in a new window and exit (see docs/TESTING.md)"),
        )
}

/// Parses `args`, including the program name. Help and version requests come back as errors
/// whose `exit_code()` is 0, as clap reports them.
pub fn parse<I, T>(args: I) -> Result<Cli, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let matches = command().try_get_matches_from(args)?;
    Ok(Cli {
        files: matches
            .get_many::<OsString>("files")
            .map(|files| files.cloned().collect())
            .unwrap_or_default(),
        line: matches.get_one::<u32>("line").copied(),
        column: matches.get_one::<u32>("column").copied(),
        self_test: matches.get_one::<PathBuf>("self-test").cloned(),
        wait: matches.get_flag("wait"),
        no_session: matches.get_flag("no-session"),
        service: matches.get_flag("gapplication-service"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    #[test]
    fn files_and_positions() {
        let cli = parse(["stet", "a.rs", "b.txt:40", "-n", "5", "-c3"]).unwrap();
        assert_eq!(
            cli.files,
            [OsString::from("a.rs"), OsString::from("b.txt:40")]
        );
        assert_eq!(cli.line, Some(5));
        assert_eq!(cli.column, Some(3));
        assert_eq!(cli.self_test, None);
        assert_eq!(parse(["stet"]).unwrap(), Cli::default());
    }

    #[test]
    fn attached_short_values() {
        let cli = parse(["stet", "-n40", "x"]).unwrap();
        assert_eq!(cli.line, Some(40));
        assert_eq!(cli.files, [OsString::from("x")]);
    }

    #[test]
    fn dash_prefixed_files_after_double_dash() {
        let cli = parse(["stet", "--", "-weird.txt"]).unwrap();
        assert_eq!(cli.files, [OsString::from("-weird.txt")]);
    }

    #[test]
    fn errors_never_exit() {
        let error = parse(["stet", "--no-such-flag"]).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
        assert_eq!(error.exit_code(), 2);
        assert_eq!(parse(["stet", "-n", "0"]).unwrap_err().exit_code(), 2);
        assert_eq!(parse(["stet", "-n", "x"]).unwrap_err().exit_code(), 2);
        let help = parse(["stet", "--help"]).unwrap_err();
        assert_eq!(help.kind(), ErrorKind::DisplayHelp);
        assert_eq!(help.exit_code(), 0);
        assert!(help.to_string().contains("FILE[:LINE[:COL]]"));
        let version = parse(["stet", "--version"]).unwrap_err();
        assert_eq!(version.kind(), ErrorKind::DisplayVersion);
    }

    #[test]
    fn wait_and_session_flags() {
        let cli = parse(["stet", "--wait", "COMMIT_EDITMSG"]).unwrap();
        assert!(cli.wait && !cli.no_session && !cli.service);
        assert_eq!(cli.files, [OsString::from("COMMIT_EDITMSG")]);
        assert!(parse(["stet", "-w", "x"]).unwrap().wait);
        let cli = parse(["stet", "--no-session"]).unwrap();
        assert!(cli.no_session && cli.files.is_empty());
        assert!(parse(["stet", "--gapplication-service"]).unwrap().service);
        let help = parse(["stet", "--help"]).unwrap_err().to_string();
        assert!(
            help.contains("--wait") && help.contains("--no-session"),
            "{help}"
        );
        assert!(!help.contains("gapplication-service"), "{help}");
    }

    #[test]
    fn self_test_takes_a_script() {
        let cli = parse(["stet", "--self-test", "tests/selftest/files.stet-test"]).unwrap();
        assert_eq!(
            cli.self_test,
            Some(PathBuf::from("tests/selftest/files.stet-test"))
        );
    }

    #[test]
    fn command_is_consistent() {
        command().debug_assert();
    }

    #[test]
    fn the_help_starts_with_the_description() {
        let help = command().render_help().to_string();
        assert_eq!(
            help.lines().next(),
            Some("A fast, keyboard-first text and code editor for Omarchy")
        );
    }
}
