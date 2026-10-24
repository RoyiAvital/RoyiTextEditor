//! Command palette state: query, filtered rows, selection and scrolling.
//! `Ctrl+Shift+P` lists every command; `Ctrl+P` lists recent commands and
//! favorites. The same list also picks one of a set of choices (languages,
//! symbols).

use std::cmp::Reverse;
use std::ops::Range;

use crate::commands::{COMMANDS, CommandDef};
use crate::favorites::Store;
use crate::fuzzy::Matcher;

/// Most characters in a query.
pub const QUERY_MAX: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    All,
    Favorites,
    /// Change Language Mode.
    Language,
    /// Go to Symbol in Editor.
    Symbol,
}

/// Rows of a choice mode: label and right-aligned hint.
#[derive(Clone, Debug, Default)]
pub struct Choices {
    pub labels: Vec<String>,
    pub hints: Vec<String>,
    /// Choices kept in list order (symbols), not sorted by score.
    pub keep_order: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Header(&'static str),
    Command {
        /// Index into [`COMMANDS`].
        index: usize,
        /// Range of [`Palette::matched`]: label characters matching the query.
        matched: Range<usize>,
    },
    Choice {
        /// Index into [`Palette::choices`].
        index: usize,
        matched: Range<usize>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    WordLeft,
    WordRight,
    Home,
    End,
}

/// One-line text field with a caret and a selection (byte offsets).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Query {
    text: String,
    caret: usize,
    anchor: usize,
}

impl Query {
    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn caret(&self) -> usize {
        self.caret
    }

    pub fn selection(&self) -> Range<usize> {
        self.caret.min(self.anchor)..self.caret.max(self.anchor)
    }

    /// Replaces the selection with `text` minus control characters, up to
    /// [`QUERY_MAX`] characters in total. Returns true when the text changed.
    pub fn insert(&mut self, text: &str) -> bool {
        let removed = self.delete_selection();
        let room = QUERY_MAX.saturating_sub(self.text.chars().count());
        let mut added = false;
        for ch in text.chars().filter(|c| !c.is_control()).take(room) {
            self.text.insert(self.caret, ch);
            self.caret += ch.len_utf8();
            added = true;
        }
        self.anchor = self.caret;
        removed || added
    }

    /// Deletes the selection, or the character or word before (after, when
    /// `forward`) the caret. Returns true when the text changed.
    pub fn delete(&mut self, forward: bool, word: bool) -> bool {
        if self.delete_selection() {
            return true;
        }
        let target = match (forward, word) {
            (false, false) => self.prev(),
            (true, false) => self.next(),
            (false, true) => self.word_left(),
            (true, true) => self.word_right(),
        };
        let range = self.caret.min(target)..self.caret.max(target);
        if range.is_empty() {
            return false;
        }
        self.caret = range.start;
        self.anchor = range.start;
        self.text.replace_range(range, "");
        true
    }

    fn delete_selection(&mut self) -> bool {
        let range = self.selection();
        if range.is_empty() {
            return false;
        }
        self.caret = range.start;
        self.anchor = range.start;
        self.text.replace_range(range, "");
        true
    }

    /// Moves the caret; `extend` keeps the anchor. Returns true on a change.
    pub fn move_caret(&mut self, motion: Motion, extend: bool) -> bool {
        let before = (self.caret, self.anchor);
        let selection = self.selection();
        self.caret = match motion {
            Motion::Left if !extend && !selection.is_empty() => selection.start,
            Motion::Right if !extend && !selection.is_empty() => selection.end,
            Motion::Left => self.prev(),
            Motion::Right => self.next(),
            Motion::WordLeft => self.word_left(),
            Motion::WordRight => self.word_right(),
            Motion::Home => 0,
            Motion::End => self.text.len(),
        };
        if !extend {
            self.anchor = self.caret;
        }
        (self.caret, self.anchor) != before
    }

    pub fn select_all(&mut self) -> bool {
        let before = (self.caret, self.anchor);
        self.anchor = 0;
        self.caret = self.text.len();
        (self.caret, self.anchor) != before
    }

