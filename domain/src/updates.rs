//! Check for Updates: the version GitHub names as Stet's latest release against the
//! running one, and the command that installs it. The tag comes from the network and ends up in
//! a command the user pastes into a terminal, so only `vX.Y.Z` with plain numbers is read, and
//! the command is built from the numbers, never from the tag's text.

use crate::actions::{Accelerator, ActionId};
use crate::settings::Keymap;
use std::fmt;

/// Stet's public repository, whose releases carry the package.
pub const REPOSITORY: &str = "PixDevsApps/Stet";

/// The name every release from 1.5.0 on gives its package, so that one download link serves
/// every release (BUILD.md, Releasing).
pub const PACKAGE: &str = "stet-x86_64.pkg.tar.zst";

/// Where the command downloads the package to.
const DOWNLOAD: &str = "/tmp/stet.pkg.tar.zst";

/// The largest answer the check reads; a release's holds its notes and its files' list.
pub const MAX_ANSWER: usize = 512 * 1024;

/// A release's version: three plain numbers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    /// `1.2.3` or a tag, `v1.2.3`; nothing else (no pre-release or build suffix, no sign, no
    /// spaces).
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.strip_prefix('v').unwrap_or(text);
        let mut parts = text.split('.');
        let mut number = || {
            let part = parts.next()?;
            if part.is_empty() || part.len() > 9 || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            part.parse().ok()
        };
        let version = Self {
            major: number()?,
            minor: number()?,
            patch: number()?,
        };
        parts.next().is_none().then_some(version)
    }

    /// The release's tag, `v1.2.3`.
    pub fn tag(&self) -> String {
        format!("v{self}")
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Why the check has no answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateError {
    /// No connection to GitHub.
    Offline,
    /// GitHub didn't answer in time.
    Timeout,
    /// The connection to GitHub couldn't be verified.
    Certificate,
    /// GitHub limits the requests from this network.
    Busy,
    /// GitHub has no release of Stet.
    NoRelease,
    /// An answer that names no version.
    Unreadable,
    /// `curl`, which makes the request, isn't installed.
    NoCurl,
}

/// The version GitHub's answer for the latest release names: its `tag_name`, and nothing else
/// of it.
pub fn release_version(answer: &[u8]) -> Result<Version, UpdateError> {
    #[derive(serde::Deserialize)]
    struct Release {
        tag_name: String,
    }
    if answer.len() > MAX_ANSWER {
        return Err(UpdateError::Unreadable);
    }
    let release: Release = serde_json::from_slice(answer).map_err(|_| UpdateError::Unreadable)?;
    Version::parse(&release.tag_name).ok_or(UpdateError::Unreadable)
}

/// The running Stet against the latest release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The running version is the latest release.
    Latest,
    /// A newer release is out.
    Newer(Version),
    /// The running version is newer than the latest release (a build from `main`).
    Ahead(Version),
}

pub fn compare(running: Version, latest: Version) -> Outcome {
    match running.cmp(&latest) {
        std::cmp::Ordering::Less => Outcome::Newer(latest),
        std::cmp::Ordering::Equal => Outcome::Latest,
        std::cmp::Ordering::Greater => Outcome::Ahead(latest),
    }
}

/// The command that installs `version`'s own package: it downloads that release's package to
/// `/tmp` and installs it with pacman, which replaces the version installed. The README's
/// install command is the same with `latest` in place of the version.
pub fn update_command(version: Version) -> String {
    format!(
        "curl -L -o {DOWNLOAD} https://github.com/{REPOSITORY}/releases/download/{}/{PACKAGE} \
         && sudo pacman -U {DOWNLOAD}",
        version.tag()
    )
}

/// What the dialog says once the check has an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub heading: String,
    pub body: String,
    /// The command that installs the newer release.
    pub command: Option<String>,
}

