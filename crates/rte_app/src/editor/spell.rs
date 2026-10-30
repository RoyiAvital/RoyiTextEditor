//! Spelling on the UI thread: marks of the drawn rows, the status line
//! hint, the Spelling palette (`Ctrl+.`), Next/Previous Misspelling (`F8`),
//! Add to Dictionary and Ignore. The spelling thread starts the first time
//! spelling is on for plain text or Markdown after the first frame; with
//! spelling off nothing runs. Replies count only for the current text,
//! generation and request; actions revalidate the word before acting.

use std::ops::Range;

use rte_text::{Bias, Edit, Transaction};

use super::*;
use crate::document::Journaled;
use crate::palette::Choices;
use crate::spelling::{
    self, Config, Event as SpellEvent, Lane, MAX_REQUEST_BYTES, Mark, Outcome, Request,
    SpellThread, Work,
};

/// What spelling checks in the current file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Plain,
    /// Prose only: the syntax worker answers which text is not prose.
    Markdown,
}

/// A row of the Spelling palette.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Row {
    Suggestion(String),
    /// Suggestions are being collected (does nothing).
    Pending,
    /// No suggestions (does nothing).
    Nothing,
    Add,
    Ignore,
    Next,
    Previous,
}

/// The misspelled word of an open Spelling palette.
struct Target {
    id: u64,
    snapshot: Snapshot,
    mark: Mark,
    suggestions: Option<Vec<String>>,
}

/// A running Next/Previous Misspelling.
struct Nav {
    id: u64,
    backwards: bool,
}

pub(super) struct Spell {
    thread: SpellThread,
    kind: Option<Kind>,
    /// Increases when verdicts change (Add, Ignore, another engine, the
    /// file kind): older replies are dropped.
    generation: u64,
    next_id: u64,
    /// Text of `marks` and `checked`.
    snapshot: Snapshot,
    /// Sorted misspelled words near the view; after an edit, shifted ones
    /// are shown until the reply for the new text replaces them.
    marks: Vec<Mark>,
    /// The ranges of `marks` (drawn).
    pub(super) ranges: Vec<Range<u64>>,
    /// Sorted, disjoint ranges of `snapshot` checked in this generation.
    checked: Vec<Range<u64>>,
    /// The Marks request in flight: id and ranges.
    requested: Option<(u64, Vec<Range<u64>>)>,
    /// No verified structure for this text and generation (Markdown).
    blocked: bool,
    /// The Markdown notice was shown.
    noticed: bool,
    target: Option<Target>,
    nav: Option<Nav>,
    /// Rows of the open Spelling palette.
    pub(super) rows: Vec<Row>,
    /// The ranges of the view, cached.
    view: RefCell<Option<ViewRanges>>,
}

/// Text, (scroll, rows, columns, tab width) and the spelling ranges.
type ViewRanges = (Snapshot, ViewKey, Vec<Range<u64>>);
type ViewKey = (Scroll, u32, u32, u32);

/// `sorted` (disjoint) minus `minus` (sorted, disjoint).
fn subtract(sorted: &[Range<u64>], minus: &[Range<u64>]) -> Vec<Range<u64>> {
    let mut out = Vec::new();
    let mut j = 0;
    for r in sorted {
        let mut start = r.start;
        while j < minus.len() && minus[j].end <= start {
            j += 1;
        }
        let mut k = j;
        while start < r.end {
            match minus.get(k) {
                Some(m) if m.start < r.end => {
                    if m.start > start {
                        out.push(start..m.start);
                    }
                    start = start.max(m.end);
                    k += 1;
                }
                _ => {
                    out.push(start..r.end);
                    break;
                }
            }
        }
    }
    out
}

