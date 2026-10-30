#![allow(clippy::single_range_in_vec_init)]

use super::*;
use crate::cursor::tests::split;
use proptest::prelude::*;

fn never() -> bool {
    false
}

fn query(text: &str, mode: Mode, match_case: bool, whole_word: bool) -> Query {
    Query {
        text: text.to_owned(),
        mode,
        match_case,
        whole_word,
    }
}

fn all(q: &Query, s: &Snapshot) -> Vec<Range<u64>> {
    let m = Matcher::new(q).unwrap().unwrap();
    let mut searcher = Searcher::new(&m, s, &never);
    let mut out = Vec::new();
    searcher.for_each(0..s.len(), |r| {
        out.push(r);
        true
    });
    out
}

fn texts(q: &Query, text: &str) -> Vec<String> {
    let s = Snapshot::from_bytes(text.as_bytes());
    all(q, &s)
        .into_iter()
        .map(|r| text[r.start as usize..r.end as usize].to_owned())
        .collect()
}

#[test]
fn empty_query_compiles_to_nothing() {
    assert!(Matcher::new(&Query::default()).unwrap().is_none());
}

#[test]
fn text_mode_is_literal_and_case_aware() {
    let text = "Cat cat CAT concatenate";
    assert_eq!(
        texts(&query("cat", Mode::Text, true, false), text),
        ["cat", "cat"]
    );
    assert_eq!(
        texts(&query("cat", Mode::Text, false, false), text).len(),
        4
    );
    assert_eq!(
        texts(&query("a.c", Mode::Text, false, false), "abc a.c"),
        ["a.c"]
    );
    assert!(
        Matcher::new(&query("x", Mode::Text, true, false))
            .unwrap()
            .unwrap()
            .is_literal()
    );
}

#[test]
fn case_insensitive_text_is_unicode_aware() {
    assert_eq!(
        texts(&query("straße", Mode::Text, false, false), "STRASSE Straße"),
        ["Straße"]
    );
    assert_eq!(
        texts(&query("ÉCOLE", Mode::Text, false, false), "école"),
        ["école"]
    );
}

#[test]
fn whole_word_uses_word_characters() {
    let q = query("cat", Mode::Text, true, true);
    assert_eq!(
        texts(&q, "cat concat cat_ (cat) cat"),
        ["cat", "cat", "cat"]
    );
    let q = query("-x", Mode::Text, true, true);
    assert_eq!(texts(&q, "a-x b -xy"), ["-x"]);
    // A combining accent belongs to its word: "cafe" + U+0301 is not "cafe".
    let q = query("cafe", Mode::Text, true, true);
    assert_eq!(texts(&q, "cafe\u{301} cafe"), ["cafe"]);
    assert_eq!(
        all(&q, &Snapshot::from_bytes("cafe\u{301} cafe".as_bytes())),
        [7..11]
    );
}

#[test]
fn literal_matches_across_leaf_boundaries() {
    let text = b"xxabcxxabcabc";
    for cut in 1..text.len() {
        let s = split(text, &[cut]);
        let q = query("abc", Mode::Text, true, false);
        assert_eq!(all(&q, &s), [2..5, 7..10, 10..13], "cut {cut}");
    }
}

#[test]
fn regex_matches_across_leaf_boundaries_and_lines() {
    let text = "foo bar\nbaz €uro\r\nend".as_bytes();
    for cut in 1..text.len() {
        let s = split(text, &[cut, (cut + 3).min(text.len())]);
        let q = query(r"\w+$", Mode::Regex, true, false);
        let got: Vec<_> = all(&q, &s);
        assert_eq!(got, [4..7, 15..18, 20..23], "cut {cut}");
        let q = query(r"bar\nbaz", Mode::Regex, true, false);
        assert_eq!(all(&q, &s), [4..11], "cut {cut}");
    }
}