/// The dialog's words for the check's `result`, with the Quit key from `keymap`.
pub fn answer(running: Version, result: Result<Version, UpdateError>, keymap: &Keymap) -> Answer {
    let latest = match result {
        Ok(latest) => latest,
        Err(error) => {
            return Answer {
                heading: "Couldn't check for updates".to_owned(),
                body: failure(error).to_owned(),
                command: None,
            };
        }
    };
    match compare(running, latest) {
        Outcome::Latest => Answer {
            heading: "You have the latest version".to_owned(),
            body: format!("Stet {running} is the newest release."),
            command: None,
        },
        Outcome::Ahead(latest) => Answer {
            heading: "You have the latest version".to_owned(),
            body: format!("This Stet, {running}, is newer than the latest release, {latest}."),
            command: None,
        },
        Outcome::Newer(latest) => {
            let quit = match keymap
                .keys(ActionId::Quit)
                .first()
                .and_then(|key| Accelerator::parse(key))
            {
                Some(key) => format!("press {} to quit Stet", key.display()),
                None => "choose File › Quit in Stet's menu".to_owned(),
            };
            Answer {
                heading: format!("Stet {latest} is available"),
                body: format!(
                    "You have {running}. To update, copy this command, paste it into a terminal \
                     and press Enter; it asks for your computer's password. Then {quit} and \
                     start it again: your tabs and unsaved text come back."
                ),
                command: Some(update_command(latest)),
            }
        }
    }
}

fn failure(error: UpdateError) -> &'static str {
    match error {
        UpdateError::Offline => {
            "GitHub, where Stet's releases are, couldn't be reached. Check your internet \
             connection and try again."
        }
        UpdateError::Timeout => "GitHub didn't answer in time. Try again later.",
        UpdateError::Certificate => {
            "The connection to GitHub couldn't be verified, so its answer wasn't used. A network \
             that intercepts secure connections can cause this."
        }
        UpdateError::Busy => {
            "GitHub is limiting requests from your network right now. Try again in an hour."
        }
        UpdateError::NoRelease => "GitHub has no Stet release to compare with.",
        UpdateError::Unreadable => "GitHub's answer couldn't be read. Try again later.",
        UpdateError::NoCurl => {
            "Stet asks GitHub with the curl program, which isn't installed. Install it with \
             sudo pacman -S curl and try again."
        }
    }
}

