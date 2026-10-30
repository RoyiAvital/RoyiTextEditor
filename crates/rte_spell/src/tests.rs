use std::time::{Duration, Instant};

use crate::*;

fn found(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    words(text.as_bytes(), 0, &mut out);
    out.iter()
        .map(|w| &text[w.start as usize..w.end as usize])
        .collect()
}

#[test]
fn words_skip_code_like_tokens_and_acronyms() {
    assert_eq!(found("Hello, wrold!"), ["Hello", "wrold"]);
    assert_eq!(
        found("don't 'quoted' it's John's"),
        ["don't", "quoted", "it", "John"]
    );
    assert_eq!(found("don\u{2019}t"), ["don\u{2019}t"]);
    assert_eq!(found("well-known e.g."), ["well", "known", "e", "g"]);
    assert_eq!(found("abc123 foo_bar NASA OK a"), ["a"]);
    assert_eq!(
        found("see https://x.org/a mail@x.org C:\\dir a/b now"),
        ["see", "now"]
    );
    assert_eq!(found(&"x".repeat(MAX_WORD_BYTES + 1)), Vec::<&str>::new());
    assert_eq!(found("naïve café"), ["naïve", "café"]);
    let mut out = Vec::new();
    words(b"ab \xFF cd", 10, &mut out);
    assert_eq!(
        out,
        [Word { start: 10, end: 12 }, Word { start: 15, end: 17 }]
    );
}

#[test]
fn builtin_checks_case_and_inflections() {
    let mut b = Builtin::new();
    assert!(b.len() > 100_000, "{}", b.len());
    for w in [
        "the",
        "The",
        "receive",
        "received",
        "receiving",
        "don't",
        "Paris",
        "unhappiness",
    ] {
        assert!(b.check(w).unwrap(), "{w}");
    }
    for w in ["recieve", "teh", "paris", "Recieve"] {
        assert!(!b.check(w).unwrap(), "{w}");
    }
}

#[test]
fn builtin_suggests_close_words_in_the_case_of_the_word() {
    let mut b = Builtin::new();
    let far = Instant::now() + Duration::from_secs(5);
    let s = b.suggest("recieve", 8, far).unwrap();
    assert_eq!(s.first().map(String::as_str), Some("receive"), "{s:?}");
    // A missing letter ranks above a wrong one.
    let s = b.suggest("mispelled", 8, far).unwrap();
    assert_eq!(s.first().map(String::as_str), Some("misspelled"), "{s:?}");
    let s = b.suggest("Teh", 8, far).unwrap();
    assert!(s.contains(&"The".to_owned()), "{s:?}");
    // Distance 2 within the deadline; nothing past an expired one.
    let s = b.suggest("acomodate", 8, far).unwrap();
    assert!(s.contains(&"accommodate".to_owned()), "{s:?}");
    assert!(s.len() <= 8);
    let late = b.suggest("acomodate", 8, Instant::now()).unwrap();
    assert!(!late.contains(&"accommodate".to_owned()));
}

#[test]
fn checker_applies_overrides_cache_and_non_ascii_rule() {
    let mut c = Checker::new(Box::new(Builtin::new()));
    assert!(!c.is_correct("Royi").unwrap());
    c.add_user("Royi");
    assert!(c.is_correct("Royi").unwrap());
    assert!(!c.is_correct("wrold").unwrap());
    c.ignore("wrold");
    assert!(c.is_correct("wrold").unwrap());
    assert!(
        c.is_correct("Zürichzz").unwrap(),
        "unknown non-ASCII is not reported"
    );
    assert!(c.is_correct("don\u{2019}t").unwrap());
    assert!(c.cached() > 0);
}

/// Added and ignored words match in either apostrophe form.
#[test]
fn overrides_match_both_apostrophes() {
    let mut c = Checker::new(Box::new(Builtin::new()));
    c.add_user("zzq\u{2019}zzq");
    c.ignore("qqz'qqz");
    for word in ["zzq\u{2019}zzq", "zzq'zzq", "qqz'qqz", "qqz\u{2019}qqz"] {
        assert!(c.is_correct(word).unwrap(), "{word}");
    }
}

/// An engine that fails or is slow, to test the callers' fallbacks.
pub struct Failing;