/// Adds `range` to sorted, disjoint `ranges`, merging touching ones.
fn insert_range(ranges: &mut Vec<Range<u64>>, range: Range<u64>) {
    let i = ranges.partition_point(|r| r.end < range.start);
    let mut j = i;
    let mut merged = range;
    while j < ranges.len() && ranges[j].start <= merged.end {
        merged = merged.start.min(ranges[j].start)..merged.end.max(ranges[j].end);
        j += 1;
    }
    ranges.splice(i..j, [merged]);
}

/// Shifts `marks` through the recorded edits from `from` to `to`; marks an
/// edit touches are dropped. Without a complete record all are dropped.
fn map_marks(
    marks: &mut Vec<Mark>,
    from: &Snapshot,
    to: &Snapshot,
    journal: Option<Vec<Journaled>>,
) {
    let mut at = from.clone();
    for step in journal.into_iter().flatten() {
        if !step.before.same_tree(&at) {
            break;
        }
        let changes = step.changes.changes();
        marks.retain_mut(|m| {
            let i = changes.partition_point(|c| c.old.end < m.range.start);
            if changes.get(i).is_some_and(|c| c.old.start <= m.range.end) {
                return false;
            }
            let start = step.changes.map(m.range.start, Bias::Right);
            m.range = start..start + (m.range.end - m.range.start);
            true
        });
        at = step.after;
    }
    if !at.same_tree(to) {
        marks.clear();
    }
}

impl Spell {
    fn rebuild(&mut self) {
        self.ranges = self.marks.iter().map(|m| m.range.clone()).collect();
    }

    /// Verdicts changed: everything is checked again; a running scan stops.
    fn reset(&mut self) {
        self.generation += 1;
        self.checked.clear();
        self.requested = None;
        self.blocked = false;
        if self.nav.take().is_some() {
            self.thread.cancel(Lane::Navigate);
        }
    }

    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// The mark containing `pos` (its ends included).
    fn mark_at(&self, pos: u64) -> Option<&Mark> {
        let i = self.marks.partition_point(|m| m.range.end < pos);
        self.marks.get(i).filter(|m| m.range.start <= pos)
    }
}

impl Editor {
    /// What spelling checks in this file: plain text (also with
    /// `--syntax none`) and Markdown; nothing while loading.
    fn spell_kind(&self) -> Option<Kind> {
        if self.no_file() || matches!(self.status, LoadStatus::Loading { .. }) {
            return None;
        }
        if !self.settings.highlighting {
            return Some(Kind::Plain);
        }
        match self.language {
            None => Some(Kind::Plain),
            Some(lang) if lang.grammar == rte_syntax::Grammar::Markdown => Some(Kind::Markdown),
            Some(_) => None,
        }
    }

    /// Byte ranges of the visible rows for spelling: the drawn columns of
    /// each row and the drawn tail of a long line. Cached for the text and
    /// view.
    fn spelling_ranges(&self) -> Vec<Range<u64>> {
        let s = &self.doc.snapshot;
        let layout = self.layout();
        let tab = self.options.tab_width;
        let key = (self.scroll, layout.rows, layout.cols, tab);
        if let Some(spell) = &self.spell
            && let Some((snapshot, k, ranges)) = &*spell.view.borrow()
            && snapshot.same_tree(s)
            && *k == key
        {
            return ranges.clone();
        }
        let rows = u64::from(layout.rows.max(1));
        let first = self.scroll.first_col;
        let end_col = first + u64::from(layout.cols) + 1;
        // A column holds at most 4 bytes (tabs hold fewer).
        let reach = (end_col * 4 + 64).min(ROW_SCAN_LIMIT as u64);
        let mut ranges: Vec<Range<u64>> = Vec::new();
        for line in self.scroll.top_line..self.scroll.top_line + rows {
            let Some(range) = s.line_range(line) else {
                break;
            };
            let bytes = s.to_vec(range.start..range.end.min(range.start + reach));
            let from = if first == 0 {
                0
            } else {
                edit::offset_at_column(&bytes, first, tab).min(bytes.len())
            };
            let to = edit::offset_at_column(&bytes, end_col, tab).min(bytes.len());
            if to > from {
                insert_range(
                    &mut ranges,
                    range.start + from as u64..range.start + to as u64,
                );
            }
            if let Some(tail) = self.long_row_tail(range) {
                insert_range(&mut ranges, tail);
            }
        }
        if let Some(spell) = &self.spell {
            *spell.view.borrow_mut() = Some((s.clone(), key, ranges.clone()));
        }
        ranges
    }

