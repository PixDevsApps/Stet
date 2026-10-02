//! Fuzzy matching for quick open (Ctrl+P) and the command palette (F1).
//!
//! A query matches a candidate when its characters appear in the candidate in order; whitespace
//! in the query is ignored. Matching ignores case unless the query has an uppercase letter
//! (smart case). The score follows fzf's scheme: points per matched character, bonuses at the
//! start of words and path segments, at camelCase humps and for consecutive characters, and a
//! penalty per skipped character; the best alignment is found by dynamic programming.
//!
//! When the query matches inside the basename (after the last `/`), only the basename is scored
//! and a bonus added, more for a prefix and most for the whole name, so file-name hits rank
//! above hits spread over directories. Candidates without `/` (palette labels) are all basename.

#[cfg(test)]
mod tests;

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::mem;

const SCORE_MATCH: i32 = 16;
const GAP_START: i32 = -3;
const GAP_EXTENSION: i32 = -1;
const BONUS_BOUNDARY: i32 = SCORE_MATCH / 2;
const BONUS_BOUNDARY_WHITE: i32 = BONUS_BOUNDARY + 2;
const BONUS_BOUNDARY_DELIMITER: i32 = BONUS_BOUNDARY + 1;
const BONUS_CAMEL: i32 = BONUS_BOUNDARY + GAP_EXTENSION;
const BONUS_NON_WORD: i32 = BONUS_BOUNDARY;
const BONUS_CONSECUTIVE: i32 = -(GAP_START + GAP_EXTENSION);
const FIRST_CHAR_MULTIPLIER: i32 = 2;
const BONUS_BASENAME: i32 = 64;
const BONUS_BASENAME_PREFIX: i32 = 32;
const BONUS_BASENAME_EXACT: i32 = 64;
/// Above this many query × candidate cells, positions come from a greedy scan.
const MAX_TRACKED_CELLS: usize = 1 << 20;
const NONE: i32 = i32::MIN / 2;

/// A matched candidate: its score and the character indices of the matched characters.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Match {
    pub score: i32,
    pub positions: Vec<usize>,
}

/// A match from [`rank`], with the candidate's index.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Ranked {
    pub index: usize,
    pub score: i32,
    pub positions: Vec<usize>,
}

/// Scores `candidate` against `query`.
pub fn score(query: &str, candidate: &str) -> Option<Match> {
    Matcher::new(query).score(candidate)
}

/// The best `limit` matches, best first. Equal scores rank the shorter candidate first, then
/// the earlier one. An empty query returns the first `limit` candidates in their order.
pub fn rank<S: AsRef<str>>(query: &str, candidates: &[S], limit: usize) -> Vec<Ranked> {
    let mut matcher = Matcher::new(query);
    if matcher.is_empty() {
        return (0..candidates.len().min(limit))
            .map(|index| Ranked {
                index,
                score: 0,
                positions: Vec::new(),
            })
            .collect();
    }
    /// Higher is better: the score, then the shorter candidate, then the earlier one.
    type Key = (i32, Reverse<usize>, Reverse<usize>);
    let mut best: BinaryHeap<Reverse<Key>> = BinaryHeap::with_capacity(limit + 1);
    for (index, candidate) in candidates.iter().enumerate() {
        let candidate = candidate.as_ref();
        let Some(score) = matcher.score_of(candidate) else {
            continue;
        };
        let key: Key = (score, Reverse(candidate.len()), Reverse(index));
        if best.len() < limit {
            best.push(Reverse(key));
        } else if best.peek().is_some_and(|Reverse(worst)| key > *worst) {
            best.pop();
            best.push(Reverse(key));
        }
    }
    let mut top: Vec<_> = best.into_iter().map(|Reverse(key)| key).collect();
    top.sort_unstable_by(|a, b| b.cmp(a));
    top.into_iter()
        .map(|(score, _, Reverse(index))| {
            let positions = matcher
                .score(candidates[index].as_ref())
                .map(|found| found.positions)
                .unwrap_or_default();
            Ranked {
                index,
                score,
                positions,
            }
        })
        .collect()
}

/// A query prepared for scoring many candidates; it reuses its buffers between them.
#[derive(Debug, Clone)]
pub struct Matcher {
    query: Vec<char>,
    case_sensitive: bool,
    path_query: bool,
    scratch: Scratch,
}

#[derive(Debug, Clone, Default)]
struct Scratch {
    chars: Vec<char>,
    folded: Vec<char>,
    bonus: Vec<i32>,
    score: [Vec<i32>; 2],
    chunk: [Vec<i32>; 2],
    from: Vec<u32>,
}

