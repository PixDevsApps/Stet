//! A scanner for Boost-style regex patterns. It rewrites the Boost-only syntax into PCRE2 and
//! works out what the translation needs to know: whether the pattern can match empty text,
//! whether it uses `\K` or `\G`, and whether a match can contain a line break.
//!
//! It is not a validator. Whatever it does not recognize is copied unchanged, so PCRE2 reports
//! the error with an offset that [`SourceMap`] maps back to the typed pattern.

use super::translate::{Note, Target};

pub(crate) struct Rewrite {
    pub(crate) pattern: String,
    pub(crate) map: SourceMap,
    /// Some match can be empty. Errs on the side of `true`.
    pub(crate) nullable: bool,
    /// Uses `\K`.
    pub(crate) keep_out: bool,
    /// Uses `\G`.
    pub(crate) continuation: bool,
    /// A match can contain a line break, or the pattern uses whole-text anchors, so it must
    /// run over whole texts rather than line by line. Errs on the side of `true`.
    pub(crate) whole_text: bool,
    pub(crate) notes: Vec<Note>,
}

/// Maps byte offsets in a translated pattern back to character offsets in the typed one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct SourceMap(Vec<Anchor>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Anchor {
    out: usize,
    src: usize,
    verbatim: bool,
}

impl SourceMap {
    /// The pattern is the typed text.
    pub(crate) fn identity() -> Self {
        Self(vec![Anchor {
            out: 0,
            src: 0,
            verbatim: true,
        }])
    }

    /// Every offset maps to the start of the typed text.
    pub(crate) fn start() -> Self {
        Self(vec![Anchor {
            out: 0,
            src: 0,
            verbatim: false,
        }])
    }

    fn push(&mut self, out: usize, src: usize, verbatim: bool) {
        if let Some(last) = self.0.last_mut()
            && last.out == out
        {
            *last = Anchor { out, src, verbatim };
            return;
        }
        self.0.push(Anchor { out, src, verbatim });
    }

    /// The character offset in the typed pattern for byte `offset` of `pattern`.
    pub(crate) fn source_offset(&self, pattern: &str, offset: usize) -> usize {
        let index = self.0.partition_point(|anchor| anchor.out <= offset);
        let Some(anchor) = index.checked_sub(1).map(|i| self.0[i]) else {
            return 0;
        };
        if !anchor.verbatim {
            return anchor.src;
        }
        let end = offset.min(pattern.len());
        anchor.src
            + pattern
                .get(anchor.out..end)
                .map_or(0, |text| text.chars().count())
    }
}

pub(crate) fn rewrite(pattern: &str, target: Target, dot_matches_newline: bool) -> Rewrite {
    let mut scanner = Scanner {
        src: pattern,
        pos: 0,
        cpos: 0,
        out: String::with_capacity(pattern.len() + 8),
        map: SourceMap::default(),
        target,
        extended: false,
        dotall: false,
        keep_out: false,
        continuation: false,
        whole_text: false,
        accept: false,
        notes: Vec::new(),
    };
    scanner.start_items();
    if dot_matches_newline {
        scanner.anchor(false);
        scanner.out.push_str("(?s)");
        scanner.dotall = true;
    }
    let mut nullable = scanner.alternation().0;
    while scanner.peek() == Some(')') {
        scanner.copy_chars(1);
        nullable |= scanner.alternation().0;
    }
    Rewrite {
        pattern: scanner.out,
        map: scanner.map,
        nullable: nullable || scanner.accept,
        keep_out: scanner.keep_out,
        continuation: scanner.continuation,
        whole_text: scanner.whole_text,
        notes: scanner.notes,
    }
}

/// A parsed item of a sequence: something that matches (and may match empty text), or
/// something that does not take part in matching, such as a comment or an option setting.
enum Atom {
    Item { nullable: bool },
    Nothing,
}

const ITEM: Atom = Atom::Item { nullable: false };
const NULLABLE: Atom = Atom::Item { nullable: true };

struct Scanner<'a> {
    src: &'a str,
    /// Byte offset in `src`.
    pos: usize,
    /// Character offset in `src`.
    cpos: usize,
    out: String,
    map: SourceMap,
    target: Target,
    /// `(?x)` is in effect: whitespace and `#` comments are ignored.
    extended: bool,
    /// `(?s)` is in effect.
    dotall: bool,
    keep_out: bool,
    continuation: bool,
    whole_text: bool,
    accept: bool,
    notes: Vec<Note>,
}

