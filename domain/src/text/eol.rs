use std::borrow::Cow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Eol {
    #[default]
    Lf,
    CrLf,
    Cr,
}

impl Eol {
    pub const ALL: [Self; 3] = [Self::Lf, Self::CrLf, Self::Cr];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
            Self::Cr => "\r",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Lf => "LF",
            Self::CrLf => "CRLF",
            Self::Cr => "CR",
        }
    }
}

/// Line-ending counts of an ASCII-compatible byte sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EolStats {
    pub lf: usize,
    pub crlf: usize,
    pub cr: usize,
}

impl EolStats {
    pub fn detect(bytes: &[u8]) -> Self {
        let mut stats = Self::default();
        for index in memchr::memchr2_iter(b'\r', b'\n', bytes) {
            if bytes[index] == b'\r' {
                if bytes.get(index + 1) == Some(&b'\n') {
                    stats.crlf += 1;
                } else {
                    stats.cr += 1;
                }
            } else if index == 0 || bytes[index - 1] != b'\r' {
                stats.lf += 1;
            }
        }
        stats
    }

    pub fn total(&self) -> usize {
        self.lf + self.crlf + self.cr
    }

    /// The most frequent line ending. A tie for the highest count falls back to LF.
    pub fn dominant(&self) -> Option<Eol> {
        let counts = [
            (Eol::Lf, self.lf),
            (Eol::CrLf, self.crlf),
            (Eol::Cr, self.cr),
        ];
        let max = counts.iter().map(|&(_, count)| count).max()?;
        if max == 0 {
            return None;
        }
        let mut leaders = counts.iter().filter(|&&(_, count)| count == max);
        match (leaders.next(), leaders.next()) {
            (Some(&(eol, _)), None) => Some(eol),
            _ => Some(Eol::Lf),
        }
    }

    pub fn is_mixed(&self) -> bool {
        [self.lf, self.crlf, self.cr]
            .iter()
            .filter(|&&count| count > 0)
            .count()
            > 1
    }
}

/// Replaces every CRLF and lone CR with LF, borrowing when there is nothing to replace.
pub fn normalize_to_lf(text: &str) -> Cow<'_, str> {
    let bytes = text.as_bytes();
    if memchr::memchr(b'\r', bytes).is_none() {
        return Cow::Borrowed(text);
    }
    let mut normalized = String::with_capacity(text.len());
    let mut start = 0;
    for index in memchr::memchr_iter(b'\r', bytes) {
        normalized.push_str(&text[start..index]);
        normalized.push('\n');
        start = if bytes.get(index + 1) == Some(&b'\n') {
            index + 2
        } else {
            index + 1
        };
    }
    normalized.push_str(&text[start..]);
    Cow::Owned(normalized)
}

/// Replaces every LF of LF-normalized text with `eol`.
pub fn convert_from_lf(text: &str, eol: Eol) -> Cow<'_, str> {
    if eol == Eol::Lf || memchr::memchr(b'\n', text.as_bytes()).is_none() {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(text.replace('\n', eol.as_str()))
    }
}

/// [`normalize_to_lf`] and [`EolStats::detect`] in one pass over text that arrives in
/// pieces, such as a file decoded in chunks. A CRLF split between two pieces counts once.
#[derive(Debug, Clone, Default)]
pub struct LfNormalizer {
    stats: EolStats,
    pending_cr: bool,
}

impl LfNormalizer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends `piece` to `out` with every CRLF and lone CR replaced by LF.
    pub fn push(&mut self, piece: &str, out: &mut String) {
        let bytes = piece.as_bytes();
        if bytes.is_empty() {
            return;
        }
        let mut start = 0;
        if std::mem::take(&mut self.pending_cr) {
            if bytes[0] == b'\n' {
                self.stats.crlf += 1;
                start = 1;
            } else {
                self.stats.cr += 1;
            }
        }
        let base = start;
        for index in memchr::memchr_iter(b'\r', &bytes[base..]).map(|index| index + base) {
            self.stats.lf += count_lf(&bytes[start..index]);
            out.push_str(&piece[start..index]);
            out.push('\n');
            start = match bytes.get(index + 1) {
                Some(b'\n') => {
                    self.stats.crlf += 1;
                    index + 2
                }
                Some(_) => {
                    self.stats.cr += 1;
                    index + 1
                }
                None => {
                    self.pending_cr = true;
                    index + 1
                }
            };
        }
        self.stats.lf += count_lf(&bytes[start..]);
        out.push_str(&piece[start..]);
    }

    /// The line-ending counts of everything pushed so far.
    pub fn finish(mut self) -> EolStats {
        if self.pending_cr {
            self.stats.cr += 1;
        }
        self.stats
    }
}

