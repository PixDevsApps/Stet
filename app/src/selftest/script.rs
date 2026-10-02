//! The `.stet-test` script format (docs/TESTING.md): one step per line, a command followed by
//! arguments separated by spaces. `#` starts a comment line. `"double quotes"` group words and
//! expand `\n`, `\r`, `\t`, `\\`, `\"` and `\$`. `$TMP` (the run's scratch directory) and `$DIR`
//! (the script's directory) are expanded everywhere. Unknown commands and wrong argument
//! counts fail before the app starts.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// 1-based line in the script.
    pub line: usize,
    /// The line as written, for the report.
    pub source: String,
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

/// Commands and their argument counts (minimum, maximum). Assertions (`assert-*`) may also
/// follow `wait-until`, which polls them.
pub const COMMANDS: &[(&str, usize, usize)] = &[
    ("launch-file", 2, 2),
    ("write", 2, 2),
    ("write-hex", 2, 2),
    ("chmod", 2, 2),
    ("mkdir", 1, 1),
    ("rename", 2, 2),
    ("remove", 1, 1),
    ("open", 1, usize::MAX),
    ("spawn-stet", 1, usize::MAX),
    ("spawn-stet-expect", 2, usize::MAX),
    ("action", 1, 2),
    ("type", 1, 1),
    ("cursor", 2, 2),
    ("key", 2, 2),
    ("find", 1, 1),
    ("find-case", 1, 1),
    ("palette", 0, 1),
    ("palette-activate", 0, 0),
    ("palette-close", 0, 0),
    ("choose-language", 1, 1),
    ("drop", 1, usize::MAX),
    ("respond", 1, 1),
    ("save-as", 1, 1),
    ("set-tab-width", 1, 1),
    ("toggle-overwrite", 0, 0),
    ("theme-set", 2, 2),
    ("wait-idle", 0, 0),
    ("wait-frames", 1, 1),
    ("sleep", 1, 1),
    ("screenshot", 1, 1),
    ("dump-layout", 0, 0),
    ("wait-until", 1, usize::MAX),
    ("assert-text", 1, 1),
    ("assert-text-contains", 1, 1),
    ("assert-selection", 1, 1),
    ("assert-current-match", 1, 1),
    ("assert-tab-count", 1, 1),
    ("assert-title", 1, 1),
    ("assert-tab-title", 2, 2),
    ("assert-cursor", 2, 2),
    ("assert-language", 1, 1),
    ("assert-dirty", 1, 1),
    ("assert-read-only", 1, 1),
    ("assert-banner", 1, 1),
    ("assert-file", 2, 2),
    ("assert-file-hex", 2, 2),
    ("assert-mode", 2, 2),
    ("assert-missing", 1, 1),
    ("assert-status", 1, 1),
    ("assert-match-count", 1, 1),
    ("assert-find-open", 1, 1),
    ("assert-theme", 3, 3),
    ("assert-chrome-fg", 1, 1),
    ("assert-dark", 1, 1),
    ("assert-theme-source", 1, 1),
    ("assert-reloads", 1, 1),
    ("assert-scheme-files", 1, 1),
    ("assert-tab-grid", 0, 0),
    ("assert-action-state", 2, 2),
    ("assert-action-enabled", 2, 2),
    ("assert-accels", 0, 0),
    ("assert-widget-keys", 0, 0),
    ("assert-language-hints", 0, 0),
    ("assert-recent", 1, 1),
    ("assert-palette-first", 1, 1),
    ("assert-palette-contains", 1, 1),
    ("assert-zoom", 1, 1),
    ("assert-font-size", 1, 1),
    ("assert-font-family", 1, 1),
    ("assert-font-reloads", 1, 1),
    ("assert-dialog", 1, 1),
    ("assert-menu", 0, 0),
    ("assert-menu-contains", 1, 1),
    ("assert-undo-levels", 1, 1),
    ("assert-closed", 1, 1),
    // Files, encodings and large files (M3).
    ("symlink", 2, 2),
    ("write-generated", 3, 3),
    ("set-mtime", 2, 2),
    ("select-tab", 1, 1),
    ("banner-click", 1, 1),
    ("dialog-goto", 1, 1),
    ("window-focus", 0, 0),
    ("unpace-frames", 0, 0),
    ("heartbeat-start", 0, 0),
    ("report-load", 0, 0),
    ("assert-no-banner", 1, 1),
    ("assert-dialog-text", 1, 1),
    ("assert-dialog-lacks", 1, 1),
    ("assert-highlight", 1, 1),
    ("assert-large", 1, 1),
    ("assert-editable", 1, 1),
    ("assert-max-block", 1, 1),
    ("assert-first-screen", 1, 2),
    ("assert-files-like", 3, 3),
    ("assert-tab-text", 2, 2),
    ("assert-symlink", 2, 2),
    ("unwatch", 0, 0),
    ("truncate", 2, 2),
    ("assert-toast", 1, 1),
    ("assert-wrap", 1, 1),
    ("type-burst", 3, 3),
    ("pick-encoding", 2, 2),
    ("report-memory", 1, 1),
    // Search (M4).
    ("find-mode", 1, 1),
    ("find-option", 2, 2),
    ("replace-text", 1, 1),
    ("fif-folder", 1, 1),
    ("fif-filters", 1, 1),
    ("select", 4, 4),
    ("fif-start", 0, 0),
    ("wait-fif", 0, 1),
    ("fif-stop", 0, 0),
    ("report-fif", 0, 0),
    ("results-activate", 1, 1),
    ("set-history", 2, 2),
    ("pick-history", 2, 2),
    ("generate-tree", 3, 3),
    ("wait-idle-low", 0, 1),
    ("text-checkpoint", 1, 1),
    ("bench-replace-all", 0, 1),
    ("bench-undo", 0, 0),
    ("assert-widget-parity", 0, 0),
    ("assert-find-message", 1, 1),
    ("assert-find-error", 1, 2),
    ("assert-routed", 1, 1),
    ("assert-own-matches", 1, 1),
    ("assert-own-tags", 1, 1),
    ("assert-smart-highlight", 1, 1),
    ("assert-bar-mode", 1, 1),
    ("assert-can-undo", 1, 1),
    ("assert-can-redo", 1, 1),
    ("assert-results-heading", 1, 1),
    ("assert-results-contains", 1, 1),
    ("assert-results-status", 1, 1),
    ("assert-results-shown", 1, 1),
    ("assert-results-focused", 1, 1),
    ("assert-results-searches", 1, 1),
    ("assert-fif-running", 1, 1),
    ("assert-fif", 2, 2),
    ("assert-history", 2, 2),
    ("assert-replace-stats", 2, 2),
    ("assert-text-checkpoint", 1, 1),
    ("assert-field", 2, 2),
    ("assert-replace-hint", 1, 1),
    ("assert-clipboard-contains", 1, 1),
    ("assert-search-highlight", 1, 1),
    // Keyboard focus (Wave A).
    ("open-start", 1, usize::MAX),
    ("choose-file", 1, 1),
    ("assert-chooser-name", 1, 1),
    ("click-tab", 1, 1),
    ("popup", 1, 1),
    ("popdown", 1, 1),
    ("assert-focus", 1, 1),
    // Sessions, --wait and D-Bus activation (M2).
    ("quit", 0, 2),
    ("session-commit", 0, 0),
    ("session-flush", 0, 0),
    ("wait-backed-up", 1, 1),
    ("tabs-digest", 1, 1),
    ("generate-tabs", 3, 3),
    ("drop-backup", 2, 2),
    ("spawn-bg", 2, usize::MAX),
    ("spawn-wait", 2, 2),
    ("child-start", 3, 3),
    ("child-run", 3, 3),
    ("child-signal", 2, 2),
    ("child-wait", 2, 2),
    ("test-bus-start", 1, 2),
    ("test-bus-spawn", 2, usize::MAX),
    ("bus-action", 1, 2),
    ("test-bus-stop", 0, 0),
    ("report-session", 0, 0),
    ("report-restore", 0, 0),
    ("wait-exists", 1, 1),
    ("assert-tabs-digest", 1, 1),
    ("assert-session", 1, 1),
    ("assert-stub", 2, 2),
    ("assert-backed-up", 1, 1),
    ("assert-waiting", 1, 1),
    ("assert-backups", 2, 2),
    ("assert-orphans", 2, 2),
    ("assert-session-tabs", 2, 2),
    ("assert-session-contains", 2, 2),
    ("assert-session-lacks", 2, 2),
    ("assert-exists", 1, 1),
    ("assert-restore-time", 1, 2),
    ("assert-bus-owner", 1, 1),
    ("assert-running", 2, 2),
    ("assert-file-contains", 2, 2),
    ("set-window-size", 2, 2),
    ("assert-window-size", 2, 2),
    // Column mode (M6).
    ("column-select", 4, 4),
    ("column-key", 1, 1),
    ("column-type", 1, 1),
    ("column-commit", 1, 1),
    ("column-drag", 4, 5),
    ("column-click", 2, 2),
    ("column-checkpoint", 1, 1),
    ("clipboard-text", 1, 1),
    ("column-editor", 1, 7),
    ("bench-undo-redo", 2, 2),
    ("add-source-mark", 1, 1),
    ("write-column-lines", 2, 2),
    ("assert-column", 1, 4),
    ("assert-burst", 1, 1),
    ("assert-undo-steps", 2, 2),
    ("assert-clipboard-mime", 2, 2),
    ("assert-column-op", 1, 2),
    ("assert-column-painted", 1, 1),
    ("assert-column-editor", 1, 1),
    ("assert-a11y-selection", 0, 0),
    ("assert-key-route", 2, 2),
    ("assert-controller-order", 0, 0),
    ("assert-visible-line", 1, 1),
    ("assert-hit-test", 1, 1),
    ("assert-source-marks", 1, 1),
    // Navigation, pinned tabs, first-line names and Replace in Files (M8).
    ("quick-open", 0, 1),
    ("quick-open-in", 1, 2),
    ("quick-open-type", 1, 1),
    ("quick-open-select", 1, 1),
    ("quick-open-activate", 0, 0),
    ("quick-open-close", 0, 0),
    ("wait-quick-open", 0, 1),
    ("report-quick-open", 0, 0),
    ("hold-ctrl", 0, 0),
    ("release-ctrl", 0, 0),
    ("switcher-key", 1, 1),
    ("mouse-button", 1, 1),
    ("wait-rif", 0, 0),
    ("report-rif", 0, 0),
    ("assert-quick-open-row", 2, 2),
    ("assert-quick-open-contains", 1, 1),
    ("assert-quick-open-lacks", 1, 1),
    ("assert-quick-open-count", 1, 1),
    ("assert-quick-open-positions", 2, 2),
    ("assert-quick-open-files", 1, 1),
    ("assert-quick-open-note", 1, 1),
    ("assert-quick-open-shown", 1, 1),
    ("assert-switcher", 1, 2),
    ("assert-switching", 1, 1),
    ("assert-recent-order", 1, 1),
    ("assert-places", 2, 2),
    ("assert-pinned", 2, 2),
    ("assert-tab-icon", 2, 2),
    ("assert-tab-menu-shown", 2, 2),
    ("assert-save-name", 1, 1),
    ("assert-rif", 2, 2),
];