    /// Replaces the text (control characters except `\n` removed, up to
    /// [`QUERY_MAX`] characters) and selects it all.
    pub fn set(&mut self, text: &str) {
        self.text.clear();
        self.caret = 0;
        self.anchor = 0;
        self.insert_lines(text);
        self.select_all();
    }

    /// Like [`Query::insert`], but keeps line breaks (`\r\n` and `\r`
    /// become `\n`).
    pub fn insert_lines(&mut self, text: &str) -> bool {
        let removed = self.delete_selection();
        let room = QUERY_MAX.saturating_sub(self.text.chars().count());
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let mut added = false;
        for ch in text
            .chars()
            .filter(|&c| c == '\n' || !c.is_control())
            .take(room)
        {
            self.text.insert(self.caret, ch);
            self.caret += ch.len_utf8();
            added = true;
        }
        self.anchor = self.caret;
        removed || added
    }

    pub fn selected_text(&self) -> &str {
        &self.text[self.selection()]
    }

    fn prev(&self) -> usize {
        self.text[..self.caret]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    fn next(&self) -> usize {
        self.text[self.caret..]
            .chars()
            .next()
            .map_or(self.caret, |c| self.caret + c.len_utf8())
    }

    /// Start of the word before the caret (spaces before the caret skipped).
    fn word_left(&self) -> usize {
        let before = self.text[..self.caret].trim_end();
        before
            .char_indices()
            .rev()
            .find(|(_, c)| c.is_whitespace())
            .map_or(0, |(i, c)| i + c.len_utf8())
    }

    /// Start of the next word: the rest of this word and the spaces after it.
    fn word_right(&self) -> usize {
        let after = &self.text[self.caret..];
        let word = after.find(char::is_whitespace).unwrap_or(after.len());
        let spaces = after[word..]
            .find(|c: char| !c.is_whitespace())
            .unwrap_or(after.len() - word);
        self.caret + word + spaces
    }
}

pub struct Palette {
    pub mode: Mode,
    pub query: Query,
    pub items: Vec<Item>,
    /// Rows of the choice modes.
    pub choices: Choices,
    /// Matched character indices of every command item.
    matched: Vec<u32>,
    /// Index into `items` of the selected command.
    pub selected: Option<usize>,
    /// First visible item.
    pub first: usize,
    matcher: Matcher,
    /// Score, command and matched range of one section (reused).
    scored: Vec<(i32, usize, Range<usize>)>,
}

impl Palette {
    pub fn new(mode: Mode, store: &Store, recent_count: usize) -> Palette {
        let mut palette = Palette {
            mode,
            query: Query::default(),
            items: Vec::new(),
            choices: Choices::default(),
            matched: Vec::new(),
            selected: None,
            first: 0,
            matcher: Matcher::default(),
            scored: Vec::new(),
        };
        palette.refresh(store, recent_count);
        palette
    }

    pub fn matched(&self, range: &Range<usize>) -> &[u32] {
        &self.matched[range.clone()]
    }

    /// Rebuilds the rows for the query; selects the first command.
    pub fn refresh(&mut self, store: &Store, recent_count: usize) {
        self.items.clear();
        self.matched.clear();
        self.matcher.set_query(self.query.as_str());
        match self.mode {
            Mode::All => self.push_matches(None, 0..COMMANDS.len()),
            Mode::Favorites => {
                let recent = &store.recent()[..recent_count.min(store.recent().len())];
                self.push_matches(Some("recently used"), recent.iter().copied());
                let favorites =
                    (0..COMMANDS.len()).filter(|i| store.is_favorite(*i) && !recent.contains(i));
                self.push_matches(Some("favorites"), favorites);
            }
            Mode::Language | Mode::Symbol => self.push_choices(),
        }
        self.first = 0;
        let first = self.commands().next();
        self.selected = first;
    }

    /// Opens a choice mode on `choices`.
    pub fn set_choices(&mut self, mode: Mode, choices: Choices) {
        self.mode = mode;
        self.choices = choices;
        self.query = Query::default();
    }

