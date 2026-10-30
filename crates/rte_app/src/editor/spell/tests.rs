use std::sync::mpsc;
use std::time::Duration;

use super::*;
use crate::save::fake::FakeFiles;

fn settings(spelling: bool) -> Settings {
    Settings {
        read_only: false,
        exit_after_first_frame: false,
        exit_after_load: false,
        exit_after_highlight: false,
        recent_count: 3,
        multi_cursor_limit: 10_000,
        replace_all_max_mib: 512,
        user_dir: None,
        notice: None,
        highlighting: true,
        syntax: None,
        syntax_max_mib: 32,
        injections: true,
        spelling,
        spelling_engine: crate::spelling::Engine::Builtin,
        hunspell: PathBuf::new(),
        early_syntax: None,
        bench: None,
    }
}

/// A shown editor on `text` loaded from `name`.
fn open(name: &str, text: &str, spelling: bool, ctx: &mut Ctx) -> Editor {
    let (tx, rx) = mpsc::channel();
    tx.send(LoadEvent::Done {
        snapshot: Snapshot::from_bytes(text.as_bytes()),
        info: FileInfo {
            encoding: Encoding::Utf8,
            line_ending: LineEnding::Lf,
            indent_tabs: false,
            malformed: false,
            cr_breaks: false,
            new_file: false,
        },
        baseline: None,
        read_ms: 0.0,
    })
    .unwrap();
    drop(tx);
    let renderer = Renderer::new(Cow::Borrowed(rte_assets::DEFAULT_FONT), 14.0).unwrap();
    let mut editor = Editor::new(
        Some(Path::new(name)),
        None,
        Some(Load::new(rx)),
        renderer,
        96,
        settings(spelling),
        Trace::new(false),
        Arc::new(FakeFiles::default()),
        Arc::new(|| {}),
    );
    editor.event(
        Event::Resized {
            width: 800,
            height: 600,
        },
        ctx,
    );
    editor.event(Event::Shown, ctx);
    editor
}

