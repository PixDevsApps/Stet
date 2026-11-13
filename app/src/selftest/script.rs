//! The `.stet-test` script format (docs/TESTING.md): one step per line, a command followed by
//! arguments separated by spaces. `#` starts a comment line. `"double quotes"` group words and
//! expand `\n`, `\r`, `\t`, `\\`, `\"` and `\$`. `$TMP` (the run's scratch directory) and `$DIR`
//! (the script's directory) are expanded everywhere. Unknown commands and wrong argument
//! counts, checked against the [`registry`](super::registry), fail before the app starts.

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

/// The argument counts (minimum, maximum) of the registered command `command`.
fn arity(command: &str) -> Option<(usize, usize)> {
    super::registry::find(command).map(|command| (command.min, command.max))
}

/// Parses a script. `vars` are the `$NAME` expansions. Assertions (`assert-*`) may also follow
/// `wait-until`, which polls them.
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
}
