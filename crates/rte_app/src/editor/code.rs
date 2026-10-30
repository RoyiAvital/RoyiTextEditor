//! Code-aware commands: language mode, comments, auto-closing pairs,
//! indentation after `Enter`, brackets, Expand/Shrink Selection and Go to
//! Symbol. Text-only edits come from `rte_edit`; tree queries from
//! `rte_syntax` (with text fallbacks when no tree is parsed).

use rte_edit::{Coded, CommentMode, CommentTokens};
use rte_syntax::{Grammar, LANGUAGES};

use super::*;
use crate::palette::Choices;
use crate::syntax::{SYMBOL_LIMIT, Symbols};

/// Expand Selection without a syntax tree: word, line, document.
fn text_enclosing(s: &Snapshot, range: std::ops::Range<u64>) -> Option<std::ops::Range<u64>> {
    let word = edit::word_at(s, range.start);
    if word.start <= range.start && range.end <= word.end && word != range && !word.is_empty() {
        return Some(word);
    }
    let first = s.line_of(range.start);
    let last = s.line_of(range.end);
    let line = edit::content_range(s, first).start..edit::content_range(s, last).end;
    if line.start <= range.start && range.end <= line.end && line != range {
        return Some(line);
    }
    (range != (0..s.len())).then(|| 0..s.len())
}

impl Editor {
    /// `Ctrl+K M`: the palette lists Plain Text and the languages.
    pub(super) fn open_language_mode(&mut self) {
        self.open_palette(Mode::All);
        let mut labels = vec!["Plain Text".to_owned()];
        let mut hints = vec!["plaintext".to_owned()];
        for lang in &LANGUAGES {
            labels.push(lang.name.to_owned());
            hints.push(lang.id.to_owned());
        }
        let current = self
            .language
            .and_then(|l| LANGUAGES.iter().position(|x| std::ptr::eq(x, l)))
            .map_or(0, |i| i + 1);
        self.open_choices(
            Mode::Language,
            Choices {
                labels,
                hints,
                keep_order: false,
                groups: Vec::new(),
            },
            current,
        );
    }

    fn open_choices(&mut self, mode: Mode, choices: Choices, selected: usize) {
        let Some(Popup::Palette(p)) = &mut self.popup else {
            return;
        };
        p.set_choices(mode, choices);
        if let Some(store) = &self.store {
            p.refresh(store, self.settings.recent_count);
        }
        let Some(Popup::Palette(p)) = &self.popup else {
            return;
        };
        let rows = self.palette_geometry(p).rows;
        if let Some(Popup::Palette(p)) = &mut self.popup {
            p.select_choice(selected, rows);
        }
    }

    /// Applies choice `index` of Change Language Mode (0: Plain Text).
    pub(super) fn set_language(&mut self, index: usize) {
        self.language = index.checked_sub(1).map(|i| &LANGUAGES[i]);
        self.language_chosen = true;
        if self.language.is_some() {
            self.highlighting = true;
        }
        let name = self.language.map_or("Plain Text", |l| l.name);
        self.notice = Some(format!("Language: {name}"));
        self.restart_syntax();
    }

    /// Applies a code-aware edit; a command without edits only moves the
    /// selections.
    pub(super) fn run_coded(&mut self, coded: Coded, typed: Option<char>) {
        if coded.cmd.tx.edits().is_empty() {
            self.doc.select(coded.selections);
            self.reveal();
            return;
        }
        match self
            .doc
            .apply_selecting(&coded.cmd, typed, Some(coded.selections))
        {
            Ok(()) => self.reveal(),
            Err(reason) => self.notice = Some(format!("Not editable: {reason}")),
        }
    }

