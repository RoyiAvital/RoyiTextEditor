use proptest::prelude::*;
use rte_text::{LineEnding, Snapshot};

use super::*;

/// Parses `|` (caret) and `[`/`]` (selection, head at `]`) markers.
fn parse(marked: &str) -> (Snapshot, Selections) {
    let mut text = Vec::new();
    let mut sels = Vec::new();
    let mut anchor = None;
    for b in marked.bytes() {
        let pos = text.len() as u64;
        match b {
            b'|' => sels.push(Selection::caret(pos)),
            b'[' => anchor = Some(pos),
            b']' => sels.push(Selection::new(anchor.take().unwrap(), pos)),
            _ => text.push(b),
        }
    }
    (Snapshot::from_bytes(&text), Selections::new(sels, 0))
}

fn show(s: &Snapshot, sels: &Selections) -> String {
    let text = s.to_vec(0..s.len());
    let mut out = Vec::new();
    for (i, &b) in text.iter().chain(std::iter::once(&0)).enumerate() {
        let i = i as u64;
        for sel in sels.as_slice() {
            if sel.is_empty() && sel.head == i {
                out.push(b'|');
            } else if !sel.is_empty() && sel.start() == i {
                out.push(if sel.anchor == i { b'[' } else { b']' });
            } else if !sel.is_empty() && sel.end() == i {
                out.push(if sel.head == i { b']' } else { b'[' });
            }
        }
        if (i as usize) < text.len() {
            out.push(b);
        }
    }
    String::from_utf8(out).unwrap()
}

fn lf() -> EditOptions {
    EditOptions {
        line_ending: LineEnding::Lf,
        ..EditOptions::default()
    }
}

fn run(marked: &str, f: impl Fn(&Snapshot, &Selections) -> Option<Command>) -> String {
    let (s, sels) = parse(marked);
    match f(&s, &sels) {
        Some(cmd) => {
            let (s, sels, _) = apply(&s, &sels, &cmd).unwrap();
            show(&s, &sels)
        }
        None => show(&s, &sels),
    }
}

#[test]
fn boundaries_skip_crlf_marks_and_multibyte() {
    let s = Snapshot::from_bytes(b"a\r\nb");
    assert_eq!(next_boundary(&s, 1), 3);
    assert_eq!(prev_boundary(&s, 3), 1);
    let s = Snapshot::from_bytes("e\u{301}\u{302}x中".as_bytes());
    assert_eq!(next_boundary(&s, 0), 5);
    assert_eq!(prev_boundary(&s, 5), 0);
    assert_eq!(next_boundary(&s, 6), 9);
    assert_eq!(prev_boundary(&s, 9), 6);
    let s = Snapshot::from_bytes(b"a\xFF\xFEb");
    assert_eq!(next_boundary(&s, 1), 2);
    assert_eq!(prev_boundary(&s, 3), 2);
    assert_eq!(next_boundary(&s, 4), 4);
    assert_eq!(prev_boundary(&s, 0), 0);
}

#[test]
fn word_motion() {
    let s = Snapshot::from_bytes(b"foo bar.baz  qux\nnext");
    let right: Vec<u64> = std::iter::successors(Some(0), |&p| {
        let n = word_right(&s, p);
        (n != p).then_some(n)
    })
    .collect();
    assert_eq!(right, vec![0, 3, 7, 8, 11, 16, 17, 21]);
    let left: Vec<u64> = std::iter::successors(Some(21), |&p| {
        let n = word_left(&s, p);
        (n != p).then_some(n)
    })
    .collect();
    assert_eq!(left, vec![21, 17, 16, 13, 8, 7, 4, 0]);
    assert_eq!(word_at(&s, 5), 4..7);
    assert_eq!(word_at(&s, 7), 7..8);
    let s = Snapshot::from_bytes("ae\u{301}b c".as_bytes());
    assert_eq!(word_right(&s, 0), 5);
    assert_eq!(word_left(&s, 5), 0);
}

#[test]
fn smart_home_and_end() {
    let s = Snapshot::from_bytes(b"x\r\n    abc\r\n");
    assert_eq!(line_home(&s, 9), 7);
    assert_eq!(line_home(&s, 7), 3);
    assert_eq!(line_home(&s, 3), 7);
    assert_eq!(line_end(&s, 4), 10);
    assert_eq!(content_range(&s, 0), 0..1);
}

#[test]
fn vertical_keeps_goal_column() {
    let s = Snapshot::from_bytes("a\tb\nabcdefgh\n中文x\nz".as_bytes());
    let (pos, goal) = vertical(&s, 2, None, 1, 4);
    assert_eq!((pos, goal), (8, 4));
    let (pos, _) = vertical(&s, pos, Some(goal), 1, 4);
    assert_eq!(pos, 19);
    let (pos, _) = vertical(&s, pos, Some(goal), 1, 4);
    assert_eq!(pos, s.len());
    assert_eq!(vertical(&s, 3, None, -1, 4).0, 0);
    assert_eq!(vertical(&s, 30, None, 1, 4).0, s.len());
}

