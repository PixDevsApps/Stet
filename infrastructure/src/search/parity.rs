//! The M4 parity table: searches and replacements with their results written out by hand, from
//! spike S2's 52 pattern and 52 template probes (their "as expected" column is what
//! GtkSourceView returned) and the research digest.
//!
//! Every row runs through our own engine. Rows the translation leaves to GtkSourceView (not
//! routed) must also give the same spans through the Find in Files matcher (`grep-pcre2`, or
//! `grep-regex` for plain text) on the same text: that is the dialect parity. Rows marked
//! `buffer_only` depend on the buffer's NUL placeholder, which files on disk do not use.
//!
//! The rows are public so that the app's self-test can run the same table against the real
//! GtkSourceView `SearchContext` (`assert-widget-parity`), which needs a display.
//!
//! Deliberate differences from S2's wishes: `\g<1>` is not a group reference in a replacement,
//! `\10` is group 1 followed by `0`, and whole word does not apply in Regex mode. In a
//! replacement, `\0` is the whole match and `${name}` a named group.

use stet_domain::search::{Query, SearchMode, SearchOptions};

const USERS: &str = "user=alice id=7\nuser=Bob id=42\nsuperuser=carol id=x\n";
const WORDS: &str = "user users user_x superuser user.";
const BOOST: &str = "<user> user users";
const CASES: &str = "ERROR Error error";
const PAIR: &str = "user=alice user=Bob";
const PAIR_PATTERN: &str = r"user=(\w+)";
const TEN_GROUPS: &str = r"(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)";

/// One search with its expected result, written out by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParityRow {
    pub name: &'static str,
    pub pattern: &'static str,
    pub options: SearchOptions,
    /// The searched text, as a buffer holds it.
    pub input: &'static str,
    /// The matched texts, in order.
    pub matches: Option<Vec<&'static str>>,
    /// The matches' start offsets in characters.
    pub at: Option<Vec<usize>>,
    /// The translation routes the query to our own matcher; GtkSourceView is not asked.
    pub routed: bool,
    /// A replacement template and the whole text after Replace All.
    pub replace: Option<(&'static str, &'static str)>,
    /// The pattern does not compile; the error is at this character of the typed pattern.
    pub error_at: Option<usize>,
    /// Depends on the buffer's NUL placeholder, so Find in Files is not compared.
    pub buffer_only: bool,
}

fn row(
    name: &'static str,
    mode: SearchMode,
    pattern: &'static str,
    input: &'static str,
) -> ParityRow {
    ParityRow {
        name,
        pattern,
        options: SearchOptions::new(mode).with_match_case(true),
        input,
        matches: None,
        at: None,
        routed: false,
        replace: None,
        error_at: None,
        buffer_only: false,
    }
}

fn regex(name: &'static str, pattern: &'static str, input: &'static str) -> ParityRow {
    row(name, SearchMode::Regex, pattern, input)
}

fn normal(name: &'static str, pattern: &'static str, input: &'static str) -> ParityRow {
    row(name, SearchMode::Normal, pattern, input)
}

fn extended(name: &'static str, pattern: &'static str, input: &'static str) -> ParityRow {
    row(name, SearchMode::Extended, pattern, input)
}