impl Scanner<'_> {
    fn rest(&self) -> &str {
        &self.src[self.pos..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn peek_nth(&self, n: usize) -> Option<char> {
        self.rest().chars().nth(n)
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        self.cpos += 1;
        Some(c)
    }

    fn bump_chars(&mut self, n: usize) {
        for _ in 0..n {
            self.bump();
        }
    }

    fn bump_bytes(&mut self, len: usize) {
        let end = (self.pos + len).min(self.src.len());
        self.cpos += self.src[self.pos..end].chars().count();
        self.pos = end;
    }

    fn anchor(&mut self, verbatim: bool) {
        self.map.push(self.out.len(), self.cpos, verbatim);
    }

    fn copy_chars(&mut self, n: usize) {
        let start = self.pos;
        self.anchor(true);
        self.bump_chars(n);
        self.out.push_str(&self.src[start..self.pos]);
    }

    fn copy_bytes(&mut self, len: usize) {
        let start = self.pos;
        self.anchor(true);
        self.bump_bytes(len);
        self.out.push_str(&self.src[start..self.pos]);
    }

    /// Replaces the next `n` characters with `replacement`.
    fn replace_chars(&mut self, n: usize, replacement: &str) {
        self.anchor(false);
        self.bump_chars(n);
        self.out.push_str(replacement);
    }

    fn note(&mut self, note: Note) {
        if !self.notes.contains(&note) {
            self.notes.push(note);
        }
    }

    /// Start-of-pattern items such as `(*UCP)` or `(*LIMIT_MATCH=1000)`; they must stay first.
    fn start_items(&mut self) {
        while let Some(len) = start_item_len(self.rest()) {
            self.copy_bytes(len);
        }
    }

    /// Branches separated by `|`, up to a `)` or the end: whether one can match empty text and
    /// how many there are.
    fn alternation(&mut self) -> (bool, usize) {
        let mut nullable = self.sequence();
        let mut branches = 1;
        while self.peek() == Some('|') {
            self.copy_chars(1);
            nullable |= self.sequence();
            branches += 1;
        }
        (nullable, branches)
    }

    fn sequence(&mut self) -> bool {
        let mut nullable = true;
        loop {
            self.skip_extended();
            if matches!(self.peek(), None | Some('|' | ')')) {
                return nullable;
            }
            let atom = self.atom();
            self.skip_extended();
            let min = self.quantifier();
            if let Atom::Item { nullable: item } = atom {
                nullable &= item || min == Some(0);
            }
        }
    }

    /// Copies whitespace and `#` comments while `(?x)` is in effect.
    fn skip_extended(&mut self) {
        while self.extended {
            match self.peek() {
                Some(c) if is_pattern_space(c) => self.copy_chars(1),
                Some('#') => {
                    let len = self.rest().find(is_newline).unwrap_or(self.rest().len());
                    self.copy_bytes(len);
                }
                _ => return,
            }
        }
    }

    fn quantifier(&mut self) -> Option<u32> {
        let (len, min) = quantifier_at(self.rest())?;
        self.copy_bytes(len);
        Some(min)
    }

    fn atom(&mut self) -> Atom {
        match self.peek() {
            Some('\\') => self.escape(),
            Some('[') => {
                self.class();
                ITEM
            }
            Some('(') => self.group(),
            Some('.') => {
                self.whole_text |= self.dotall;
                self.copy_chars(1);
                ITEM
            }
            Some('^' | '$') => {
                self.copy_chars(1);
                NULLABLE
            }
            Some('\n') => {
                self.anchor(false);
                self.bump();
                self.line_break();
                ITEM
            }
            Some('\r') => {
                self.anchor(false);
                self.bump();
                self.carriage_return()
            }
            Some('\0') => {
                self.anchor(false);
                self.bump();
                self.nul();
                ITEM
            }
            _ => {
                self.copy_chars(1);
                ITEM
            }
        }
    }

    fn escape(&mut self) -> Atom {
        let Some(c) = self.peek_nth(1) else {
            self.copy_chars(1);
            return ITEM;
        };
        match c {
            '<' => {
                self.note(Note::BoostWordStart);
                self.replace_chars(2, r"\b(?=\w)");
                NULLABLE
            }
            '>' => {
                self.note(Note::BoostWordEnd);
                self.replace_chars(2, r"\b(?<=\w)");
                NULLABLE
            }
            '`' | '\'' => {
                self.note(Note::BoostTextAnchor);
                self.whole_text = true;
                self.replace_chars(2, if c == '`' { r"\A" } else { r"\z" });
                NULLABLE
            }
            'Z' => {
                self.note(Note::BoostEndOfText);
                self.whole_text = true;
                self.replace_chars(2, r"(?=\v*\z)");
                NULLABLE
            }
            'A' | 'z' => {
                self.whole_text = true;
                self.copy_chars(2);
                NULLABLE
            }
            'G' => {
                self.continuation = true;
                self.whole_text = true;
                self.copy_chars(2);
                NULLABLE
            }
            'K' => {
                self.keep_out = true;
                self.copy_chars(2);
                NULLABLE
            }
            'b' | 'B' => {
                self.copy_chars(2);
                NULLABLE
            }
            'E' => {
                self.copy_chars(2);
                Atom::Nothing
            }
            'Q' => self.quoted(),
            'C' => {
                self.note(Note::AnyCharacter);
                self.whole_text |= self.dotall;
                self.replace_chars(2, ".");
                ITEM
            }
            'l' | 'u' | 'L' | 'U' => {
                self.note(Note::BoostCaseClass);
                self.whole_text |= c.is_uppercase();
                let class = match c {
                    'l' => "[[:lower:]]",
                    'u' => "[[:upper:]]",
                    'L' => "[^[:lower:]]",
                    _ => "[^[:upper:]]",
                };
                self.replace_chars(2, class);
                ITEM
            }
            'n' => {
                self.anchor(false);
                self.bump_chars(2);
                self.line_break();
                ITEM
            }
            'r' => {
                self.anchor(false);
                self.bump_chars(2);
                self.carriage_return()
            }
            'x' | 'o' | '0' | 'c' | 'N' => self.code_point(),
            '1'..='9' => {
                if self.peek_nth(2).is_some_and(|next| next.is_ascii_digit()) {
                    self.note(Note::SingleDigitBackreference);
                    self.replace_chars(2, &format!(r"\g{{{c}}}"));
                } else {
                    self.copy_chars(2);
                }
                NULLABLE
            }
            'g' | 'k' => {
                let len = reference_len(self.rest());
                self.copy_bytes(len);
                NULLABLE
            }
            'p' | 'P' => {
                let len = property_len(self.rest());
                self.whole_text = true;
                self.copy_bytes(len);
                ITEM
            }
            'R' | 's' | 'v' | 'H' | 'D' | 'W' | 'X' => {
                self.whole_text = true;
                self.copy_chars(2);
                ITEM
            }
            _ => {
                self.copy_chars(2);
                ITEM
            }
        }
    }

    /// `\xHH`, `\x{H..}`, `\o{O..}`, `\0OOO`, `\cX` and `\N{U+H..}` (or a bare `\N`).
    fn code_point(&mut self) -> Atom {
        let Some((len, value)) = code_point_at(self.rest()) else {
            self.copy_chars(2);
            return ITEM;
        };
        let octal = self.rest().starts_with(r"\0");
        match value {
            0 => {
                self.anchor(false);
                self.bump_bytes(len);
                self.nul();
            }
            0x0A => {
                self.anchor(false);
                self.bump_bytes(len);
                self.line_break();
            }
            0x0D => {
                self.anchor(false);
                self.bump_bytes(len);
                return self.carriage_return();
            }
            0xD800..=0xDBFF => {
                let low = code_point_at(&self.rest()[len..])
                    .filter(|&(_, low)| (0xDC00..0xE000).contains(&low));
                match low {
                    Some((low_len, low)) => {
                        self.note(Note::SurrogatePair);
                        let joined = 0x10000 + ((value - 0xD800) << 10) + (low - 0xDC00);
                        self.anchor(false);
                        self.bump_bytes(len + low_len);
                        self.out.push_str(&format!(r"\x{{{joined:X}}}"));
                    }
                    None => self.copy_bytes(len),
                }
            }
            _ if octal => {
                self.note(Note::OctalEscape);
                self.anchor(false);
                self.bump_bytes(len);
                self.out.push_str(&format!(r"\x{{{value:X}}}"));
            }
            _ => self.copy_bytes(len),
        }
        ITEM
    }

    /// A line break was consumed (and anchored): LF in the buffer, any line ending in files.
    fn line_break(&mut self) {
        self.whole_text = true;
        if self.target == Target::Files {
            self.note(Note::LineBreak);
        }
        self.out.push_str(match self.target {
            Target::Buffer => r"\n",
            Target::Files => ANY_LINE_BREAK,
        });
    }

    /// A carriage return was consumed (and anchored). `\r\n` is one line break; a lone `\r`
    /// is a line break too, unless it is optional (`\r?`, `\r*`), which keeps `\r?\n` working.
    fn carriage_return(&mut self) -> Atom {
        if let Some(len) = self.line_feed_len() {
            let optional = quantifier_at(&self.rest()[len..]).is_some_and(|(_, min)| min == 0);
            if !optional {
                self.bump_bytes(len);
                self.note(Note::LineBreak);
                self.line_break();
                return ITEM;
            }
        }
        if quantifier_at(self.rest()).is_some_and(|(_, min)| min == 0) {
            self.out.push_str(r"\r");
            return ITEM;
        }
        self.note(Note::LineBreak);
        self.line_break();
        ITEM
    }

    /// The length of a line feed at the current position, if there is one.
    fn line_feed_len(&self) -> Option<usize> {
        let rest = self.rest();
        if rest.starts_with(r"\n") {
            return Some(2);
        }
        if rest.starts_with('\n') && !self.extended {
            return Some(1);
        }
        code_point_at(rest)
            .filter(|&(_, value)| value == 0x0A)
            .map(|(len, _)| len)
    }

    /// A NUL was consumed (and anchored).
    fn nul(&mut self) {
        match self.target {
            Target::Buffer => {
                self.note(Note::NulPlaceholder);
                self.out.push_str(r"\x{2400}");
            }
            Target::Files => self.out.push_str(r"\x{0}"),
        }
    }

    /// `\Q...\E`: literal text. Line breaks and NUL inside it are translated like elsewhere.
    fn quoted(&mut self) -> Atom {
        self.copy_chars(2);
        let mut empty = true;
        loop {
            if self.rest().starts_with(r"\E") {
                self.copy_chars(2);
                break;
            }
            let Some(c) = self.peek() else { break };
            empty = false;
            if matches!(c, '\n' | '\r' | '\0') {
                self.anchor(false);
                self.bump();
                if c == '\r' && self.peek() == Some('\n') {
                    self.bump();
                }
                self.out.push_str(r"\E");
                match c {
                    '\0' => self.nul(),
                    '\r' => {
                        self.note(Note::LineBreak);
                        self.line_break();
                    }
                    _ => self.line_break(),
                }
                self.out.push_str(r"\Q");
            } else {
                self.copy_chars(1);
            }
        }
        Atom::Item { nullable: empty }
    }

    fn class(&mut self) {
        self.copy_chars(1);
        if self.peek() == Some('^') {
            self.whole_text = true;
            self.copy_chars(1);
        }
        let mut first = true;
        let mut previous: Option<u32> = None;
        let mut range_from: Option<u32> = None;
        loop {
            let Some(c) = self.peek() else { return };
            if c == ']' && !first {
                self.copy_chars(1);
                return;
            }
            first = false;
            let value = match c {
                '[' if matches!(self.peek_nth(1), Some(':' | '.' | '=')) => {
                    self.posix_class();
                    None
                }
                '\\' => self.class_escape(),
                '-' if previous.is_some() && self.peek_nth(1).is_some_and(|next| next != ']') => {
                    self.copy_chars(1);
                    range_from = previous;
                    previous = None;
                    continue;
                }
                _ => {
                    self.copy_chars(1);
                    Some(u32::from(c))
                }
            };
            if let (Some(low), Some(high)) = (range_from.take(), value) {
                self.whole_text |= (low..=high).contains(&0x0A);
            }
            self.whole_text |= value == Some(0x0A);
            previous = value;
        }
    }

    /// `[:alpha:]`, `[:^space:]` and the collating forms PCRE2 rejects.
    fn posix_class(&mut self) {
        let delimiter = self.peek_nth(1).unwrap_or(':');
        let closing = format!("{delimiter}]");
        let len = self
            .rest()
            .get(2..)
            .and_then(|body| body.find(&closing))
            .map_or(2, |end| end + 4);
        let name = self.rest().get(2..len.saturating_sub(2)).unwrap_or("");
        self.whole_text |= name.starts_with('^') || matches!(name, "space" | "cntrl");
        self.copy_bytes(len);
    }

    /// An escape inside a class, and the single code point it stands for, if it is one.
    fn class_escape(&mut self) -> Option<u32> {
        let Some(c) = self.peek_nth(1) else {
            self.copy_chars(1);
            return None;
        };
        match c {
            'l' | 'u' | 'L' | 'U' => {
                self.note(Note::BoostCaseClass);
                self.whole_text |= c.is_uppercase();
                let class = match c {
                    'l' => "[:lower:]",
                    'u' => "[:upper:]",
                    'L' => "[:^lower:]",
                    _ => "[:^upper:]",
                };
                self.replace_chars(2, class);
                None
            }
            'Q' => {
                self.copy_chars(2);
                while !self.rest().is_empty() && !self.rest().starts_with(r"\E") {
                    self.whole_text |= self.peek() == Some('\n');
                    self.copy_chars(1);
                }
                if self.rest().starts_with(r"\E") {
                    self.copy_chars(2);
                }
                None
            }
            'x' | 'o' | '0' | 'c' | 'N' => match code_point_at(self.rest()) {
                Some((len, value)) if self.rest().starts_with(r"\0") => {
                    self.note(Note::OctalEscape);
                    self.anchor(false);
                    self.bump_bytes(len);
                    self.out.push_str(&format!(r"\x{{{value:X}}}"));
                    Some(value)
                }
                Some((len, value)) => {
                    self.copy_bytes(len);
                    Some(value)
                }
                None => {
                    self.copy_chars(2);
                    None
                }
            },
            'p' | 'P' => {
                let len = property_len(self.rest());
                self.whole_text = true;
                self.copy_bytes(len);
                None
            }
            's' | 'v' | 'D' | 'W' | 'H' | 'R' | 'X' => {
                self.whole_text = true;
                self.copy_chars(2);
                None
            }
            'd' | 'w' | 'S' | 'V' | 'h' | 'E' => {
                self.copy_chars(2);
                None
            }
            _ => {
                self.copy_chars(2);
                match c {
                    'n' => Some(0x0A),
                    'r' => Some(0x0D),
                    't' => Some(0x09),
                    'f' => Some(0x0C),
                    'a' => Some(0x07),
                    'e' => Some(0x1B),
                    'b' => Some(0x08),
                    c if c.is_ascii_alphanumeric() => None,
                    c => Some(u32::from(c)),
                }
            }
        }
    }

    fn group(&mut self) -> Atom {
        let rest = self.rest();
        if rest.starts_with("(?#") {
            let len = rest.find(')').map_or(rest.len(), |end| end + 1);
            self.copy_bytes(len);
            return Atom::Nothing;
        }
        if rest.starts_with("(*") {
            return self.verb();
        }
        if !rest.starts_with("(?") {
            self.copy_chars(1);
            let nullable = self.group_body();
            return Atom::Item { nullable };
        }
        let mut chars = rest[2..].chars();
        let first = chars.next();
        let second = chars.next();
        match (first, second) {
            (Some(':' | '|' | '>'), _) => {
                self.copy_chars(3);
                Atom::Item {
                    nullable: self.group_body(),
                }
            }
            (Some('=' | '!' | '*'), _) | (Some('<'), Some('=' | '!' | '*')) => {
                self.copy_chars(if first == Some('<') { 4 } else { 3 });
                self.group_body();
                NULLABLE
            }
            (Some('<' | '\''), _) | (Some('P'), Some('<')) => {
                let close = if first == Some('\'') { '\'' } else { '>' };
                let skip = if first == Some('P') { 4 } else { 3 };
                let len = rest[skip..]
                    .find(close)
                    .map_or(rest.len(), |end| skip + end + 1);
                self.copy_bytes(len);
                Atom::Item {
                    nullable: self.group_body(),
                }
            }
            (Some('P'), Some('=' | '>')) | (Some('&' | 'R' | '0'..='9' | '+'), _) => {
                self.copy_through_paren();
                NULLABLE
            }
            (Some('-'), Some('0'..='9')) => {
                self.copy_through_paren();
                NULLABLE
            }
            (Some('('), _) => self.conditional(),
            (Some('C'), _) => {
                self.callout();
                Atom::Nothing
            }
            (Some('['), _) => {
                let len = rest.find("])").map_or(rest.len(), |end| end + 2);
                self.whole_text = true;
                self.copy_bytes(len);
                ITEM
            }
            _ => self.options(),
        }
    }

    /// The rest of a group after its opening, through its `)`. Option changes inside the group
    /// end with it.
    fn group_body(&mut self) -> bool {
        let saved = (self.extended, self.dotall);
        let (nullable, _) = self.alternation();
        if self.peek() == Some(')') {
            self.copy_chars(1);
        }
        (self.extended, self.dotall) = saved;
        nullable
    }

    fn copy_through_paren(&mut self) {
        let len = self
            .rest()
            .find(')')
            .map_or(self.rest().len(), |end| end + 1);
        self.copy_bytes(len);
    }

    /// `(?i)`, `(?x-s)`, `(?^)` for the rest of the group, or `(?i:...)` for a group.
    fn options(&mut self) -> Atom {
        let rest = self.rest();
        let flags_len = rest[2..]
            .find(|c: char| !(c.is_ascii_alphabetic() || c == '^' || c == '-'))
            .unwrap_or(rest.len() - 2);
        let flags = &rest[2..2 + flags_len];
        let terminator = rest[2 + flags_len..].chars().next();
        let (mut extended, mut dotall) = (self.extended, self.dotall);
        let mut on = true;
        for flag in flags.chars() {
            match flag {
                '^' => (extended, dotall) = (false, false),
                '-' => on = false,
                'x' => extended = on,
                's' => dotall = on,
                _ => {}
            }
        }
        self.copy_bytes(2 + flags_len);
        match terminator {
            Some(')') => {
                self.copy_chars(1);
                (self.extended, self.dotall) = (extended, dotall);
                Atom::Nothing
            }
            Some(':') => {
                self.copy_chars(1);
                let saved = (self.extended, self.dotall);
                (self.extended, self.dotall) = (extended, dotall);
                let nullable = self.group_body();
                (self.extended, self.dotall) = saved;
                Atom::Item { nullable }
            }
            _ => Atom::Nothing,
        }
    }

    /// `(?(condition)yes|no)`.
    fn conditional(&mut self) -> Atom {
        let condition = &self.rest()[3..];
        let assertion = ["?=", "?!", "?<=", "?<!", "*"]
            .iter()
            .any(|start| condition.starts_with(start));
        let define = condition.starts_with("DEFINE)");
        if assertion {
            self.copy_chars(2);
            self.group();
        } else {
            self.copy_through_paren();
        }
        let saved = (self.extended, self.dotall);
        let (nullable, branches) = self.alternation();
        if self.peek() == Some(')') {
            self.copy_chars(1);
        }
        (self.extended, self.dotall) = saved;
        Atom::Item {
            nullable: define || nullable || branches < 2,
        }
    }

    /// `(?C)`, `(?C1)` and `(?C"text")`, whose text may contain `)`.
    fn callout(&mut self) {
        let rest = self.rest();
        let body = &rest[3..];
        let len = match body.chars().next() {
            Some(open @ ('`' | '\'' | '"' | '^' | '%' | '#' | '$' | '{')) => {
                let close = if open == '{' { '}' } else { open };
                let mut end = None;
                let mut chars = body.char_indices().skip(1).peekable();
                while let Some((i, c)) = chars.next() {
                    if c == close {
                        if chars.peek().is_some_and(|&(_, next)| next == close) {
                            chars.next();
                            continue;
                        }
                        end = Some(i + c.len_utf8());
                        break;
                    }
                }
                end.map_or(rest.len(), |end| {
                    3 + end + usize::from(body[end..].starts_with(')'))
                })
            }
            _ => rest.find(')').map_or(rest.len(), |end| end + 1),
        };
        self.copy_bytes(len);
    }

    /// `(*VERB)`, `(*MARK:name)` and the alphabetic assertions such as `(*pla:...)`.
    fn verb(&mut self) -> Atom {
        let body = &self.rest()[2..];
        let name_len = body
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(body.len());
        let name = &body[..name_len];
        let lookaround = matches!(
            name,
            "pla"
                | "positive_lookahead"
                | "nla"
                | "negative_lookahead"
                | "plb"
                | "positive_lookbehind"
                | "nlb"
                | "negative_lookbehind"
                | "napla"
                | "non_atomic_positive_lookahead"
                | "naplb"
                | "non_atomic_positive_lookbehind"
        );
        let grouping = matches!(
            name,
            "atomic" | "sr" | "script_run" | "asr" | "atomic_script_run"
        );
        if body[name_len..].starts_with(':') && (lookaround || grouping) {
            self.copy_bytes(2 + name_len + 1);
            let nullable = self.group_body();
            return Atom::Item {
                nullable: lookaround || nullable,
            };
        }
        let name = name.to_owned();
        self.copy_through_paren();
        match name.as_str() {
            "ACCEPT" => {
                self.accept = true;
                NULLABLE
            }
            "FAIL" | "F" => ITEM,
            _ => NULLABLE,
        }
    }
}

const ANY_LINE_BREAK: &str = r"(?:\r\n|\n|\r)";

/// PCRE2's `(?x)` whitespace.
fn is_pattern_space(c: char) -> bool {
    matches!(
        c,
        ' ' | '\t'
            | '\n'
            | '\u{0B}'
            | '\u{0C}'
            | '\r'
            | '\u{85}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{2028}'
            | '\u{2029}'
    )
}

/// Line breaks under PCRE2's `NEWLINE_ANY`, which ends a `(?x)` comment.
fn is_newline(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

fn start_item_len(rest: &str) -> Option<usize> {
    let body = rest.strip_prefix("(*")?;
    let end = body.find(')')?;
    let head = body[..end].split('=').next()?;
    let valid = !head.is_empty()
        && head
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        && !matches!(
            head,
            "ACCEPT" | "FAIL" | "F" | "COMMIT" | "PRUNE" | "SKIP" | "THEN" | "MARK"
        );
    valid.then_some(2 + end + 1)
}

/// A quantifier at the start of `rest`: its length and its minimum count.
pub(crate) fn quantifier_at(rest: &str) -> Option<(usize, u32)> {
    let (len, min) = match rest.as_bytes().first()? {
        b'*' | b'?' => (1, 0),
        b'+' => (1, 1),
        b'{' => braces(rest)?,
        _ => return None,
    };
    let suffix = matches!(rest.as_bytes().get(len), Some(b'?' | b'+'));
    Some((len + usize::from(suffix), min))
}

/// `{n}`, `{n,}`, `{n,m}` and `{,m}`, with optional blanks around the numbers.
fn braces(rest: &str) -> Option<(usize, u32)> {
    let close = rest.find('}')?;
    let inner = &rest[1..close];
    let blank = |c: char| c == ' ' || c == '\t';
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    let (low, high) = match inner.split_once(',') {
        Some((low, high)) => (low.trim_matches(blank), Some(high.trim_matches(blank))),
        None => (inner.trim_matches(blank), None),
    };
    let min = if low.is_empty() {
        if !high.is_some_and(digits) {
            return None;
        }
        0
    } else {
        if !digits(low) || high.is_some_and(|high| !high.is_empty() && !digits(high)) {
            return None;
        }
        low.parse().unwrap_or(u32::MAX)
    };
    Some((close + 1, min))
}

/// A single-code-point escape at the start of `rest` (which starts with a backslash): its
/// length and value. Octal `\0` takes up to three more digits, as in Boost.
fn code_point_at(rest: &str) -> Option<(usize, u32)> {
    let body = rest.strip_prefix('\\')?;
    let mut chars = body.chars();
    match chars.next()? {
        'x' => {
            if let Some(inner) = body[1..].strip_prefix('{') {
                let end = inner.find('}')?;
                let value = u32::from_str_radix(inner[..end].trim(), 16).ok()?;
                Some((1 + 1 + 1 + end + 1, value))
            } else {
                let digits = body[1..]
                    .bytes()
                    .take(2)
                    .take_while(u8::is_ascii_hexdigit)
                    .count();
                let value = u32::from_str_radix(&body[1..1 + digits], 16).unwrap_or(0);
                Some((2 + digits, value))
            }
        }
        'o' => {
            let inner = body[1..].strip_prefix('{')?;
            let end = inner.find('}')?;
            let value = u32::from_str_radix(inner[..end].trim(), 8).ok()?;
            Some((1 + 1 + 1 + end + 1, value))
        }
        '0' => {
            let digits = body[1..]
                .bytes()
                .take(3)
                .take_while(|b| (b'0'..=b'7').contains(b))
                .count();
            let value = u32::from_str_radix(&body[..1 + digits], 8).ok()?;
            Some((2 + digits, value))
        }
        'c' => {
            let control = chars.next().filter(char::is_ascii)?;
            Some((3, u32::from(control.to_ascii_uppercase()) ^ 0x40))
        }
        'N' => {
            let inner = body[1..].strip_prefix("{U+")?;
            let end = inner.find('}')?;
            let value = u32::from_str_radix(&inner[..end], 16).ok()?;
            Some((1 + 1 + 3 + end + 1, value))
        }
        _ => None,
    }
}

/// `\g1`, `\g-1`, `\g{name}`, `\g<name>`, `\g'name'`, `\k<name>`, `\k{name}`, `\k'name'`.
fn reference_len(rest: &str) -> usize {
    let after = &rest[2..];
    let close = match after.chars().next() {
        Some('{') => '}',
        Some('<') => '>',
        Some('\'') => '\'',
        Some('-' | '+' | '0'..='9') => {
            let digits = after[1..].bytes().take_while(u8::is_ascii_digit).count();
            let sign = usize::from(!after.starts_with(|c: char| c.is_ascii_digit()));
            return 2 + sign + digits + usize::from(sign == 0);
        }
        _ => return 2,
    };
    after[1..].find(close).map_or(2, |end| 2 + 1 + end + 1)
}

/// `\pL`, `\p{Lu}`, `\P{^Greek}`.
fn property_len(rest: &str) -> usize {
    let after = &rest[2..];
    match after.chars().next() {
        Some('{') => after.find('}').map_or(2, |end| 2 + end + 1),
        Some(c) => 2 + c.len_utf8(),
        None => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn buffer(pattern: &str) -> Rewrite {
        rewrite(pattern, Target::Buffer, false)
    }

    fn scan_everything(pattern: &str) {
        for target in [Target::Buffer, Target::Files] {
            for dot in [false, true] {
                let rewritten = rewrite(pattern, target, dot);
                for offset in 0..=rewritten.pattern.len() + 1 {
                    let source = rewritten.map.source_offset(&rewritten.pattern, offset);
                    assert!(source <= pattern.chars().count());
                }
            }
        }
    }

    proptest! {
        #[test]
        fn never_panics_on_pattern_syntax(
            pattern in "[\\\\()\\[\\]{}?*+:<>=!^$|.a-zA-Z0-9\\n\\r\\x00 #'`\"&,-]{0,40}",
        ) {
            scan_everything(&pattern);
        }

        #[test]
        fn never_panics_on_any_text(pattern in "\\PC{0,30}") {
            scan_everything(&pattern);
        }
    }

    fn translated(pattern: &str) -> String {
        buffer(pattern).pattern
    }

    fn nullable(pattern: &str) -> bool {
        buffer(pattern).nullable
    }

    #[test]
    fn copies_plain_pcre2_unchanged() {
        for pattern in [
            r"(?<=user=)\w+",
            r"(?<key>\w+)=(?<val>\d+)",
            r"\((?:[^()]|(?R))*\)",
            r"(?x) u s e r # comment \<",
            r"[\<\]a-z]",
            r"\Qa\<b\E",
            r"(?i:abc)|(?-s)x",
            r"\p{Lu}+\x{263A}\x41\t",
            r"(*UCP)(*LIMIT_MATCH=100)a",
            r"(?(1)a|b)(?(<n>)c)(?(DEFINE)(?<d>x))",
            r"a{2,3}?b{,4}c{ 1 , 2 }+",
            r#"(?C"")x(?C'a)b')y"#,
        ] {
            assert_eq!(translated(pattern), pattern, "{pattern}");
        }
    }

    #[test]
    fn rewrites_boost_syntax() {
        assert_eq!(translated(r"\<user\>"), r"\b(?=\w)user\b(?<=\w)");
        assert_eq!(translated(r"\`a\'"), r"\Aa\z");
        assert_eq!(translated(r"x\Z"), r"x(?=\v*\z)");
        assert_eq!(
            translated(r"\u\l+\L"),
            r"[[:upper:]][[:lower:]]+[^[:lower:]]"
        );
        assert_eq!(translated(r"[\u\d]"), r"[[:upper:]\d]");
        assert_eq!(translated(r"a\Cb"), r"a.b");
        assert_eq!(translated(r"(a)\10"), r"(a)\g{1}0");
        assert_eq!(translated(r"\0101\012x"), r"\x{41}\nx");
        assert_eq!(translated(r"\x{D83D}\x{DE82}"), r"\x{1F682}");
        assert_eq!(translated(r"a\x00b\0"), r"a\x{2400}b\x{2400}");
    }

    #[test]
    fn translates_line_breaks_for_the_lf_buffer() {
        assert_eq!(translated(r"a\r\nb"), r"a\nb");
        assert_eq!(translated(r"a\rb"), r"a\nb");
        assert_eq!(translated(r"a\r?\nb"), r"a\r?\nb");
        assert_eq!(translated(r"a\r*$"), r"a\r*$");
        assert_eq!(translated(r"a\r\n?b"), r"a\n\n?b");
        assert_eq!(translated(r"(\r\n)+"), r"(\n)+");
        assert_eq!(translated("a\r\nb"), r"a\nb");
        assert_eq!(translated(r"[^\r\n]+"), r"[^\r\n]+");
        assert_eq!(translated(r"\x0d\x0a"), r"\n");
        let files = rewrite(r"a\r\nb\nc\r?\n", Target::Files, false).pattern;
        assert_eq!(files, r"a(?:\r\n|\n|\r)b(?:\r\n|\n|\r)c\r?(?:\r\n|\n|\r)");
    }

    #[test]
    fn inserts_dotall_after_start_items() {
        let rewritten = rewrite(r"(*UCP)a.b", Target::Buffer, true);
        assert_eq!(rewritten.pattern, r"(*UCP)(?s)a.b");
        assert!(rewritten.whole_text);
    }

    #[test]
    fn finds_patterns_that_can_match_empty_text() {
        for pattern in [
            "^",
            "$",
            "a*",
            "a?",
            "a{0,3}",
            "(a|)",
            "(?:)",
            r"\b",
            "^$",
            r"^\s*$",
            "^(.*)$",
            r"(?=a)",
            r"(?<=\d)(?=(\d{3})+\b)",
            "a|b*",
            "(a*)+",
            r"(a?)\1",
            "(?(1)a)",
            r"\Q\E",
            "(*ACCEPT)a",
            r"x{,2}",
            "(?i)",
            "",
        ] {
            assert!(nullable(pattern), "{pattern} can match empty text");
        }
        for pattern in [
            "a",
            "a+",
            r"\w+",
            "[a-z]",
            ".",
            r"(\w)\1",
            "a|b",
            r"\R",
            "(?:a)",
            r"(?=a)b",
            "a{1,3}",
            r"\Qx\E",
            r"\d++1",
            r"(?>\d+)1",
            r"\((?:[^()]|(?R))*\)",
            "(*FAIL)|a",
            r"\<user\>",
            r"user=\K\w+",
            "x(?i)",
        ] {
            assert!(!nullable(pattern), "{pattern} cannot match empty text");
        }
    }

    #[test]
    fn flags_keep_out_continuation_and_whole_text() {
        assert!(buffer(r"user=\K\w+").keep_out);
        assert!(buffer(r"\Ga").continuation);
        for pattern in [
            r"a\nb",
            r"a\sb",
            r"[^x]",
            r"\Aa",
            r"a\z",
            "(?s)a.b",
            r"\R",
            r"[\x00-\x1f]",
            r"\p{Cc}",
            "[[:space:]]",
        ] {
            assert!(buffer(pattern).whole_text, "{pattern} needs whole texts");
        }
        for pattern in [
            r"\w+",
            "a.b",
            r"[a-z]\d",
            r"\bfoo\b",
            "^x$",
            r"(?<=user=)\w+",
        ] {
            assert!(!buffer(pattern).whole_text, "{pattern} works line by line");
        }
    }

    #[test]
    fn maps_offsets_back_to_the_typed_pattern() {
        let rewritten = buffer(r"\<(ab");
        let pattern = &rewritten.pattern;
        assert_eq!(pattern, r"\b(?=\w)(ab");
        assert_eq!(rewritten.map.source_offset(pattern, pattern.len()), 5);
        assert_eq!(rewritten.map.source_offset(pattern, 3), 0);
        assert_eq!(rewritten.map.source_offset(pattern, 9), 3);
        for grown in ["\n", "a\rb", "\0", "\\Qx\ny\\E"] {
            scan_everything(grown);
        }
    }
}