#[test]
fn line_walks_agree_with_single_lookups() {
    // Tabs, wide characters, zero-width marks, CRLF, and a line past the scan limit.
    let unit = "a\tb\u{4E2D}e\u{301} ";
    let long = "x".repeat(COLUMN_SCAN_LIMIT + 100);
    let text = format!("{}\r\n{}\n{long}\n\tq", unit.repeat(50), unit.repeat(30));
    let s = Snapshot::from_bytes(text.as_bytes());
    let positions: Vec<u64> = (0..=s.len())
        .filter(|&p| snap_to_boundary(&s, p) == p)
        .step_by(61)
        .collect();
    let mut walk = LineWalk::default();
    for &p in &positions {
        assert_eq!(walk.column(&s, p, 4), column_at(&s, p, 4), "column at {p}");
    }
    for line in 0..s.line_count() {
        let mut walk = LineWalk::default();
        let range = content_range(&s, line);
        for col in (0..400).chain([COLUMN_SCAN_LIMIT as u64 + 50, u64::MAX]) {
            let expected = offset_in_line(&s, range.clone(), col, 4);
            assert_eq!(
                walk.offset(&s, line, col, 4),
                expected,
                "line {line} col {col}"
            );
        }
        // A smaller column restarts the walk.
        assert_eq!(walk.offset(&s, line, 3, 4), offset_in_line(&s, range, 3, 4));
    }
    let mut v = Vertical::default();
    for &p in &positions {
        assert_eq!(
            v.step(&s, p, None, 1, 4),
            vertical(&s, p, None, 1, 4),
            "{p}"
        );
    }
}

#[test]
fn columns_follow_cell_rules() {
    let line = "a\tb中e\u{301}\u{1}".as_bytes();
    assert_eq!(column_of(line, 1, 4), 1);
    assert_eq!(column_of(line, 2, 4), 4);
    assert_eq!(column_of(line, 3, 4), 5);
    assert_eq!(column_of(line, 6, 4), 7);
    assert_eq!(column_of(line, 9, 4), 8);
    assert_eq!(column_of(line, 10, 4), 9);
    assert_eq!(offset_at_column(line, 2, 4), 1);
    assert_eq!(offset_at_column(line, 3, 4), 2);
    assert_eq!(offset_at_column(line, 6, 4), 6);
    assert_eq!(offset_at_column(line, 8, 4), 9);
    assert_eq!(offset_at_column(line, 20, 4), 10 + 11);
    // A huge column clamps to the line end instead of wrapping.
    let s = Snapshot::from_bytes("\u{e9}\u{e9}".as_bytes());
    assert_eq!(offset_in_line(&s, 0..4, u64::MAX, 4), 4);
    assert_eq!(offset_at_column(b"ab", u64::MAX, 4), usize::MAX);
}

#[test]
fn the_scan_end_keeps_a_straddling_character_whole() {
    for ch in ['é', '中', '😀'] {
        let mut line = vec![b'a'; COLUMN_SCAN_LIMIT - 1];
        let mut buf = [0; 4];
        line.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        line.extend_from_slice(b"xyz");
        let prefix = &line[..COLUMN_SCAN_LIMIT];
        let seam = scan_end(prefix);
        assert_eq!(seam, COLUMN_SCAN_LIMIT - 1, "{ch}");
        // One column per byte from the seam, on either side of the cut.
        for offset in seam..line.len() {
            assert_eq!(
                column_of(prefix, offset, 4),
                (COLUMN_SCAN_LIMIT - 1 + offset - seam) as u64
            );
        }
    }
    // Complete text, a lone continuation byte and a real invalid byte stay.
    assert_eq!(scan_end(b"abc"), 3);
    assert_eq!(scan_end(b"ab\x80"), 3);
    assert_eq!(scan_end(b"ab\xFF"), 3);
    assert_eq!(scan_end("a中".as_bytes()), 4);
    assert_eq!(scan_end(b"a\xE4\xB8"), 1);
    assert_eq!(column_until(b"a\tb", 10, 4), Some(5));
    assert_eq!(column_until(b"a\tb", 4, 4), None);
    assert_eq!(column_until(b"a\tb", 5, 4), Some(5));
}

#[test]
fn typing_overtype_and_newline() {
    let opts = lf();
    assert_eq!(
        run("a|b", |s, sel| Some(insert(s, sel, b"x", &opts))),
        "ax|b"
    );
    assert_eq!(
        run("[ab]c", |s, sel| Some(insert(s, sel, b"x", &opts))),
        "x|c"
    );
    let over = EditOptions {
        overtype: true,
        ..opts
    };
    assert_eq!(
        run("a|bc", |s, sel| Some(insert(s, sel, b"x", &over))),
        "ax|c"
    );
    assert_eq!(
        run("ab|\nc", |s, sel| Some(insert(s, sel, b"x", &over))),
        "abx|\nc"
    );
    assert_eq!(
        run("  \tfoo|bar", |s, sel| Some(newline(s, sel, &opts))),
        "  \tfoo\n  \t|bar"
    );
    let crlf = EditOptions::default();
    assert_eq!(run("a|", |s, sel| Some(newline(s, sel, &crlf))), "a\r\n|");
}

#[test]
fn deletions() {
    assert_eq!(run("a\r\n|b", backspace), "a|b");
    assert_eq!(run("|a", backspace), "|a");
    assert_eq!(run("a|\r\nb", delete_forward), "a|b");
    assert_eq!(run("[ab]c", delete_forward), "|c");
    assert_eq!(run("foo bar|", delete_word_left), "foo |");
    assert_eq!(run("|foo bar", delete_word_right), "| bar");
    assert_eq!(run("ab|c|d", backspace), "a|d");
}