fn arity(command: &str) -> Option<(usize, usize)> {
    COMMANDS
        .iter()
        .chain(super::tools::COMMANDS)
        .chain(super::views::COMMANDS)
        .find(|(name, _, _)| *name == command)
        .map(|&(_, min, max)| (min, max))
}

/// Parses a script. `vars` are the `$NAME` expansions.
pub fn parse(text: &str, vars: &[(&str, &str)]) -> Result<Vec<Step>, ParseError> {
    let mut steps = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let source = raw.trim();
        if source.is_empty() || source.starts_with('#') {
            continue;
        }
        let error = |message: String| ParseError { line, message };
        let mut words = tokenize(source, vars).map_err(error)?;
        let command = words.remove(0);
        check_arity(&command, words.len()).map_err(error)?;
        let first_steps = ["sandbox-home", "launch-file"];
        if command == "launch-file"
            && steps
                .iter()
                .any(|step: &Step| !first_steps.contains(&step.command.as_str()))
        {
            return Err(error(
                "launch-file must come before every other step".to_owned(),
            ));
        }
        if steps
            .last()
            .is_some_and(|step: &Step| step.command == "quit")
        {
            return Err(error("quit must be the last step".to_owned()));
        }
        if command == "sandbox-home" && !steps.is_empty() {
            return Err(error("sandbox-home must be the first step".to_owned()));
        }
        if command == "wait-until" {
            if !words[0].starts_with("assert-") {
                return Err(error(format!(
                    "wait-until needs an assertion, not {}",
                    words[0]
                )));
            }
            check_arity(&words[0], words.len() - 1).map_err(error)?;
        }
        steps.push(Step {
            line,
            source: source.to_owned(),
            command,
            args: words,
        });
    }
    Ok(steps)
}

