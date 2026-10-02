use std::sync::atomic::AtomicBool;
use std::time::Instant;

use proptest::prelude::*;
use stet_domain::search::{Query, SearchMode, SearchOptions, Template};
use stet_domain::text::{TextEdit, apply};

use super::*;

fn matcher(query: Query) -> Matcher {
    Matcher::new(&query).unwrap()
}

fn regex(pattern: &str) -> Matcher {
    matcher(
        Query::regex(pattern)
            .with_options(SearchOptions::new(SearchMode::Regex).with_match_case(true)),
    )
}

fn spans(matcher: &Matcher, text: &str) -> Vec<(usize, usize)> {
    matcher
        .find_all(text, None, usize::MAX, &AtomicBool::new(false))
        .unwrap()
        .matches
        .iter()
        .map(|found| (found.start, found.end))
        .collect()
}

fn replaced(matcher: &Matcher, text: &str, template: &str) -> String {
    let result = matcher
        .replace_all(
            text,
            &Template::parse(template),
            None,
            &AtomicBool::new(false),
        )
        .unwrap();
    apply(text, result.edits).unwrap()
}

#[test]
fn empty_matches_follow_perl() {
    assert_eq!(spans(&regex("a*"), "aab"), [(0, 2), (2, 2), (3, 3)]);
    assert_eq!(spans(&regex("(?=b)|b"), "b"), [(0, 0), (0, 1)]);
    assert_eq!(spans(&regex(r"a\K"), "aa"), [(1, 1), (2, 2)]);
    assert_eq!(spans(&regex("x*"), "é"), [(0, 0), (1, 1)]);
    assert_eq!(replaced(&regex("a*"), "aab", "X"), "XXbX");
}