#[test]
fn tab_indent_and_outdent() {
    let opts = lf();
    assert_eq!(run("ab|", |s, sel| Some(tab(s, sel, &opts))), "ab  |");
    let tabs = EditOptions {
        insert_spaces: false,
        ..opts
    };
    assert_eq!(run("ab|", |s, sel| Some(tab(s, sel, &tabs))), "ab\t|");
    assert_eq!(
        run("[a\n\nb]\nc", |s, sel| Some(tab(s, sel, &opts))),
        "[    a\n\n    b]\nc"
    );
    assert_eq!(
        run("[a\nb\n]c", |s, sel| Some(tab(s, sel, &opts))),
        "[    a\n    b\n]c"
    );
    assert_eq!(
        run("\t[x\n      y]", |s, sel| outdent(s, sel, &opts)),
        "[x\n  y]"
    );
    assert_eq!(run("x|", |s, sel| outdent(s, sel, &opts)), "x|");
}

#[test]
fn copy_cut_paste() {
    let opts = lf();
    let (s, sels) = parse("one\ntwo|\nthree");
    let clip = copy(&s, &sels, &opts);
    assert_eq!(
        clip,
        Clip {
            pieces: vec![b"two\n".to_vec()],
            line_mode: true
        }
    );
    assert_eq!(
        run("on|e\nx", |s, sel| paste(s, sel, &clip, &opts)),
        "two\non|e\nx"
    );
    assert_eq!(
        run("[on]e", |s, sel| paste(s, sel, &clip, &opts)),
        "two\n|e"
    );

    let (s, sels) = parse("a\nb|");
    let (clip, cmd) = cut(&s, &sels, &opts);
    assert_eq!(clip.text(LineEnding::Lf), b"b\n");
    let (s, sels, _) = apply(&s, &sels, &cmd.unwrap()).unwrap();
    assert_eq!(show(&s, &sels), "a|");

    let (s, sels) = parse("[ab] [cd]");
    let clip = copy(&s, &sels, &opts);
    assert_eq!(clip.text(LineEnding::CrLf), b"ab\r\ncd");
    assert_eq!(
        run("x| y|", |s, sel| paste(s, sel, &clip, &opts)),
        "xab| ycd|"
    );
    assert_eq!(run("x|", |s, sel| paste(s, sel, &clip, &opts)), "xab\ncd|");
    let crlf = EditOptions::default();
    let text = Clip {
        pieces: vec![b"p\nq\r\nr\rs".to_vec()],
        line_mode: false,
    };
    assert_eq!(
        run("|", |s, sel| paste(s, sel, &text, &crlf)),
        "p\r\nq\r\nr\rs|"
    );
}

#[test]
fn plain_paste_spreads_lines_over_as_many_cursors() {
    let opts = lf();
    let plain = |text: &str| Clip {
        pieces: vec![text.as_bytes().to_vec()],
        line_mode: false,
    };
    let two = "|a |b";
    for text in ["X\nY", "X\r\nY", "X\nY\n", "X\r\nY\r\n"] {
        assert_eq!(
            run(two, |s, sel| paste(s, sel, &plain(text), &opts)),
            "X|a Y|b",
            "{text:?}"
        );
    }
    assert_eq!(
        run(two, |s, sel| paste(s, sel, &plain("X\nY\nZ"), &opts)),
        "X\nY\nZ|a X\nY\nZ|b"
    );
    assert_eq!(
        run("|a", |s, sel| paste(s, sel, &plain("X\nY"), &opts)),
        "X\nY|a"
    );
    let lines = Clip {
        pieces: vec![b"X\nY\n".to_vec()],
        line_mode: true,
    };
    assert_eq!(
        run("|a\n|b", |s, sel| paste(s, sel, &lines, &opts)),
        "X\nY\n|a\nX\nY\n|b"
    );
}

#[test]
fn long_runs_are_crossed_in_bounded_steps() {
    const LIMIT: u64 = MOTION_SCAN_LIMIT;
    let word = Snapshot::from_bytes(&vec![b'a'; 4 << 20]);
    assert_eq!(word_right(&word, 0), LIMIT);
    assert_eq!(word_left(&word, word.len()), word.len() - LIMIT);
    let mid = 2u64 << 20;
    assert_eq!(word_at(&word, mid), mid - LIMIT..mid + LIMIT);

    let mut spaces = vec![b' '; 4 << 20];
    spaces.push(b'x');
    let s = Snapshot::from_bytes(&spaces);
    assert_eq!(word_right(&s, 0), LIMIT);
    let before_x = s.len() - 1;
    assert_eq!(word_left(&s, before_x), before_x - LIMIT);

    let mut marks = "e".to_owned();
    marks.push_str(&"\u{301}".repeat(200_000));
    let s = Snapshot::from_bytes(marks.as_bytes());
    let next = next_boundary(&s, 0);
    assert!((LIMIT..=LIMIT + 2).contains(&next), "{next}");
    let prev = prev_boundary(&s, s.len());
    assert!((LIMIT..=LIMIT + 2).contains(&(s.len() - prev)), "{prev}");
    let (mut pos, mut steps) = (0, 0);
    while pos < s.len() {
        pos = next_boundary(&s, pos);
        steps += 1;
    }
    assert!(steps <= 8, "{steps} steps");
}