fn check_arity(command: &str, count: usize) -> Result<(), String> {
    let (min, max) = arity(command).ok_or_else(|| format!("unknown command {command}"))?;
    if count < min || count > max {
        let expected = match (min, max) {
            (min, max) if min == max => format!("{min}"),
            (min, usize::MAX) => format!("at least {min}"),
            (min, max) => format!("{min} to {max}"),
        };
        return Err(format!("{command} takes {expected} arguments, got {count}"));
    }
    Ok(())
}

fn tokenize(line: &str, vars: &[(&str, &str)]) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        let Some(&first) = chars.peek() else {
            break;
        };
        let mut word = String::new();
        if first == '"' {
            chars.next();
            let mut closed = false;
            while let Some(c) = chars.next() {
                match c {
                    '"' => {
                        closed = true;
                        break;
                    }
                    '\\' => match chars.next() {
                        Some('n') => word.push('\n'),
                        Some('r') => word.push('\r'),
                        Some('t') => word.push('\t'),
                        Some('\\') => word.push('\\'),
                        Some('"') => word.push('"'),
                        Some('$') => word.push('\u{0}'),
                        other => {
                            return Err(format!("unknown escape \\{}", other.unwrap_or(' ')));
                        }
                    },
                    c => word.push(c),
                }
            }
            if !closed {
                return Err("unterminated quote".to_owned());
            }
            if chars.peek().is_some_and(|c| !c.is_whitespace()) {
                return Err("a quoted word must end at a space".to_owned());
            }
        } else {
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                word.push(c);
                chars.next();
            }
        }
        words.push(expand(&word, vars)?);
    }
    if words.is_empty() {
        return Err("empty step".to_owned());
    }
    Ok(words)
}

