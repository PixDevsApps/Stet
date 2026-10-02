//! Replacement templates.
//!
//! In Regex mode the replacement is read in Boost's extended format syntax (`boost::format_all`),
//! where `$`, `\`, `(`, `)`, `?` and `:` are special and every other character is literal. In
//! Normal mode it is literal text, and in Extended mode literal text after [`super::unescape`].
//!
//! | Form | Inserts |
//! |---|---|
//! | `$&` `$0` `${0}` `$MATCH` `${^MATCH}` | the whole match |
//! | `\0` | the whole match. A Stet addition: Boost inserts NUL. `\0` with octal digits is a code point, as in Boost |
//! | `$1` … `$99` …, `${12}` | group n. `$` takes every following digit, so `$10` is group 10 |
//! | `\1` … `\9` | group 1–9. One digit only: `\10` is group 1, then `0` |
//! | `$+{name}` | the named group |
//! | `${name}` | the named group if the pattern has one; otherwise the text as typed. A Stet addition: Boost always inserts it as typed |
//! | `` $` `` `$PREMATCH` `${^PREMATCH}` | the text between the previous match (or the start of the searched range) and this one |
//! | `$'` `$POSTMATCH` `${^POSTMATCH}` | the text from this match to the end of the searched range |
//! | `$+` `$LAST_PAREN_MATCH` `${^LAST_PAREN_MATCH}` | the highest-numbered group |
//! | `$^N` `$LAST_SUBMATCH_RESULT` `${^LAST_SUBMATCH_RESULT}` | the group that closed last |
//! | `$$` `\$` | `$` |
//! | `\n` `\r` `\t` `\a` `\e` `\f` `\v` | LF, CR, TAB, BEL, ESC, FF, VT |
//! | `\xHH` `\x{H…}` | that code point; a UTF-16 surrogate pair (`\x{D83D}\x{DE82}`) joins |
//! | `\0OOO` | the code point with 1–3 octal digits |
//! | `\cX` | the control character X mod 32 |
//! | `\U` `\L` … `\E` | upper or lower case until `\E` |
//! | `\u` `\l` | upper or lower case for the next character only |
//! | `(` … `)` | grouping; the parentheses are not inserted |
//! | `?Nyes:no` `?{N}yes:no` `?{name}yes:no` | `yes` if group N (up to two digits without braces) took part in the match, else `no`. An unknown name takes `no` |
//! | `\` and any other character | that character: `\\` `\(` `\)` `\?` `\:` `\q` |
//! | a trailing `\` or `$`, or `$` before anything else | itself |
//!
//! As in Boost, a `)` that closes nothing ends the template and the rest is ignored
//! ([`TemplateWarning::Truncated`]). Case conversion maps each character to one character,
//! like Boost and GtkSourceView: `ß` stays `ß` under `\U`.
//!
//! The expansion is buffer text: CRLF and CR become LF and NUL becomes U+2400 (ADR-007,
//! ADR-008). So `\r\n` inserts a line break, which is saved with the document's line ending.

use super::extended::{EscapeError, unescape};
use super::{SearchMode, to_buffer_text};

/// The groups of one match, as a template sees them.
pub trait Captures {
    /// The text of group `index`, 0 being the whole match. `None` when the group did not take
    /// part in the match or does not exist.
    fn get(&self, index: usize) -> Option<&str>;

    /// The number of capturing groups in the pattern, not counting group 0.
    fn group_count(&self) -> usize;

    /// The number of the group called `name`. With duplicate names, the first one that took
    /// part in the match.
    fn name_to_index(&self, name: &str) -> Option<usize>;

    /// The text between the previous match (or the start of the searched range) and this one.
    fn prefix(&self) -> &str {
        ""
    }

    /// The text from the end of this match to the end of the searched range.
    fn suffix(&self) -> &str {
        ""
    }

    /// The group that closed last (Perl's `$^N`).
    fn last_closed(&self) -> Option<usize> {
        None
    }
}

/// Something about a template that is easy to get wrong, for the UI to point out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TemplateWarning {
    /// Parentheses group text and are not inserted; `\(` and `\)` insert them.
    Grouping,
    /// `?` followed by a group number or `{…}` is a conditional; `\?` inserts a `?`.
    Conditional,
    /// A `)` at character `offset` closes nothing: the template ends there.
    Truncated { offset: usize },
}