#[test]
fn snapping_avoids_split_characters_crlf_and_marks() {
    let s = Snapshot::from_bytes("a\u{4E2D}b\r\nc\u{301}d\n\u{301}x".as_bytes());
    assert_eq!(snap_to_boundary(&s, 2), 1);
    assert_eq!(snap_to_boundary(&s, 3), 1);
    assert_eq!(snap_to_boundary(&s, 4), 4);
    assert_eq!(snap_to_boundary(&s, 6), 5);
    assert_eq!(snap_to_boundary(&s, 8), 7);
    assert_eq!(snap_to_boundary(&s, 9), 7);
    assert_eq!(snap_to_boundary(&s, 10), 10);
    assert_eq!(snap_to_boundary(&s, 11), 11);
    // A mark at a line start stays on its line.
    assert_eq!(snap_to_boundary(&s, 13), 12);
    let truncated = Snapshot::from_bytes(b"a\xE4\xB8");
    assert_eq!(snap_to_boundary(&truncated, 2), 2);
}

#[test]
fn estimated_columns_never_split_characters() {
    let mut text = vec![b'x'; 70_000];
    text.push(b'\n');
    let second = text.len() as u64;
    text.extend(std::iter::repeat_n(b'a', 65_536));
    for _ in 0..100 {
        text.extend_from_slice("\u{4E2D}".as_bytes());
    }
    let s = Snapshot::from_bytes(&text);
    let opts = lf();
    for col in 65_530..65_560u64 {
        let (pos, _) = vertical(&s, col, None, 1, 4);
        assert!(
            std::str::from_utf8(&s.to_vec(second..pos)).is_ok(),
            "column {col} gave {pos}"
        );
        let sels = Selections::single(Selection::caret(pos));
        let (typed, sels, _) = apply(&s, &sels, &insert(&s, &sels, b"X", &opts)).unwrap();
        assert!(std::str::from_utf8(&typed.to_vec(0..typed.len())).is_ok());
        for cmd in [backspace(&typed, &sels), delete_forward(&typed, &sels)] {
            let (after, _, _) = apply(&typed, &sels, &cmd.unwrap()).unwrap();
            assert!(std::str::from_utf8(&after.to_vec(0..after.len())).is_ok());
        }
    }
}

#[test]
fn selections_merge_and_keep_primary() {
    let sels = Selections::new(
        vec![
            Selection::new(5, 8),
            Selection::caret(1),
            Selection::new(7, 3),
            Selection::caret(8),
        ],
        3,
    );
    assert_eq!(
        sels.as_slice(),
        &[Selection::caret(1), Selection::new(8, 3)]
    );
    assert_eq!(sels.primary_index(), 1);
    assert!(Selections::new(Vec::new(), 0).primary().is_empty());
}

fn step(
    history: &mut History,
    s: &mut Snapshot,
    sels: &mut Selections,
    cmd: Command,
    typed: Option<char>,
) {
    let (next, next_sels, _) = apply(s, sels, &cmd).unwrap();
    let inserted = cmd.tx.edits().iter().map(|e| e.text.len() as u64).sum();
    history.record(
        Step {
            before: s.clone(),
            after: next.clone(),
            sels_before: sels.clone(),
            sels_after: next_sels.clone(),
        },
        cmd.kind,
        inserted,
        typed,
    );
    *s = next;
    *sels = next_sels;
}

#[test]
fn typing_coalesces_by_word() {
    let opts = lf();
    let (mut s, mut sels) = parse("|");
    let original = s.clone();
    let mut history = History::default();
    for ch in "ab cd".chars() {
        let cmd = insert(&s, &sels, ch.to_string().as_bytes(), &opts);
        step(&mut history, &mut s, &mut sels, cmd, Some(ch));
    }
    assert_eq!(history.undo_len(), 2);
    let (after_first, _) = history.undo().unwrap();
    assert_eq!(after_first.to_vec(0..after_first.len()), b"ab ");
    let (start, sels0) = history.undo().unwrap();
    assert!(start.same_tree(&original));
    assert_eq!(show(&start, &sels0), "|");
    assert!(history.undo().is_none());
    let (redone, _) = history.redo().unwrap();
    assert_eq!(redone.to_vec(0..redone.len()), b"ab ");
    let (mut s, mut sels) = history.redo().unwrap();
    assert_eq!(show(&s, &sels), "ab cd|");
    let cmd = insert(&s, &sels, b"!", &opts);
    step(&mut history, &mut s, &mut sels, cmd, Some('!'));
    assert_eq!(history.redo_len(), 0);
}

#[test]
fn history_retention_drops_oldest() {
    let opts = lf();
    let (mut s, mut sels) = parse("|");
    let mut history = History::with_limits(3, 1000);
    for _ in 0..5 {
        let cmd = insert(&s, &sels, b"\n", &opts);
        step(&mut history, &mut s, &mut sels, cmd, None);
    }
    assert_eq!(history.undo_len(), 3);
    // Each step holds 5 bytes and two single-caret selection sets.
    let per_step = 5 + 2 * size_of::<Selection>() as u64;
    let mut history = History::with_limits(100, 2 * per_step);
    for _ in 0..3 {
        let cmd = insert(&s, &sels, b"12345", &opts);
        step(&mut history, &mut s, &mut sels, cmd, None);
    }
    assert_eq!(history.undo_len(), 2);
}