    /// Starts spelling when needed, follows edits and the view, and asks for
    /// the marks of the rows not checked yet. Called after each event.
    pub(super) fn sync_spelling(&mut self) {
        if !self.spell_on || !self.shown {
            return;
        }
        let kind = self.spell_kind();
        if self.spell.is_none() {
            if kind.is_none() {
                return;
            }
            let config = Config {
                engine: self.settings.spelling_engine,
                hunspell: self.settings.hunspell.clone(),
                storage: self.settings.user_dir.as_ref().map(|dir| {
                    let path = dir.join("Spelling").join(spelling::USER_FILE);
                    Box::new(FileStorage(path)) as Box<dyn favorites::Storage>
                }),
            };
            match SpellThread::spawn(config, self.wake.clone()) {
                Ok(thread) => {
                    self.spell = Some(Box::new(Spell {
                        thread,
                        kind,
                        generation: 0,
                        next_id: 0,
                        snapshot: self.doc.snapshot.clone(),
                        marks: Vec::new(),
                        ranges: Vec::new(),
                        checked: Vec::new(),
                        requested: None,
                        blocked: false,
                        noticed: false,
                        target: None,
                        nav: None,
                        rows: Vec::new(),
                        view: RefCell::new(None),
                    }));
                }
                Err(e) => {
                    self.spell_on = false;
                    self.notice = Some(format!("Spelling off: {e}"));
                    return;
                }
            }
        }
        let port = match kind {
            Some(Kind::Markdown) => self.syntax.as_ref().map(Syntax::mask_port),
            _ => None,
        };
        let ranges = if kind.is_some() {
            self.spelling_ranges()
        } else {
            Vec::new()
        };
        let journal = if kind.is_some() {
            self.doc.spell_journal.replace(Vec::new())
        } else {
            self.doc.spell_journal.take()
        };
        let mut notice = None;
        let mut changed = false;
        let spell = self.spell.as_deref_mut().expect("started above");
        if spell.kind != kind {
            spell.kind = kind;
            spell.reset();
            spell.marks.clear();
            changed = true;
        }
        if !spell.snapshot.same_tree(&self.doc.snapshot) {
            let before = spell.marks.len();
            map_marks(
                &mut spell.marks,
                &spell.snapshot,
                &self.doc.snapshot,
                journal,
            );
            changed |= before > 0;
            spell.snapshot = self.doc.snapshot.clone();
            spell.checked.clear();
            spell.requested = None;
            spell.blocked = false;
            if spell.nav.take().is_some() {
                spell.thread.cancel(Lane::Navigate);
                if self
                    .notice
                    .as_deref()
                    .is_some_and(|n| n.starts_with("Searching for misspellings"))
                {
                    notice = Some(String::new());
                }
            }
        }
        if kind == Some(Kind::Markdown) && port.is_none() {
            // No verified structure: no marks.
            changed |= !spell.marks.is_empty();
            spell.marks.clear();
            spell.checked.clear();
            spell.requested = None;
            if !spell.noticed {
                spell.noticed = true;
                notice = Some(
                    "Spelling paused: Markdown is checked with syntax highlighting on".to_owned(),
                );
            }
        } else if kind.is_some() {
            // Keep only what is near the view.
            if let (Some(first), Some(last)) = (ranges.first(), ranges.last()) {
                let hull = first.start..last.end;
                let before = spell.marks.len();
                spell
                    .marks
                    .retain(|m| m.range.end >= hull.start && m.range.start <= hull.end);
                changed |= spell.marks.len() != before;
                spell
                    .checked
                    .retain(|r| r.end >= hull.start && r.start <= hull.end);
            }
            let mut missing = subtract(&ranges, &spell.checked);
            let mut budget = MAX_REQUEST_BYTES;
            missing.retain_mut(|r| {
                let take = (r.end - r.start).min(budget);
                r.end = r.start + take;
                budget -= take;
                take > 0
            });
            if missing.is_empty() {
                if spell.requested.take().is_some() {
                    spell.thread.cancel(Lane::Marks);
                }
            } else if !spell.blocked && spell.requested.as_ref().is_none_or(|(_, r)| *r != missing)
            {
                let id = spell.id();
                spell.requested = Some((id, missing.clone()));
                spell.thread.send(Request {
                    id,
                    generation: spell.generation,
                    snapshot: self.doc.snapshot.clone(),
                    mask: port,
                    work: Work::Marks(missing),
                });
            }
        }
        if changed {
            spell.rebuild();
            self.spell_rev += 1;
        }
        match notice.as_deref() {
            Some("") => self.notice = None,
            Some(_) => self.notice = notice,
            None => {}
        }
    }

