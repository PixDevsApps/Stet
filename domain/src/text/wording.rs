//! Counts as the status bar, the find bar, the dialogs and the banners word them.

/// `one` for a count of 1, `many` for any other.
pub fn plural<'a>(count: usize, one: &'a str, many: &'a str) -> &'a str {
    if count == 1 { one } else { many }
}

/// `1 match`, `3 matches`.
pub fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", plural(count, one, many))
}

/// `12345678` as `12,345,678`.
pub fn grouped(number: usize) -> String {
    let digits = number.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_are_separated_by_commas() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000), "1,000");
        assert_eq!(grouped(100_000), "100,000");
        assert_eq!(grouped(12_345_678), "12,345,678");
    }

    #[test]
    fn plurals() {
        assert_eq!(plural(1, "line", "lines"), "line");
        assert_eq!(plural(0, "line", "lines"), "lines");
        assert_eq!(plural(2, "line", "lines"), "lines");
        assert_eq!(counted(1, "hit", "hits"), "1 hit");
        assert_eq!(counted(0, "hit", "hits"), "0 hits");
    }
}