#[test]
fn history_budget_counts_selections() {
    let opts = lf();
    let mut s = Snapshot::from_bytes(&b" ".repeat(1000));
    let mut sels = Selections::new((0..1000).map(Selection::caret).collect(), 0);
    // One step holds 2,000 selections: far over a 10 KiB budget, so only the last stays.
    let mut history = History::with_limits(100, 10 * 1024);
    for _ in 0..3 {
        let cmd = insert(&s, &sels, b"x", &opts);
        step(&mut history, &mut s, &mut sels, cmd, None);
        history.seal();
    }
    assert_eq!(history.undo_len(), 1);
    let mut history = History::with_limits(100, 1024 * 1024);
    for _ in 0..3 {
        let cmd = insert(&s, &sels, b"x", &opts);
        step(&mut history, &mut s, &mut sels, cmd, None);
        history.seal();
    }
    assert_eq!(history.undo_len(), 3);
}

#[derive(Clone, Debug)]
enum Op {
    Type(char),
    Paste(String),
    Newline,
    Back,
    Delete,
    Move(u16),
    Seal,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        prop::char::range('a', 'e').prop_map(Op::Type),
        Just(Op::Type(' ')),
        "[a-c\n]{0,6}".prop_map(Op::Paste),
        Just(Op::Newline),
        Just(Op::Back),
        Just(Op::Delete),
        any::<u16>().prop_map(Op::Move),
        Just(Op::Seal),
    ]
}

fn multi(marked: &str, f: impl Fn(&Snapshot, &Selections) -> Selections) -> String {
    let (s, sels) = parse(marked);
    show(&s, &f(&s, &sels))
}

#[test]
fn add_cursors_above_and_below_keep_the_goal_column() {
    let down = |s: &Snapshot, sels: &Selections| add_cursors_vertical(s, sels, 1, 4);
    let up = |s: &Snapshot, sels: &Selections| add_cursors_vertical(s, sels, -1, 4);
    assert_eq!(multi("ab|cd\nx\nefgh", down), "ab|cd\nx|\nefgh");
    let (s, sels) = parse("ab|cd\nx\nefgh");
    let once = add_cursors_vertical(&s, &sels, 1, 4);
    let twice = add_cursors_vertical(&s, &once, 1, 4);
    assert_eq!(show(&s, &twice), "ab|cd\nx|\nef|gh");
    assert_eq!(twice.primary().head, 9);
    assert_eq!(multi("a\nb|c", down), "a\nb|c");
    assert_eq!(multi("a|bc\nd", up), "a|bc\nd");
    assert_eq!(multi("ab\n\tc|d", up), "ab|\n\tc|d");
}

#[test]
fn cursors_at_line_ends_of_selections() {
    let ends = |s: &Snapshot, sels: &Selections| cursors_at_line_ends(s, sels, 100).unwrap();
    assert_eq!(multi("a[b\ncd\nef]g", ends), "ab|\ncd|\nef|g");
    assert_eq!(multi("[ab\ncd\n]ef", ends), "ab|\ncd|\nef");
    assert_eq!(multi("a[bc]d", ends), "abc|d");
    let (s, sels) = parse("a[b\ncd\nef]g");
    assert!(cursors_at_line_ends(&s, &sels, 3).is_some());
    assert!(cursors_at_line_ends(&s, &sels, 2).is_none());
}

#[test]
fn line_end_and_column_limits_refuse_before_building() {
    let text = "x\n".repeat(50_000);
    let s = Snapshot::from_bytes(text.as_bytes());
    let all = Selections::single(Selection::new(0, s.len()));
    let started = std::time::Instant::now();
    assert!(cursors_at_line_ends(&s, &all, 2).is_none());
    assert!(box_select(&s, (0, 0), (49_999, 1), 4, 2).is_none());
    assert!(started.elapsed() < std::time::Duration::from_millis(50));
    assert_eq!(box_select(&s, (0, 0), (1, 1), 4, 2).unwrap().len(), 2);
}

#[test]
fn select_lines_grows_by_whole_lines() {
    let f = select_lines;
    assert_eq!(multi("a|b\ncd\nef", f), "[ab\n]cd\nef");
    assert_eq!(multi("[ab\n]cd\nef", f), "[ab\ncd\n]ef");
    assert_eq!(multi("ab\ncd\ne|f", f), "ab\ncd\n[ef]");
    assert_eq!(multi("a[b\nc]d\nef", f), "[ab\ncd\n]ef");
}

#[test]
fn box_selection_follows_columns_tabs_and_short_lines() {
    let s = Snapshot::from_bytes(b"abcdef\nab\n\tcdef\nabcdef\nx");
    let sels = box_select(&s, (0, 1), (3, 5), 4, 100).unwrap();
    assert_eq!(show(&s, &sels), "a[bcde]f\na[b]\n[\tc]def\na[bcde]f\nx");
    assert_eq!(sels.primary().head, 21);
    let short = box_select(&s, (3, 3), (4, 5), 4, 100).unwrap();
    assert_eq!(show(&s, &short), "abcdef\nab\n\tcdef\nabc[de]f\nx|");
    let back = box_select(&s, (3, 5), (0, 1), 4, 100).unwrap();
    assert_eq!(back.len(), 4);
    assert_eq!(
        back.primary(),
        Selection {
            goal: Some(1),
            ..Selection::new(5, 1)
        }
    );
    assert_eq!(cell_of(&s, 13, 4), (2, 6));
}