/// Expands `$NAME`; an escaped `\$` was stored as NUL and becomes a literal `$`.
fn expand(word: &str, vars: &[(&str, &str)]) -> Result<String, String> {
    let mut out = String::with_capacity(word.len());
    let mut rest = word;
    while let Some(index) = rest.find('$') {
        out.push_str(&rest[..index]);
        let after = &rest[index + 1..];
        let length = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(after.len());
        let name = &after[..length];
        let value = vars
            .iter()
            .find(|(var, _)| *var == name)
            .map(|(_, value)| *value)
            .ok_or_else(|| format!("unknown variable ${name}"))?;
        out.push_str(value);
        rest = &after[length..];
    }
    out.push_str(rest);
    Ok(out.replace('\u{0}', "$"))
}

/// `true`/`false`.
pub fn boolean(word: &str) -> Result<bool, String> {
    match word {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(format!("expected true or false, got {other}")),
    }
}

/// `63 61 66 e9` or `636166e9`.
pub fn hex_bytes(word: &str) -> Result<Vec<u8>, String> {
    let digits: String = word.chars().filter(|c| !c.is_whitespace()).collect();
    if !digits.len().is_multiple_of(2) {
        return Err(format!("odd number of hex digits in {word}"));
    }
    (0..digits.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&digits[index..index + 2], 16)
                .map_err(|_| format!("bad hex byte {}", &digits[index..index + 2]))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const VARS: &[(&str, &str)] = &[("TMP", "/tmp/run"), ("DIR", "/repo/tests")];

    #[test]
    fn parses_steps_skipping_comments() {
        let steps = parse(
            "# a comment\n\nopen $TMP/a.rs b.txt:40\n  type \"fn main() {\\n\\t\\\"x\\\"\\n}\"\n",
            VARS,
        )
        .unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].line, 3);
        assert_eq!(steps[0].command, "open");
        assert_eq!(steps[0].args, ["/tmp/run/a.rs", "b.txt:40"]);
        assert_eq!(steps[1].args, ["fn main() {\n\t\"x\"\n}"]);
        assert_eq!(steps[1].source, "type \"fn main() {\\n\\t\\\"x\\\"\\n}\"");
    }

    #[test]
    fn expands_variables_and_keeps_escaped_dollars() {
        let steps = parse("write $TMP/a \"cost: \\$5 in $DIR\"", VARS).unwrap();
        assert_eq!(steps[0].args, ["/tmp/run/a", "cost: $5 in /repo/tests"]);
    }

    #[test]
    fn rejects_mistakes_before_running() {
        for (script, message) in [
            ("frobnicate", "unknown command frobnicate"),
            ("open", "open takes at least 1 arguments, got 0"),
            ("cursor 1", "cursor takes 2 arguments, got 1"),
            ("type \"open", "unterminated quote"),
            ("type \"a\"b", "a quoted word must end at a space"),
            ("open $HOME/x", "unknown variable $HOME"),
            ("type \"\\q\"", "unknown escape \\q"),
            (
                "wait-until sleep 5",
                "wait-until needs an assertion, not sleep",
            ),
            (
                "wait-until assert-tab-count",
                "assert-tab-count takes 1 arguments, got 0",
            ),
        ] {
            let error = parse(script, VARS).unwrap_err();
            assert_eq!(error.message, message, "{script}");
            assert_eq!(error.line, 1);
        }
    }

    #[test]
    fn launch_files_come_first() {
        let steps = parse("launch-file $TMP/a x\nassert-focus editor", VARS).unwrap();
        assert_eq!(steps[0].args, ["/tmp/run/a", "x"]);
        let error = parse("assert-focus editor\nlaunch-file $TMP/a x", VARS).unwrap_err();
        assert_eq!(error.line, 2);
        assert_eq!(
            error.message,
            "launch-file must come before every other step"
        );
    }

    #[test]
    fn quit_ends_the_script() {
        let steps = parse("session-commit\nquit signal TERM", VARS).unwrap();
        assert_eq!(steps[1].args, ["signal", "TERM"]);
        let error = parse("quit\nsession-commit", VARS).unwrap_err();
        assert_eq!(error.line, 2);
        assert_eq!(error.message, "quit must be the last step");
    }

    #[test]
    fn wait_until_takes_an_assertion() {
        let steps = parse("wait-until assert-tab-count 2", VARS).unwrap();
        assert_eq!(steps[0].args, ["assert-tab-count", "2"]);
    }

    #[test]
    fn helpers() {
        assert_eq!(boolean("true"), Ok(true));
        assert!(boolean("yes").is_err());
        assert_eq!(hex_bytes("63 61 66e9"), Ok(vec![0x63, 0x61, 0x66, 0xe9]));
        assert!(hex_bytes("6").is_err());
        assert!(hex_bytes("zz").is_err());
    }

    #[test]
    fn every_script_in_the_repository_parses() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/selftest");
        let mut scripts = 0;
        // The child scripts that session tests start are in a folder of their own.
        for dir in [dir.clone(), dir.join("session")] {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.extension().is_some_and(|ext| ext == "stet-test") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    parse(&text, VARS)
                        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
                    scripts += 1;
                }
            }
        }
        assert!(scripts > 0, "no scripts in {}", dir.display());
    }
}
