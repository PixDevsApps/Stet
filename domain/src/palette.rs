//! Command-palette matching. A query matches a text when its characters appear in it in
//! order, ignoring case; whole-substring matches, word starts and runs of consecutive
//! characters score higher. Deliberately small: quick open (M8) brings a real fuzzy matcher.

/// How well `query` matches `text`; `None` when it does not. An empty query matches
/// everything with score 0. Higher is better.
pub fn score(query: &str, text: &str) -> Option<u32> {
    let query: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(fold)
        .collect();
    if query.is_empty() {
        return Some(0);
    }
    let text: Vec<char> = text.chars().collect();
    let lower: Vec<char> = text.iter().copied().map(fold).collect();

    if let Some(start) = find_run(&lower, &query) {
        let word_start = is_word_start(&text, start);
        let bonus = if start == 0 {
            3000
        } else if word_start {
            2000
        } else {
            1000
        };
        return Some(bonus + 500u32.saturating_sub(start as u32));
    }

    let mut total = 0u32;
    let mut next = 0;
    let mut previous: Option<usize> = None;
    for &wanted in &query {
        let found = (next..lower.len()).find(|&index| lower[index] == wanted)?;
        total += if previous == Some(found.wrapping_sub(1)) {
            8
        } else if is_word_start(&text, found) {
            6
        } else {
            1
        };
        previous = Some(found);
        next = found + 1;
    }
    Some(total)
}

/// One lowercase character per character, so query and text stay aligned.
fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

fn find_run(haystack: &[char], needle: &[char]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn is_word_start(text: &[char], index: usize) -> bool {
    match index.checked_sub(1).map(|before| text[before]) {
        None => true,
        Some(before) => {
            !before.is_alphanumeric() || (before.is_lowercase() && text[index].is_uppercase())
        }
    }
}

/// Indexes of `items` that match `query`, best first; ties keep their original order.
pub fn rank<'a>(query: &str, items: impl IntoIterator<Item = &'a str>) -> Vec<usize> {
    let mut scored: Vec<(usize, u32)> = items
        .into_iter()
        .enumerate()
        .filter_map(|(index, text)| score(query, text).map(|score| (index, score)))
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    scored.into_iter().map(|(index, _)| index).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_matches_everything() {
        assert_eq!(score("", "Save"), Some(0));
        assert_eq!(score("  ", ""), Some(0));
    }

    #[test]
    fn matches_in_order_ignoring_case() {
        assert!(score("sv", "Save").is_some());
        assert!(score("SAVE", "save all").is_some());
        assert!(score("zi", "Zoom In").is_some());
        assert_eq!(score("vs", "Save"), None);
        assert_eq!(score("x", "Save"), None);
    }

    #[test]
    fn substrings_beat_scattered_matches() {
        let exact = score("save", "Save All").unwrap();
        let scattered = score("sael", "Save All").unwrap();
        assert!(exact > scattered);
        assert!(score("all", "Save All").unwrap() > score("al", "Go to Line").unwrap_or(0));
    }

    #[test]
    fn word_starts_beat_the_middle_of_words() {
        assert!(score("line", "Go to Line…").unwrap() > score("ine", "Go to Line…").unwrap());
        assert!(score("tl", "Go to Line").unwrap() > score("ol", "Go to Line").unwrap());
    }

    #[test]
    fn ranking_is_stable() {
        let items = ["Save", "Save As…", "Save All", "Open…", "Close"];
        assert_eq!(rank("save", items), vec![0, 1, 2]);
        assert_eq!(rank("", items), vec![0, 1, 2, 3, 4]);
        assert_eq!(rank("clo", items), vec![4]);
        assert_eq!(rank("sa", items).first(), Some(&0));
    }

    #[test]
    fn unicode_text_does_not_panic() {
        assert!(score("ø", "Åpne fil ø").is_some());
        assert!(score("İst", "İstanbul").is_some());
        assert_eq!(score("日本", "日本語"), Some(3000 + 500));
    }
}