#[test]
fn glob_star_is_shortest_within_a_line() {
    let q = query("foo*bar", Mode::Glob, true, false);
    assert_eq!(
        texts(&q, "foo bar foo bar\nfoo\nbar"),
        ["foo bar", "foo bar"]
    );
    let q = query("TODO*", Mode::Glob, true, false);
    assert_eq!(texts(&q, "a TODO: fix\r\nTODO"), ["TODO: fix", "TODO"]);
    let q = query("b?d", Mode::Glob, false, false);
    assert_eq!(texts(&q, "bad BED b\nd"), ["bad", "BED"]);
    let q = query("[!a]x", Mode::Glob, true, false);
    assert_eq!(texts(&q, "ax bx"), ["bx"]);
}

#[test]
fn lookaround_and_backreferences_use_the_backtracking_engine() {
    let q = query(r"foo(?=bar)", Mode::Regex, true, false);
    assert_eq!(texts(&q, "foobar foobaz"), ["foo"]);
    let q = query(r"(?<!x)y", Mode::Regex, true, false);
    assert_eq!(all(&q, &Snapshot::from_bytes(b"xy y")), [3..4]);
    let q = query(r"(\w+)\s+\1", Mode::Regex, true, false);
    assert_eq!(texts(&q, "the the cat\nis is"), ["the the", "is is"]);
}

#[test]
fn backtracking_skips_invalid_lines_and_reports_complex_ones() {
    let q = query(r"(?=a)a", Mode::Regex, true, false);
    let m = Matcher::new(&q).unwrap().unwrap();
    let s = Snapshot::from_bytes(b"a\n\xFFa\na");
    let mut searcher = Searcher::new(&m, &s, &never);
    let mut found = Vec::new();
    searcher.for_each(0..s.len(), |r| {
        found.push(r);
        true
    });
    assert_eq!(found, [0..1, 5..6]);
    assert_eq!(searcher.skipped_lines, 1);

    let q = query(r"(?:(?=a)a|a)+b", Mode::Regex, true, false);
    let m = Matcher::new(&q).unwrap().unwrap();
    let text = format!("{}c", "a".repeat(40));
    let s = Snapshot::from_bytes(text.as_bytes());
    let mut searcher = Searcher::new(&m, &s, &never);
    assert_eq!(searcher.find(0, s.len()), None);
    assert!(searcher.complex_lines >= 1);
}

#[test]
fn invalid_patterns_report_one_line() {
    let err = Matcher::new(&query(r"(\w+_cfg", Mode::Regex, true, false))
        .err()
        .unwrap();
    assert!(!err.contains('\n'), "{err}");
    assert!(err.to_lowercase().contains("unclosed"), "{err}");
    let err = Matcher::new(&query(r"(?<=a", Mode::Regex, true, false))
        .err()
        .unwrap();
    assert!(!err.contains('\n'), "{err}");
}

#[test]
fn zero_width_matches_advance() {
    let q = query(r"x*", Mode::Regex, true, false);
    let s = Snapshot::from_bytes("aé".as_bytes());
    assert_eq!(all(&q, &s), [0..0, 1..1, 3..3]);
    let q = query(r"a*", Mode::Regex, true, false);
    let s = Snapshot::from_bytes(b"aab");
    assert_eq!(all(&q, &s), [0..2, 3..3]);
}

#[test]
fn find_prev_returns_the_last_match_before() {
    let text = "a1 a2 a3".repeat(30_000);
    let s = Snapshot::from_bytes(text.as_bytes());
    let q = query(r"a\d", Mode::Regex, true, false);
    let m = Matcher::new(&q).unwrap().unwrap();
    let mut searcher = Searcher::new(&m, &s, &never);
    let end = s.len();
    assert_eq!(searcher.find_prev(end, 0), Some(end - 2..end));
    assert_eq!(searcher.find_prev(4, 0), Some(0..2));
    assert_eq!(searcher.find_prev(1, 0), None);
    let lone = Snapshot::from_bytes(format!("zz{}", " ".repeat(500_000)).as_bytes());
    let q = query("zz", Mode::Text, true, false);
    let m = Matcher::new(&q).unwrap().unwrap();
    let mut searcher = Searcher::new(&m, &lone, &never);
    assert_eq!(searcher.find_prev(lone.len(), 0), Some(0..2));
}

