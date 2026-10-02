use std::time::Instant;

use proptest::prelude::*;

use super::*;

fn positions(query: &str, candidate: &str) -> Option<Vec<usize>> {
    score(query, candidate).map(|found| found.positions)
}

fn points(query: &str, candidate: &str) -> i32 {
    score(query, candidate).expect("a match").score
}

fn ranked<'a>(query: &str, candidates: &[&'a str]) -> Vec<&'a str> {
    rank(query, candidates, candidates.len())
        .into_iter()
        .map(|found| candidates[found.index])
        .collect()
}

#[test]
fn matches_characters_in_order() {
    assert_eq!(positions("mn", "main"), Some(vec![0, 3]));
    assert_eq!(positions("m a", "main"), Some(vec![0, 1]));
    assert_eq!(positions("nm", "main"), None);
    assert_eq!(positions("mainly", "main"), None);
    assert_eq!(
        score(" ", "anything"),
        Some(Match {
            score: 0,
            positions: Vec::new(),
        })
    );
}

#[test]
fn case_is_smart() {
    assert!(score("main", "src/Main.rs").is_some());
    assert!(score("Main", "src/Main.rs").is_some());
    assert!(score("Main", "src/main.rs").is_none());
    assert_eq!(positions("æø", "XÆYØ"), Some(vec![1, 3]));
    assert_eq!(positions("ÆØ", "xæyø"), None);
}

#[test]
fn runs_and_word_starts_score_higher() {
    assert!(points("abc", "xabcx") > points("abc", "xaxbxcx"));
    assert!(points("fb", "foo_bar") > points("fb", "xfxbx"));
    assert!(points("fb", "fooBar") > points("fb", "xfxbx"));
    assert!(points("fb", "foo bar") > points("fb", "foo_bar"));
    assert!(points("st", "src/stet.rs") > points("st", "src/best.rs"));
    assert!(points("ma", "my_app") > points("ma", "xmxa"));
}

#[test]
fn file_names_rank_above_directories() {
    let paths = [
        "main/src/lib.rs",
        "domain/mainly/x.rs",
        "src/domain.rs",
        "src/main.rs",
    ];
    assert_eq!(
        ranked("main", &paths),
        [
            "src/main.rs",
            "src/domain.rs",
            "main/src/lib.rs",
            "domain/mainly/x.rs",
        ]
    );
    assert_eq!(ranked("lib", &["lib/a.rs", "x/lib.rs"])[0], "x/lib.rs");
}

#[test]
fn a_slash_in_the_query_matches_across_directories() {
    assert_eq!(positions("src/m", "src/main.rs"), Some(vec![0, 1, 2, 3, 4]));
    assert!(score("d/x", "domain/mainly/x.rs").is_some());
    assert!(score("d/x", "x.rs").is_none());
}

#[test]
fn palette_labels_match_word_starts() {
    let labels = [
        "Toggle Bookmark",
        "Go to Line…",
        "Close Tab",
        "Next Bookmark",
    ];
    assert_eq!(ranked("gtl", &labels), ["Go to Line…"]);
    assert_eq!(
        ranked("bookm", &labels),
        ["Next Bookmark", "Toggle Bookmark"]
    );
    assert_eq!(ranked("ct", &labels)[0], "Close Tab");
}

#[test]
fn ties_prefer_shorter_then_earlier_candidates() {
    let found = rank("ab", &["xab", "zab", "ab-long", "ab"], 3);
    let indexes: Vec<usize> = found.iter().map(|found| found.index).collect();
    assert_eq!(indexes, [3, 2, 0]);
    assert!(found.windows(2).all(|pair| pair[0].score >= pair[1].score));
    assert_eq!(found[0].positions, [0, 1]);
    assert!(rank("ab", &["ab"], 0).is_empty());
}

#[test]
fn an_empty_query_keeps_the_given_order() {
    let found = rank("  ", &["b", "a", "c"], 2);
    let indexes: Vec<usize> = found.iter().map(|found| found.index).collect();
    assert_eq!(indexes, [0, 1]);
}

#[test]
fn very_long_candidates_fall_back_to_greedy_positions() {
    let long = format!("{}needle", "x".repeat(400_000));
    assert_eq!(
        positions("ndl", &long),
        Some(vec![400_000, 400_003, 400_004])
    );
}

