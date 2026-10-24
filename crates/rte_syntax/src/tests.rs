use std::path::Path;

use rte_text::Snapshot;

use crate::session::{Painter, Session, Work};
use crate::{Grammar, LANGUAGES, Span, Style, by_fence, by_id, detect, highlighter};

/// A failed tree-sitter allocation aborts the process (no null result, no
/// unwinding into C); checked in a child run of this test.
#[test]
fn a_failed_allocation_aborts() {
    if std::env::var_os("RTE_ALLOCATION_CHILD").is_some() {
        crate::alloc::checked(std::ptr::null_mut(), 16);
        return;
    }
    assert!(crate::alloc::checked(std::ptr::null_mut(), 0).is_null());
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::a_failed_allocation_aborts",
            "--nocapture",
        ])
        .env("RTE_ALLOCATION_CHILD", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_ne!(output.status.code(), Some(101), "a panic, not an abort");
}

#[test]
fn queries_are_generated_and_compile() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("queries");
    let files = crate::generate::generate().unwrap();
    for (path, content) in &files {
        let on_disk = std::fs::read_to_string(dir.join(path)).unwrap_or_default();
        assert_eq!(
            on_disk.replace("\r\n", "\n"),
            *content,
            "{path} is out of date: run `cargo xtask syntax-queries`"
        );
    }
    for grammar in Grammar::ALL {
        let hl = highlighter(grammar).unwrap();
        assert!(
            hl.tags().is_some() == crate::highlight::tags_source(grammar).is_some(),
            "{grammar:?} tags"
        );
    }
}

#[test]
fn detection() {
    let d = |name: &str, head: &str| detect(Path::new(name), head.as_bytes()).map(|l| l.id);
    assert_eq!(d("main.rs", ""), Some("rust"));
    assert_eq!(d("A.CPP", ""), Some("cpp"));
    assert_eq!(d("x.h", ""), Some("c"));
    assert_eq!(d("Cargo.lock", ""), Some("toml"));
    assert_eq!(d("notes.ipynb", "{}"), None);
    assert_eq!(d("data.csv", ""), None);
    assert_eq!(d("run", "#!/usr/bin/env python3.12\n"), Some("python"));
    assert_eq!(d("run", "#!/bin/bash -e\n"), Some("bash"));
    assert_eq!(
        d("f.m", "function y = f(x)\n  y = x;\nend\n"),
        Some("matlab")
    );
    assert_eq!(d("f.m", "# octave\nx = 1;\n"), Some("octave"));
    assert_eq!(d("f.m", "if a != b\n  disp(1)\nendif\n"), Some("octave"));
    assert_eq!(d("x.tsx", ""), Some("tsx"));
    assert_eq!(by_id("SHELLSCRIPT").map(|l| l.id), Some("bash"));
    assert_eq!(by_fence("py").map(|l| l.id), Some("python"));
    assert_eq!(by_fence("c++").map(|l| l.id), Some("cpp"));
    assert_eq!(by_fence("{.rust}").map(|l| l.id), Some("rust"));
    assert_eq!(LANGUAGES.len(), 24);
    for pair in LANGUAGES.windows(2) {
        assert!(pair[0].name.to_lowercase() < pair[1].name.to_lowercase());
    }
}

/// Parses `text` completely and returns its spans with their text.
fn spans(lang: &str, text: &str) -> Vec<(String, Style)> {
    let snapshot = Snapshot::from_bytes(text.as_bytes());
    let mut session = Session::new(by_id(lang).unwrap(), snapshot.clone(), true).unwrap();
    session.set_interest(0..snapshot.len());
    while session.work(&|| false) != Work::Idle {}
    let mut out = Vec::new();
    Painter::default().paint(
        &session.view(),
        &snapshot,
        0..snapshot.len(),
        None,
        &mut out,
    );
    out.iter()
        .map(|s: &Span| (text[s.start as usize..s.end as usize].to_string(), s.style))
        .collect()
}

fn has(spans: &[(String, Style)], text: &str, style: Style) -> bool {
    spans.iter().any(|(t, s)| t == text && *s == style)
}