impl ParityRow {
    fn finds(mut self, matches: &[&'static str]) -> Self {
        self.matches = Some(matches.to_vec());
        self
    }

    fn at(mut self, starts: &[usize]) -> Self {
        self.at = Some(starts.to_vec());
        self
    }

    fn routed(mut self) -> Self {
        self.routed = true;
        self
    }

    fn ignore_case(mut self) -> Self {
        self.options.match_case = false;
        self
    }

    fn whole_word(mut self) -> Self {
        self.options.whole_word = true;
        self
    }

    fn dot_matches_newline(mut self) -> Self {
        self.options.dot_matches_newline = true;
        self
    }

    fn replaces(mut self, template: &'static str, output: &'static str) -> Self {
        self.replace = Some((template, output));
        self
    }

    fn fails_at(mut self, offset: usize) -> Self {
        self.error_at = Some(offset);
        self
    }

    fn buffer_only(mut self) -> Self {
        self.buffer_only = true;
        self
    }

    pub fn query(&self) -> Query {
        Query::new(self.pattern, self.options)
    }
}

fn template_row(name: &'static str, template: &'static str, output: &'static str) -> ParityRow {
    regex(name, PAIR_PATTERN, PAIR).replaces(template, output)
}

/// The whole table.
pub fn rows() -> Vec<ParityRow> {
    vec![
        // Spike S2, appendix A: pattern probes.
        regex("lookbehind", r"(?<=user=)\w+", USERS).finds(&["alice", "Bob", "carol"]),
        regex("negative lookbehind", r"(?<!super)user=\w+", USERS)
            .finds(&["user=alice", "user=Bob"]),
        regex(
            "bounded variable-length lookbehind",
            r"(?<=\w{2,4}=)\d+",
            USERS,
        )
        .finds(&["7", "42"]),
        regex("lookbehind across a line break", r"(?<=7\n)user", USERS)
            .finds(&["user"])
            .at(&[16]),
        regex("lookahead", r"\w+(?==\d)", USERS).finds(&["id", "id"]),
        regex(r"\K resets the match start", r"user=\K\w+", USERS)
            .finds(&["alice", "Bob", "carol"])
            .routed(),
        regex(r"\R matches LF, CRLF and CR", r"\R", "a\nb\r\nc\rd").finds(&["\n", "\r\n", "\r"]),
        regex(r"\R inside a pattern", r"a\Rb", "a\nb").finds(&["a\nb"]),
        regex("named groups (?<n>)", r"(?<key>\w+)=(?<val>\d+)", USERS).finds(&["id=7", "id=42"]),
        regex("named groups (?P<n>)", r"(?P<key>\w+)=\d+", USERS).finds(&["id=7", "id=42"]),
        regex(r"backreference \1", r"(\w)\1", "aa bc dd").finds(&["aa", "dd"]),
        regex(r"named backreference \k<n>", r"(?<ch>\w)\k<ch>", "aa bc dd").finds(&["aa", "dd"]),
        regex(
            "named backreference (?P=n)",
            r"(?P<ch>\w)(?P=ch)",
            "aa bc dd",
        )
        .finds(&["aa", "dd"]),
        regex(
            r"\n spanning two lines",
            r"foo\nbar",
            "foo\nbar\nfoo\nbaz\n",
        )
        .finds(&["foo\nbar"]),
        regex(r"\n spanning three lines", r"a\nb\nc", "a\nb\nc\n").finds(&["a\nb\nc"]),
        normal(
            "real newline in plain text",
            "foo\nbar",
            "foo\nbar\nfoo\nbaz\n",
        )
        .finds(&["foo\nbar"]),
        regex("^ at every line start", "^bar", "foo\nbar\nbar").finds(&["bar", "bar"]),
        regex("$ at every line end", "foo$", "foo\nfoo bar\nfoo").finds(&["foo", "foo"]),
        regex("$ before CRLF", "c$", "abc\r\nabc").finds(&["c", "c"]),
        regex(". does not match a newline", "foo.bar", "foo\nbar").finds(&[]),
        regex("(?s) lets . match a newline", "(?s)foo.bar", "foo\nbar").finds(&["foo\nbar"]),
        regex(r"\s matches a newline", r"foo\sbar", "foo\nbar").finds(&["foo\nbar"]),
        regex("negated class matches a newline", "foo[^x]bar", "foo\nbar").finds(&["foo\nbar"]),
        regex(r"\A only at the start", r"\Aa", "a\na\na")
            .finds(&["a"])
            .at(&[0]),
        regex(r"\z only at the end", r"a\z", "a\na\na")
            .finds(&["a"])
            .at(&[4]),
        regex("empty match ^, one per line", "^", "a\nb\nc")
            .finds(&["", "", ""])
            .at(&[0, 2, 4])
            .routed(),
        regex("empty match $, one per line", "$", "a\nb\nc")
            .finds(&["", "", ""])
            .at(&[1, 3, 5])
            .routed(),
        regex("match case off", "error", CASES)
            .ignore_case()
            .finds(&["ERROR", "Error", "error"]),
        regex("match case on", "error", CASES).finds(&["error"]),
        regex("inline (?i)", "(?i)error", CASES).finds(&["ERROR", "Error", "error"]),
        regex("match case off, non-ASCII", "café", "CAFÉ Café café")
            .ignore_case()
            .finds(&["CAFÉ", "Café", "café"]),
        regex(r"\w is Unicode-aware", r"\w+", "café 漢字").finds(&["café", "漢字"]),
        regex(r"\b word boundary", r"\buser\b", WORDS)
            .finds(&["user", "user"])
            .at(&[0, 28]),
        normal("whole word, text", "user", WORDS)
            .whole_word()
            .finds(&["user", "user"])
            .at(&[0, 28]),
        regex("whole word does not apply to a regex", r"use\w", WORDS)
            .whole_word()
            .at(&[0, 5, 11, 23, 28]),
        regex(r"Boost \< \> word anchors", r"\<user\>", BOOST)
            .finds(&["user", "user"])
            .at(&[1, 7]),
        regex(r"\< \> spelled in PCRE2", r"\b(?=\w)user\b(?<=\w)", BOOST).at(&[1, 7]),
        regex("possessive quantifier", r"\d++1", "111").finds(&[]),
        regex("atomic group", r"(?>\d+)1", "111").finds(&[]),
        regex(r"Unicode property \p{Lu}", r"\p{Lu}+", "abcDEF ÅÄÖ").finds(&["DEF", "ÅÄÖ"]),
        regex(r"\x{hhhh} escape", r"\x{263A}", "smile ☺").finds(&["☺"]),
        regex(r"\xhh and \t escapes", r"\x41\t", "A\tB A").finds(&["A\t"]),
        regex(r"\Q...\E quoting", r"\Qa.b\E", "a.b axb").finds(&["a.b"]),
        regex(r"\h horizontal space", r"\h", "a b\tc\nd").finds(&[" ", "\t"]),
        regex(r"\N non-newline", r"a\Nb", "axb a\nb").finds(&["axb"]),
        regex("recursion (?R)", r"\((?:[^()]|(?R))*\)", "(a(b)c) (d").finds(&["(a(b)c)"]),
        regex("(?x) extended syntax", "(?x) u s e r", "user").finds(&["user"]),
        regex("invalid pattern reports its offset", "(unclosed", "x").fails_at(9),
        regex("unknown escape reports its offset", r"\i", "x").fails_at(2),
        // Spike S2, appendix B: template probes, with Stet's substitution rules.
        template_row(r"\0 is the whole match", r"<\0>", "<user=alice> <user=Bob>"),
        template_row(r"\1", r"<\1>", "<alice> <Bob>"),
        template_row(
            r"\g<1> is not a reference in a replacement",
            r"<\g<1>>",
            "<g<1>> <g<1>>",
        ),
        template_row("$1", "<$1>", "<alice> <Bob>"),
        template_row("${1}", "<${1}>", "<alice> <Bob>"),
        template_row("$0", "<$0>", "<user=alice> <user=Bob>"),
        template_row("$&", "<$&>", "<user=alice> <user=Bob>"),
        template_row(r"\U\1\E", r"<\U\1\E>", "<ALICE> <BOB>"),
        template_row(r"\U\1 without \E", r"<\U\1>", "<ALICE> <BOB>"),
        template_row(r"\U$1\E", r"<\U$1\E>", "<ALICE> <BOB>"),
        template_row(r"\L\1\E", r"<\L\1\E>", "<alice> <bob>"),
        template_row(r"\u\1", r"<\u\1>", "<Alice> <Bob>"),
        template_row(r"\l\1", r"<\l\1>", "<alice> <bob>"),
        template_row(r"\U\1\E-\1", r"<\U\1\E-\1>", "<ALICE-alice> <BOB-Bob>"),
        template_row(r"\n", r"<\n>", "<\n> <\n>"),
        template_row(r"\t", r"<\t>", "<\t> <\t>"),
        template_row(r"\\", r"<\\>", r"<\> <\>"),
        template_row(r"\x41", r"<\x41>", "<A> <A>"),
        template_row("& is literal", "<&>", "<&> <&>"),
        template_row("a missing group is empty", r"<\2>", "<> <>"),
        template_row("a trailing backslash is literal", r"<\", r"<\ <\"),
        regex("$+{name}", r"user=(?<name>\w+)", PAIR).replaces("<$+{name}>", "<alice> <Bob>"),
        regex("${name}", r"user=(?<name>\w+)", PAIR).replaces("<${name}>", "<alice> <Bob>"),
        regex(r"\10 is group 1 then 0", TEN_GROUPS, "abcdefghij").replaces(r"<\10>", "<a0>"),
        regex("${10}", TEN_GROUPS, "abcdefghij").replaces("<${10}>", "<j>"),
        regex("$10 takes both digits", TEN_GROUPS, "abcdefghij").replaces("<$10>", "<j>"),
        regex(r"\U is Unicode-aware", r"(\w+)", "café").replaces(r"\U\1", "CAFÉ"),
        regex("^ prefixes every line", "^", "a\nb\nc")
            .routed()
            .replaces("> ", "> a\n> b\n> c"),
        regex("$ suffixes every line", "$", "a\nb\nc")
            .routed()
            .replaces(";", "a;\nb;\nc;"),
        regex(r"^\R removes empty lines", r"^\R", "a\n\nb\n\n\nc").replaces("", "a\nb\nc"),
        normal("Normal-mode replacements are literal", "alice", PAIR)
            .replaces(r"\U\0$1", r"user=\U\0$1 user=Bob"),
        template_row("a real newline in a template", "<\n>", "<\n> <\n>"),
        template_row(r"\\1 is a backslash and 1", r"<\\1>", r"<\1> <\1>"),
        template_row(r"\\\\ is two backslashes", r"<\\\\>", r"<\\> <\\>"),
        template_row("unknown escapes are the character", r"<\q>", "<q> <q>"),
        template_row("a lone $ is literal", "<$>", "<$> <$>"),
        template_row(r"\\ then \1", r"<\\\1>", r"<\alice> <\Bob>"),
        template_row(r"\1 then \n", r"<\1\n>", "<alice\n> <Bob\n>"),
        template_row(r"\U on literal text", r"<\Uab\E>", "<AB> <AB>"),
        regex(r"\K in Replace All", r"user=\K\w+", PAIR)
            .routed()
            .replaces("X", "user=X user=X"),
        regex("^(.*)$ keeps empty lines", "^(.*)$", "a\n\nc")
            .routed()
            .replaces(r"> \1", "> a\n> \n> c"),
        // Usage from the research digest.
        extended(r"Extended \r\n in a CRLF document", r"\r\n", "one\ntwo\n")
            .finds(&["\n", "\n"])
            .replaces(", ", "one, two, "),
        extended(r"Extended replacement \r\n adds line breaks", ",", "a,b,c")
            .replaces(r"\r\n", "a\nb\nc"),
        extended(
            "Extended numeric escapes",
            r"\x41☺\d066\o103\b01000100",
            "A☺BCD",
        )
        .finds(&["A☺BCD"]),
        extended(
            r"Extended \0 finds the NUL placeholder",
            r"a\0",
            "a\u{2400}b",
        )
        .finds(&["a\u{2400}"])
        .buffer_only(),
        normal("Normal mode escapes metacharacters", "a.b*", "a.b* axb").finds(&["a.b*"]),
        normal("Normal $ is literal", "$", "a$b").finds(&["$"]),
        normal("whole word with punctuation", "->", "a->b a -> b a-->b")
            .whole_word()
            .at(&[1, 7]),
        normal("whole word, ignoring case", "USER", WORDS)
            .whole_word()
            .ignore_case()
            .at(&[0, 28]),
        regex(". matches newline option", "a.b", "a\nb")
            .dot_matches_newline()
            .finds(&["a\nb"]),
        regex(r"\r\n finds LF in the buffer", r"x\r\ny", "x\ny").finds(&["x\ny"]),
        regex(r"\r?\n finds LF in the buffer", r"\r?\n", "x\ny").finds(&["\n"]),
        regex("thousands separators", r"(?<=\d)(?=(\d{3})+\b)", "1234567")
            .at(&[1, 4])
            .routed()
            .replaces(",", "1,234,567"),
        regex("conditional replacement", "(a)|b", "ab").replaces("(?1A:B)", "AB"),
        regex(
            "groups in a replacement",
            r"(\w+)@(\w+)\.com",
            "joe@example.com",
        )
        .replaces("$2 at $1", "example at joe"),
        regex("parentheses group in a replacement", r"(\w+)", "hi").replaces(r"f(\1)", "fhi"),
        regex("escaped parentheses in a replacement", r"(\w+)", "hi").replaces(r"f\(\1\)", "f(hi)"),
        regex(r"Boost \u and \l classes", r"\u\l+", "Hello world Foo").finds(&["Hello", "Foo"]),
        regex(
            r"Boost \Z allows trailing line breaks",
            r"\d+\Z",
            "a 12\n\n",
        )
        .finds(&["12"]),
        regex(r"\10 in a pattern is \1 then 0", r"(a)\10", "aa0 aa").finds(&["aa0"]),
        regex(
            "a surrogate pair names one character",
            r"\x{D83D}\x{DE82}",
            "train 🚂",
        )
        .finds(&["🚂"]),
        regex("(?-i) overrides match case off", "(?-i)Error", CASES)
            .ignore_case()
            .finds(&["Error"]),
        regex(r"\C is any character", r"a\Cc", "abc a\nc").finds(&["abc"]),
        regex(r"Boost \` is the start of the text", r"\`a", "a\na").at(&[0]),
        regex("trailing blanks", r"\h+$", "a  \nb\t\nc")
            .finds(&["  ", "\t"])
            .replaces("", "a\nb\nc"),
        regex("blank lines", r"^\h*\R", "a\n\n  \nb")
            .finds(&["\n", "  \n"])
            .replaces("", "a\nb"),
        regex(
            "named groups in a replacement",
            r"(?<y>\d{4})-(?<m>\d\d)",
            "2026-10",
        )
        .replaces("$+{m}/$+{y}", "10/2026"),
        regex(r"\U$& capitalizes", r"\b\w", "hello world").replaces(r"\U$&", "Hello World"),
        regex("wrap every line", "^(.+)$", "a\nb").replaces(r#""$1","#, "\"a\",\n\"b\","),
        regex("lazy quantifier", "<.+?>", "<a><b>").finds(&["<a>", "<b>"]),
        regex(r"\x00 finds the NUL placeholder", r"a\x00", "a\u{2400}")
            .finds(&["a\u{2400}"])
            .buffer_only(),
        regex("error offsets map through the translation", r"\<(ab", "x").fails_at(5),
        extended("a lone surrogate is an error", r"x\uD83D", "x").fails_at(1),
    ]
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use stet_domain::search::Template;
    use stet_domain::text::apply;

    use super::*;
    use crate::find_in_files::file_spans;
    use crate::search::{Matcher, PatternError};

    fn chars(text: &str, start: usize, end: usize) -> String {
        text.chars().skip(start).take(end - start).collect()
    }

    fn byte_to_char(text: &str, byte: usize) -> usize {
        text[..byte].chars().count()
    }

    fn check(row: &ParityRow) -> Result<(), String> {
        let query = row.query();
        let matcher = match (Matcher::new(&query), row.error_at) {
            (Err(PatternError { offset, .. }), Some(expected)) if offset == expected => {
                return Ok(());
            }
            (Err(error), _) => return Err(format!("pattern error {error:?}")),
            (Ok(_), Some(_)) => return Err("expected a pattern error".to_owned()),
            (Ok(matcher), None) => matcher,
        };
        if matcher.needs_own_matcher() != row.routed {
            return Err(format!(
                "routed to our own matcher: {}, expected {}",
                matcher.needs_own_matcher(),
                row.routed
            ));
        }
        let cancel = AtomicBool::new(false);
        let found = matcher
            .find_all(row.input, None, usize::MAX, &cancel)
            .map_err(|error| error.to_string())?
            .matches;
        let spans: Vec<(usize, usize)> = found.iter().map(|m| (m.start, m.end)).collect();
        let texts: Vec<String> = spans
            .iter()
            .map(|&(start, end)| chars(row.input, start, end))
            .collect();
        if let Some(expected) = &row.matches
            && texts != *expected
        {
            return Err(format!("found {texts:?}, expected {expected:?}"));
        }
        if let Some(expected) = &row.at {
            let starts: Vec<usize> = spans.iter().map(|&(start, _)| start).collect();
            if starts != *expected {
                return Err(format!(
                    "found matches at {starts:?}, expected {expected:?}"
                ));
            }
        }
        if !row.routed && !row.buffer_only {
            let files: Vec<(usize, usize)> = file_spans(&query, row.input)
                .map_err(|error| format!("Find in Files matcher: {error:?}"))?
                .into_iter()
                .map(|(start, end)| (byte_to_char(row.input, start), byte_to_char(row.input, end)))
                .collect();
            if files != spans {
                return Err(format!(
                    "Find in Files found {files:?}, the document search {spans:?}"
                ));
            }
        }
        if let Some((template, output)) = row.replace {
            let template =
                Template::for_mode(query.options.mode, template).map_err(|e| e.to_string())?;
            let result = matcher
                .replace_all(row.input, &template, None, &cancel)
                .map_err(|error| error.to_string())?;
            let applied =
                apply(row.input, result.edits.clone()).map_err(|error| error.to_string())?;
            if applied != output {
                return Err(format!("replaced to {applied:?}, expected {output:?}"));
            }
            if result.apply(row.input) != output {
                return Err("the bulk edit disagrees with the edits".to_owned());
            }
        }
        Ok(())
    }

    #[test]
    fn parity_table() {
        let rows = rows();
        assert!(rows.len() >= 60, "the table has {} rows", rows.len());
        let failures: Vec<String> = rows
            .iter()
            .filter_map(|row| {
                check(row)
                    .err()
                    .map(|error| format!("{}: {error}", row.name))
            })
            .collect();
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn parity_table_summary() {
        let rows = rows();
        let routed = rows.iter().filter(|row| row.routed).count();
        let replacements = rows.iter().filter(|row| row.replace.is_some()).count();
        let checked_in_files = rows
            .iter()
            .filter(|row| !row.routed && !row.buffer_only && row.error_at.is_none())
            .count();
        println!(
            "parity table: {} rows, {routed} routed to our own matcher, {replacements} with replacements, {checked_in_files} also checked through Find in Files",
            rows.len()
        );
    }
}