    /// Applies the spelling thread's replies that still match the current
    /// text, generation and request.
    pub(super) fn drain_spelling(&mut self) {
        let Some(spell) = self.spell.as_deref_mut() else {
            return;
        };
        let mut notices = Vec::new();
        let mut found = None;
        let mut suggestions = false;
        let mut changed = false;
        while let Some(event) = spell.thread.try_recv() {
            let reply = match event {
                SpellEvent::Notice { text, reset } => {
                    notices.push(text);
                    if reset {
                        spell.reset();
                        spell.nav = None;
                        if let Some(t) = &mut spell.target {
                            t.suggestions = Some(Vec::new());
                            suggestions = true;
                        }
                    }
                    continue;
                }
                SpellEvent::Reply(reply) => reply,
            };
            if reply.generation != spell.generation || !reply.snapshot.same_tree(&self.doc.snapshot)
            {
                continue;
            }
            match (reply.lane, reply.outcome) {
                (Lane::Marks, outcome) => {
                    if spell
                        .requested
                        .as_ref()
                        .is_none_or(|(id, _)| *id != reply.id)
                    {
                        continue;
                    }
                    spell.requested = None;
                    match outcome {
                        Outcome::Marks { covered, marks } => {
                            for range in &covered {
                                spell.marks.retain(|m| {
                                    m.range.end <= range.start || m.range.start >= range.end
                                });
                                insert_range(&mut spell.checked, range.clone());
                            }
                            spell.marks.extend(marks);
                            spell.marks.sort_by_key(|m| m.range.start);
                            spell.marks.dedup_by(|a, b| a.range == b.range);
                            changed = true;
                        }
                        _ => spell.blocked = true,
                    }
                }
                (Lane::Suggest, Outcome::Suggestions(list)) => {
                    if let Some(t) = &mut spell.target
                        && t.id == reply.id
                    {
                        t.suggestions = Some(list);
                        suggestions = true;
                    }
                }
                (Lane::Suggest, _) => {}
                (Lane::Navigate, outcome) => {
                    if spell.nav.as_ref().is_none_or(|n| n.id != reply.id) {
                        continue;
                    }
                    match outcome {
                        Outcome::Progress { scanned, total } => {
                            let pct = scanned * 100 / total.max(1);
                            notices.push(format!("Searching for misspellings\u{2026} {pct}%"));
                        }
                        Outcome::Found(mark) => {
                            spell.nav = None;
                            found = Some((mark, None));
                        }
                        Outcome::Exhausted(Some(mark)) => {
                            spell.nav = None;
                            found = Some((mark, Some("No other misspellings")));
                        }
                        Outcome::Exhausted(None) => {
                            spell.nav = None;
                            notices.push("No misspellings found".to_owned());
                        }
                        Outcome::Unavailable | Outcome::Marks { .. } | Outcome::Suggestions(_) => {
                            spell.nav = None;
                            notices.push(
                                "Spelling: the Markdown structure is not available".to_owned(),
                            );
                        }
                    }
                }
            }
        }
        if changed {
            spell.rebuild();
            self.spell_rev += 1;
        }
        if let Some(text) = notices.pop() {
            self.notice = Some(text);
        }
        if suggestions {
            self.refresh_spelling_palette();
        }
        if let Some((mark, notice)) = found {
            // The text is the reply's: the word is there unless the reply
            // were wrong.
            let s = &self.doc.snapshot;
            if s.to_vec(mark.range.clone()) == mark.word.as_bytes() {
                self.select_single(Selection::new(mark.range.start, mark.range.end));
                self.reveal();
                self.notice = notice.map(str::to_owned);
            }
        }
    }