    fn push_choices(&mut self) {
        self.scored.clear();
        for (index, label) in self.choices.labels.iter().enumerate() {
            let start = self.matched.len();
            if let Some(score) = self.matcher.score(label, &mut self.matched) {
                self.scored.push((score, index, start..self.matched.len()));
            }
        }
        if !self.choices.keep_order {
            self.scored.sort_by_key(|(score, ..)| Reverse(*score));
        }
        self.items.extend(
            self.scored
                .drain(..)
                .map(|(_, index, matched)| Item::Choice { index, matched }),
        );
    }

    /// Index into [`Palette::choices`] of the choice at `item`.
    pub fn choice_at(&self, item: usize) -> Option<usize> {
        match self.items.get(item) {
            Some(Item::Choice { index, .. }) => Some(*index),
            _ => None,
        }
    }

    /// Selects the row of choice `index` when listed.
    pub fn select_choice(&mut self, index: usize, rows: usize) {
        if let Some(item) = self
            .items
            .iter()
            .position(|i| matches!(i, Item::Choice { index: c, .. } if *c == index))
        {
            self.selected = Some(item);
            self.reveal(rows);
        }
    }

    /// Rebuilds after a list change, keeping the selection near its row.
    pub fn refresh_in_place(&mut self, store: &Store, recent_count: usize) {
        let (selected, first) = (self.selected, self.first);
        self.refresh(store, recent_count);
        let near = selected.and_then(|s| self.commands().find(|&c| c >= s));
        self.selected = near.or_else(|| self.commands().last());
        self.first = first.min(self.items.len().saturating_sub(1));
    }

    fn push_matches(
        &mut self,
        header: Option<&'static str>,
        indices: impl IntoIterator<Item = usize>,
    ) {
        self.scored.clear();
        for index in indices {
            let start = self.matched.len();
            if let Some(score) = self.matcher.score(COMMANDS[index].label, &mut self.matched) {
                self.scored.push((score, index, start..self.matched.len()));
            }
        }
        if self.scored.is_empty() {
            return;
        }
        // Stable: equal scores keep their section order.
        self.scored.sort_by_key(|(score, ..)| Reverse(*score));
        if let Some(header) = header {
            self.items.push(Item::Header(header));
        }
        self.items.extend(
            self.scored
                .drain(..)
                .map(|(_, index, matched)| Item::Command { index, matched }),
        );
    }

    /// Item indices of selectable rows (commands and choices).
    fn commands(&self) -> impl Iterator<Item = usize> + '_ {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| !matches!(item, Item::Header(_)))
            .map(|(i, _)| i)
    }

    pub fn command_at(&self, item: usize) -> Option<&'static CommandDef> {
        match self.items.get(item) {
            Some(Item::Command { index, .. }) => Some(&COMMANDS[*index]),
            _ => None,
        }
    }