#[test]
fn captures_and_expand_check_the_exact_match() {
    let s = Snapshot::from_bytes(b"let user_cfg = read_cfg();");
    let q = query(r"(\w+)_cfg\b", Mode::Regex, true, false);
    let m = Matcher::new(&q).unwrap().unwrap();
    let mut searcher = Searcher::new(&m, &s, &never);
    let rep = Replacement::parse("$1_config");
    assert_eq!(
        searcher.expand(&rep, 4..12, false),
        Some(b"user_config".to_vec())
    );
    assert_eq!(searcher.expand(&rep, 0..12, false), None);
    assert_eq!(searcher.expand(&rep, 4..11, false), None);

    let q = query(r"(?<w>\w+)(?=\()", Mode::Regex, true, false);
    let m = Matcher::new(&q).unwrap().unwrap();
    let mut searcher = Searcher::new(&m, &s, &never);
    let rep = Replacement::parse(r"\U$<w>");
    assert_eq!(
        searcher.expand(&rep, 15..23, false),
        Some(b"READ_CFG".to_vec())
    );

    let q = query("CFG", Mode::Text, false, false);
    let m = Matcher::new(&q).unwrap().unwrap();
    let mut searcher = Searcher::new(&m, &s, &never);
    let rep = Replacement::literal("conf");
    assert_eq!(searcher.expand(&rep, 9..12, true), Some(b"conf".to_vec()));
}

#[test]
fn find_prev_keeps_a_long_match_that_begins_before_its_window() {
    let s = Snapshot::from_bytes(&vec![b'a'; 100_000]);
    let q = query("a+", Mode::Regex, true, false);
    let m = Matcher::new(&q).unwrap().unwrap();
    let mut searcher = Searcher::new(&m, &s, &never);
    assert_eq!(searcher.find_prev(s.len(), 0), Some(0..100_000));
    let text = format!("{}\nb{}", "x".repeat(70_000), "a".repeat(70_000));
    let s = Snapshot::from_bytes(text.as_bytes());
    let q = query(r"b\w+|a+", Mode::Regex, true, false);
    let m = Matcher::new(&q).unwrap().unwrap();
    let mut searcher = Searcher::new(&m, &s, &never);
    let forward = all(&q, &s);
    assert_eq!(searcher.find_prev(s.len(), 0), forward.last().cloned());
}

#[test]
fn look_behind_sees_previous_lines_in_find_and_replace() {
    let s = Snapshot::from_bytes(b"a\nb\nc\nb");
    let q = query(r"(?<=a\n)b", Mode::Regex, true, false);
    assert_eq!(all(&q, &s), [2..3]);
    let m = Matcher::new(&q).unwrap().unwrap();
    let mut searcher = Searcher::new(&m, &s, &never);
    let rep = Replacement::parse("[$0]");
    assert_eq!(searcher.expand(&rep, 2..3, false), Some(b"[b]".to_vec()));
    assert_eq!(searcher.expand(&rep, 6..7, false), None);
    let q = query(r"(?<!a\n)b", Mode::Regex, true, false);
    assert_eq!(all(&q, &s), [6..7]);
    // Single-line patterns see the preceding line break.
    let q = query(r"(?<=\n)c(?=\n)", Mode::Regex, true, false);
    assert_eq!(all(&q, &s), [4..5]);
}

#[test]
fn a_match_iterator_resumes_with_a_later_end() {
    let s = Snapshot::from_bytes(b"ab ab ab");
    let q = query("ab", Mode::Text, true, false);
    let m = Matcher::new(&q).unwrap().unwrap();
    let mut searcher = Searcher::new(&m, &s, &never);
    let mut it = MatchIter::new(0..4);
    assert_eq!(searcher.next_match(&mut it), Some(0..2));
    assert_eq!(searcher.next_match(&mut it), None);
    it.end = s.len();
    assert_eq!(searcher.next_match(&mut it), Some(3..5));
    assert_eq!(searcher.next_match(&mut it), Some(6..8));
    let q = query("^", Mode::Regex, true, false);
    let m = Matcher::new(&q).unwrap().unwrap();
    let s = Snapshot::from_bytes(b"a\nb");
    let mut searcher = Searcher::new(&m, &s, &never);
    let mut it = MatchIter::new(0..1);
    assert_eq!(searcher.next_match(&mut it), Some(0..0));
    assert_eq!(searcher.next_match(&mut it), None);
    it.end = s.len();
    assert_eq!(searcher.next_match(&mut it), Some(2..2));
}