impl Matcher {
    pub fn new(query: &str) -> Self {
        let case_sensitive = query.chars().any(char::is_uppercase);
        let query: Vec<char> = query
            .chars()
            .filter(|c| !c.is_whitespace())
            .map(|c| if case_sensitive { c } else { fold(c) })
            .collect();
        let path_query = query.contains(&'/');
        Self {
            query,
            case_sensitive,
            path_query,
            scratch: Scratch::default(),
        }
    }

    /// Whether the query has no characters to match; it then matches everything with score 0.
    pub fn is_empty(&self) -> bool {
        self.query.is_empty()
    }

    pub fn score(&mut self, candidate: &str) -> Option<Match> {
        self.run(candidate, true)
            .map(|(score, positions)| Match { score, positions })
    }

    /// The score alone, which is cheaper than [`score`](Self::score).
    pub fn score_of(&mut self, candidate: &str) -> Option<i32> {
        self.run(candidate, false).map(|(score, _)| score)
    }

    fn run(&mut self, candidate: &str, positions: bool) -> Option<(i32, Vec<usize>)> {
        if self.query.is_empty() {
            return Some((0, Vec::new()));
        }
        let Scratch {
            chars,
            folded,
            bonus,
            score,
            chunk,
            from,
        } = &mut self.scratch;
        chars.clear();
        chars.extend(candidate.chars());
        if chars.len() < self.query.len() {
            return None;
        }
        let compared: &[char] = if self.case_sensitive {
            chars
        } else {
            folded.clear();
            folded.extend(chars.iter().map(|&c| fold(c)));
            folded
        };
        let mut tables = Tables {
            bonus,
            score,
            chunk,
            from,
        };

        if !self.path_query {
            let base = chars
                .iter()
                .rposition(|&c| c == '/')
                .map_or(0, |slash| slash + 1);
            let name = &compared[base..];
            if is_subsequence(&self.query, name) {
                let (found, positions) =
                    tables.align(&self.query, chars, compared, base, positions);
                let extra = if name == self.query.as_slice() {
                    BONUS_BASENAME_EXACT
                } else if name.starts_with(&self.query) {
                    BONUS_BASENAME_PREFIX
                } else {
                    0
                };
                return Some((found + BONUS_BASENAME + extra, positions));
            }
            if base == 0 {
                return None;
            }
        }
        if !is_subsequence(&self.query, compared) {
            return None;
        }
        Some(tables.align(&self.query, chars, compared, 0, positions))
    }
}

struct Tables<'s> {
    bonus: &'s mut Vec<i32>,
    score: &'s mut [Vec<i32>; 2],
    chunk: &'s mut [Vec<i32>; 2],
    from: &'s mut Vec<u32>,
}

