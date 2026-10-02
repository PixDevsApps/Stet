//! Pairs the lines of a changed block that are versions of each other.
//!
//! Similarity is the Dice coefficient of the character bigrams of the lines, with whitespace
//! trimmed and collapsed so that indentation alone never makes lines alike. The search compares
//! 512-bit bigram signatures, which is cheap; the chosen pairs are then checked against the
//! exact bigram sets.

use std::cmp::{Ordering, Reverse};

/// The lowest similarity, in per mille, for two lines to pair.
const MIN_SIMILARITY: u32 = 400;
/// Long lines are compared by their first characters only.
const MAX_CHARS: usize = 512;
/// Blocks up to this many line combinations get the pairing with the highest total similarity;
/// larger blocks, and all blocks once a comparison has used [`SEARCH_BUDGET`], are paired along
/// the diagonal.
const FULL_SEARCH_CELLS: usize = 1 << 16;
const SEARCH_BUDGET: usize = 1 << 22;
const WINDOW: usize = 16;
/// A counterpart on the diagonal this similar is taken without searching around it.
const STRONG_SIMILARITY: u32 = 700;

/// Pairs lines block by block, sharing one search budget across a comparison.
pub(super) struct Pairer {
    budget: usize,
}

impl Pairer {
    pub(super) fn new() -> Self {
        Self {
            budget: SEARCH_BUDGET,
        }
    }

    /// Pairs lines of `left` with lines of `right` in order. Returns index pairs, increasing on
    /// both sides.
    pub(super) fn pair(&mut self, left: &[&str], right: &[&str]) -> Vec<(usize, usize)> {
        if left.is_empty() || right.is_empty() {
            return Vec::new();
        }
        let signatures = |lines: &[&str]| -> Vec<Signature> {
            lines.iter().map(|line| Signature::new(line)).collect()
        };
        let (left_signatures, right_signatures) = (signatures(left), signatures(right));
        let cells = left.len() * right.len();
        let mut pairs = if cells <= FULL_SEARCH_CELLS && cells <= self.budget {
            self.budget -= cells;
            best_pairs(&left_signatures, &right_signatures)
        } else {
            diagonal_pairs(&left_signatures, &right_signatures)
        };
        pairs.retain(|&(i, j)| similarity(left[i], right[j]) >= MIN_SIMILARITY);
        pairs
    }
}

/// The bigrams of a line with whitespace trimmed and collapsed, including the line's start and
/// end.
fn bigrams(line: &str) -> impl Iterator<Item = (char, char)> + '_ {
    let mut previous = '\0';
    let mut space = false;
    line.trim()
        .chars()
        .take(MAX_CHARS)
        .chain(['\0'])
        .filter_map(move |c| {
            if c.is_whitespace() {
                space = true;
                return None;
            }
            let pending = space.then_some((previous, ' '));
            let from = if space { ' ' } else { previous };
            space = false;
            previous = c;
            Some(pending.into_iter().chain([(from, c)]))
        })
        .flatten()
}

struct Signature {
    bits: [u64; 8],
    ones: u32,
}

impl Signature {
    fn new(line: &str) -> Self {
        let mut bits = [0u64; 8];
        for (first, second) in bigrams(line) {
            let hash = (u64::from(u32::from(first)) << 32 | u64::from(u32::from(second)))
                .wrapping_mul(0x9E37_79B9_7F4A_7C15);
            let bit = (hash >> 55) as usize;
            bits[bit >> 6] |= 1 << (bit & 63);
        }
        let ones = bits.iter().map(|word| word.count_ones()).sum();
        Self { bits, ones }
    }

    /// The Dice coefficient of the signatures in per mille, or 0 below [`MIN_SIMILARITY`].
    fn similarity(&self, other: &Self) -> u32 {
        let shared: u32 = self
            .bits
            .iter()
            .zip(&other.bits)
            .map(|(a, b)| (a & b).count_ones())
            .sum();
        let score = 2000 * shared / (self.ones + other.ones);
        if score >= MIN_SIMILARITY { score } else { 0 }
    }
}

/// The Dice coefficient of the bigram sets of two lines, in per mille.
fn similarity(a: &str, b: &str) -> u32 {
    let sorted = |line| {
        let mut grams: Vec<(char, char)> = bigrams(line).collect();
        grams.sort_unstable();
        grams.dedup();
        grams
    };
    let (a, b) = (sorted(a), sorted(b));
    let (mut i, mut j, mut shared) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            Ordering::Less => i += 1,
            Ordering::Greater => j += 1,
            Ordering::Equal => {
                shared += 1;
                i += 1;
                j += 1;
            }
        }
    }
    (2000 * shared / (a.len() + b.len())) as u32
}

#[derive(Clone, Copy)]
enum Step {
    SkipLeft,
    SkipRight,
    Pair,
}