proptest! {
    #[test]
    fn undo_all_restores_original_and_redo_all_the_result(
        start in "[a-c \n]{0,20}",
        ops in prop::collection::vec(op(), 0..60),
    ) {
        let opts = lf();
        let original = Snapshot::from_bytes(start.as_bytes());
        let mut s = original.clone();
        let mut sels = Selections::single(Selection::caret(0));
        let mut history = History::default();
        for op in ops {
            let cmd = match op {
                Op::Type(ch) => Some((insert(&s, &sels, ch.to_string().as_bytes(), &opts), Some(ch))),
                Op::Paste(text) => paste(&s, &sels, &Clip { pieces: vec![text.into_bytes()], line_mode: false }, &opts).map(|c| (c, None)),
                Op::Newline => Some((newline(&s, &sels, &opts), None)),
                Op::Back => backspace(&s, &sels).map(|c| (c, None)),
                Op::Delete => delete_forward(&s, &sels).map(|c| (c, None)),
                Op::Move(to) => {
                    let pos = u64::from(to) % (s.len() + 1);
                    sels = Selections::single(Selection::caret(prev_boundary(&s, next_boundary(&s, pos))));
                    history.seal();
                    None
                }
                Op::Seal => {
                    history.seal();
                    None
                }
            };
            if let Some((cmd, typed)) = cmd {
                step(&mut history, &mut s, &mut sels, cmd, typed);
            }
        }
        let fin = s.to_vec(0..s.len());
        let mut last = None;
        while let Some(state) = history.undo() {
            last = Some(state);
        }
        if let Some((first, _)) = &last {
            prop_assert!(first.same_tree(&original));
        }
        let mut redone = original.clone();
        while let Some((next, _)) = history.redo() {
            redone = next;
        }
        prop_assert_eq!(redone.to_vec(0..redone.len()), fin);
    }
}

/// Runs a code-aware edit and shows the text with the returned selections.
fn coded(marked: &str, f: impl Fn(&Snapshot, &Selections) -> Option<Coded>) -> String {
    let (s, sels) = parse(marked);
    match f(&s, &sels) {
        Some(c) => {
            let (s, _, _) = apply(&s, &sels, &c.cmd).unwrap();
            show(&s, &c.selections)
        }
        None => format!("none: {}", show(&s, &sels)),
    }
}

const SLASH: CommentTokens<'static> = CommentTokens {
    line: Some("//"),
    block: Some(("/*", "*/")),
};

#[test]
fn line_comments_toggle_on_touched_lines() {
    let line = |m: &str| coded(m, |s, sels| toggle_line_comment(s, sels, &|_| SLASH));
    assert_eq!(line("  a|\n    b\n"), "  // a|\n    b\n");
    assert_eq!(line("  [a\n\n    b]\nc"), "  // [a\n\n  //   b]\nc");
    assert_eq!(line("  // a|\n  //b\n"), "  a|\n  //b\n");
    assert_eq!(line("  [// a\n  //b]\n"), "  [a\n  b]\n");
    // Mixed lines are commented, not uncommented.
    assert_eq!(line("[// a\nb]"), "// [// a\n// b]");
    let css = CommentTokens {
        line: None,
        block: Some(("/*", "*/")),
    };
    let block = |m: &str| coded(m, |s, sels| toggle_line_comment(s, sels, &|_| css));
    assert_eq!(block("a { }|  \n"), "/* a { } */|  \n");
    assert_eq!(block("/* a { } */|\n"), "a { }|\n");
    assert_eq!(line("   |\n"), "none:    |\n");
}

/// Block tokens close before trailing whitespace spanning many chunks.
#[test]
fn line_comments_close_before_long_trailing_whitespace() {
    let css = CommentTokens {
        line: None,
        block: Some(("/*", "*/")),
    };
    let block = |m: &str| coded(m, |s, sels| toggle_line_comment(s, sels, &|_| css));
    let tail = " \t".repeat(300_000);
    let wrapped = block(&format!("a {{ }}|{tail}\n"));
    assert!(wrapped == format!("/* a {{ }} */|{tail}\n"));
    assert!(block(&wrapped) == format!("a {{ }}|{tail}\n"));
}

/// Each line uses the tokens of the language at its text (embedded code).
#[test]
fn add_and_remove_line_comments() {
    let mode = |m: &str, mode| coded(m, |s, sels| line_comment(s, sels, mode, &|_| SLASH));
    assert_eq!(mode("[// a\nb]", CommentMode::Add), "// [// a\n// b]");
    assert_eq!(mode("[// a\nb]", CommentMode::Remove), "[a\nb]");
    assert_eq!(mode("a|", CommentMode::Remove), "none: a|");
    assert_eq!(mode("// a|", CommentMode::Toggle), "a|");
}