impl Tables<'_> {
    /// The best alignment of `query` in `compared[start..]`, which must contain it as a
    /// subsequence. Positions are indices into the whole candidate.
    fn align(
        &mut self,
        query: &[char],
        chars: &[char],
        compared: &[char],
        start: usize,
        want_positions: bool,
    ) -> (i32, Vec<usize>) {
        let text = &compared[start..];
        let (n, m) = (text.len(), query.len());
        self.bonus.clear();
        let mut previous = start
            .checked_sub(1)
            .map_or(Class::White, |before| Class::of(chars[before]));
        for &c in &chars[start..] {
            let class = Class::of(c);
            self.bonus.push(previous.bonus_before(class));
            previous = class;
        }
        let track = want_positions && n * m <= MAX_TRACKED_CELLS;
        if track {
            self.from.clear();
            self.from.resize(n * m, u32::MAX);
        }
        let [mut row, mut above] = mem::take(self.score);
        let [mut chunks, mut chunks_above] = mem::take(self.chunk);
        for buffer in [&mut row, &mut above, &mut chunks, &mut chunks_above] {
            buffer.clear();
            buffer.resize(n, 0);
        }

        for (i, &c) in text.iter().enumerate() {
            row[i] = if c == query[0] {
                SCORE_MATCH + self.bonus[i] * FIRST_CHAR_MULTIPLIER
            } else {
                NONE
            };
            chunks[i] = self.bonus[i];
        }
        for (j, &wanted) in query.iter().enumerate().skip(1) {
            mem::swap(&mut row, &mut above);
            mem::swap(&mut chunks, &mut chunks_above);
            let mut gap = NONE;
            let mut gap_from = u32::MAX;
            for i in 0..n {
                if i >= 2 {
                    let opened = above[i - 2].saturating_add(GAP_START);
                    let extended = gap.saturating_add(GAP_EXTENSION);
                    if above[i - 2] > NONE && opened >= extended {
                        gap = opened;
                        gap_from = (i - 2) as u32;
                    } else {
                        gap = extended.max(NONE);
                    }
                }
                row[i] = NONE;
                if text[i] != wanted {
                    continue;
                }
                let bonus = self.bonus[i];
                let mut best = NONE;
                let mut chunk = bonus;
                let mut source = u32::MAX;
                if i >= 1 && above[i - 1] > NONE {
                    let first = chunks_above[i - 1];
                    let (gain, run) = if bonus >= BONUS_BOUNDARY && bonus > first {
                        (bonus, bonus)
                    } else {
                        (bonus.max(first).max(BONUS_CONSECUTIVE), first)
                    };
                    best = above[i - 1] + SCORE_MATCH + gain;
                    chunk = run;
                    source = (i - 1) as u32;
                }
                if gap > NONE && gap + SCORE_MATCH + bonus > best {
                    best = gap + SCORE_MATCH + bonus;
                    chunk = bonus;
                    source = gap_from;
                }
                row[i] = best;
                chunks[i] = chunk;
                if track {
                    self.from[j * n + i] = source;
                }
            }
        }

        let (end, best) = row
            .iter()
            .copied()
            .enumerate()
            .filter(|&(_, score)| score > NONE)
            .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
            .expect("the query is a subsequence of the text");
        let positions = if !want_positions {
            Vec::new()
        } else if track {
            let mut positions = vec![0; m];
            let mut i = end;
            for j in (0..m).rev() {
                positions[j] = start + i;
                if j > 0 {
                    i = self.from[j * n + i] as usize;
                }
            }
            positions
        } else {
            greedy_positions(query, text, start)
        };
        *self.score = [row, above];
        *self.chunk = [chunks, chunks_above];
        (best, positions)
    }
}

fn is_subsequence(query: &[char], text: &[char]) -> bool {
    let mut wanted = query.iter();
    let mut next = wanted.next();
    for c in text {
        if next == Some(c) {
            next = wanted.next();
            if next.is_none() {
                return true;
            }
        }
    }
    next.is_none()
}

fn greedy_positions(query: &[char], text: &[char], start: usize) -> Vec<usize> {
    let mut positions = Vec::with_capacity(query.len());
    let mut from = 0;
    for wanted in query {
        let found = text[from..]
            .iter()
            .position(|c| c == wanted)
            .expect("the query is a subsequence of the text");
        positions.push(start + from + found);
        from += found + 1;
    }
    positions
}

fn fold(c: char) -> char {
    if c.is_ascii() {
        c.to_ascii_lowercase()
    } else {
        c.to_lowercase().next().unwrap_or(c)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    White,
    Delimiter,
    NonWord,
    Lower,
    Upper,
    Letter,
    Number,
}

impl Class {
    fn of(c: char) -> Self {
        if c.is_ascii() {
            match c {
                'a'..='z' => Self::Lower,
                'A'..='Z' => Self::Upper,
                '0'..='9' => Self::Number,
                ' ' | '\t' | '\n' | '\r' => Self::White,
                '/' | '\\' | ',' | ':' | ';' | '|' => Self::Delimiter,
                _ => Self::NonWord,
            }
        } else if c.is_whitespace() {
            Self::White
        } else if c.is_lowercase() {
            Self::Lower
        } else if c.is_uppercase() {
            Self::Upper
        } else if c.is_numeric() {
            Self::Number
        } else if c.is_alphabetic() {
            Self::Letter
        } else {
            Self::NonWord
        }
    }

    /// The bonus for matching a character of class `next` right after one of this class.
    fn bonus_before(self, next: Self) -> i32 {
        match next {
            Self::White => BONUS_BOUNDARY_WHITE,
            Self::Delimiter | Self::NonWord => BONUS_NON_WORD,
            _ => match self {
                Self::White => BONUS_BOUNDARY_WHITE,
                Self::Delimiter => BONUS_BOUNDARY_DELIMITER,
                Self::NonWord => BONUS_BOUNDARY,
                Self::Lower if next == Self::Upper => BONUS_CAMEL,
                previous if previous != Self::Number && next == Self::Number => BONUS_CAMEL,
                _ => 0,
            },
        }
    }
}