#[test]
fn trusted_expansion_skips_verification_only_without_groups() {
    let s = Snapshot::from_bytes(b"cat cat");
    let q = query("cat", Mode::Text, true, false);
    let m = Matcher::new(&q).unwrap().unwrap();
    let mut searcher = Searcher::new(&m, &s, &never);
    let rep = Replacement::literal("dog");
    let big = u64::MAX;
    assert_eq!(
        searcher.expand_within(&rep, 4..7, false, big),
        Expansion::Text(b"dog".to_vec())
    );
    let q = query(r"(c)at", Mode::Regex, true, false);
    let m = Matcher::new(&q).unwrap().unwrap();
    let mut searcher = Searcher::new(&m, &s, &never);
    let rep = Replacement::parse("$1");
    assert_eq!(
        searcher.expand_within(&rep, 1..4, false, big),
        Expansion::NotAMatch
    );
    assert_eq!(
        searcher.expand_within(&rep, 4..7, false, big),
        Expansion::Text(b"c".to_vec())
    );
}

#[test]
fn expansion_is_refused_before_copying_beyond_its_budget() {
    let s = Snapshot::from_bytes(&vec![b'a'; 1 << 16]);
    let q = query("a+", Mode::Regex, true, false);
    let m = Matcher::new(&q).unwrap().unwrap();
    let mut searcher = Searcher::new(&m, &s, &never);
    let all = 0..s.len();
    let twice = Replacement::parse("$&$&");
    assert_eq!(
        searcher.expand_within(&twice, all.clone(), false, 1 << 17),
        Expansion::OverBudget
    );
    assert!(matches!(
        searcher.expand_within(&twice, all.clone(), false, 1 << 18),
        Expansion::Text(t) if t.len() == 1 << 17
    ));
    let upper = Replacement::parse(r"\U$&");
    assert_eq!(
        searcher.expand_within(&upper, all, false, 1 << 18),
        Expansion::OverBudget,
        "case conversion may triple the text"
    );
    assert_eq!(Replacement::parse(r"x\U$1y").bound(&mut |_| Some(10)), 32);
}

#[test]
fn captures_stay_inside_the_search_end() {
    let s = Snapshot::from_bytes(b"aaaa");
    let q = query("a+", Mode::Regex, true, false);
    let m = Matcher::new(&q).unwrap().unwrap();
    let mut searcher = Searcher::new(&m, &s, &never);
    let rep = Replacement::parse("[$&]");
    assert_eq!(
        searcher.expand(&rep, 0..2, false),
        None,
        "not a match of the whole text"
    );
    searcher.set_end(2);
    assert_eq!(searcher.find(0, 4), Some(0..2));
    assert_eq!(searcher.expand(&rep, 0..2, false), Some(b"[aa]".to_vec()));
    let q = query(r"(?=a)a+", Mode::Regex, true, false);
    let m = Matcher::new(&q).unwrap().unwrap();
    let mut searcher = Searcher::new(&m, &s, &never);
    searcher.set_end(2);
    assert_eq!(
        searcher.find(0, 4),
        None,
        "the backtracking match is longer"
    );
}

#[test]
fn backward_search_keeps_the_forward_partition() {
    let s = Snapshot::from_bytes(&vec![b'a'; 300_001]);
    for q in [
        query("aa", Mode::Text, true, false),
        query("aa", Mode::Text, false, false),
    ] {
        let m = Matcher::new(&q).unwrap().unwrap();
        assert!(!m.never_overlaps());
        let mut searcher = Searcher::new(&m, &s, &never);
        assert_eq!(searcher.find_prev(s.len(), 0), Some(299_998..300_000));
    }
    let q = query("ab", Mode::Text, true, false);
    assert!(Matcher::new(&q).unwrap().unwrap().never_overlaps());
}