#[test]
fn rust_highlights() {
    let s = spans(
        "rust",
        "fn main() {\n    let x = 42; // hi\n    println!(\"a\");\n}\n",
    );
    assert!(has(&s, "fn", Style::Keyword), "{s:?}");
    assert!(has(&s, "main", Style::Function), "{s:?}");
    assert!(has(&s, "42", Style::Number), "{s:?}");
    assert!(has(&s, "// hi", Style::Comment), "{s:?}");
    assert!(has(&s, "\"a\"", Style::String), "{s:?}");
}

/// Language, text and expected (text, style) spans.
type Case = (&'static str, &'static str, &'static [(&'static str, Style)]);

#[test]
fn every_language_highlights() {
    use Style::*;
    let cases: &[Case] = &[
        (
            "bash",
            "echo \"x\" # c\n",
            &[("echo", Function), ("# c", Comment)],
        ),
        (
            "c",
            "int main(void) { return 0; } // c\n",
            &[
                ("int", Type),
                ("return", Keyword),
                ("0", Number),
                ("// c", Comment),
            ],
        ),
        (
            "cpp",
            "class A { public: int x; };\n",
            &[("class", Keyword), ("int", Type)],
        ),
        (
            "csharp",
            "class A { void M() { var s = \"x\"; } }\n",
            &[("class", Keyword), ("\"x\"", String)],
        ),
        ("css", "a { color: red; }\n", &[("color", Property)]),
        (
            "go",
            "package main\nfunc main() { return }\n",
            &[("func", Keyword), ("main", Function)],
        ),
        (
            "html",
            "<p class=\"a\">x</p>\n",
            &[("p", Tag), ("class", Attribute)],
        ),
        (
            "java",
            "class A { int x = 1; }\n",
            &[("class", Keyword), ("1", Number)],
        ),
        (
            "javascript",
            "function f() { return 1; }\n",
            &[("function", Keyword), ("f", Function), ("1", Number)],
        ),
        (
            "json",
            "{\"a\": 1, \"b\": true}\n",
            &[("1", Number), ("true", Constant), ("\"a\"", Property)],
        ),
        (
            "julia",
            "function f(x)\n  return 1 # c\nend\n",
            &[("function", Keyword), ("# c", Comment)],
        ),
        (
            "lua",
            "local x = 1 -- c\n",
            &[("local", Keyword), ("-- c", Comment)],
        ),
        (
            "markdown",
            "# Title\n\nSome `code` here.\n",
            &[
                ("#", Punctuation),
                ("Title", Keyword),
                ("`", Punctuation),
                ("code", String),
            ],
        ),
        (
            "matlab",
            "function y = f(x)\n% c\ny = 1;\nend\n",
            &[("function", Keyword), ("% c", Comment)],
        ),
        (
            "octave",
            "function y = f(x)\n% c\ny = 1;\nend\n",
            &[("function", Keyword), ("% c", Comment)],
        ),
        (
            "powershell",
            "function F { Write-Host \"x\" }\n",
            &[("function", Keyword), ("\"x\"", String)],
        ),
        (
            "python",
            "def f(x):\n    return x.y(\"s\")  # c\n",
            &[
                ("def", Keyword),
                ("f", Function),
                ("y", Function),
                ("\"s\"", String),
                ("# c", Comment),
            ],
        ),
        (
            "rust",
            "fn f(v: Vec<u8>) { v.len(); }\n",
            &[("len", Function), ("Vec", Type), ("u8", Type)],
        ),
        (
            "sql",
            "SELECT a FROM t;\n",
            &[("SELECT", Keyword), ("FROM", Keyword)],
        ),
        ("toml", "[a]\nb = \"c\"\n", &[("\"c\"", String)]),
        ("tsx", "const a = <div>hi</div>;\n", &[("const", Keyword)]),
        (
            "typescript",
            "let x: number = 1;\n",
            &[("let", Keyword), ("number", Type)],
        ),
        ("xml", "<a b=\"c\">t</a>\n", &[("a", Tag)]),
        (
            "yaml",
            "a: 1\n# c\n",
            &[("1", Number), ("# c", Comment), ("a", Property)],
        ),
    ];
    let mut failures = Vec::new();
    for (lang, text, expected) in cases {
        let s = spans(lang, text);
        for (piece, style) in *expected {
            if !has(&s, piece, *style) {
                failures.push(format!("{lang}: {piece:?} {style:?} in {s:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn injections() {
    let md = "# Title\n\n```rust\nfn x() {}\n```\n\n<div class=\"a\">x</div>\n\n> quoted `code`\n> more\n";
    let s = spans("markdown", md);
    assert!(has(&s, "fn", Style::Keyword), "{s:?}");
    assert!(has(&s, "div", Style::Tag), "{s:?}");
    assert!(has(&s, "code", Style::String), "{s:?}");
    let s = spans("markdown", "```python\ndef f(x):\n    return x\n```\n");
    assert!(has(&s, "f", Style::Function), "{s:?}");
    let sample = "---\ntitle: Demo\n---\n\n# Heading *one*\n\nSome `code` here.\n\n```python\ndef f(x):\n    return x + 1  # note\n```\n";
    let s = spans("markdown", sample);
    assert!(has(&s, "f", Style::Function), "{s:?}");
    // Code fence content is plain text where the injected grammar has no style.
    assert!(!has(&s, " ", Style::String), "{s:?}");
    let front = "---\ntitle: x\n---\n\nText\n";
    let s = spans("markdown", front);
    assert!(s.iter().any(|(t, _)| t == "title"), "{s:?}");
    let html = "<script>let y = 1;</script><style>a { color: red; }</style>\n";
    let s = spans("html", html);
    assert!(has(&s, "let", Style::Keyword), "{s:?}");
    assert!(has(&s, "color", Style::Property), "{s:?}");
    let json = "<script type=\"application/json\">{\"a\": 1}</script>\n";
    let s = spans("html", json);
    assert!(has(&s, "1", Style::Number), "{s:?}");
}

/// Spans of a session after `edits`, compared with a fresh parse.
#[test]
fn incremental_parse_matches_fresh() {
    use rte_text::{Edit, Transaction};
    let text = "fn main() {\n    let x = 1;\n}\n\nstruct S { a: u8 }\n";
    let mut snapshot = Snapshot::from_bytes(text.as_bytes());
    let lang = by_id("rust").unwrap();
    let mut session = Session::new(lang, snapshot.clone(), false).unwrap();
    session.set_interest(0..u64::MAX);
    while session.work(&|| false) != Work::Idle {}
    let mut view = session.view();
    let edits = [
        Transaction::single(Edit::insert(16, &b"/* c */ "[..])),
        Transaction::new(vec![Edit::delete(0..2), Edit::insert(30, &b"\"s\""[..])]),
        Transaction::single(Edit::new(3..7, &b"other"[..])),
    ];
    for tx in &edits {
        let (next, changes) = snapshot.apply(tx).unwrap();
        let input = crate::input_edits(&snapshot, &next, &changes);
        view.edit(&input);
        session.edit(&input, next.clone());
        snapshot = next;
    }
    while session.work(&|| false) != Work::Idle {}
    let text = String::from_utf8(snapshot.to_vec(0..snapshot.len())).unwrap();
    let fresh = spans("rust", &text);
    let mut out = Vec::new();
    Painter::default().paint(
        &session.view(),
        &snapshot,
        0..snapshot.len(),
        None,
        &mut out,
    );
    let incremental: Vec<(String, Style)> = out
        .iter()
        .map(|s| (text[s.start as usize..s.end as usize].to_string(), s.style))
        .collect();
    assert_eq!(incremental, fresh);
    let tree = session.view();
    assert_eq!(tree.tree().unwrap().root_node().to_sexp(), {
        let mut p = tree_sitter::Parser::new();
        p.set_language(&lang.grammar.language()).unwrap();
        p.parse(&text, None).unwrap().root_node().to_sexp()
    });
    // The view edited forward still highlights the moved text.
    let mut stale = Vec::new();
    Painter::default().paint(&view, &snapshot, 0..snapshot.len(), None, &mut stale);
    assert!(stale.iter().any(|s| s.style == Style::Keyword));
}

/// Edits exact only near a range keep the highlights there; elsewhere one
/// edit per side stands for all changes.
#[test]
fn near_edits_keep_the_visible_rows_exact() {
    use rte_text::{Edit, Transaction};
    let text = "fn a() { let x = 1; }\n".repeat(200);
    let old = Snapshot::from_bytes(text.as_bytes());
    let (_, view) = parsed("rust", &text);
    let line = 22u64;
    let edits: Vec<Edit> = (0..200)
        .map(|i| Edit::insert(i * line + 3, &b"z"[..]))
        .collect();
    let (new, changes) = old.apply(&Transaction::new(edits)).unwrap();
    let near = 100 * (line + 1)..105 * (line + 1);
    let reduced = crate::input_edits_near(&old, &new, &changes, near.clone());
    assert!(reduced.len() <= 8, "{} edits", reduced.len());
    let exact = crate::input_edits(&old, &new, &changes);
    let mut a = view.clone();
    a.edit(&exact);
    let mut b = view.clone();
    b.edit(&reduced);
    let (mut sa, mut sb) = (Vec::new(), Vec::new());
    Painter::default().paint(&a, &new, near.clone(), None, &mut sa);
    Painter::default().paint(&b, &new, near, None, &mut sb);
    assert_eq!(sa, sb);
    assert!(sa.iter().any(|s| s.style == Style::Keyword));
}

#[test]
fn cancelled_parse_resumes() {
    let text = "fn f() { let a = 1; }\n".repeat(2000);
    let snapshot = Snapshot::from_bytes(text.as_bytes());
    let mut session = Session::new(by_id("rust").unwrap(), snapshot.clone(), false).unwrap();
    session.set_interest(0..100);
    assert_eq!(session.work(&|| true), Work::Cancelled);
    assert!(session.view().tree().is_none());
    while session.work(&|| false) != Work::Idle {}
    assert!(!session.view().tree().unwrap().root_node().has_error());
}

#[test]
fn large_files_parse_a_window_first() {
    let line = "fn f() { let a = \"text\"; }\n";
    let text = line.repeat((crate::WINDOW_THRESHOLD as usize * 2) / line.len());
    let snapshot = Snapshot::from_bytes(text.as_bytes());
    let len = snapshot.len();
    let mut session = Session::new(by_id("rust").unwrap(), snapshot.clone(), false).unwrap();
    let middle = len / 2;
    session.set_interest(middle..middle + 2000);
    assert_eq!(session.work(&|| false), Work::Updated);
    let view = session.view();
    let root = view.tree().unwrap().root_node();
    assert!(root.end_byte() as u64 - (root.start_byte() as u64) <= crate::WINDOW + 4096);
    let mut out = Vec::new();
    Painter::default().paint(&view, &snapshot, middle..middle + 2000, None, &mut out);
    assert!(out.iter().any(|s| s.style == Style::String));
    // Layers (none), then the full parse.
    while session.work(&|| false) != Work::Idle {}
    let view = session.view();
    let root = view.tree().unwrap().root_node();
    assert_eq!((root.start_byte(), root.end_byte() as u64), (0, len));
}

/// The whole-file parse runs on its own thread: the session stays free for
/// edits meanwhile, applies them to the finished tree and is notified.
#[test]
fn the_whole_file_parses_on_its_own_thread() {
    use rte_text::{Edit, Transaction};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let line = "fn f() { let a = 1; }\n";
    let text = line.repeat((crate::WINDOW_THRESHOLD as usize * 4) / line.len());
    let mut snapshot = Snapshot::from_bytes(text.as_bytes());
    let mut session = Session::new(by_id("rust").unwrap(), snapshot.clone(), false).unwrap();
    let notified = std::sync::Arc::new(AtomicUsize::new(0));
    let counter = notified.clone();
    session.set_notify(std::sync::Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    }));
    session.set_interest(0..100);
    assert_eq!(session.work(&|| false), Work::Updated);
    assert!(!session.is_complete());
    // An edit while the whole file parses.
    let (next, changes) = snapshot
        .apply(&Transaction::single(Edit::insert(0, &b"// c\n"[..])))
        .unwrap();
    session.edit(
        &crate::input_edits(&snapshot, &next, &changes),
        next.clone(),
    );
    snapshot = next;
    while session.work(&|| false) != Work::Idle {}
    assert!(session.is_complete());
    assert_eq!(notified.load(Ordering::SeqCst), 1);
    let view = session.view();
    let root = view.tree().unwrap().root_node();
    assert_eq!(root.end_byte() as u64, snapshot.len());
    assert!(!root.has_error());
    assert_eq!(root.child(0).unwrap().kind(), "line_comment");
}

/// A finished whole-file tree is adopted before a scroll parses another
/// window.
#[test]
fn a_finished_whole_file_tree_comes_before_a_new_window() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let line = "int f(void) { return 1; }\n";
    let text = line.repeat((crate::WINDOW_THRESHOLD as usize * 4) / line.len());
    let snapshot = Snapshot::from_bytes(text.as_bytes());
    let mut session = Session::new(by_id("c").unwrap(), snapshot.clone(), false).unwrap();
    let done = std::sync::Arc::new(AtomicUsize::new(0));
    let flag = done.clone();
    session.set_notify(std::sync::Arc::new(move || {
        flag.fetch_add(1, Ordering::SeqCst);
    }));
    session.set_interest(0..100);
    assert_eq!(session.work(&|| false), Work::Updated);
    while done.load(Ordering::SeqCst) == 0 {
        std::thread::yield_now();
    }
    let middle = snapshot.len() / 2;
    session.set_interest(middle..middle + 100);
    assert_eq!(session.work(&|| false), Work::Updated);
    assert!(session.is_complete(), "adopted, no window parsed");
    let view = session.view();
    assert_eq!(
        view.tree().unwrap().root_node().end_byte() as u64,
        snapshot.len()
    );
}

/// A minified file gets a bounded window, not its whole line; an interest
/// range covering the file parses it once, as the full tree.
#[test]
fn windows_are_bounded_and_whole_file_windows_are_full_parses() {
    let text = format!("[{}0]", "0,".repeat(80_000));
    let snapshot = Snapshot::from_bytes(text.as_bytes());
    let len = snapshot.len();
    assert!(len > crate::WINDOW_THRESHOLD && len <= 8 * crate::WINDOW);
    let mut session = Session::new(by_id("json").unwrap(), snapshot.clone(), false).unwrap();
    session.set_interest(0..1024);
    assert_eq!(session.work(&|| false), Work::Updated);
    let view = session.view();
    let root = view.tree().unwrap().root_node();
    assert!(root.end_byte() as u64 <= crate::WINDOW + crate::WINDOW / 4);
    assert!(!session.is_complete());

    let mut session = Session::new(by_id("json").unwrap(), snapshot.clone(), false).unwrap();
    session.set_interest(0..len);
    assert_eq!(session.work(&|| false), Work::Updated);
    assert!(session.is_complete(), "no second, full parse is scheduled");
    let view = session.view();
    assert_eq!(view.tree().unwrap().root_node().end_byte() as u64, len);
}

/// A window starting inside a block comment lacks its opening: its colors
/// are provisional until the full parse, then final.
#[test]
fn windows_after_the_file_start_are_provisional() {
    let mut text = String::from("/*\n");
    text.push_str(&"int x = 42;\n".repeat(60_000));
    text.push_str("*/\n");
    let snapshot = Snapshot::from_bytes(text.as_bytes());
    let mut session = Session::new(by_id("c").unwrap(), snapshot.clone(), false).unwrap();
    let interest = 360_003..360_135;
    session.set_interest(interest.clone());
    assert_eq!(session.work(&|| false), Work::Updated);
    assert!(session.current_range().is_empty(), "provisional window");
    while session.work(&|| false) != Work::Idle {}
    assert!(session.current_range().contains(&interest.start));
    let mut spans = Vec::new();
    Painter::default().paint(&session.view(), &snapshot, interest, None, &mut spans);
    assert!(spans.iter().all(|s| s.style == Style::Comment), "{spans:?}");

    // A window at the file start has all the context before it.
    let mut session = Session::new(by_id("c").unwrap(), snapshot, false).unwrap();
    session.set_interest(0..100);
    assert_eq!(session.work(&|| false), Work::Updated);
    assert!(session.current_range().contains(&50));
}

/// Huge tokens and ranges cost by capture count: predicates read a prefix,
/// and flattening sweeps capture boundaries.
#[test]
fn highlight_cost_does_not_grow_with_token_length() {
    let text = format!("const {}: u32 = 1;\n", "A".repeat(4 << 20));
    let (snapshot, view) = parsed("rust", &text);
    let mut spans = Vec::new();
    let complete = Painter::default().paint(&view, &snapshot, 0..1024, None, &mut spans);
    assert!(complete);
    assert_eq!(spans[0].style, Style::Keyword);
    let mut bytes = vec![b'a'; 4 << 20];
    bytes[0] = b'"';
    let last = bytes.len() - 1;
    bytes[last] = b'"';
    let (snapshot, view) = parsed("json", std::str::from_utf8(&bytes).unwrap());
    let mut spans = Vec::new();
    Painter::default().paint(&view, &snapshot, 0..snapshot.len(), None, &mut spans);
    assert_eq!(
        spans,
        [Span {
            start: 0,
            end: snapshot.len(),
            style: Style::String
        }]
    );
}

/// Ranges with a gap (an undrawn long-line tail) are painted separately.
#[test]
fn ranges_skip_undrawn_bytes() {
    let text = "let a = 1;\nlet b = 2;\nlet c = 3;\n";
    let (snapshot, view) = parsed("javascript", text);
    let mut spans = Vec::new();
    let ranges = [0..5, 22..33];
    Painter::default().paint_ranges(&view, &snapshot, &ranges, &|| false, &mut spans);
    assert!(
        spans.iter().all(|s| s.end <= 5 || s.start >= 22),
        "{spans:?}"
    );
    assert!(
        spans
            .iter()
            .any(|s| s.start == 22 && s.style == Style::Keyword)
    );
}

#[test]
fn injection_discovery_seeks_the_interest_range() {
    let mut text = "<p>x</p>\n".repeat(20_000);
    let at = text.len() as u64;
    text.push_str("<script>let y = 1;</script>\n");
    text.push_str(&"<p>x</p>\n".repeat(20_000));
    let snapshot = Snapshot::from_bytes(text.as_bytes());
    let mut session = Session::new(by_id("html").unwrap(), snapshot.clone(), true).unwrap();
    session.set_interest(at..at + 30);
    while session.work(&|| false) != Work::Idle {}
    let mut spans = Vec::new();
    Painter::default().paint(&session.view(), &snapshot, at..at + 30, None, &mut spans);
    assert!(spans.iter().any(|s| s.style == Style::Keyword), "{spans:?}");
}

/// A stop request ends work that finds nothing (no matches to poll
/// between) and is never reported as a complete result.
#[test]
fn cancellation_does_not_wait_for_matches() {
    let (s, view) = parsed("python", &"pass\n".repeat(100_000));
    assert_eq!(
        crate::symbols(&view, &s, 10, &|| false),
        Some((Vec::new(), false))
    );
    assert_eq!(crate::symbols(&view, &s, 10, &|| true), None);
    // A stop arriving during the walk.
    let calls = std::cell::Cell::new(0);
    let later = || {
        calls.set(calls.get() + 1);
        calls.get() > 1
    };
    assert_eq!(crate::symbols(&view, &s, 10, &later), None);
    assert!(calls.get() > 1);
    let (s, view) = parsed("json", "{\"a\": 1}\n");
    let mut out = Vec::new();
    let all = 0..s.len();
    let all = std::slice::from_ref(&all);
    assert!(!Painter::default().paint_ranges(&view, &s, all, &|| true, &mut out));
    assert!(Painter::default().paint_ranges(&view, &s, all, &|| false, &mut out));
}

#[test]
fn budget_stops_highlighting() {
    let text = "fn f() { let a = 1; }\n".repeat(20_000);
    let snapshot = Snapshot::from_bytes(text.as_bytes());
    let mut session = Session::new(by_id("rust").unwrap(), snapshot.clone(), false).unwrap();
    session.set_interest(0..100);
    while session.work(&|| false) != Work::Idle {}
    let mut out = Vec::new();
    let past = std::time::Instant::now();
    let complete = Painter::default().paint(
        &session.view(),
        &snapshot,
        0..snapshot.len(),
        Some(past),
        &mut out,
    );
    assert!(!complete);
}

#[test]
fn spans_are_sorted_and_disjoint() {
    let s = spans(
        "python",
        "def f(x):\n    \"\"\"doc\"\"\"\n    return x.y(1, 'a')\n",
    );
    assert!(!s.is_empty());
    let snapshot = Snapshot::from_bytes(b"x = f'{a}' # c\n");
    let mut session = Session::new(by_id("python").unwrap(), snapshot.clone(), false).unwrap();
    while session.work(&|| false) != Work::Idle {}
    let mut out: Vec<Span> = Vec::new();
    Painter::default().paint(&session.view(), &snapshot, 2..12, None, &mut out);
    for pair in out.windows(2) {
        assert!(pair[0].end <= pair[1].start);
    }
    assert!(out.iter().all(|s| s.start >= 2 && s.end <= 12));
}

/// A parsed view of `text` in `lang`.
fn parsed(lang: &str, text: &str) -> (Snapshot, crate::View) {
    parsed_with(lang, text, false)
}

/// Predicates on tokens longer than the text they read fail rather than
/// judge the token by its prefix.
#[test]
fn predicates_do_not_judge_a_long_token_by_its_prefix() {
    let max = crate::highlight::PREDICATE_TEXT_MAX as usize;
    let constant = |name: &str| {
        let (s, view) = parsed("c", &format!("int x = {name};\n"));
        let mut spans = Vec::new();
        Painter::default().paint(&view, &s, 8..10, None, &mut spans);
        spans.iter().any(|sp| sp.style == Style::Constant)
    };
    assert!(constant("AA"));
    assert!(!constant("AAa"));
    assert!(constant(&"A".repeat(max)));
    assert!(!constant(&format!("{}a", "A".repeat(max))));
    let methods = |name: &str| {
        let (s, view) = parsed("javascript", &format!("class C {{ {name}() {{}} }}\n"));
        let (list, _) = crate::symbols(&view, &s, 10, &|| false).unwrap();
        list.into_iter().filter(|sym| sym.kind == "method").count()
    };
    assert_eq!(methods("constructor"), 0);
    assert_eq!(methods(&format!("constructor{}", "x".repeat(max))), 1);
}

fn parsed_with(lang: &str, text: &str, injections: bool) -> (Snapshot, crate::View) {
    let snapshot = Snapshot::from_bytes(text.as_bytes());
    let mut session = Session::new(by_id(lang).unwrap(), snapshot.clone(), injections).unwrap();
    session.set_interest(0..snapshot.len());
    while session.work(&|| false) != Work::Idle {}
    (snapshot, session.view())
}

/// The language of code outside the parsed layers (an offscreen fence) or
/// after an edit comes from the host tree, never from a missing layer.
#[test]
fn the_language_outside_parsed_layers_comes_from_the_host_tree() {
    use rte_text::{Edit, Transaction};
    let text = format!(
        "# Notes\n\n```python\ndef shown(): pass\n```\n\n{}```python\ndef hidden(): pass\n```\n",
        "Paragraph.\n\n".repeat(200)
    );
    let mut snapshot = Snapshot::from_bytes(text.as_bytes());
    let mut session = Session::new(by_id("markdown").unwrap(), snapshot.clone(), true).unwrap();
    session.set_interest(0..100);
    while session.work(&|| false) != Work::Idle {}
    let mut view = session.view();
    let shown = text.find("def shown").unwrap() as u64;
    let hidden = text.find("def hidden").unwrap() as u64;
    let injected = &view.layers[1..];
    assert!(!injected.iter().any(|l| l.start <= hidden && hidden < l.end));
    assert_eq!(view.lang_at(shown, &snapshot).id, "python");
    assert_eq!(
        view.lang_at(hidden, &snapshot).id,
        "python",
        "offscreen fence"
    );
    assert_eq!(view.lang_at(0, &snapshot).id, "markdown");
    // Renaming the fence language: the edited view follows the new text.
    let info = text.rfind("python").unwrap() as u64;
    let (next, changes) = snapshot
        .apply(&Transaction::single(Edit::new(
            info..info + 6,
            &b"rust"[..],
        )))
        .unwrap();
    view.edit(&crate::input_edits(&snapshot, &next, &changes));
    snapshot = next;
    let hidden = hidden - 2;
    assert_eq!(view.lang_at(hidden, &snapshot).id, "rust");
    assert_eq!(view.lang_at(shown, &snapshot).id, "python");
}

#[test]
fn brackets_match_by_tree_and_by_text() {
    use crate::matching_bracket;
    let brackets = &[('(', ')'), ('[', ']'), ('{', '}')];
    let text = "fn f() { let s = \"{\"; g(a[1]); }\n";
    let (s, view) = parsed("rust", text);
    let at = |c: &str| text.find(c).unwrap() as u64;
    // Before the caret first, then after it.
    assert_eq!(
        matching_bracket(&s, Some(&view), brackets, at("{ let") + 1),
        Some((at("{ let"), at("}\n")))
    );
    assert_eq!(
        matching_bracket(&s, Some(&view), brackets, at("}\n")),
        Some((at("}\n"), at("{ let")))
    );
    assert_eq!(
        matching_bracket(&s, Some(&view), brackets, at("[1")),
        Some((at("[1"), at("])")))
    );
    // The brace inside the string is not a bracket.
    let quoted = text.find("\"{").unwrap() as u64 + 1;
    assert_eq!(
        matching_bracket(&s, Some(&view), brackets, quoted + 1),
        None
    );
    // Plain text scans.
    assert_eq!(
        matching_bracket(&s, None, brackets, at("(a") + 1),
        Some((at("(a"), at(");")))
    );
    assert_eq!(matching_bracket(&s, None, brackets, 5), Some((4, 5)));
    assert_eq!(matching_bracket(&s, None, brackets, 3), None);

    // Injected code: the JavaScript tree inside HTML.
    let html = "<script>function f() { return \"}\"; }</script>";
    let (s, view) = parsed_with("html", html, true);
    let open = html.find('{').unwrap() as u64;
    let close = html.rfind('}').unwrap() as u64;
    assert_eq!(
        matching_bracket(&s, Some(&view), brackets, open + 1),
        Some((open, close))
    );
    assert_eq!(view.lang_at(open, &s).id, "javascript");
    assert_eq!(view.lang_at(2, &s).id, "html");

    // The partner of a block brace is its parent's edge child: no walk over
    // the statements between.
    let block = format!("fn f() {{\n{}}}\n", "x();\n".repeat(50_000));
    let (s, view) = parsed("rust", &block);
    let open = block.find('{').unwrap() as u64;
    let close = block.rfind('}').unwrap() as u64;
    let started = std::time::Instant::now();
    assert_eq!(
        matching_bracket(&s, Some(&view), brackets, open + 1),
        Some((open, close))
    );
    assert_eq!(
        matching_bracket(&s, Some(&view), brackets, close + 1),
        Some((close, open))
    );
    assert!(started.elapsed() < std::time::Duration::from_millis(50));
}

#[test]
fn enclosing_nodes_grow() {
    let text = "fn f() { g(a + b); }\n";
    let (_, view) = parsed("rust", text);
    let a = text.find('a').unwrap() as u64;
    let mut range = a..a;
    let mut seen = Vec::new();
    while let Some(next) = crate::enclosing_node(&view, range.clone()) {
        assert!(next.start <= range.start && range.end <= next.end && next != range);
        seen.push(text[next.start as usize..next.end as usize].to_owned());
        range = next;
    }
    assert_eq!(seen[0], "a");
    assert!(seen.contains(&"a + b".to_owned()));
    assert!(seen.contains(&"(a + b)".to_owned()));
    assert_eq!(seen.last().unwrap(), text);
}

#[test]
fn symbols_from_tags() {
    let names = |lang: &str, text: &str| -> Vec<(String, String)> {
        let (s, view) = parsed(lang, text);
        crate::symbols(&view, &s, usize::MAX, &|| false)
            .unwrap()
            .0
            .into_iter()
            .map(|sym| (sym.name, sym.kind))
            .collect()
    };
    let rust = names("rust", "struct S;\nfn f() {}\nimpl S { fn g(&self) {} }\n");
    assert!(rust.contains(&("S".into(), "class".into())), "{rust:?}");
    assert!(rust.contains(&("f".into(), "function".into())), "{rust:?}");
    assert!(rust.iter().any(|(n, _)| n == "g"), "{rust:?}");
    let md = names("markdown", "# One\n\ntext\n\nTwo\n===\n");
    assert_eq!(
        md,
        [
            ("One".into(), "heading".into()),
            ("Two".into(), "heading".into())
        ]
    );
    let py = names("python", "class A:\n    def m(self):\n        pass\n");
    assert_eq!(
        py.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
        ["A", "m"]
    );
    let toml = names("toml", "[package]\nname = \"x\"\n[[bin]]\n");
    assert_eq!(
        toml.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
        ["package", "bin"]
    );
    let json = names("json", "{\"a\": {\"b\": 1}, \"c\": 2}");
    assert_eq!(
        json.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
        ["a", "c"]
    );
    let julia = names(
        "julia",
        "function f(x)\n  x\nend\nstruct P\n  a\nend\nmodule M\nend\n",
    );
    assert_eq!(
        julia.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
        ["f", "P", "M"]
    );
}