proptest! {
    #[test]
    fn positions_spell_the_query(query in "[a-cA-C/_ ]{0,6}", candidate in "[a-cA-C/_.x]{0,24}") {
        let mut matcher = Matcher::new(&query);
        let found = matcher.score(&candidate);
        prop_assert_eq!(matcher.score_of(&candidate), found.as_ref().map(|found| found.score));
        let case_sensitive = query.chars().any(char::is_uppercase);
        let fold = |c: char| if case_sensitive { c } else { c.to_ascii_lowercase() };
        let wanted: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).map(fold).collect();
        let text: Vec<char> = candidate.chars().map(fold).collect();
        prop_assert_eq!(found.is_some(), is_subsequence(&wanted, &text));
        if let Some(found) = found {
            prop_assert_eq!(found.positions.len(), wanted.len());
            prop_assert!(found.positions.windows(2).all(|pair| pair[0] < pair[1]));
            for (&position, &c) in found.positions.iter().zip(&wanted) {
                prop_assert_eq!(text[position], c);
            }
        }
    }

    #[test]
    fn an_exact_file_name_outranks_a_scattered_match(
        query in "[a-m]{1,8}",
        dirs in prop::collection::vec("[a-z]{1,6}", 0..3),
        other_dirs in prop::collection::vec("[a-z]{1,6}", 0..3),
        fillers in prop::collection::vec("[n-z0-9_.-]{1,3}", 8),
    ) {
        let path = |dirs: &[String], name: &str| {
            dirs.iter().map(|dir| format!("{dir}/")).collect::<String>() + name
        };
        let exact = path(&dirs, &query);
        let scattered_name: String = query
            .chars()
            .zip(&fillers)
            .map(|(c, filler)| format!("{c}{filler}"))
            .collect();
        let scattered = path(&other_dirs, &scattered_name);
        prop_assert!(points(&query, &exact) > points(&query, &scattered));
        let found = rank(&query, &[scattered.as_str(), exact.as_str()], 1);
        prop_assert_eq!(found[0].index, 1);
    }
}

struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) % n as u64) as usize
    }
}

fn project_paths(count: usize) -> Vec<String> {
    const DIRS: [&str; 16] = [
        "src",
        "domain",
        "app",
        "infrastructure",
        "tests",
        "docs",
        "tools",
        "spikes",
        "widgets",
        "text",
        "search",
        "session",
        "vendor",
        "node_modules",
        "target",
        "assets",
    ];
    const NAMES: [&str; 16] = [
        "main",
        "lib",
        "mod",
        "window",
        "buffer",
        "index",
        "edit",
        "theme",
        "palette",
        "bookmarks",
        "compare",
        "history",
        "encoding",
        "session_store",
        "find_in_files",
        "README",
    ];
    const EXTENSIONS: [&str; 6] = ["rs", "md", "toml", "json", "txt", "js"];
    let mut rng = Lcg(3);
    (0..count)
        .map(|i| {
            let mut path = String::new();
            for _ in 0..1 + rng.below(5) {
                path.push_str(DIRS[rng.below(DIRS.len())]);
                path.push('/');
            }
            let name = NAMES[rng.below(NAMES.len())];
            let extension = EXTENSIONS[rng.below(EXTENSIONS.len())];
            path.push_str(&format!("{name}_{i}.{extension}"));
            path
        })
        .collect()
}

#[test]
#[ignore = "timing; run with --release -- --ignored --nocapture"]
fn times_ranking_100k_paths() {
    let paths = project_paths(100_000);
    for query in [
        "main",
        "srcmodrs",
        "e",
        "palette",
        "dombuf",
        "zzzz",
        "READ",
        "tests/idx",
    ] {
        let started = Instant::now();
        let found = rank(query, &paths, 50);
        let elapsed = started.elapsed();
        println!(
            "rank {query:?} over {} paths: {elapsed:?}, best {:?}",
            paths.len(),
            found.first().map(|found| &paths[found.index])
        );
        assert!(elapsed.as_millis() < 500, "{elapsed:?}");
    }
}