/// Timings for the review's dense cases (`cargo test --release -p rte_search
/// -- --ignored --nocapture profile`); not a pass/fail speed gate.
#[test]
#[ignore]
fn profile_dense_cases() {
    let median = |f: &mut dyn FnMut()| {
        let mut ms: Vec<f64> = (0..9)
            .map(|_| {
                let t = std::time::Instant::now();
                f();
                t.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        ms.sort_by(f64::total_cmp);
        ms[4]
    };
    for kib in [16, 32, 64] {
        let s = Snapshot::from_bytes(&vec![b'a'; kib * 1024]);
        let q = query("(?=a)", Mode::Regex, true, false);
        let ms = median(&mut || assert_eq!(all(&q, &s).len(), kib * 1024));
        println!("profile.look_ahead_{kib}KiB_ms={ms:.2}");
    }
    let s = Snapshot::from_bytes(&vec![b'a'; 300_001]);
    for (name, q) in [
        ("literal", query("aa", Mode::Text, true, false)),
        ("regex", query("a{2}", Mode::Regex, true, false)),
    ] {
        let m = Matcher::new(&q).unwrap().unwrap();
        let ms = median(&mut || {
            let mut searcher = Searcher::new(&m, &s, &never);
            assert_eq!(searcher.find_prev(s.len(), 0), Some(299_998..300_000));
        });
        println!("profile.find_prev_{name}_300KB_ms={ms:.2}");
    }
}

#[test]
fn dense_look_around_reads_each_line_once() {
    let s = Snapshot::from_bytes(&vec![b'a'; 64 * 1024]);
    let q = query("(?=a)", Mode::Regex, true, false);
    let started = std::time::Instant::now();
    assert_eq!(all(&q, &s).len(), 64 * 1024);
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    let s = Snapshot::from_bytes(b"a\xFFb\nab\nab");
    let q = query("(?=a)a", Mode::Regex, true, false);
    let m = Matcher::new(&q).unwrap().unwrap();
    let mut searcher = Searcher::new(&m, &s, &never);
    let mut found = Vec::new();
    searcher.for_each(0..s.len(), |r| {
        found.push(r);
        true
    });
    assert_eq!(found, [4..5, 7..8]);
    assert_eq!(searcher.skipped_lines, 1);
    let q = query(r"(?<=\n)a(?=b\n)", Mode::Regex, true, false);
    assert_eq!(
        all(&q, &s),
        [4..5],
        "multiline windows leave out invalid lines"
    );
}

#[test]
fn stop_ends_a_scan() {
    let s = Snapshot::from_bytes(&vec![b'a'; 1 << 20]);
    for q in [
        query("b", Mode::Text, true, false),
        query("b", Mode::Text, false, false),
        query("(?=b)b", Mode::Regex, true, false),
    ] {
        let m = Matcher::new(&q).unwrap().unwrap();
        let stop = || true;
        let mut searcher = Searcher::new(&m, &s, &stop);
        assert_eq!(searcher.find(0, s.len()), None);
        assert!(searcher.stopped());
    }
}

fn naive(hay: &[u8], needle: &[u8]) -> Vec<Range<u64>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        if &hay[i..i + needle.len()] == needle {
            out.push(i as u64..(i + needle.len()) as u64);
            i += needle.len();
        } else {
            i += 1;
        }
    }
    out
}

proptest! {
    #[test]
    fn literal_and_case_insensitive_text_match_a_naive_scan(
        hay in proptest::collection::vec(prop_oneof![Just(b'a'), Just(b'b'), Just(b'\n')], 0..200),
        needle in proptest::collection::vec(prop_oneof![Just(b'a'), Just(b'b')], 1..4),
        cuts in proptest::collection::vec(0usize..200, 0..6),
    ) {
        let mut cuts: Vec<usize> = cuts.into_iter().filter(|&c| c > 0 && c < hay.len()).collect();
        cuts.sort_unstable();
        cuts.dedup();
        let s = split(&hay, &cuts);
        let text = String::from_utf8(needle.clone()).unwrap();
        let expected = naive(&hay, &needle);
        prop_assert_eq!(all(&query(&text, Mode::Text, true, false), &s), expected.clone());
        prop_assert_eq!(all(&query(&text, Mode::Text, false, false), &s), expected.clone());
        prop_assert_eq!(all(&query(&text, Mode::Glob, true, false), &s), expected);
    }
}