    /// Language at `pos`: the embedded language inside injected code.
    pub(super) fn lang_at(&self, pos: u64) -> Option<&'static Lang> {
        let lang = self.language?;
        let view = self.syntax.as_ref().and_then(Syntax::view);
        Some(view.map_or(lang, |v| v.lang_at(pos, &self.doc.snapshot)))
    }

    /// The language at any position can be told: the host embeds none, or
    /// the trees' positions are exact (far edits make them approximate
    /// until the next parse).
    fn languages_known(&self) -> bool {
        !self
            .language
            .is_some_and(|l| matches!(l.grammar, Grammar::Markdown | Grammar::Html))
            || self.syntax.as_ref().is_none_or(Syntax::exact)
    }

    /// `Ctrl+/` and `Shift+Alt+A`, with the tokens of the language at each
    /// line or selection.
    /// Line comments of `mode`, or Toggle Block Comment when `None`.
    pub(super) fn comment(&mut self, mode: Option<CommentMode>) {
        let block = mode.is_none();
        let Some(lang) = self.language else {
            self.notice = Some("Plain text has no comments".to_owned());
            return;
        };
        if !self.languages_known() {
            self.notice = Some("Embedded languages are still being parsed; try again".to_owned());
            return;
        }
        let (s, sels) = (&self.doc.snapshot, &self.doc.selections);
        let coded = if block {
            edit::toggle_block_comment(s, sels, &|pos| {
                self.lang_at(pos).and_then(|l| l.block_comment)
            })
        } else {
            let mode = mode.unwrap_or(CommentMode::Toggle);
            edit::line_comment(s, sels, mode, &|pos| {
                let l = self.lang_at(pos).unwrap_or(lang);
                CommentTokens {
                    line: l.line_comment,
                    block: l.block_comment,
                }
            })
        };
        match coded {
            Some(coded) => self.run_coded(coded, None),
            None if block => self.notice = Some(format!("{} has no block comments", lang.name)),
            None => {}
        }
    }

    /// Typing `ch`: closes pairs of the language. True when handled.
    pub(super) fn auto_close(&mut self, ch: char) -> bool {
        if self.language.is_none() || self.doc.options.overtype || !self.languages_known() {
            return false;
        }
        let pairs_at = |pos: u64| self.lang_at(pos).map_or(&[][..], |l| l.auto_close);
        match edit::auto_close(&self.doc.snapshot, &self.doc.selections, ch, &pairs_at) {
            Some(coded) => {
                self.run_coded(coded, Some(ch));
                true
            }
            None => false,
        }
    }

    /// `Backspace` inside an empty pair. True when handled.
    pub(super) fn delete_pair(&mut self) -> bool {
        if self.language.is_none() || !self.languages_known() {
            return false;
        }
        let pairs_at = |pos: u64| self.lang_at(pos).map_or(&[][..], |l| l.auto_close);
        match edit::delete_pair(&self.doc.snapshot, &self.doc.selections, &pairs_at) {
            Some(coded) => {
                self.run_coded(coded, None);
                true
            }
            None => false,
        }
    }

    /// `Enter` after an opening bracket (or a Python/YAML `:`). True when
    /// handled.
    pub(super) fn newline_indent(&mut self) -> bool {
        if self.language.is_none() || !self.languages_known() {
            return false;
        }
        let opts = self.edit_options();
        let rules_at = |pos: u64| match self.lang_at(pos) {
            Some(l) => (
                l.brackets,
                matches!(l.grammar, Grammar::Python | Grammar::Yaml),
            ),
            None => (&[][..], false),
        };
        match edit::newline_indent(&self.doc.snapshot, &self.doc.selections, &opts, &rules_at) {
            Some(coded) => {
                self.run_coded(coded, None);
                true
            }
            None => false,
        }
    }

    /// The bracket next to `at` and its partner, from final trees only.
    fn find_bracket(&self, at: u64) -> Option<(u64, u64)> {
        let lang = self.lang_at(at)?;
        let view = self
            .syntax
            .as_ref()
            .and_then(|syntax| syntax.final_view(&self.doc.snapshot, at.saturating_sub(1)..at + 1));
        rte_syntax::matching_bracket(&self.doc.snapshot, view, lang.brackets, at)
    }

    /// The bracket next to the primary caret and its partner.
    pub(super) fn bracket_pair(&self) -> Option<(u64, u64)> {
        self.language?;
        let primary = self.doc.selections.primary();
        if !primary.is_empty() {
            return None;
        }
        let at = primary.head;
        let generation = self.syntax.as_ref().map_or(0, Syntax::generation);
        if let Some((s, caret, g, pair)) = &*self.bracket_cache.borrow()
            && s.same_tree(&self.doc.snapshot)
            && (*caret, *g) == (at, generation)
        {
            return *pair;
        }
        let pair = self.find_bracket(at);
        *self.bracket_cache.borrow_mut() = Some((self.doc.snapshot.clone(), at, generation, pair));
        pair
    }

    /// `Ctrl+Shift+\`: moves the caret to the matching bracket.
    pub(super) fn go_to_bracket(&mut self) {
        if self.language.is_none() {
            return;
        }
        let sels = self
            .doc
            .selections
            .map(|sel| match self.find_bracket(sel.head) {
                Some((_, partner)) => Selection::caret(partner),
                None => *sel,
            });
        self.doc.select(sels);
        self.reveal();
    }

    /// `Shift+Alt+Right`: each selection grows to its enclosing syntax node
    /// (word, line and document without a tree).
    pub(super) fn expand_selection(&mut self) {
        let s = &self.doc.snapshot;
        let before = self.doc.selections.clone();
        let sels = before.map(|sel| {
            let range = sel.range();
            let grown = self
                .syntax
                .as_ref()
                .and_then(|syntax| syntax.final_view(s, range.clone()))
                .and_then(|v| rte_syntax::enclosing_node(v, range.clone()))
                .or_else(|| text_enclosing(s, range));
            match grown {
                Some(r) => Selection::new(r.start, r.end),
                None => *sel,
            }
        });
        if sels == before {
            return;
        }
        if self.expanded.as_ref() != Some(&before) {
            self.expansions.clear();
        }
        self.expansions.push(before);
        self.expanded = Some(sels.clone());
        self.doc.select(sels);
        self.reveal();
    }

    /// `Shift+Alt+Left`: undoes the last Expand Selection.
    pub(super) fn shrink_selection(&mut self) {
        if self.expanded.as_ref() != Some(&self.doc.selections) {
            self.expansions.clear();
            return;
        }
        let Some(previous) = self.expansions.pop() else {
            return;
        };
        self.expanded = (!self.expansions.is_empty()).then(|| previous.clone());
        self.doc.select(previous);
        self.reveal();
    }

    /// `Ctrl+Shift+O`: the palette lists the definitions of the document,
    /// collected by the syntax thread.
    pub(super) fn open_symbols(&mut self) {
        let Some(syntax) = &mut self.syntax else {
            self.notice = Some(match self.language {
                None => "Plain text has no symbols".to_owned(),
                Some(_) => "Symbols need syntax highlighting".to_owned(),
            });
            return;
        };
        syntax.request_symbols();
        let ready = syntax.take_symbols();
        self.symbols_pending = true;
        self.symbol_offsets.clear();
        self.open_palette(Mode::All);
        self.open_choices(
            Mode::Symbol,
            Choices {
                labels: Vec::new(),
                hints: Vec::new(),
                keep_order: true,
                groups: Vec::new(),
            },
            0,
        );
        if let Some(symbols) = ready {
            self.show_symbols(&symbols);
        }
    }

    /// Lists the symbols collected for an open Go to Symbol palette, keeping
    /// a query typed meanwhile.
    pub(super) fn show_symbols(&mut self, symbols: &Symbols) {
        if !std::mem::take(&mut self.symbols_pending) {
            return;
        }
        let Some(Popup::Palette(p)) = &mut self.popup else {
            return;
        };
        if p.mode != Mode::Symbol {
            return;
        }
        let query = std::mem::take(&mut p.query);
        let s = &self.doc.snapshot;
        let head = self.doc.selections.primary().head;
        let list = &symbols.list;
        let current = list.iter().rposition(|sym| sym.start <= head).unwrap_or(0);
        let mut labels = Vec::with_capacity(list.len());
        let mut hints = Vec::with_capacity(list.len());
        for sym in list {
            labels.push(sym.name.clone());
            hints.push(format!("{}  Ln {}", sym.kind, s.line_of(sym.start) + 1));
        }
        self.symbol_offsets = list.iter().map(|sym| sym.start).collect();
        self.open_choices(
            Mode::Symbol,
            Choices {
                labels,
                hints,
                keep_order: true,
                groups: Vec::new(),
            },
            current,
        );
        if !query.as_str().is_empty()
            && let Some(Popup::Palette(p)) = &mut self.popup
        {
            p.query = query;
            if let Some(store) = &self.store {
                p.refresh(store, self.settings.recent_count);
            }
        }
        if symbols.partial {
            self.notice = Some("Symbols of the parsed part; the file is still parsing".to_owned());
        } else if symbols.truncated {
            self.notice = Some(format!("Showing the first {SYMBOL_LIMIT} symbols"));
        }
    }

    /// Moves the caret to symbol `index` of the last Go to Symbol list.
    pub(super) fn go_to_symbol(&mut self, index: usize) {
        let Some(&at) = self.symbol_offsets.get(index) else {
            return;
        };
        let at = at.min(self.doc.snapshot.len());
        self.select_single(Selection::caret(at));
        // Show the definition near the top third of the view.
        let line = self.doc.snapshot.line_of(at);
        let page = self.page();
        if line < self.scroll.top_line || line >= self.scroll.top_line + page {
            self.scroll.top_line = line.saturating_sub(page / 3);
            self.clamp_scroll();
        }
        self.reveal();
    }
}