#[test]
fn move_lines_swap_with_their_neighbor() {
    let up = |m: &str| coded(m, |s, sels| move_lines(s, sels, true));
    let down = |m: &str| coded(m, |s, sels| move_lines(s, sels, false));
    assert_eq!(up("a\nb|c\nd"), "b|c\na\nd");
    assert_eq!(down("a|b\nc\nd"), "c\na|b\nd");
    // The last line keeps having no break, either way.
    assert_eq!(up("a\nb|"), "b|\na");
    assert_eq!(down("a|\nb"), "b\na|");
    // A selection over whole lines, ending at the next line start.
    assert_eq!(down("[a\nb\n]c\nd"), "c\n[a\nb\n]d");
    assert_eq!(up("a\n[b\nc\n]d"), "[b\nc\n]a\nd");
    // Two blocks; a block at the edge stays.
    assert_eq!(down("a|\nb\nc|\nd\ne"), "b\na|\nd\nc|\ne");
    assert_eq!(up("|a\nb\nc|"), "|a\nc|\nb");
    assert_eq!(up("|a"), "none: |a");
    assert_eq!(up("|a\nb\nc\nd|"), "|a\nb\nd|\nc");
    // CRLF breaks stay in place; empty lines move too.
    assert_eq!(down("|\r\nx\r\n"), "x\r\n|\r\n");
    // A selection ending after an LF ends after the CRLF that follows the
    // moved line, in both directions and for reversed selections.
    assert_eq!(down("[a\n]b\r\nc"), "b\n[a\r\n]c");
    let s = Snapshot::from_bytes(b"a\nb\r\nc");
    let reversed = Selections::single(Selection::new(2, 0));
    let moved = move_lines(&s, &reversed, false).unwrap();
    assert_eq!(moved.selections.primary(), Selection::new(5, 2));
    // Typing over the moved selection replaces exactly that line.
    let (after, _, _) = apply(&s, &reversed, &moved.cmd).unwrap();
    assert_eq!(show(&after, &moved.selections), "b\n]a\r\n[c");
    let typed = insert(&after, &moved.selections, b"z", &lf());
    let (after, _, _) = apply(&after, &moved.selections, &typed).unwrap();
    assert_eq!(after.to_vec(0..after.len()), b"b\nzc");
    assert_eq!(up("a\r\n[b\n]c"), "[b\r\n]a\nc");
    assert_eq!(down("[a\r\n]b\nc"), "b\r\n[a\n]c");
}

#[test]
fn copy_lines_duplicate_blocks_and_select_the_copy() {
    let opts = lf();
    let up = |m: &str| coded(m, |s, sels| Some(copy_lines(s, sels, true, &opts)));
    let down = |m: &str| coded(m, |s, sels| Some(copy_lines(s, sels, false, &opts)));
    assert_eq!(down("a|b\nc"), "ab\na|b\nc");
    assert_eq!(up("a|b\nc"), "a|b\nab\nc");
    // A last line without a break gets one between the copies.
    assert_eq!(down("a\nb|"), "a\nb\nb|");
    assert_eq!(up("a\nb|"), "a\nb|\nb");
    assert_eq!(down("[a\nb]\nc"), "a\nb\n[a\nb]\nc");
    assert_eq!(down("a|\nb\nc|"), "a\na|\nb\nc\nc|");
}

#[test]
fn delete_lines_keep_the_column_on_the_next_line() {
    let opts = lf();
    let del = |m: &str| coded(m, |s, sels| Some(delete_lines(s, sels, &opts)));
    assert_eq!(del("ab|c\nxyz\nq"), "xy|z\nq");
    assert_eq!(del("[a\nb]\nc"), "c|");
    // The last line takes the break before it.
    assert_eq!(del("abc\nx|y"), "a|bc");
    assert_eq!(del("only|"), "|");
    assert_eq!(del("a|\nb\nc|\nd"), "b|\nd|");
    assert_eq!(del("a\r\nb|\r\nc"), "a\r\nc|");
}

#[test]
fn insert_lines_and_indent_lines() {
    let opts = lf();
    let below = |m: &str| coded(m, |s, sels| Some(insert_line(s, sels, true, &opts)));
    let above = |m: &str| coded(m, |s, sels| Some(insert_line(s, sels, false, &opts)));
    assert_eq!(below("  a|b\nc"), "  ab\n  |\nc");
    assert_eq!(above("  a|b\nc"), "  |\n  ab\nc");
    assert_eq!(below("a|\nb|"), "a\n|\nb\n|");
    // Two carets on one line add one line.
    assert_eq!(below("|a|b"), "ab\n|");
    let indent = |m: &str| coded(m, |s, sels| indent_lines(s, sels, &opts));
    assert_eq!(indent("a|b\n\nc"), "    a|b\n\nc");
    assert_eq!(indent("[a\n\nb]"), "[    a\n\n    b]");
    assert_eq!(indent("|\n"), "none: |\n");
}

/// Moving and copying share the text: no bytes are copied, and one block
/// over millions of lines is two edits.
#[test]
fn line_commands_scale_with_blocks_not_lines() {
    let big = vec![b'x'; 65 << 20];
    let mut text = b"a\n".to_vec();
    text.extend_from_slice(&big);
    let s = Snapshot::from_bytes(&text);
    let sels = Selections::single(Selection::caret(0));
    let moved = move_lines(&s, &sels, false).unwrap();
    assert!(moved.cmd.tx.edits().iter().all(|e| e.text.owned_len() == 0));
    let (after, _, _) = apply(&s, &sels, &moved.cmd).unwrap();
    assert_eq!(after.len(), s.len());
    assert_eq!(after.to_vec(after.len() - 2..after.len()), b"\na");
    let copied = copy_lines(&s, &Selections::single(Selection::caret(3)), false, &lf());
    assert_eq!(copied.cmd.tx.edits()[0].text.owned_len(), 1);
    let lines = Snapshot::from_bytes(&b"ab\n".repeat(2_000_000));
    let all = Selections::single(Selection::new(0, lines.len() - 3));
    for coded in [
        move_lines(&lines, &all, false).unwrap(),
        copy_lines(&lines, &all, false, &lf()),
        delete_lines(&lines, &all, &lf()),
    ] {
        assert!(coded.cmd.tx.edits().len() <= 2);
    }
}