/// GitHub's answer for Stet's latest release: drafts and pre-releases are never it.
pub fn latest_release_url() -> String {
    format!("https://api.github.com/repos/{REPOSITORY}/releases/latest")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(major: u32, minor: u32, patch: u32) -> Version {
        Version {
            major,
            minor,
            patch,
        }
    }

    #[test]
    fn reads_plain_versions_and_tags_only() {
        assert_eq!(Version::parse("1.5.0"), Some(v(1, 5, 0)));
        assert_eq!(Version::parse("v1.5.0"), Some(v(1, 5, 0)));
        assert_eq!(Version::parse("v1.10.200"), Some(v(1, 10, 200)));
        for text in [
            "",
            "v",
            "1.2",
            "1.2.3.4",
            "1..3",
            "V1.2.3",
            "vv1.2.3",
            " 1.2.3",
            "1.2.3 ",
            "1.2.3-rc1",
            "1.2.3+build",
            "+1.2.3",
            "1.-2.3",
            "1.2.3;rm -rf ~",
            "1.2.3$(id)",
            "١.٢.٣",
            "1.2.9999999999",
        ] {
            assert_eq!(Version::parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn the_running_version_against_the_latest() {
        assert_eq!(compare(v(1, 5, 0), v(1, 5, 0)), Outcome::Latest);
        assert_eq!(compare(v(1, 5, 0), v(1, 6, 0)), Outcome::Newer(v(1, 6, 0)));
        assert_eq!(compare(v(1, 5, 0), v(1, 5, 1)), Outcome::Newer(v(1, 5, 1)));
        assert_eq!(compare(v(1, 9, 9), v(2, 0, 0)), Outcome::Newer(v(2, 0, 0)));
        assert_eq!(compare(v(1, 10, 0), v(1, 9, 0)), Outcome::Ahead(v(1, 9, 0)));
        assert_eq!(compare(v(1, 5, 1), v(1, 5, 0)), Outcome::Ahead(v(1, 5, 0)));
    }

    #[test]
    fn the_command_installs_that_release() {
        assert_eq!(
            update_command(v(1, 6, 0)),
            "curl -L -o /tmp/stet.pkg.tar.zst \
             https://github.com/PixDevsApps/Stet/releases/download/v1.6.0/\
             stet-x86_64.pkg.tar.zst && sudo pacman -U /tmp/stet.pkg.tar.zst"
        );
        assert_eq!(
            latest_release_url(),
            "https://api.github.com/repos/PixDevsApps/Stet/releases/latest"
        );
    }

    #[test]
    fn reads_only_the_answers_tag() {
        let answer = br#"{"tag_name": "v1.6.0", "name": "Stet 1.6.0", "draft": false,
            "body": "Notes", "assets": [{"name": "stet-x86_64.pkg.tar.zst"}]}"#;
        assert_eq!(release_version(answer), Ok(v(1, 6, 0)));
        for answer in [
            &br#"{"tag_name": "v1.6.0; rm -rf ~"}"#[..],
            br#"{"tag_name": "nightly"}"#,
            br#"{"tag_name": 160}"#,
            b"{}",
            b"not json",
            b"",
        ] {
            assert_eq!(
                release_version(answer),
                Err(UpdateError::Unreadable),
                "{}",
                String::from_utf8_lossy(answer)
            );
        }
        let mut long = br#"{"tag_name": "v1.6.0", "body": ""#.to_vec();
        long.resize(MAX_ANSWER, b'x');
        long.extend_from_slice(br#""}"#);
        assert_eq!(release_version(&long), Err(UpdateError::Unreadable));
    }

    #[test]
    fn says_whether_this_is_the_latest() {
        let keymap = Keymap::default();
        let same = answer(v(1, 5, 0), Ok(v(1, 5, 0)), &keymap);
        assert_eq!(same.heading, "You have the latest version");
        assert_eq!(same.body, "Stet 1.5.0 is the newest release.");
        assert_eq!(same.command, None);
        let ahead = answer(v(1, 5, 0), Ok(v(1, 3, 4)), &keymap);
        assert_eq!(ahead.heading, "You have the latest version");
        assert!(
            ahead
                .body
                .contains("1.5.0, is newer than the latest release, 1.3.4"),
            "{}",
            ahead.body
        );
        assert_eq!(ahead.command, None);
    }

    #[test]
    fn a_newer_release_comes_with_its_command() {
        let newer = answer(v(1, 5, 0), Ok(v(1, 6, 0)), &Keymap::default());
        assert_eq!(newer.heading, "Stet 1.6.0 is available");
        assert!(newer.body.starts_with("You have 1.5.0."), "{}", newer.body);
        assert!(
            newer.body.contains("press Ctrl+Alt+Q to quit Stet"),
            "{}",
            newer.body
        );
        assert_eq!(newer.command, Some(update_command(v(1, 6, 0))));
    }

    #[test]
    fn failures_say_what_to_do() {
        for error in [
            UpdateError::Offline,
            UpdateError::Timeout,
            UpdateError::Certificate,
            UpdateError::Busy,
            UpdateError::NoRelease,
            UpdateError::Unreadable,
            UpdateError::NoCurl,
        ] {
            let failed = answer(v(1, 5, 0), Err(error), &Keymap::default());
            assert_eq!(failed.heading, "Couldn't check for updates");
            assert!(!failed.body.is_empty());
            assert_eq!(failed.command, None);
        }
    }

    #[test]
    fn the_workspace_version_reads() {
        assert!(Version::parse(env!("CARGO_PKG_VERSION")).is_some());
    }
}