    /// Registry index of the command at `item`.
    pub fn index_at(&self, item: usize) -> Option<usize> {
        match self.items.get(item) {
            Some(Item::Command { index, .. }) => Some(*index),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn selected_command(&self) -> Option<&'static CommandDef> {
        self.selected.and_then(|i| self.command_at(i))
    }

    /// Moves the selection by `delta` commands; single steps wrap around.
    pub fn move_selection(&mut self, delta: isize, rows: usize) {
        let n = self.commands().count() as isize;
        if n == 0 {
            return;
        }
        let current = self
            .selected
            .and_then(|s| self.commands().position(|c| c == s))
            .unwrap_or(0) as isize;
        let next = if delta.abs() == 1 {
            (current + delta).rem_euclid(n)
        } else {
            (current + delta).clamp(0, n - 1)
        };
        let next = self.commands().nth(next as usize);
        self.selected = next;
        self.reveal(rows);
    }

    /// Scrolls so the selection (and a header right above it) is visible.
    pub fn reveal(&mut self, rows: usize) {
        let Some(selected) = self.selected else {
            return;
        };
        let rows = rows.max(1);
        let top = if selected > 0 && matches!(self.items[selected - 1], Item::Header(_)) {
            selected - 1
        } else {
            selected
        };
        if top < self.first {
            self.first = top;
        } else if selected >= self.first + rows {
            self.first = selected + 1 - rows;
        }
    }

    /// Scrolls by `delta` rows without moving the selection.
    pub fn scroll(&mut self, delta: isize, rows: usize) {
        let last = self.items.len().saturating_sub(rows.max(1));
        self.first = self.first.saturating_add_signed(delta).min(last);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{by_id, index_of};

    const SAVE: &str = "workbench.action.files.save";
    const UNDO: &str = "undo";
    const ABOUT: &str = "workbench.action.showAboutDialog";

    fn labels(p: &Palette) -> Vec<&str> {
        p.items
            .iter()
            .map(|item| match item {
                Item::Header(h) => h,
                Item::Command { index, .. } => COMMANDS[*index].label,
                Item::Choice { index, .. } => p.choices.labels[*index].as_str(),
            })
            .collect()
    }

    /// A store with `favorites` starred and `recent` run, most recent first.
    fn store(favorites: &[&str], recent: &[&str]) -> Store {
        let mut store = Store::memory();
        for id in favorites {
            store.set_favorite(index_of(id).unwrap(), true);
        }
        for id in recent.iter().rev() {
            store.push_recent(index_of(id).unwrap());
        }
        store
    }

    fn set_query(p: &mut Palette, text: &str, store: &Store) {
        p.query.select_all();
        p.query.insert(text);
        p.refresh(store, 3);
    }

    #[test]
    fn choices_filter_and_select() {
        let store = Store::memory();
        let mut p = Palette::new(Mode::All, &store, 3);
        let names = ["Plain Text", "Python", "Rust", "TypeScript"];
        p.set_choices(
            Mode::Language,
            Choices {
                labels: names.iter().map(|s| s.to_string()).collect(),
                hints: vec![String::new(); 4],
                keep_order: false,
            },
        );
        p.refresh(&store, 3);
        assert_eq!(labels(&p), names);
        p.select_choice(2, 10);
        assert_eq!(p.selected.and_then(|i| p.choice_at(i)), Some(2));
        assert_eq!(p.command_at(p.selected.unwrap()).map(|c| c.id), None);
        set_query(&mut p, "ty", &store);
        assert_eq!(labels(&p), ["TypeScript"]);
        assert_eq!(p.selected.and_then(|i| p.choice_at(i)), Some(3));
        set_query(&mut p, "t", &store);
        p.move_selection(1, 10);
        assert!(p.selected.and_then(|i| p.choice_at(i)).is_some());
    }

    #[test]
    fn all_commands_filter_by_fuzzy_score() {
        let empty = Store::memory();
        let mut p = Palette::new(Mode::All, &empty, 3);
        assert_eq!(p.items.len(), COMMANDS.len());
        assert_eq!(p.selected, Some(0));
        set_query(&mut p, "fsave", &empty);
        assert_eq!(labels(&p)[0], "File: Save");
        let Item::Command { matched, .. } = &p.items[0] else {
            panic!("a command row");
        };
        assert_eq!(p.matched(matched), [0, 6, 7, 8, 9]);
        set_query(&mut p, "zoom", &empty);
        // Equal scores keep the registry order.
        assert_eq!(
            labels(&p),
            vec!["View: Reset Zoom", "View: Zoom In", "View: Zoom Out"]
        );
        set_query(&mut p, "qqq", &empty);
        assert!(p.items.is_empty());
        assert_eq!(p.selected, None);
    }

    #[test]
    fn favorites_show_recent_first_then_starred_without_duplicates() {
        let s = store(&[UNDO, SAVE], &[SAVE, ABOUT, UNDO]);
        let p = Palette::new(Mode::Favorites, &s, 2);
        assert_eq!(
            labels(&p),
            vec![
                "recently used",
                "File: Save",
                "Application: About",
                "favorites",
                "Edit: Undo"
            ]
        );
        assert_eq!(p.selected, Some(1), "headers are never selected");
        let p = Palette::new(Mode::Favorites, &s, 0);
        assert_eq!(labels(&p), vec!["favorites", "Edit: Undo", "File: Save"]);
        let p = Palette::new(Mode::Favorites, &Store::memory(), 3);
        assert!(p.items.is_empty());
    }

    #[test]
    fn filtering_drops_empty_sections() {
        let s = store(&[UNDO], &[SAVE]);
        let mut p = Palette::new(Mode::Favorites, &s, 3);
        set_query(&mut p, "undo", &s);
        assert_eq!(labels(&p), vec!["favorites", "Edit: Undo"]);
        assert_eq!(p.selected_command().map(|c| c.id), Some(UNDO));
    }

    #[test]
    fn selection_skips_headers_wraps_and_pages() {
        let s = store(&[UNDO, ABOUT], &[SAVE]);
        let mut p = Palette::new(Mode::Favorites, &s, 3);
        // recently used, Save, favorites, Undo, About
        assert_eq!(p.selected, Some(1));
        p.move_selection(1, 10);
        assert_eq!(p.selected, Some(3));
        p.move_selection(1, 10);
        p.move_selection(1, 10);
        assert_eq!(p.selected, Some(1), "wraps to the first command");
        p.move_selection(-1, 10);
        assert_eq!(p.selected, Some(4), "wraps to the last command");
        p.move_selection(-12, 10);
        assert_eq!(p.selected, Some(1), "pages stop at the ends");
        assert_eq!(
            p.selected_command().map(|c| c.id),
            by_id(SAVE).map(|c| c.id)
        );
    }

    #[test]
    fn scrolling_keeps_the_selection_visible() {
        let mut p = Palette::new(Mode::All, &Store::memory(), 3);
        p.move_selection(5, 3);
        assert_eq!(p.selected, Some(5));
        assert_eq!(p.first, 3);
        p.move_selection(-1, 3);
        p.move_selection(-1, 3);
        assert_eq!(p.first, 3);
        p.move_selection(-1, 3);
        assert_eq!(p.first, 2);
        p.scroll(100, 3);
        assert_eq!(p.first, COMMANDS.len() - 3);
        p.scroll(-100, 3);
        assert_eq!(p.first, 0);
    }

    #[test]
    fn query_editing_moves_selects_and_deletes() {
        let mut q = Query::default();
        assert!(q.insert("save file"));
        assert!(q.move_caret(Motion::WordLeft, false));
        assert_eq!(q.caret(), 5);
        assert!(q.move_caret(Motion::Left, true));
        assert_eq!(q.selection(), 4..5);
        assert!(q.insert("_"));
        assert_eq!(q.as_str(), "save_file");
        assert!(q.move_caret(Motion::Home, false));
        assert!(q.move_caret(Motion::WordRight, true));
        assert_eq!(q.selection(), 0..9);
        assert!(q.move_caret(Motion::Right, false), "collapses to the end");
        assert_eq!(q.caret(), 9);
        assert!(q.delete(false, false));
        assert_eq!(q.as_str(), "save_fil");
        q.insert(" two words");
        assert!(q.delete(false, true));
        assert_eq!(q.as_str(), "save_fil two ");
        q.move_caret(Motion::Home, false);
        assert!(q.delete(true, true));
        assert_eq!(q.as_str(), "two ");
        assert!(q.delete(true, false));
        assert_eq!(q.as_str(), "wo ");
        assert!(!q.move_caret(Motion::Left, false), "already at the start");
        assert!(q.select_all());
        assert!(q.insert("undo"));
        assert_eq!(q.as_str(), "undo");
    }

    #[test]
    fn queries_drop_control_characters_and_stay_bounded() {
        let mut q = Query::default();
        q.insert("a\tb\r\nc\u{7}d");
        assert_eq!(q.as_str(), "abcd");
        q.insert(&"\u{4E2D}".repeat(QUERY_MAX * 2));
        assert_eq!(q.as_str().chars().count(), QUERY_MAX);
        assert!(!q.insert("x"), "full");
        q.move_caret(Motion::Left, false);
        assert_eq!(q.caret(), q.as_str().len() - "\u{4E2D}".len());
    }
}