/// A parsed replacement.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Template {
    nodes: Vec<Node>,
    warnings: Vec<TemplateWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    Text(String),
    Group(usize),
    Named(String),
    /// `${name}`: the named group, or the text as typed when there is no such group.
    NamedOrText(String),
    Prefix,
    Suffix,
    LastParen,
    LastClosed,
    Case(CaseOp),
    Conditional {
        group: GroupRef,
        yes: Vec<Node>,
        no: Vec<Node>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GroupRef {
    Number(usize),
    Name(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaseOp {
    NextLower,
    NextUpper,
    Lower,
    Upper,
    End,
}

impl Template {
    /// A Regex-mode replacement. Parsing never fails: whatever is not special is literal.
    pub fn parse(text: &str) -> Self {
        let mut parser = Parser {
            chars: text.chars().collect(),
            pos: 0,
            warnings: Vec::new(),
        };
        let (nodes, _) = parser.sequence(false, false);
        Self {
            nodes,
            warnings: parser.warnings,
        }
    }

    /// A Normal-mode replacement: the text as typed.
    pub fn literal(text: &str) -> Self {
        let nodes = if text.is_empty() {
            Vec::new()
        } else {
            vec![Node::Text(text.to_owned())]
        };
        Self {
            nodes,
            warnings: Vec::new(),
        }
    }

    /// An Extended-mode replacement: the text after [`super::unescape`].
    pub fn extended(text: &str) -> Result<Self, EscapeError> {
        Ok(Self::literal(&unescape(text)?))
    }

    /// The replacement field read in `mode`.
    pub fn for_mode(mode: SearchMode, text: &str) -> Result<Self, EscapeError> {
        match mode {
            SearchMode::Normal => Ok(Self::literal(text)),
            SearchMode::Extended => Self::extended(text),
            SearchMode::Regex => Ok(Self::parse(text)),
        }
    }

    pub fn warnings(&self) -> &[TemplateWarning] {
        &self.warnings
    }

    /// The expansion when it does not depend on the match, as buffer text.
    pub fn as_literal(&self) -> Option<String> {
        match self.nodes.as_slice() {
            [] => Some(String::new()),
            [Node::Text(text)] => Some(to_buffer_text(text).into_owned()),
            _ => None,
        }
    }

    pub fn expand(&self, captures: &dyn Captures) -> String {
        let mut out = String::new();
        self.expand_into(captures, &mut out);
        out
    }

    /// Appends the expansion for `captures` to `out`.
    pub fn expand_into(&self, captures: &dyn Captures, out: &mut String) {
        let start = out.len();
        let mut writer = Writer {
            out,
            mode: Mode::Copy,
            restore: Mode::Copy,
        };
        writer.nodes(&self.nodes, captures, true);
        if memchr::memchr2(b'\r', 0, &out.as_bytes()[start..]).is_some() {
            let normalized = to_buffer_text(&out[start..]).into_owned();
            out.truncate(start);
            out.push_str(&normalized);
        }
    }
}

/// Expands `template` for one match.
pub fn expand(template: &Template, captures: &dyn Captures) -> String {
    template.expand(captures)
}

enum Stop {
    End,
    Close,
    Colon,
    Truncated,
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    warnings: Vec<TemplateWarning>,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        Some(c)
    }

    fn starts_with(&self, word: &str) -> bool {
        let mut chars = self.chars[self.pos..].iter();
        word.chars().all(|c| chars.next() == Some(&c))
    }

    fn find(&self, target: char) -> Option<usize> {
        self.chars[self.pos..]
            .iter()
            .position(|&c| c == target)
            .map(|i| self.pos + i)
    }

    fn warn(&mut self, warning: TemplateWarning) {
        if !self.warnings.contains(&warning) {
            self.warnings.push(warning);
        }
    }

    /// Items up to the end, a `)` (which closes a group, or ends a top-level template), or a
    /// `:` that ends a conditional's first branch.
    fn sequence(&mut self, in_group: bool, in_yes: bool) -> (Vec<Node>, Stop) {
        let mut nodes = Vec::new();
        loop {
            let Some(c) = self.peek() else {
                return (nodes, Stop::End);
            };
            match c {
                ')' if in_group => return (nodes, Stop::Close),
                ')' => {
                    self.warn(TemplateWarning::Truncated { offset: self.pos });
                    return (nodes, Stop::Truncated);
                }
                ':' if in_yes => return (nodes, Stop::Colon),
                '(' => {
                    self.bump();
                    self.warn(TemplateWarning::Grouping);
                    let (inner, stop) = self.sequence(true, false);
                    nodes.extend(inner);
                    if matches!(stop, Stop::Close) {
                        self.bump();
                    }
                }
                '?' => {
                    self.bump();
                    let start = self.pos;
                    let Some(group) = self.condition() else {
                        self.pos = start;
                        push_text(&mut nodes, '?');
                        continue;
                    };
                    self.warn(TemplateWarning::Conditional);
                    let (yes, stop) = self.sequence(in_group, true);
                    let (no, stop) = if matches!(stop, Stop::Colon) {
                        self.bump();
                        self.sequence(in_group, false)
                    } else {
                        (Vec::new(), stop)
                    };
                    nodes.push(Node::Conditional { group, yes, no });
                    if matches!(stop, Stop::Truncated) {
                        return (nodes, Stop::Truncated);
                    }
                }
                '$' => self.dollar(&mut nodes),
                '\\' => self.escape(&mut nodes),
                c => {
                    self.bump();
                    push_text(&mut nodes, c);
                }
            }
        }
    }

    /// After `?`: a group number (up to two digits), `{number}` or `{name}`.
    fn condition(&mut self) -> Option<GroupRef> {
        match self.peek()? {
            '{' => {
                let close = self.find('}')?;
                let inner: String = self.chars[self.pos + 1..close].iter().collect();
                let group = if inner.starts_with(|c: char| c.is_ascii_digit()) {
                    GroupRef::Number(inner.parse().ok()?)
                } else if inner.is_empty() {
                    return None;
                } else {
                    GroupRef::Name(inner)
                };
                self.pos = close + 1;
                Some(group)
            }
            c if c.is_ascii_digit() => {
                let mut value = 0;
                for _ in 0..2 {
                    match self.peek().and_then(|c| c.to_digit(10)) {
                        Some(digit) => {
                            value = value * 10 + digit as usize;
                            self.pos += 1;
                        }
                        None => break,
                    }
                }
                Some(GroupRef::Number(value))
            }
            _ => None,
        }
    }

    fn digits(&mut self) -> Option<usize> {
        let start = self.pos;
        let mut value: usize = 0;
        while let Some(digit) = self.peek().and_then(|c| c.to_digit(10)) {
            value = value.saturating_mul(10).saturating_add(digit as usize);
            self.pos += 1;
        }
        (self.pos > start).then_some(value)
    }

    fn dollar(&mut self, nodes: &mut Vec<Node>) {
        self.bump();
        let Some(c) = self.peek() else {
            push_text(nodes, '$');
            return;
        };
        let node = match c {
            '&' => Node::Group(0),
            '`' => Node::Prefix,
            '\'' => Node::Suffix,
            '$' => {
                self.bump();
                push_text(nodes, '$');
                return;
            }
            '+' => {
                self.bump();
                if self.peek() == Some('{')
                    && let Some(close) = self.find('}')
                {
                    let name = self.chars[self.pos + 1..close].iter().collect();
                    self.pos = close + 1;
                    nodes.push(Node::Named(name));
                    return;
                }
                nodes.push(Node::LastParen);
                return;
            }
            '{' => {
                let open = self.pos;
                self.bump();
                if let Some(group) = self.digits() {
                    if self.peek() == Some('}') {
                        self.bump();
                        nodes.push(Node::Group(group));
                    } else {
                        self.pos = open;
                        push_text(nodes, '$');
                    }
                    return;
                }
                if let Some(node) = self.verb(true) {
                    nodes.push(node);
                    return;
                }
                if let Some(close) = self.find('}') {
                    let name: String = self.chars[self.pos..close].iter().collect();
                    if is_group_name(&name) {
                        self.pos = close + 1;
                        nodes.push(Node::NamedOrText(name));
                        return;
                    }
                }
                self.pos = open;
                push_text(nodes, '$');
                return;
            }
            c if c.is_ascii_digit() => {
                let group = self.digits().unwrap_or(0);
                nodes.push(Node::Group(group));
                return;
            }
            _ => {
                match self.verb(false) {
                    Some(node) => nodes.push(node),
                    None => push_text(nodes, '$'),
                }
                return;
            }
        };
        self.bump();
        nodes.push(node);
    }

    /// Perl's named placeholders, after `$` or `${`.
    fn verb(&mut self, braced: bool) -> Option<Node> {
        let save = self.pos;
        if braced && self.peek() == Some('^') {
            self.pos += 1;
        }
        let verbs = [
            ("MATCH", Node::Group(0)),
            ("PREMATCH", Node::Prefix),
            ("POSTMATCH", Node::Suffix),
            ("LAST_PAREN_MATCH", Node::LastParen),
            ("LAST_SUBMATCH_RESULT", Node::LastClosed),
            ("^N", Node::LastClosed),
        ];
        for (word, node) in verbs {
            if !self.starts_with(word) {
                continue;
            }
            self.pos += word.len();
            if braced {
                if self.peek() != Some('}') {
                    self.pos = save;
                    return None;
                }
                self.pos += 1;
            }
            return Some(node);
        }
        self.pos = save;
        None
    }

    fn escape(&mut self, nodes: &mut Vec<Node>) {
        self.bump();
        let Some(c) = self.bump() else {
            push_text(nodes, '\\');
            return;
        };
        let text = match c {
            'a' => '\u{07}',
            'e' => '\u{1B}',
            'f' => '\u{0C}',
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'v' => '\u{0B}',
            'x' => {
                self.hex(nodes);
                return;
            }
            'c' => match self.bump() {
                Some(control) => char::from_u32(u32::from(control) % 32).unwrap_or('\0'),
                None => 'c',
            },
            'l' | 'u' | 'L' | 'U' | 'E' => {
                let op = match c {
                    'l' => CaseOp::NextLower,
                    'u' => CaseOp::NextUpper,
                    'L' => CaseOp::Lower,
                    'U' => CaseOp::Upper,
                    _ => CaseOp::End,
                };
                nodes.push(Node::Case(op));
                return;
            }
            '1'..='9' => {
                nodes.push(Node::Group(c as usize - '0' as usize));
                return;
            }
            '0' => {
                let mut value = 0;
                let mut digits = 0;
                while digits < 3 {
                    match self.peek().and_then(|c| c.to_digit(8)) {
                        Some(digit) => {
                            value = value * 8 + digit;
                            digits += 1;
                            self.pos += 1;
                        }
                        None => break,
                    }
                }
                if digits == 0 {
                    nodes.push(Node::Group(0));
                    return;
                }
                char::from_u32(value).unwrap_or(char::REPLACEMENT_CHARACTER)
            }
            c => c,
        };
        push_text(nodes, text);
    }

    /// After `\x`: `HH` (one or two digits) or `{H…}`; otherwise `x` is literal.
    fn hex(&mut self, nodes: &mut Vec<Node>) {
        let after_x = self.pos;
        let value = if self.peek() == Some('{') {
            self.pos += 1;
            let digits_start = self.pos;
            while self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                self.pos += 1;
            }
            if self.pos == digits_start {
                push_text(nodes, 'x');
                push_text(nodes, '{');
                return;
            }
            if self.peek() != Some('}') {
                self.pos = after_x;
                push_text(nodes, 'x');
                return;
            }
            let digits: String = self.chars[digits_start..self.pos].iter().collect();
            self.pos += 1;
            u32::from_str_radix(&digits, 16).unwrap_or(u32::MAX)
        } else {
            let mut value = 0;
            let mut digits = 0;
            while digits < 2 {
                match self.peek().and_then(|c| c.to_digit(16)) {
                    Some(digit) => {
                        value = value * 16 + digit;
                        digits += 1;
                        self.pos += 1;
                    }
                    None => break,
                }
            }
            if digits == 0 {
                push_text(nodes, 'x');
                return;
            }
            value
        };
        let character = if (0xD800..0xDC00).contains(&value) && self.starts_with(r"\x{") {
            let save = self.pos;
            self.pos += 3;
            let digits_start = self.pos;
            while self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                self.pos += 1;
            }
            let digits: String = self.chars[digits_start..self.pos].iter().collect();
            let low = u32::from_str_radix(&digits, 16)
                .ok()
                .filter(|low| (0xDC00..0xE000).contains(low) && self.peek() == Some('}'));
            match low {
                Some(low) => {
                    self.pos += 1;
                    char::from_u32(0x10000 + ((value - 0xD800) << 10) + (low - 0xDC00))
                }
                None => {
                    self.pos = save;
                    None
                }
            }
        } else {
            char::from_u32(value)
        };
        push_text(nodes, character.unwrap_or(char::REPLACEMENT_CHARACTER));
    }
}

