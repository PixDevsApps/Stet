//! Blocks of lines removed in one place and added in another.
//!
//! A block grows around an anchor: a line that occurs exactly once among the removed lines and
//! once among the added lines, and has at least three alphanumeric characters, so that lines
//! such as `}` never count as moved on their own.

use std::ops::Range;

use imara_diff::{Hunk, Token};

const UNSEEN: u32 = u32::MAX;
const REPEATED: u32 = u32::MAX - 1;

/// Moved lines, indexed by position in the compared (kept) line sequences.
pub(super) struct Moves {
    pub(super) blocks: Vec<(Range<usize>, Range<usize>)>,
    pub(super) left: Vec<bool>,
    pub(super) right: Vec<bool>,
}

impl Moves {
    pub(super) fn none(left: usize, right: usize) -> Self {
        Self {
            blocks: Vec::new(),
            left: vec![false; left],
            right: vec![false; right],
        }
    }

    pub(super) fn detect(
        left: &[Token],
        right: &[Token],
        hunks: &[Hunk],
        token_count: usize,
        significant: impl Fn(Token) -> bool,
    ) -> Self {
        let mut removed = vec![false; left.len()];
        let mut added = vec![false; right.len()];
        for hunk in hunks {
            removed[hunk.before.start as usize..hunk.before.end as usize].fill(true);
            added[hunk.after.start as usize..hunk.after.end as usize].fill(true);
        }
        let left_at = unique_positions(left, &removed, token_count);
        let right_at = unique_positions(right, &added, token_count);
        let mut moves = Self::none(left.len(), right.len());
        let joinable = |l: usize, r: usize, moves: &Self| {
            removed[l] && added[r] && !moves.left[l] && !moves.right[r] && left[l] == right[r]
        };
        for (anchor, &token) in left.iter().enumerate() {
            let partner = right_at[token.0 as usize];
            if !removed[anchor]
                || moves.left[anchor]
                || left_at[token.0 as usize] != anchor as u32
                || partner >= REPEATED
                || moves.right[partner as usize]
                || !significant(token)
            {
                continue;
            }
            let partner = partner as usize;
            let (mut start, mut partner_start) = (anchor, partner);
            while start > 0 && partner_start > 0 && joinable(start - 1, partner_start - 1, &moves) {
                start -= 1;
                partner_start -= 1;
            }
            let (mut end, mut partner_end) = (anchor + 1, partner + 1);
            while end < left.len()
                && partner_end < right.len()
                && joinable(end, partner_end, &moves)
            {
                end += 1;
                partner_end += 1;
            }
            moves.left[start..end].fill(true);
            moves.right[partner_start..partner_end].fill(true);
            moves.blocks.push((start..end, partner_start..partner_end));
        }
        moves
    }
}

/// For each token, its position among the changed lines if it occurs there exactly once.
fn unique_positions(tokens: &[Token], changed: &[bool], token_count: usize) -> Vec<u32> {
    let mut at = vec![UNSEEN; token_count];
    for (index, token) in tokens.iter().enumerate() {
        if changed[index] {
            let slot = &mut at[token.0 as usize];
            *slot = if *slot == UNSEEN {
                index as u32
            } else {
                REPEATED
            };
        }
    }
    at
}

pub(super) fn is_significant(key: &str) -> bool {
    key.chars().filter(|c| c.is_alphanumeric()).nth(2).is_some()
}