#[test]
fn find_all_reports_characters_and_lines() {
    let found = regex(r"é\d")
        .find_all("é1\nx é2\n\né3", None, usize::MAX, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(
        found.matches,
        [
            Match {
                start: 0,
                end: 2,
                line: 0
            },
            Match {
                start: 5,
                end: 7,
                line: 1
            },
            Match {
                start: 9,
                end: 11,
                line: 3
            },
        ]
    );
    assert!(!found.truncated);
}

#[test]
fn a_range_sees_the_context_before_it_and_ends_the_text() {
    let cancel = AtomicBool::new(false);
    let found = regex(r"(?<=user=)\w+")
        .find_all("user=alice user=bob", Some(16..18), usize::MAX, &cancel)
        .unwrap();
    assert_eq!(found.matches[0].start..found.matches[0].end, 16..18);
    let found = regex(r"^\w")
        .find_all("ab\ncd", Some(1..5), usize::MAX, &cancel)
        .unwrap();
    assert_eq!(found.matches.len(), 1);
    assert_eq!(found.matches[0].start, 3);
    assert_eq!(
        regex("a").find_all("abc", Some(2..9), 10, &cancel),
        Err(SearchError::Range {
            range: 2..9,
            len: 3
        })
    );
}

#[test]
fn limit_marks_truncation() {
    let cancel = AtomicBool::new(false);
    let matcher = regex("a");
    let found = matcher.find_all("a a a", None, 2, &cancel).unwrap();
    assert_eq!(found.matches.len(), 2);
    assert!(found.truncated);
    let found = matcher.find_all("a a a", None, 3, &cancel).unwrap();
    assert_eq!(found.matches.len(), 3);
    assert!(!found.truncated);
}

#[test]
fn cancelling_stops_find_all_and_replace_all() {
    let cancel = AtomicBool::new(true);
    let matcher = regex("a");
    assert_eq!(
        matcher.find_all("aaa", None, usize::MAX, &cancel),
        Err(SearchError::Cancelled)
    );
    assert_eq!(
        matcher.replace_all("aaa", &Template::literal("b"), None, &cancel),
        Err(SearchError::Cancelled)
    );
}

#[test]
fn find_next_skips_an_empty_match_at_the_caret_and_wraps() {
    let caret = regex("^");
    let next = caret.find_next("a\nb\nc", 0, false, true).unwrap().unwrap();
    assert_eq!(
        (next.found.start, next.found.line, next.wrapped),
        (2, 1, false)
    );
    let wrapped = caret.find_next("a\nb\nc", 4, false, true).unwrap().unwrap();
    assert_eq!((wrapped.found.start, wrapped.wrapped), (0, true));
    assert_eq!(caret.find_next("a\nb\nc", 4, false, false).unwrap(), None);
    let word = regex("ab");
    let found = |from| {
        word.find_next("ab ab", from, false, true)
            .unwrap()
            .map(|next| (next.found.start, next.found.end, next.wrapped))
    };
    assert_eq!(found(0), Some((0, 2, false)));
    assert_eq!(found(2), Some((3, 5, false)));
    assert_eq!(found(5), Some((0, 2, true)));
}

#[test]
fn find_previous_and_its_window() {
    let word = regex("ab");
    let found = |from, wrap| {
        word.find_next("ab ab ab", from, true, wrap)
            .unwrap()
            .map(|next| (next.found.start, next.wrapped))
    };
    assert_eq!(found(8, false), Some((6, false)));
    assert_eq!(found(6, false), Some((3, false)));
    assert_eq!(found(0, false), None);
    assert_eq!(found(0, true), Some((6, true)));
    let long = format!("needle\n{}\nend", "x".repeat(300_000));
    let len = long.chars().count();
    let previous = regex("needle")
        .find_next(&long, len, true, false)
        .unwrap()
        .unwrap();
    assert_eq!(previous.found.start, 0);
    let spanning = regex(r"x\nend")
        .find_next(&long, len, true, false)
        .unwrap()
        .unwrap();
    assert_eq!(spanning.found.line, 1);
}

#[test]
fn find_next_sees_the_text_before_the_caret() {
    let at = |pattern: &str, text: &str, from| {
        let next = regex(pattern)
            .find_next(text, from, false, false)
            .unwrap()
            .unwrap();
        (next.found.start, next.found.end)
    };
    assert_eq!(at(r"(?<=user=)\w+", "user=alice", 5), (5, 10));
    assert_eq!(at(r"\bser\w*", "users serx", 1), (6, 10));
    assert_eq!(at(r"^ser\w*", "users\nserx", 1), (6, 10));
}

#[test]
fn replace_one_rechecks_the_match() {
    let matcher = regex(r"user=(\w+)");
    let text = "user=alice user=Bob";
    let found = matcher
        .find_next(text, 0, false, false)
        .unwrap()
        .unwrap()
        .found;
    let template = Template::parse(r"<\U\1>");
    assert_eq!(
        matcher.replace_one(text, &found, &template),
        Ok(TextEdit::new(0..10, "<ALICE>"))
    );
    let stale = Match {
        start: 1,
        end: 10,
        line: 0,
    };
    assert_eq!(
        matcher.replace_one(text, &stale, &template),
        Err(SearchError::NotAMatch)
    );
    let after_empty = regex("(?=b)|b");
    let second = Match {
        start: 0,
        end: 1,
        line: 0,
    };
    assert_eq!(
        after_empty.replace_one("b", &second, &Template::literal("x")),
        Ok(TextEdit::new(0..1, "x"))
    );
    assert_eq!(
        replace_one(
            text,
            &found,
            &Query::regex(r"user=(\w+)"),
            &Template::parse("$1")
        ),
        Ok(TextEdit::new(0..10, "alice"))
    );
}

#[test]
fn replace_all_counts_unchanged_matches_without_editing_them() {
    let result = regex(r"\w+")
        .replace_all(
            "ABC def",
            &Template::parse(r"\U$0"),
            None,
            &AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(result.count, 2);
    assert_eq!(result.edits, [TextEdit::new(4..7, "DEF")]);
}

#[test]
fn replace_all_in_a_range() {
    let result = regex("a")
        .replace_all(
            "a a a a",
            &Template::literal("b"),
            Some(2..5),
            &AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(result.apply("a a a a"), "a b b a");
}

#[test]
fn prefix_suffix_and_last_group() {
    assert_eq!(
        replaced(&regex(r"\d"), "a1b2c", "[$`|$']"),
        "a[a|b2c]b[b|c]c"
    );
    assert_eq!(replaced(&regex("((a)(b))"), "ab", "$^N"), "ab");
    assert_eq!(replaced(&regex("(a)(x)?"), "a", "[$+]"), "[]");
}

#[test]
fn case_insensitive_matching_is_unicode_aware() {
    let query = Query::regex("straße|ÉTÉ");
    assert_eq!(
        spans(&matcher(query), "STRASSE Straße été"),
        [(8, 14), (15, 18)]
    );
}

#[test]
fn pattern_errors_carry_a_message_and_the_typed_offset() {
    let error = Matcher::new(&Query::regex("(abc")).unwrap_err();
    assert_eq!(error.offset, 4);
    assert!(
        error.message.contains("missing closing parenthesis"),
        "{}",
        error.message
    );
    let error = Matcher::new(&Query::regex("")).unwrap_err();
    assert_eq!(error.offset, 0);
}

#[test]
fn a_single_bulk_edit_covers_the_first_to_the_last_change() {
    let text = "a1b22c3d";
    let result = regex(r"\d")
        .replace_all(text, &Template::literal("#"), None, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(result.edits.len(), 4);
    assert!(!result.is_bulk());
    let edit = result.as_single_edit(text).unwrap();
    assert_eq!(edit, TextEdit::new(1..7, "#b##c#"));
    assert_eq!(result.apply(text), "a#b##c#d");
    let mut changed = result.clone();
    changed.edits.pop();
    assert_eq!(changed.apply(text), "a#b##c3d");
    assert_eq!(ReplaceAll::default().as_single_edit(text), None);
}

#[test]
fn find_all_hits_group_by_line() {
    let text = "alpha beta\ngamma\nbeta beta";
    let found = regex("beta")
        .find_all(text, None, usize::MAX, &AtomicBool::new(false))
        .unwrap();
    let hits = document_hits(text, &found.matches);
    assert_eq!(hits.len(), 2);
    assert_eq!(
        (hits[0].line_number, hits[0].line_text.as_str()),
        (1, "alpha beta")
    );
    assert_eq!(
        (hits[1].line_number, hits[1].match_ranges.clone()),
        (3, vec![0..4, 5..9])
    );
}

fn naive_find(text: &str, needle: &str, ignore_case: bool) -> Vec<(usize, usize)> {
    let fold = |c: char| {
        if ignore_case {
            c.to_ascii_lowercase()
        } else {
            c
        }
    };
    let text: Vec<char> = text.chars().map(fold).collect();
    let needle: Vec<char> = needle.chars().map(fold).collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i + needle.len() <= text.len() {
        if text[i..i + needle.len()] == needle[..] {
            found.push((i, i + needle.len()));
            i += needle.len();
        } else {
            i += 1;
        }
    }
    found
}

fn swap_word_digit(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let word = |c: char| c.is_alphanumeric() || c == '_';
        if i + 1 < chars.len() && word(chars[i]) && chars[i + 1].is_ascii_digit() {
            out.push(chars[i + 1]);
            out.push(chars[i]);
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn nullable_patterns() -> impl Strategy<Value = String> {
    let atom = prop_oneof![
        Just("a"),
        Just("b"),
        Just("."),
        Just("^"),
        Just("$"),
        Just(r"\b"),
        Just(r"\d"),
        Just("(?=a)"),
        Just("(?!b)"),
        Just(r"\s"),
        Just(r"\<"),
        Just(r"\>"),
    ]
    .prop_map(String::from);
    atom.prop_recursive(3, 16, 3, |inner| {
        prop_oneof![
            (
                inner.clone(),
                prop_oneof![
                    Just(""),
                    Just("*"),
                    Just("+"),
                    Just("?"),
                    Just("{0,2}"),
                    Just("{1,2}"),
                    Just("*?")
                ]
            )
                .prop_map(|(item, quantifier)| format!("(?:{item}){quantifier}")),
            prop::collection::vec(inner.clone(), 1..3).prop_map(|items| items.concat()),
            (inner.clone(), inner.clone()).prop_map(|(a, b)| format!("{a}|{b}")),
            inner.prop_map(|item| format!("({item})")),
        ]
    })
}

proptest! {
    #[test]
    fn literal_search_finds_what_a_naive_search_finds(
        text in "[ab.*é\n ]{0,40}",
        needle in "[ab.*é\n]{1,4}",
    ) {
        let options = SearchOptions::new(SearchMode::Normal).with_match_case(true);
        let matcher = matcher(Query::new(needle.clone(), options));
        prop_assert_eq!(spans(&matcher, &text), naive_find(&text, &needle, false));
    }

    #[test]
    fn caseless_literal_search_finds_what_a_naive_search_finds(
        text in "[aAbB. ]{0,40}",
        needle in "[aAbB]{1,3}",
    ) {
        let matcher = matcher(Query::normal(needle.clone()));
        prop_assert_eq!(spans(&matcher, &text), naive_find(&text, &needle, true));
    }

    #[test]
    fn replace_all_edits_rebuild_the_expected_text(
        text in "[ab\né ]{0,40}",
        needle in "[ab]{1,3}",
        replacement in "[xyé\n]{0,3}",
    ) {
        let options = SearchOptions::new(SearchMode::Normal).with_match_case(true);
        let query = Query::new(needle.clone(), options);
        let result = replace_all(&text, &query, &Template::literal(&replacement), None, &AtomicBool::new(false)).unwrap();
        let expected = text.replace(&needle, &replacement);
        prop_assert_eq!(apply(&text, result.edits.clone()).unwrap(), expected.clone());
        prop_assert_eq!(result.apply(&text), expected);
    }

    #[test]
    fn regex_replacements_rebuild_the_expected_text(text in "[a1b2 _é]{0,30}") {
        let result = regex(r"(\w)(\d)")
            .replace_all(&text, &Template::parse("$2$1"), None, &AtomicBool::new(false))
            .unwrap();
        prop_assert_eq!(apply(&text, result.edits).unwrap(), swap_word_digit(&text));
    }

    #[test]
    fn patterns_that_match_empty_text_are_routed(
        pattern in nullable_patterns(),
        text in "[ab1 \n]{0,12}",
    ) {
        let matcher = regex(&pattern);
        let found = matcher.find_all(&text, None, usize::MAX, &AtomicBool::new(false)).unwrap();
        if found.matches.iter().any(|m| m.start == m.end) {
            prop_assert!(matcher.needs_own_matcher(), "{} matched empty text in {:?}", pattern, text);
        }
    }

    #[test]
    fn pattern_errors_point_into_the_typed_pattern(
        pattern in "[\\\\()\\[\\]{}?*+<>=!^$|.a-zA-Z0-9]{1,24}",
    ) {
        if let Err(error) = Matcher::new(&Query::regex(pattern.clone())) {
            prop_assert!(error.offset <= pattern.chars().count(), "{:?} for {}", error, pattern);
        }
    }

    #[test]
    fn find_next_visits_what_find_all_finds(text in "[ab \n]{0,30}") {
        let matcher = regex("ab|b");
        let all = spans(&matcher, &text);
        let mut visited = Vec::new();
        let mut from = 0;
        while let Some(next) = matcher.find_next(&text, from, false, false).unwrap() {
            visited.push((next.found.start, next.found.end));
            from = next.found.end;
        }
        prop_assert_eq!(&visited, &all);
        let mut backward = Vec::new();
        let mut from = text.chars().count();
        while let Some(previous) = matcher.find_next(&text, from, true, false).unwrap() {
            backward.push((previous.found.start, previous.found.end));
            from = previous.found.start;
        }
        backward.reverse();
        prop_assert_eq!(backward, all);
    }
}

/// A log like `tools/gen-fixtures.py`'s `search-1m.log`: timestamps, levels, workers,
/// requests and `user=<name>` on every line.
fn generate_log(lines: usize) -> String {
    const NAMES: [&str; 14] = [
        "alice", "bob", "carol", "dave", "erin", "frank", "grace", "heidi", "ivan", "judy",
        "mallory", "oscar", "peggy", "trent",
    ];
    const LEVELS: [&str; 5] = ["TRACE", "DEBUG", "INFO ", "WARN ", "ERROR"];
    const WORDS: [&str; 8] = [
        "buffer", "cursor", "search", "replace", "session", "backup", "theme", "window",
    ];
    let mut state: u64 = 20_260_930;
    let mut next = move |bound: u64| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state % bound
    };
    let mut text = String::with_capacity(lines * 150);
    let mut millis = 0;
    for _ in 0..lines {
        millis += next(40);
        let level = LEVELS[next(5) as usize];
        text.push_str(&format!(
            "2026-09-30T{:02}:{:02}:{:02}.{:03}Z {level} [worker-{:02}] GET /api/v1/{}/{} status=200 duration_ms={}.{} ip=10.{}.{}.{} user={} msg=\"{} {}\"\n",
            millis / 3_600_000 % 24,
            millis / 60_000 % 60,
            millis / 1000 % 60,
            millis % 1000,
            next(16),
            WORDS[next(8) as usize],
            next(99_999) + 1,
            next(900),
            next(10),
            next(256),
            next(256),
            next(254) + 1,
            NAMES[next(14) as usize],
            WORDS[next(8) as usize],
            WORDS[next(8) as usize],
        ));
    }
    text
}

/// `user=(\w+)` → `user=\U\1\E` without our engine.
fn uppercase_users(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("user=") {
        out.push_str(&rest[..at + 5]);
        rest = &rest[at + 5..];
        let name_len = rest
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        out.push_str(&rest[..name_len].to_uppercase());
        rest = &rest[name_len..];
    }
    out.push_str(rest);
    out
}

#[test]
#[ignore = "benchmark: cargo test --release -p stet-infrastructure -- --ignored --nocapture"]
fn replace_all_over_a_million_log_lines() {
    let started = Instant::now();
    let text = match std::env::var_os("STET_SEARCH_LOG") {
        Some(path) => std::fs::read_to_string(path).unwrap(),
        None => generate_log(1_000_000),
    };
    let generated = started.elapsed();
    let cancel = AtomicBool::new(false);
    let query = Query::regex(r"user=(\w+)");
    let template = Template::parse(r"user=\U\1\E");

    let started = Instant::now();
    let matcher = Matcher::new(&query).unwrap();
    let count = matcher
        .find_all(&text, None, usize::MAX, &cancel)
        .unwrap()
        .matches
        .len();
    let counted = started.elapsed();

    let started = Instant::now();
    let result = matcher
        .replace_all(&text, &template, None, &cancel)
        .unwrap();
    let replaced = started.elapsed();

    let started = Instant::now();
    let edit = result.as_single_edit(&text).unwrap();
    let single = started.elapsed();

    let started = Instant::now();
    let applied = apply(&text, vec![edit]).unwrap();
    let applied_in = started.elapsed();

    assert_eq!(count, 1_000_000);
    assert_eq!(result.count, 1_000_000);
    assert_eq!(applied, uppercase_users(&text));
    println!(
        "1M-line log: {} bytes, {} lines (generated in {generated:?})\n\
         find_all: {count} matches in {counted:?}\n\
         replace_all (user=(\\w+) -> user=\\U\\1\\E): {} edits in {replaced:?}\n\
         as_single_edit: {single:?}; applying the bulk edit to a String: {applied_in:?}",
        text.len(),
        text.lines().count(),
        result.edits.len(),
    );
}