fn push_text(nodes: &mut Vec<Node>, c: char) {
    if let Some(Node::Text(text)) = nodes.last_mut() {
        text.push(c);
    } else {
        nodes.push(Node::Text(c.to_string()));
    }
}

fn is_group_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Copy,
    NextLower,
    NextUpper,
    Lower,
    Upper,
}

/// Boost's case-conversion state machine: `\l` and `\u` apply to one character and then
/// restore the previous mode; `\L`, `\U` and `\E` switch the mode.
struct Writer<'a> {
    out: &'a mut String,
    mode: Mode,
    restore: Mode,
}

impl Writer<'_> {
    fn nodes(&mut self, nodes: &[Node], captures: &dyn Captures, active: bool) {
        for node in nodes {
            match node {
                Node::Conditional { group, yes, no } => {
                    let matched = match group {
                        GroupRef::Number(index) => captures.get(*index).is_some(),
                        GroupRef::Name(name) => captures
                            .name_to_index(name)
                            .and_then(|index| captures.get(index))
                            .is_some(),
                    };
                    self.nodes(yes, captures, active && matched);
                    self.nodes(no, captures, active && !matched);
                }
                _ if !active => {}
                Node::Text(text) => self.put_str(text),
                Node::Group(index) => self.put_group(captures, Some(*index)),
                Node::Named(name) => self.put_group(captures, captures.name_to_index(name)),
                Node::NamedOrText(name) => match captures.name_to_index(name) {
                    Some(index) => self.put_group(captures, Some(index)),
                    None => {
                        self.put_str("${");
                        self.put_str(name);
                        self.put_str("}");
                    }
                },
                Node::Prefix => self.put_str(captures.prefix()),
                Node::Suffix => self.put_str(captures.suffix()),
                Node::LastParen => {
                    self.put_group(captures, Some(captures.group_count().max(1)));
                }
                Node::LastClosed => self.put_group(captures, captures.last_closed()),
                Node::Case(op) => match op {
                    CaseOp::NextLower | CaseOp::NextUpper => {
                        self.restore = self.mode;
                        self.mode = if *op == CaseOp::NextLower {
                            Mode::NextLower
                        } else {
                            Mode::NextUpper
                        };
                    }
                    CaseOp::Lower => self.mode = Mode::Lower,
                    CaseOp::Upper => self.mode = Mode::Upper,
                    CaseOp::End => self.mode = Mode::Copy,
                },
            }
        }
    }

    fn put_group(&mut self, captures: &dyn Captures, index: Option<usize>) {
        if let Some(text) = index.and_then(|index| captures.get(index)) {
            self.put_str(text);
        }
    }

    fn put_str(&mut self, text: &str) {
        if self.mode == Mode::Copy {
            self.out.push_str(text);
            return;
        }
        for c in text.chars() {
            let c = match self.mode {
                Mode::Copy => c,
                Mode::Lower => lower(c),
                Mode::Upper => upper(c),
                Mode::NextLower => {
                    self.mode = self.restore;
                    lower(c)
                }
                Mode::NextUpper => {
                    self.mode = self.restore;
                    upper(c)
                }
            };
            self.out.push(c);
        }
    }
}