/// Delivers wake-ups until spelling has nothing in flight.
fn settle(editor: &mut Editor, ctx: &mut Ctx) {
    for _ in 0..5000 {
        editor.event(Event::Wake, ctx);
        if !editor.spelling_busy() {
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("spelling did not settle");
}

fn words(editor: &Editor) -> Vec<String> {
    editor
        .spell
        .as_ref()
        .map(|s| s.marks.iter().map(|m| m.word.clone()).collect())
        .unwrap_or_default()
}

fn key(vk: u16, ctrl: bool, shift: bool) -> Event {
    Event::Key(Key {
        vk,
        ctrl,
        shift,
        alt: false,
    })
}

fn text(editor: &Editor) -> String {
    String::from_utf8(editor.doc.snapshot.to_vec(0..editor.doc.snapshot.len())).unwrap()
}

fn selected(editor: &Editor) -> String {
    let range = editor.doc.selections.primary().range();
    String::from_utf8(editor.doc.snapshot.to_vec(range)).unwrap()
}

#[test]
fn spelling_is_off_by_default_and_starts_nothing() {
    let mut ctx = Ctx::headless(800, 600, 96);
    let mut editor = open(r"C:\docs\a.txt", "teh recieve\n", false, &mut ctx);
    editor.event(Event::Wake, &mut ctx);
    assert!(editor.spell.is_none() && editor.doc.spell_journal.is_none());
    editor.perform(Action::NextMisspelling, &mut ctx);
    assert!(
        editor
            .notice
            .as_deref()
            .unwrap()
            .contains("Spelling is off")
    );
    assert!(editor.spell.is_none());
}

#[test]
fn plain_text_is_marked_and_the_caret_word_is_hinted() {
    let mut ctx = Ctx::headless(800, 600, 96);
    let mut editor = open(r"C:\docs\a.txt", "The recieve of teh day\n", true, &mut ctx);
    settle(&mut editor, &mut ctx);
    assert_eq!(words(&editor), ["recieve", "teh"]);
    let mut pixels = vec![0u32; 800 * 600];
    editor.paint(&mut pixels, 800, 600);
    assert_eq!(editor.list.waves.len(), 2);
    editor.select_single(Selection::caret(6));
    assert_eq!(editor.status_text().0, "Spelling: recieve (Ctrl+.)");
    // Typing inside a mark drops it at once; the reply marks the new word.
    editor.event(Event::Char('x'), &mut ctx);
    assert_eq!(words(&editor), ["teh"]);
    settle(&mut editor, &mut ctx);
    assert_eq!(words(&editor), ["rexcieve", "teh"]);
}

#[test]
fn code_files_are_not_checked() {
    let mut ctx = Ctx::headless(800, 600, 96);
    let mut editor = open(r"C:\src\main.rs", "fn mian() { teh(); }\n", true, &mut ctx);
    settle(&mut editor, &mut ctx);
    assert!(editor.spell.is_none());
    editor.perform(Action::NextMisspelling, &mut ctx);
    assert!(
        editor
            .notice
            .as_deref()
            .unwrap()
            .contains("plain text and Markdown")
    );
}

#[test]
fn suggestions_come_first_and_enter_replaces_in_one_undo_step() {
    let mut ctx = Ctx::headless(800, 600, 96);
    let mut editor = open(r"C:\docs\a.txt", "I recieve it\n", true, &mut ctx);
    settle(&mut editor, &mut ctx);
    editor.select_single(Selection::caret(4));
    editor.event(key(VK_OEM_PERIOD, true, false), &mut ctx);
    // Enter before the suggestions arrive does nothing.
    editor.event(key(VK_RETURN, false, false), &mut ctx);
    assert!(editor.popup.is_some());
    settle(&mut editor, &mut ctx);
    let Some(Popup::Palette(p)) = &editor.popup else {
        panic!("no palette");
    };
    assert_eq!(p.mode, Mode::Spelling);
    assert_eq!(p.choices.labels[0], "receive");
    assert!(p.choices.labels.iter().any(|l| l.starts_with("Add ")));
    // One separator, between the suggestions and the commands.
    let separator = p.items.iter().position(|i| *i == Item::Separator).unwrap();
    assert_eq!(p.items.iter().filter(|i| **i == Item::Separator).count(), 1);
    let label = |i: &Item| match i {
        Item::Choice { index, .. } => p.choices.labels[*index].as_str(),
        _ => "",
    };
    assert!(label(&p.items[separator + 1]).starts_with("Add "));
    assert!(
        p.items[..separator]
            .iter()
            .all(|i| !label(i).starts_with("Add "))
    );
    // Every row is visible: up to one row more than the Command Palette.
    assert_eq!(p.max_rows, Some(PALETTE_ROWS + 1));
    assert_eq!(editor.palette_geometry(p).rows, p.items.len());
    editor.event(key(VK_RETURN, false, false), &mut ctx);
    assert_eq!(text(&editor), "I receive it\n");
    editor.event(key(VK_Z, true, false), &mut ctx);
    assert_eq!(text(&editor), "I recieve it\n");
    // Switching to the Command Palette restores its row limit.
    editor.select_single(Selection::caret(4));
    editor.event(key(VK_OEM_PERIOD, true, false), &mut ctx);
    settle(&mut editor, &mut ctx);
    editor.event(key(VK_P, true, true), &mut ctx);
    let Some(Popup::Palette(p)) = &editor.popup else {
        panic!("no palette");
    };
    assert_eq!((p.mode, p.max_rows), (Mode::All, None));
    assert_eq!(editor.palette_geometry(p).rows, PALETTE_ROWS);
}

#[test]
fn a_late_suggestion_never_edits_a_changed_word() {
    let mut ctx = Ctx::headless(800, 600, 96);
    let mut editor = open(r"C:\docs\a.txt", "I recieve it\n", true, &mut ctx);
    settle(&mut editor, &mut ctx);
    editor.select_single(Selection::caret(4));
    editor.perform(Action::SpellingSuggestions, &mut ctx);
    settle(&mut editor, &mut ctx);
    // The text changes while the palette is open (another command).
    editor
        .doc
        .apply(
            &Command {
                tx: Transaction::single(Edit::insert(0, b"A ".as_slice())),
                kind: edit::EditKind::Other,
                collapse: false,
            },
            None,
        )
        .unwrap();
    editor.run_spelling_row(0);
    assert_eq!(text(&editor), "A I recieve it\n");
    assert!(
        editor
            .notice
            .as_deref()
            .unwrap()
            .contains("nothing was replaced")
    );
}

#[test]
fn next_and_previous_misspelling_select_and_wrap() {
    let mut ctx = Ctx::headless(800, 600, 96);
    let mut editor = open(r"C:\docs\a.txt", "one teh two wrod three\n", true, &mut ctx);
    settle(&mut editor, &mut ctx);
    editor.event(key(VK_F8, false, false), &mut ctx);
    settle(&mut editor, &mut ctx);
    assert_eq!(selected(&editor), "teh");
    editor.event(key(VK_F8, false, false), &mut ctx);
    settle(&mut editor, &mut ctx);
    assert_eq!(selected(&editor), "wrod");
    editor.event(key(VK_F8, false, false), &mut ctx);
    settle(&mut editor, &mut ctx);
    assert_eq!(selected(&editor), "teh", "wraps");
    editor.event(key(VK_F8, false, true), &mut ctx);
    settle(&mut editor, &mut ctx);
    assert_eq!(selected(&editor), "wrod");
    // An edit cancels a scan; Cancelled is not shown as "none".
    editor.event(key(VK_F8, false, false), &mut ctx);
    editor.event(Event::Char('x'), &mut ctx);
    assert!(editor.spell.as_ref().unwrap().nav.is_none());
    assert_ne!(editor.notice.as_deref(), Some("No misspellings found"));
}

#[test]
fn added_and_ignored_words_lose_their_marks() {
    let mut ctx = Ctx::headless(800, 600, 96);
    let mut editor = open(r"C:\docs\a.txt", "teh wrod teh\n", true, &mut ctx);
    settle(&mut editor, &mut ctx);
    assert_eq!(words(&editor), ["teh", "wrod", "teh"]);
    editor.select_single(Selection::caret(1));
    editor.perform(Action::AddToDictionary, &mut ctx);
    assert_eq!(words(&editor), ["wrod"]);
    settle(&mut editor, &mut ctx);
    assert_eq!(words(&editor), ["wrod"], "the thread knows the word");
    editor.select_single(Selection::caret(5));
    editor.perform(Action::IgnoreWord, &mut ctx);
    settle(&mut editor, &mut ctx);
    assert!(words(&editor).is_empty());
    // Toggling off stops the thread and clears the marks.
    editor.perform(Action::ToggleSpelling, &mut ctx);
    assert!(editor.spell.is_none() && editor.doc.spell_journal.is_none());
    let mut pixels = vec![0u32; 800 * 600];
    editor.paint(&mut pixels, 800, 600);
    assert!(editor.list.waves.is_empty());
}

/// An undo then a different edit can repeat a revision: replies and marks
/// of the first branch never come back.
#[test]
fn replies_of_another_branch_are_dropped() {
    let mut ctx = Ctx::headless(800, 600, 96);
    let mut editor = open(r"C:\docs\a.txt", "o\n", true, &mut ctx);
    settle(&mut editor, &mut ctx);
    editor.select_single(Selection::caret(1));
    editor.event(Event::Char('q'), &mut ctx);
    editor.event(Event::Char('z'), &mut ctx);
    let branch_a = editor.doc.snapshot.clone();
    editor.event(key(VK_Z, true, false), &mut ctx);
    editor.event(Event::Char('n'), &mut ctx);
    editor.event(Event::Char('e'), &mut ctx);
    assert_eq!(branch_a.rev(), editor.doc.snapshot.rev());
    assert!(!branch_a.same_tree(&editor.doc.snapshot));
    settle(&mut editor, &mut ctx);
    assert!(words(&editor).is_empty(), "{:?}", words(&editor));
}

#[test]
fn markdown_marks_only_prose_and_needs_syntax() {
    let mut ctx = Ctx::headless(800, 600, 96);
    let text = "# Titel\n\nSome wrod and `codez` here.\n\n```\nfenc\n```\n";
    let mut editor = open(r"C:\docs\a.md", text, true, &mut ctx);
    settle(&mut editor, &mut ctx);
    assert_eq!(words(&editor), ["Titel", "wrod"]);
    editor.perform(Action::ToggleSyntax, &mut ctx);
    settle(&mut editor, &mut ctx);
    assert!(words(&editor).is_empty());
    assert!(
        editor
            .notice
            .as_deref()
            .unwrap()
            .contains("syntax highlighting")
    );
    // `--syntax none` checks the file as plain text.
    let mut plain = open(r"C:\docs\b.md", "`codez`\n", true, &mut ctx);
    plain.settings.highlighting = false;
    plain.highlighting = false;
    plain.restart_syntax();
    settle(&mut plain, &mut ctx);
    assert_eq!(words(&plain), ["codez"]);
}

/// Text that becomes code by an edit is never marked once the reply for
/// the new text arrives.
#[test]
fn markdown_edits_that_make_code_drop_the_marks() {
    let mut ctx = Ctx::headless(800, 600, 96);
    let mut editor = open(r"C:\docs\a.md", "Some wrod here.\n", true, &mut ctx);
    settle(&mut editor, &mut ctx);
    assert_eq!(words(&editor), ["wrod"]);
    editor.select_single(Selection::caret(5));
    editor.event(Event::Char('`'), &mut ctx);
    editor.select_single(Selection::caret(10));
    editor.event(Event::Char('`'), &mut ctx);
    assert_eq!(text(&editor), "Some `wrod` here.\n");
    settle(&mut editor, &mut ctx);
    assert!(words(&editor).is_empty(), "{:?}", words(&editor));
}

/// Above `syntax.maxFileMiB` Markdown has no verified structure: no marks.
#[test]
fn markdown_above_the_syntax_limit_is_not_checked() {
    let mut ctx = Ctx::headless(800, 600, 96);
    let mut editor = open(r"C:\docs\a.md", "Some wrod here.\n", true, &mut ctx);
    editor.settings.syntax_max_mib = 0;
    editor.restart_syntax();
    settle(&mut editor, &mut ctx);
    assert!(editor.syntax.is_none());
    assert!(words(&editor).is_empty());
}

/// A view with more misspellings than one reply holds is marked
/// completely, over several partial replies.
#[test]
fn dense_views_keep_every_visible_mark() {
    let mut ctx = Ctx::headless(2400, 1800, 96);
    let text = format!("{}\n", "zzq ".repeat(200)).repeat(200);
    let mut editor = open(r"C:\docs\a.txt", &text, true, &mut ctx);
    settle(&mut editor, &mut ctx);
    let ranges = editor.spelling_ranges();
    let visible = text
        .match_indices("zzq")
        .map(|(at, _)| at as u64)
        .filter(|&at| ranges.iter().any(|r| r.start < at + 3 && at < r.end))
        .count();
    assert!(visible > crate::spelling::MAX_MARKS, "{visible}");
    assert_eq!(words(&editor).len(), visible);
    assert!(editor.spelling_complete());
}

/// Add or Ignore during a scan stops it: F8 is not left busy and works
/// again.
#[test]
fn ignore_during_a_scan_cancels_it() {
    let mut ctx = Ctx::headless(800, 600, 96);
    let text = format!(
        "teh {}wrod\n",
        "the quick brown fox jumps over the lazy dog\n".repeat(100_000)
    );
    let mut editor = open(r"C:\docs\a.txt", &text, true, &mut ctx);
    settle(&mut editor, &mut ctx);
    editor.select_single(Selection::caret(1));
    editor.event(key(VK_F8, false, false), &mut ctx);
    assert!(editor.spelling_busy());
    editor.perform(Action::IgnoreWord, &mut ctx);
    assert!(editor.spell.as_ref().unwrap().nav.is_none());
    settle(&mut editor, &mut ctx);
    editor.event(key(VK_F8, false, false), &mut ctx);
    settle(&mut editor, &mut ctx);
    assert_eq!(selected(&editor), "wrod");
}