fn count_lf(bytes: &[u8]) -> usize {
    memchr::memchr_iter(b'\n', bytes).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn stats(lf: usize, crlf: usize, cr: usize) -> EolStats {
        EolStats { lf, crlf, cr }
    }

    #[test]
    fn detect_counts_each_kind() {
        assert_eq!(EolStats::detect(b""), stats(0, 0, 0));
        assert_eq!(EolStats::detect(b"no newline"), stats(0, 0, 0));
        assert_eq!(EolStats::detect(b"a\nb\r\nc\rd"), stats(1, 1, 1));
        assert_eq!(EolStats::detect(b"\r\r\n"), stats(0, 1, 1));
        assert_eq!(EolStats::detect(b"\n\r"), stats(1, 0, 1));
        assert_eq!(EolStats::detect(b"\r\n\r\n\n"), stats(1, 2, 0));
        assert_eq!(EolStats::detect(b"\r"), stats(0, 0, 1));
        assert_eq!(EolStats::detect(b"\n"), stats(1, 0, 0));
    }

    #[test]
    fn dominant_picks_the_most_frequent() {
        assert_eq!(stats(0, 0, 0).dominant(), None);
        assert_eq!(stats(3, 0, 0).dominant(), Some(Eol::Lf));
        assert_eq!(stats(1, 5, 2).dominant(), Some(Eol::CrLf));
        assert_eq!(stats(0, 1, 2).dominant(), Some(Eol::Cr));
    }

    #[test]
    fn dominant_ties_fall_back_to_lf() {
        assert_eq!(stats(2, 2, 0).dominant(), Some(Eol::Lf));
        assert_eq!(stats(0, 4, 4).dominant(), Some(Eol::Lf));
        assert_eq!(stats(1, 1, 1).dominant(), Some(Eol::Lf));
    }

    #[test]
    fn mixed_needs_two_kinds() {
        assert!(!stats(0, 0, 0).is_mixed());
        assert!(!stats(0, 9, 0).is_mixed());
        assert!(stats(1, 9, 0).is_mixed());
        assert!(stats(0, 1, 1).is_mixed());
    }

    #[test]
    fn normalize_replaces_crlf_and_cr() {
        assert_eq!(normalize_to_lf("a\r\nb\rc\nd"), "a\nb\nc\nd");
        assert_eq!(normalize_to_lf("\r\r\n\r"), "\n\n\n");
        assert_eq!(normalize_to_lf("é\r\n漢\r"), "é\n漢\n");
    }

    #[test]
    fn normalize_borrows_without_cr() {
        assert!(matches!(normalize_to_lf("a\nb"), Cow::Borrowed(_)));
    }

    #[test]
    fn convert_writes_the_target_eol() {
        assert_eq!(convert_from_lf("a\nb\n", Eol::CrLf), "a\r\nb\r\n");
        assert_eq!(convert_from_lf("a\nb\n", Eol::Cr), "a\rb\r");
        assert!(matches!(convert_from_lf("a\nb", Eol::Lf), Cow::Borrowed(_)));
        assert!(matches!(convert_from_lf("ab", Eol::CrLf), Cow::Borrowed(_)));
    }

    #[test]
    fn cr_only_text_round_trips_through_lf() {
        let original = "first\rsecond\r\rlast\r";
        let counts = EolStats::detect(original.as_bytes());
        assert_eq!(counts, stats(0, 0, 4));
        assert_eq!(counts.dominant(), Some(Eol::Cr));
        assert!(!counts.is_mixed());
        let normalized = normalize_to_lf(original);
        assert_eq!(normalized, "first\nsecond\n\nlast\n");
        assert_eq!(convert_from_lf(&normalized, Eol::Cr), original);
    }

    fn normalize_in_pieces(pieces: &[&str]) -> (String, EolStats) {
        let mut normalizer = LfNormalizer::new();
        let mut out = String::new();
        for piece in pieces {
            normalizer.push(piece, &mut out);
        }
        (out, normalizer.finish())
    }

    #[test]
    fn normalizer_joins_a_crlf_split_between_pieces() {
        let (text, counts) = normalize_in_pieces(&["a\r", "\nb"]);
        assert_eq!(text, "a\nb");
        assert_eq!(counts, stats(0, 1, 0));
    }

    #[test]
    fn normalizer_keeps_a_pending_cr_across_empty_pieces() {
        let (text, counts) = normalize_in_pieces(&["a\r", "", "", "\nb\r"]);
        assert_eq!(text, "a\nb\n");
        assert_eq!(counts, stats(0, 1, 1));
    }

    #[test]
    fn normalizer_counts_a_trailing_cr_as_cr() {
        let (text, counts) = normalize_in_pieces(&["a\r", "b\r"]);
        assert_eq!(text, "a\nb\n");
        assert_eq!(counts, stats(0, 0, 2));
    }

    #[test]
    fn normalizer_handles_cr_runs_and_mixed_endings() {
        let (text, counts) = normalize_in_pieces(&["\r\r", "\r\n\n", "x\r\ny\rz\n"]);
        assert_eq!(text, "\n\n\n\nx\ny\nz\n");
        assert_eq!(counts, stats(2, 2, 3));
    }

    fn lf_text() -> impl Strategy<Value = String> {
        proptest::collection::vec(prop_oneof![Just("\n".to_owned()), "[^\r\n]{0,8}"], 0..64)
            .prop_map(|parts| parts.concat())
    }

    fn any_eol_text() -> impl Strategy<Value = String> {
        proptest::collection::vec(
            prop_oneof![
                Just("\n".to_owned()),
                Just("\r".to_owned()),
                Just("\r\n".to_owned()),
                "[^\r\n]{0,6}"
            ],
            0..64,
        )
        .prop_map(|parts| parts.concat())
    }

    fn split_at_char_boundaries(text: &str, cuts: &[usize]) -> Vec<String> {
        let mut bounds: Vec<usize> = cuts
            .iter()
            .map(|cut| text.floor_char_boundary(cut % (text.len() + 1)))
            .collect();
        bounds.push(0);
        bounds.push(text.len());
        bounds.sort_unstable();
        bounds
            .windows(2)
            .map(|pair| text[pair[0]..pair[1]].to_owned())
            .collect()
    }

    proptest! {
        #[test]
        fn convert_then_normalize_round_trips(text in lf_text()) {
            for eol in Eol::ALL {
                let converted = convert_from_lf(&text, eol);
                prop_assert_eq!(normalize_to_lf(&converted), text.as_str());
            }
        }

        #[test]
        fn normalizer_matches_the_whole_text_functions(
            text in any_eol_text(),
            cuts in proptest::collection::vec(any::<usize>(), 0..8),
        ) {
            let pieces = split_at_char_boundaries(&text, &cuts);
            let pieces: Vec<&str> = pieces.iter().map(String::as_str).collect();
            let (normalized, stats) = normalize_in_pieces(&pieces);
            let expected = normalize_to_lf(&text);
            prop_assert_eq!(normalized.as_str(), expected.as_ref());
            prop_assert_eq!(stats, EolStats::detect(text.as_bytes()));
        }

        #[test]
        fn dominant_eol_round_trips_single_eol_text(text in lf_text()) {
            for eol in Eol::ALL {
                let converted = convert_from_lf(&text, eol);
                let (normalized, stats) = normalize_in_pieces(&[converted.as_ref()]);
                prop_assert_eq!(normalized.as_str(), text.as_str());
                let restored = convert_from_lf(&normalized, stats.dominant().unwrap_or_default());
                prop_assert_eq!(restored.as_ref(), converted.as_ref());
            }
        }

        #[test]
        fn converted_text_has_a_single_eol(text in lf_text()) {
            let lines = text.matches('\n').count();
            for eol in Eol::ALL {
                let detected = EolStats::detect(convert_from_lf(&text, eol).as_bytes());
                prop_assert_eq!(detected.total(), lines);
                prop_assert!(!detected.is_mixed());
                prop_assert_eq!(detected.dominant(), (lines > 0).then_some(eol));
            }
        }
    }
}