fn upper(c: char) -> char {
    let mut mapped = c.to_uppercase();
    match (mapped.next(), mapped.next()) {
        (Some(single), None) => single,
        _ => c,
    }
}

fn lower(c: char) -> char {
    let mut mapped = c.to_lowercase();
    match (mapped.next(), mapped.next()) {
        (Some(single), None) => single,
        _ => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Groups as text; `None` for a group that did not take part.
    struct Groups<'a> {
        groups: Vec<Option<&'a str>>,
        names: Vec<(&'a str, usize)>,
    }

    impl Captures for Groups<'_> {
        fn get(&self, index: usize) -> Option<&str> {
            self.groups.get(index).copied().flatten()
        }

        fn group_count(&self) -> usize {
            self.groups.len() - 1
        }

        fn name_to_index(&self, name: &str) -> Option<usize> {
            self.names
                .iter()
                .find(|(candidate, _)| *candidate == name)
                .map(|&(_, index)| index)
        }

        fn prefix(&self) -> &str {
            "<pre>"
        }

        fn suffix(&self) -> &str {
            "<post>"
        }

        fn last_closed(&self) -> Option<usize> {
            Some(1)
        }
    }

    fn user() -> Groups<'static> {
        Groups {
            groups: vec![Some("user=alice"), Some("alice"), None],
            names: vec![("name", 1), ("missing", 2)],
        }
    }

    fn expanded(template: &str) -> String {
        Template::parse(template).expand(&user())
    }

    #[test]
    fn placeholders() {
        assert_eq!(
            expanded("<$&|$0|${0}|$MATCH|${^MATCH}|\\0>"),
            "<user=alice|user=alice|user=alice|user=alice|user=alice|user=alice>"
        );
        assert_eq!(
            expanded("$1-${1}-\\1-$+{name}-${name}"),
            "alice-alice-alice-alice-alice"
        );
        assert_eq!(
            expanded("[$2][\\2][$9][${nosuch}][$+{nosuch}]"),
            "[][][][${nosuch}][]"
        );
        assert_eq!(
            expanded("$`|$'|$PREMATCH|${^POSTMATCH}"),
            "<pre>|<post>|<pre>|<post>"
        );
        assert_eq!(expanded("$+|$^N|$LAST_SUBMATCH_RESULT"), "|alice|alice");
        assert_eq!(expanded("$$ \\$ $ $x $"), "$ $ $ $x $");
        assert_eq!(expanded("${1"), "${1");
        assert_eq!(expanded("$10"), "");
        assert_eq!(expanded("\\10"), "alice0");
    }

    #[test]
    fn escapes() {
        assert_eq!(expanded(r"\n\t\\\q\(\)\?\:\b"), "\n\t\\q()?:b");
        assert_eq!(
            expanded(r"\x41\x{263A}\x{D83D}\x{DE82}\0101\cA"),
            "A☺🚂A\u{1}"
        );
        assert_eq!(expanded(r"\xZ\x{zz}\x{41"), "xZx{zz}x{41");
        assert_eq!(expanded("end\\"), "end\\");
        assert_eq!(expanded(r"a\r\nb\rc\x00"), "a\nb\nc\u{2400}");
    }

    #[test]
    fn case_conversion() {
        assert_eq!(expanded(r"\U$1\E-$1"), "ALICE-alice");
        assert_eq!(expanded(r"\u$1 \U$1"), "Alice ALICE");
        assert_eq!(expanded(r"\L\uhELLO \Uwo\lRLD"), "Hello WOrLD");
        assert_eq!(expanded(r"\u\LfOO"), "foo");
        assert_eq!(expanded(r"\Uabc\E"), "ABC");
        let cafe = Groups {
            groups: vec![Some("café straße"), Some("café straße")],
            names: vec![],
        };
        assert_eq!(Template::parse(r"\U\1").expand(&cafe), "CAFÉ STRAßE");
    }

    #[test]
    fn grouping_and_conditionals() {
        assert_eq!(expanded("f(x)"), "fx");
        assert_eq!(expanded(r"f\(x\)"), "f(x)");
        assert_eq!(expanded("a)b"), "a");
        assert_eq!(expanded("(?1yes:no)|(?2yes:no)"), "yes|no");
        assert_eq!(expanded("?1yes:no:more"), "yes");
        assert_eq!(expanded("?2yes:no:more"), "no:more");
        assert_eq!(
            expanded("(?{name}[$1]:none)(?{missing}x:y)(?{nosuch}x:y)"),
            "[alice]yy"
        );
        assert_eq!(expanded("?x a?b"), "?x a?b");
        assert_eq!(expanded("(a:b)"), "a:b");
        let template = Template::parse("(?1a:b)x)y");
        assert_eq!(template.expand(&user()), "ax");
        assert_eq!(
            template.warnings(),
            [
                TemplateWarning::Grouping,
                TemplateWarning::Conditional,
                TemplateWarning::Truncated { offset: 8 }
            ]
        );
    }

    #[test]
    fn modes() {
        let normal = Template::for_mode(SearchMode::Normal, r"\U\0$1").unwrap();
        assert_eq!(normal.expand(&user()), r"\U\0$1");
        let extended = Template::for_mode(SearchMode::Extended, r",\r\n\x41").unwrap();
        assert_eq!(extended.as_literal().as_deref(), Some(",\nA"));
        assert!(Template::parse(r"\1").as_literal().is_none());
        assert_eq!(Template::parse("").as_literal().as_deref(), Some(""));
    }

    proptest! {
        #[test]
        fn plain_text_is_literal(text in "[^\\\\$()?:]{0,24}") {
            let template = Template::parse(&text);
            prop_assert_eq!(template.expand(&user()), to_buffer_text(&text).into_owned());
        }

        #[test]
        fn escaping_specials_makes_them_literal(text in "[a-z$()?:\\\\]{0,24}") {
            let escaped: String = text
                .chars()
                .flat_map(|c| if "$()?:\\".contains(c) { vec!['\\', c] } else { vec![c] })
                .collect();
            prop_assert_eq!(Template::parse(&escaped).expand(&user()), text);
        }
    }
}