    /// Spelling work a benchmark step waits for.
    pub(super) fn spelling_busy(&self) -> bool {
        let palette = matches!(&self.popup, Some(Popup::Palette(p)) if p.mode == Mode::Spelling);
        self.spell.as_ref().is_some_and(|s| {
            s.requested.is_some()
                || s.nav.is_some()
                || palette && s.target.as_ref().is_some_and(|t| t.suggestions.is_none())
        })
    }

    /// Spelling is on for this file, so a startup benchmark waits for marks.
    pub(super) fn spelling_wanted(&self) -> bool {
        self.spell_on && self.spell_kind().is_some()
    }

    /// The marks of the view are complete.
    pub(super) fn spelling_complete(&self) -> bool {
        self.spell
            .as_ref()
            .is_some_and(|s| s.requested.is_none() && !s.checked.is_empty())
    }

    /// Status line hint with the caret on a misspelled word.
    pub(super) fn spelling_hint(&self) -> Option<String> {
        let spell = self.spell.as_ref().filter(|_| self.spell_on)?;
        let mark = spell.mark_at(self.doc.selections.primary().head)?;
        Some(format!("Spelling: {} (Ctrl+.)", mark.word))
    }

    /// The misspelled word at the caret, or the selection when it is exactly
    /// one.
    fn mark_at_caret(&self) -> Option<Mark> {
        let spell = self.spell.as_ref()?;
        let primary = self.doc.selections.primary();
        let mark = spell.mark_at(primary.head)?;
        let range = primary.range();
        (range.is_empty() || range == mark.range).then(|| mark.clone())
    }