/// The pairing with the highest total similarity (dynamic programming).
fn best_pairs(left: &[Signature], right: &[Signature]) -> Vec<(usize, usize)> {
    let width = right.len() + 1;
    let mut score = vec![0u32; (left.len() + 1) * width];
    let mut steps = vec![Step::SkipLeft; (left.len() + 1) * width];
    for (i, a) in left.iter().enumerate() {
        for (j, b) in right.iter().enumerate() {
            let here = (i + 1) * width + j + 1;
            let skip_left = score[i * width + j + 1];
            let skip_right = score[here - 1];
            let (mut best, mut step) = if skip_left >= skip_right {
                (skip_left, Step::SkipLeft)
            } else {
                (skip_right, Step::SkipRight)
            };
            let similar = a.similarity(b);
            if similar > 0 && score[i * width + j] + similar > best {
                best = score[i * width + j] + similar;
                step = Step::Pair;
            }
            score[here] = best;
            steps[here] = step;
        }
    }
    let mut pairs = Vec::new();
    let (mut i, mut j) = (left.len(), right.len());
    while i > 0 && j > 0 {
        match steps[i * width + j] {
            Step::SkipLeft => i -= 1,
            Step::SkipRight => j -= 1,
            Step::Pair => {
                i -= 1;
                j -= 1;
                pairs.push((i, j));
            }
        }
    }
    pairs.reverse();
    pairs
}

/// Pairs each left line with the right line that continues the last pair's diagonal when that
/// is a strong match, and otherwise with the most similar right line near that diagonal or
/// near the line's proportional position.
fn diagonal_pairs(left: &[Signature], right: &[Signature]) -> Vec<(usize, usize)> {
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for (i, a) in left.iter().enumerate() {
        let next_right = pairs.last().map_or(0, |&(_, j)| j + 1);
        if next_right >= right.len() {
            break;
        }
        let proportional = (i * right.len() / left.len()).max(next_right);
        let continued = pairs
            .last()
            .map_or(proportional, |&(paired_left, paired_right)| {
                paired_right + (i - paired_left)
            })
            .min(right.len() - 1);
        if a.similarity(&right[continued]) >= STRONG_SIMILARITY {
            pairs.push((i, continued));
            continue;
        }
        let around = |center: usize| {
            center.saturating_sub(WINDOW).max(next_right)..(center + WINDOW + 1).min(right.len())
        };
        let best = around(continued)
            .chain(around(proportional))
            .map(|j| (a.similarity(&right[j]), j))
            .filter(|&(similar, _)| similar > 0)
            .max_by_key(|&(similar, j)| (similar, Reverse(j)));
        if let Some((_, j)) = best {
            pairs.push((i, j));
        }
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(left: &[&str], right: &[&str]) -> Vec<(usize, usize)> {
        Pairer::new().pair(left, right)
    }

    fn numbered(count: usize, line: impl Fn(usize) -> String) -> Vec<String> {
        (0..count).map(line).collect()
    }

    fn strs(lines: &[String]) -> Vec<&str> {
        lines.iter().map(String::as_str).collect()
    }

    #[test]
    fn pairs_similar_lines_and_skips_unrelated_ones() {
        let left = [
            "fn parse(input: &str) -> Result<Value> {",
            "    parse_value(input)",
        ];
        let right = [
            "// Parses a value.",
            "fn parse(input: &str) -> Option<Value> {",
            "    parse_value(input).ok()",
            "}",
        ];
        assert_eq!(pair(&left, &right), vec![(0, 1), (1, 2)]);
        assert!(pair(&["alpha beta"], &["1234567"]).is_empty());
    }

    #[test]
    fn indentation_alone_does_not_make_lines_similar() {
        assert!(
            pair(
                &["                return None;"],
                &["                let total = x;"]
            )
            .is_empty()
        );
        assert_eq!(
            pair(&["\t\tlet  total = x;"], &["let total = x; "]),
            vec![(0, 0)]
        );
    }

    #[test]
    fn similarity_is_the_dice_coefficient_of_bigram_sets() {
        assert_eq!(similarity("same", "same"), 1000);
        assert_eq!(similarity("", ""), 1000);
        assert_eq!(similarity("", "x"), 0);
        assert_eq!(similarity("ab", "ac"), 333);
        let collapsed: Vec<(char, char)> = bigrams("  a  b ").collect();
        assert_eq!(
            collapsed,
            [('\0', 'a'), ('a', ' '), (' ', 'b'), ('b', '\0')]
        );
    }

    #[test]
    fn large_blocks_pair_along_the_diagonal() {
        let left = numbered(400, |i| format!("let value_{i} = {i};"));
        let right = numbered(400, |i| format!("let value_{i} = {i}; // checked"));
        let pairs = pair(&strs(&left), &strs(&right));
        assert_eq!(pairs.len(), 400);
        assert!(pairs.iter().all(|&(i, j)| i == j));
    }

    #[test]
    fn the_diagonal_follows_lines_inserted_inside_a_large_block() {
        let left = numbered(300, |i| format!("let value_{i} = compute({i});"));
        let mut right = numbered(300, |i| format!("let value_{i} = compute({i})?;"));
        right.splice(150..150, numbered(10, |i| format!("// note {i}")));
        let pairs = pair(&strs(&left), &strs(&right));
        assert_eq!(pairs.len(), 300);
        assert!(
            pairs
                .iter()
                .all(|&(i, j)| j == if i < 150 { i } else { i + 10 })
        );
    }
}