/// Each line uses the tokens of the language at its text (embedded code).
#[test]
fn comments_follow_the_language_at_each_line() {
    let hash = CommentTokens {
        line: Some("#"),
        block: None,
    };
    // Bytes from 5 on (the second line) are "Python".
    let tokens = move |pos: u64| if pos >= 5 { hash } else { SLASH };
    let mixed = |m: &str| coded(m, |s, sels| toggle_line_comment(s, sels, &tokens));
    assert_eq!(mixed("[a();\nb = 1]"), "// [a();\n# b = 1]");
    let none = |_| CommentTokens::default();
    let plain = coded("a|", |s, sels| toggle_line_comment(s, sels, &none));
    assert_eq!(plain, "none: a|");
}

#[test]
fn block_comments_wrap_and_unwrap() {
    let block = |m: &str| {
        coded(m, |s, sels| {
            toggle_block_comment(s, sels, &|_| Some(("/*", "*/")))
        })
    };
    assert_eq!(block("a [b] c"), "a /* [b] */ c");
    assert_eq!(block("a [/* b */] c"), "a [b] c");
    assert_eq!(block("a [  /* b */\n] c"), "a [  b\n] c");
    assert_eq!(block("a | c"), "a /* | */ c");
    // Only selections with block tokens change.
    let some = |pos: u64| (pos < 3).then_some(("/*", "*/"));
    let partial = coded("a| b| c", |s, sels| toggle_block_comment(s, sels, &some));
    assert_eq!(partial, "a/* | */ b| c");
}

/// Whitespace edges are scanned in place, never copied.
#[test]
fn block_comment_edges_of_a_large_selection() {
    let mut text = vec![b' '; 100_000];
    text.extend_from_slice(b"/* x */");
    text.extend(std::iter::repeat_n(b'\n', 10_000));
    let s = Snapshot::from_bytes(&text);
    let sels = Selections::single(Selection::new(0, s.len()));
    let c = toggle_block_comment(&s, &sels, &|_| Some(("/*", "*/"))).unwrap();
    // The leading run exceeds the scan limit: the selection is wrapped.
    assert_eq!(c.cmd.tx.edits().len(), 2);
    assert_eq!(c.cmd.tx.edits()[0].text.bytes(), b"/* ");
    let sels = Selections::single(Selection::new(99_000, s.len()));
    let c = toggle_block_comment(&s, &sels, &|_| Some(("/*", "*/"))).unwrap();
    assert!(
        c.cmd.tx.edits().iter().all(|e| e.text.is_empty()),
        "unwrapped"
    );
}

#[test]
fn auto_closing_pairs() {
    let pairs: &[(char, char)] = &[('(', ')'), ('{', '}'), ('"', '"')];
    let at = |_| pairs;
    let typed = |m: &str, ch| coded(m, |s, sels| auto_close(s, sels, ch, &at));
    assert_eq!(typed("f|", '('), "f(|)");
    assert_eq!(typed("f| x", '('), "f(|) x");
    assert_eq!(typed("f|x", '('), "none: f|x");
    assert_eq!(typed("f(|)", ')'), "f()|");
    assert_eq!(typed("a [b] c", '('), "a ([b]) c");
    assert_eq!(typed("x = |;", '"'), "x = \"|\";");
    assert_eq!(typed("don|", '"'), "none: don|");
    assert_eq!(typed("\"|\"", '"'), "\"\"|");
    assert_eq!(typed("|a\n|b", '{'), "none: |a\n|b");
    assert_eq!(typed("a|\nb|", '{'), "a{|}\nb{|}");
    let back = |m: &str| coded(m, |s, sels| delete_pair(s, sels, &at));
    assert_eq!(back("f(|)"), "f|");
    assert_eq!(back("f(|x)"), "none: f(|x)");
    // A caret whose language closes nothing types plainly.
    let quotes_only: &[(char, char)] = &[('"', '"')];
    let mixed = move |pos: u64| if pos < 2 { pairs } else { quotes_only };
    let typed = coded("a|\nb|", |s, sels| auto_close(s, sels, '(', &mixed));
    assert_eq!(typed, "none: a|\nb|");
}

#[test]
fn enter_indents_after_an_opening_bracket() {
    let opts = lf();
    let brackets: &[(char, char)] = &[('(', ')'), ('{', '}')];
    let enter = |m: &str, colon| {
        coded(m, |s, sels| {
            newline_indent(s, sels, &opts, &|_| (brackets, colon))
        })
    };
    assert_eq!(enter("  f {|}", false), "  f {\n      |\n  }");
    assert_eq!(enter("  f(|", false), "  f(\n      |");
    assert_eq!(enter("  x|", false), "none:   x|");
    assert_eq!(enter("def f():|", true), "def f():\n    |");
    assert_eq!(enter("x = {'a':| 1}", true), "none: x = {'a':| 1}");
    // Replacing a multi-line body keeps the closing brace on its own line.
    assert_eq!(enter("{[\n  foo\n]}", false), "{\n    |\n}");
    // Past the 64 KiB scan limit the opener is the text before the caret.
    let long = "a".repeat(COLUMN_SCAN_LIMIT + 10);
    let s = Snapshot::from_bytes(format!("{long}{{").as_bytes());
    let sels = Selections::single(Selection::caret(s.len()));
    assert!(newline_indent(&s, &sels, &opts, &|_| (brackets, false)).is_some());
    let spaces = " ".repeat(COLUMN_SCAN_LIMIT + 10);
    let s = Snapshot::from_bytes(format!("{{{spaces}x").as_bytes());
    let sels = Selections::single(Selection::caret(s.len()));
    assert!(newline_indent(&s, &sels, &opts, &|_| (brackets, false)).is_none());
}