impl Speller for Failing {
    fn name(&self) -> &'static str {
        "failing"
    }
    fn check(&mut self, _: &str) -> Result<bool, String> {
        Err("engine failed".to_owned())
    }
    fn suggest(&mut self, _: &str, _: usize, _: Instant) -> Result<Vec<String>, String> {
        Err("engine failed".to_owned())
    }
}

#[test]
fn engine_errors_reach_the_caller() {
    let mut c = Checker::new(Box::new(Failing));
    assert!(c.is_correct("word").is_err());
    c.set_engine(Box::new(Builtin::new()));
    assert!(c.is_correct("word").unwrap());
}

#[test]
fn checker_cache_is_bounded() {
    let mut c = Checker::new(Box::new(Builtin::new()));
    for i in 0..CACHE_MAX_WORDS + 10 {
        let w: String = format!("{i:x}")
            .chars()
            .map(|c| (b'g' + c as u8 % 16) as char)
            .collect();
        c.is_correct(&w).unwrap();
    }
    assert!(c.cached() <= CACHE_MAX_WORDS);
}

const AFF: &str = "SET UTF-8\nTRY esianrtolcdugmphbyfvkwz\nSFX S Y 1\nSFX S 0 s .\n";
const DIC: &str = "3\nhello/S\nworld/S\nParis\n";

#[test]
fn hunspell_checks_affixes_and_suggests() {
    let mut h = Hunspell::new(AFF, DIC).unwrap();
    assert!(h.check("hello").unwrap());
    assert!(h.check("worlds").unwrap());
    assert!(h.check("Hello").unwrap());
    assert!(!h.check("wrold").unwrap());
    let s = h.suggest("wrold", 8, Instant::now()).unwrap();
    assert!(s.contains(&"world".to_owned()), "{s:?}");
}

#[test]
fn hunspell_reports_missing_and_oversized_files() {
    let dir = std::env::temp_dir().join(format!("rte_spell_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let base = dir.join("en_XX");
    assert!(Hunspell::load(&base).is_err(), "missing");
    std::fs::write(dir.join("en_XX.aff"), AFF).unwrap();
    std::fs::write(dir.join("en_XX.dic"), DIC).unwrap();
    assert!(Hunspell::load(&base).is_ok());
    let big = vec![b'a'; (MAX_HUNSPELL_BYTES + 1) as usize];
    std::fs::write(dir.join("en_XX.dic"), big).unwrap();
    assert!(Hunspell::load(&base).err().unwrap().contains("16 MiB"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn shipped_hunspell_dictionary_agrees_on_common_words() {
    let base =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/dictionaries/en_US");
    let mut h = Hunspell::load(&base).unwrap();
    for w in ["receive", "unhappiness", "Paris", "don't"] {
        assert!(h.check(w).unwrap(), "{w}");
    }
    assert!(!h.check("recieve").unwrap());
}

#[test]
fn user_dictionary_limits_and_merge() {
    assert_eq!(
        parse_user(b"b\r\na\n\nb\n"),
        Some(vec!["a".to_owned(), "b".to_owned()])
    );
    assert_eq!(
        parse_user("\u{FEFF}x\n".as_bytes()),
        Some(vec!["x".to_owned()])
    );
    assert_eq!(parse_user(b"two words\n"), None);
    assert_eq!(parse_user(&[0xFF]), None);
    assert_eq!(parse_user(&vec![b'a'; USER_MAX_BYTES + 1]), None);
    let letter = |i: usize| (b'a' + (i % 26) as u8) as char;
    let many: Vec<String> = (0..USER_MAX_WORDS)
        .map(|i| {
            [letter(i / 676), letter(i / 26), letter(i)]
                .iter()
                .collect()
        })
        .collect();
    assert!(encode_user(&many).is_some());
    let more = merge_user(many.clone(), &["zz".to_owned()]);
    assert_eq!(encode_user(&more), None, "over the word limit");
    let long: Vec<String> = (0..1100).map(|i| format!("{i:0>60}")).collect();
    assert_eq!(encode_user(&long), None, "over the byte limit");
    let merged = merge_user(vec!["b".to_owned()], &["a".to_owned(), "b".to_owned()]);
    assert_eq!(merged, ["a", "b"]);
    let text = encode_user(&merged).unwrap();
    assert_eq!(parse_user(&text), Some(merged));
}
