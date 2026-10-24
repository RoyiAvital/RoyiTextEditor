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
    let mut history = History::with_limits(100, 10);
    for _ in 0..3 {
        let cmd = insert(&s, &sels, b"12345", &opts);
        step(&mut history, &mut s, &mut sels, cmd, None);
    }
    assert_eq!(history.undo_len(), 2);
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