    /// Why spelling cannot act here, if so.
    fn spelling_refusal(&self) -> Option<&'static str> {
        if !self.spell_on {
            Some("Spelling is off (View: Toggle Spelling)")
        } else if self.spell_kind().is_none() {
            Some("Spelling checks plain text and Markdown files")
        } else if self.spell.is_none() {
            Some("Spelling is starting")
        } else {
            None
        }
    }

    /// `View: Toggle Spelling` for the session.
    pub(super) fn toggle_spelling(&mut self) {
        self.spell_on = !self.spell_on;
        if !self.spell_on {
            self.spell = None;
            self.doc.spell_journal = None;
            self.spell_rev += 1;
        }
        let state = if self.spell_on { "on" } else { "off" };
        self.notice = Some(format!("Spelling {state}"));
    }

    /// `Ctrl+.`: the Spelling palette for the misspelled word at the caret
    /// (suggestions first), or only the commands elsewhere.
    pub(super) fn show_spelling(&mut self) {
        if let Some(reason) = self.spelling_refusal() {
            self.notice = Some(reason.to_owned());
            return;
        }
        let mark = self.mark_at_caret();
        let snapshot = self.doc.snapshot.clone();
        let mask = None;
        let spell = self.spell.as_deref_mut().expect("checked above");
        spell.target = match mark {
            Some(mark) => {
                let id = spell.id();
                spell.thread.send(Request {
                    id,
                    generation: spell.generation,
                    snapshot: snapshot.clone(),
                    mask,
                    work: Work::Suggest(mark.clone()),
                });
                Some(Target {
                    id,
                    snapshot,
                    mark,
                    suggestions: None,
                })
            }
            None => None,
        };
        self.open_palette(Mode::All);
        let choices = self.spelling_choices();
        self.open_spelling_choices(choices, false);
    }

    /// Rows of the Spelling palette for the current target.
    fn spelling_choices(&mut self) -> Choices {
        let Some(spell) = self.spell.as_deref_mut() else {
            return Choices::default();
        };
        let mut rows = Vec::new();
        if let Some(t) = &spell.target {
            match &t.suggestions {
                None => rows.push(Row::Pending),
                Some(list) if list.is_empty() => rows.push(Row::Nothing),
                Some(list) => rows.extend(list.iter().cloned().map(Row::Suggestion)),
            }
            rows.extend([Row::Add, Row::Ignore]);
        }
        rows.extend([Row::Next, Row::Previous]);
        let word = spell.target.as_ref().map_or("", |t| t.mark.word.as_str());
        let hint = |action| {
            commands::COMMANDS
                .iter()
                .position(|c| c.action == action)
                .map_or("", commands::key_hint)
                .to_owned()
        };
        let mut choices = Choices {
            labels: Vec::new(),
            hints: Vec::new(),
            keep_order: true,
            groups: Vec::new(),
        };
        for row in &rows {
            let suggestion = matches!(row, Row::Suggestion(_) | Row::Pending | Row::Nothing);
            choices.groups.push(u8::from(!suggestion));
            let (label, hint) = match row {
                Row::Suggestion(s) => (s.clone(), String::new()),
                Row::Pending => ("Collecting suggestions\u{2026}".to_owned(), String::new()),
                Row::Nothing => ("No suggestions".to_owned(), String::new()),
                Row::Add => (
                    format!("Add \u{201C}{word}\u{201D} to Dictionary"),
                    String::new(),
                ),
                Row::Ignore => (format!("Ignore \u{201C}{word}\u{201D}"), String::new()),
                Row::Next => ("Next Misspelling".to_owned(), hint(Action::NextMisspelling)),
                Row::Previous => (
                    "Previous Misspelling".to_owned(),
                    hint(Action::PreviousMisspelling),
                ),
            };
            choices.labels.push(label);
            choices.hints.push(hint);
        }
        spell.rows = rows;
        choices
    }

    /// Shows `choices` in the open palette, keeping its query when
    /// `refresh`; the first row is selected.
    fn open_spelling_choices(&mut self, choices: Choices, refresh: bool) {
        let Some(Popup::Palette(p)) = &mut self.popup else {
            return;
        };
        if refresh {
            p.choices = choices;
        } else {
            p.set_choices(Mode::Spelling, choices);
        }
        // Room for the 8 suggestions, the separator and the 4 commands.
        p.max_rows = Some(PALETTE_ROWS + 1);
        if let Some(store) = &self.store {
            p.refresh(store, self.settings.recent_count);
        }
    }

    /// Suggestions arrived for the open Spelling palette.
    fn refresh_spelling_palette(&mut self) {
        if matches!(&self.popup, Some(Popup::Palette(p)) if p.mode == Mode::Spelling) {
            let choices = self.spelling_choices();
            self.open_spelling_choices(choices, true);
        }
    }

    /// Rows that do nothing keep the palette open.
    pub(super) fn spelling_row_inert(&self, index: usize) -> bool {
        self.spell
            .as_ref()
            .and_then(|s| s.rows.get(index))
            .is_none_or(|r| matches!(r, Row::Pending | Row::Nothing))
    }

    /// Runs row `index` of the Spelling palette.
    pub(super) fn run_spelling_row(&mut self, index: usize) {
        let Some(row) = self.spell.as_ref().and_then(|s| s.rows.get(index)).cloned() else {
            return;
        };
        match row {
            Row::Suggestion(text) => self.replace_misspelling(&text),
            Row::Add => self.add_word(false),
            Row::Ignore => self.add_word(true),
            Row::Next => self.next_misspelling(false),
            Row::Previous => self.next_misspelling(true),
            Row::Pending | Row::Nothing => {}
        }
    }

    /// The palette's word, when it is still at its place in the current text.
    fn current_target(&self) -> Option<Mark> {
        let t = self.spell.as_ref()?.target.as_ref()?;
        let s = &self.doc.snapshot;
        (t.snapshot.same_tree(s) && s.to_vec(t.mark.range.clone()) == t.mark.word.as_bytes())
            .then(|| t.mark.clone())
    }

    /// Replaces the palette's word with `text` (one undo step).
    fn replace_misspelling(&mut self, text: &str) {
        let Some(mark) = self.current_target() else {
            self.notice = Some("The word changed; nothing was replaced".to_owned());
            return;
        };
        let cmd = Command {
            tx: Transaction::single(Edit::new(mark.range, text.as_bytes())),
            kind: edit::EditKind::Other,
            collapse: true,
        };
        self.run(Some(cmd), None);
    }

    /// Adds the misspelled word (of the palette, else at the caret) to the
    /// user dictionary, or ignores it for the session (`ignore`).
    pub(super) fn add_word(&mut self, ignore: bool) {
        if let Some(reason) = self.spelling_refusal() {
            self.notice = Some(reason.to_owned());
            return;
        }
        let palette = matches!(&self.popup, Some(Popup::Palette(p)) if p.mode == Mode::Spelling);
        let mark = if palette {
            self.current_target()
        } else {
            self.mark_at_caret()
        };
        let Some(mark) = mark else {
            self.notice = Some("No misspelled word at the caret".to_owned());
            return;
        };
        let spell = self.spell.as_deref_mut().expect("checked above");
        if ignore {
            spell.thread.ignore(&mark.word);
        } else {
            spell.thread.add(&mark.word);
        }
        let lower = mark.word.to_lowercase();
        spell
            .marks
            .retain(|m| m.word != mark.word && m.word.to_lowercase() != lower);
        spell.target = None;
        spell.reset();
        spell.rebuild();
        self.spell_rev += 1;
        self.notice = Some(if ignore {
            format!("Ignoring \u{201C}{}\u{201D} for this session", mark.word)
        } else {
            format!("Added \u{201C}{}\u{201D} to the dictionary", mark.word)
        });
    }

    /// `F8` / `Shift+F8`: selects the next (previous) misspelled word,
    /// wrapping once. Repeating the key while a scan runs continues it.
    pub(super) fn next_misspelling(&mut self, backwards: bool) {
        if let Some(reason) = self.spelling_refusal() {
            self.notice = Some(reason.to_owned());
            return;
        }
        let port = match self.spell_kind() {
            Some(Kind::Markdown) => match &self.syntax {
                Some(syntax) => Some(syntax.mask_port()),
                None => {
                    self.notice = Some(
                        "Spelling: Markdown is checked with syntax highlighting on".to_owned(),
                    );
                    return;
                }
            },
            _ => None,
        };
        let primary = self.doc.selections.primary();
        let origin = if backwards {
            primary.start()
        } else {
            primary.end()
        };
        let snapshot = self.doc.snapshot.clone();
        let spell = self.spell.as_deref_mut().expect("checked above");
        if spell.nav.as_ref().is_some_and(|n| n.backwards == backwards) {
            return;
        }
        let id = spell.id();
        spell.nav = Some(Nav { id, backwards });
        spell.thread.send(Request {
            id,
            generation: spell.generation,
            snapshot,
            mask: port,
            work: Work::Navigate { origin, backwards },
        });
    }
}

#[cfg(test)]
mod tests;
