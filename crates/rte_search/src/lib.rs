//! Search over snapshots: Text (literal), Glob and RegEx modes with Match
//! Case and Whole Word, plus replacement expansion.
//!
//! Text with Match Case uses `memchr::memmem`; everything else compiles to
//! the automata engine (`regex-cursor`, linear time over chunks). Patterns
//! with lookaround or backreferences use `fancy-regex` per line (multiline
//! patterns in windows of at most [`WINDOW`] bytes), bounded by a backtrack
//! limit. Matches are found on demand; nothing stores a document-wide list.

mod cursor;
mod glob;
mod replace;

use std::ops::Range;

use fancy_regex::RegexBuilder;
use memchr::memmem;
use regex_cursor::Input;
use regex_cursor::engines::meta;
use regex_cursor::regex_automata::util::syntax;
use regex_cursor::regex_automata::{Anchored, PatternID};
use rte_text::Snapshot;

use crate::cursor::{Bytes, DocCursor};

use replace::CASE_GROWTH;
pub use replace::{Group, Replacement, preserve_case};

/// Text a multiline backtracking pattern sees at once.
pub const WINDOW: u64 = 1024 * 1024;
/// Longer lines are skipped by the backtracking engine.
pub const LONG_LINE: u64 = 16 * 1024 * 1024;
/// Backtracking steps per search before "too complex".
const BACKTRACK_LIMIT: usize = 1_000_000;
/// Compiled automata size limit.
const NFA_LIMIT: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Mode {
    #[default]
    Text,
    Glob,
    Regex,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Query {
    pub text: String,
    pub mode: Mode,
    pub match_case: bool,
    pub whole_word: bool,
}

enum Engine {
    Literal(Box<memmem::Finder<'static>>),
    Automata(meta::Regex),
    Backtrack {
        re: fancy_regex::Regex,
        multiline: bool,
    },
}

/// A compiled query.
pub struct Matcher {
    engine: Engine,
    whole_word: bool,
}

impl Matcher {
    /// Compiles `query`; `Ok(None)` for an empty query. Errors are one line.
    pub fn new(query: &Query) -> Result<Option<Matcher>, String> {
        if query.text.is_empty() {
            return Ok(None);
        }
        let engine = match query.mode {
            Mode::Text if query.match_case => Engine::Literal(Box::new(
                memmem::Finder::new(query.text.as_bytes()).into_owned(),
            )),
            Mode::Text => automata(&glob::escape(&query.text), query.match_case)?,
            Mode::Glob => automata(&glob::to_regex(&query.text), query.match_case)?,
            Mode::Regex => regex(&query.text, query.match_case)?,
        };
        Ok(Some(Matcher {
            engine,
            whole_word: query.whole_word,
        }))
    }

    /// True for the literal engine, cheap enough for bounded UI-thread work.
    pub fn is_literal(&self) -> bool {
        matches!(self.engine, Engine::Literal(_))
    }

    /// True when two matches can never overlap (a literal that does not
    /// overlap itself): every occurrence is then a match of any forward
    /// search, so a search may start anywhere.
    pub fn never_overlaps(&self) -> bool {
        let Engine::Literal(finder) = &self.engine else {
            return false;
        };
        let n = finder.needle();
        (1..n.len()).all(|k| n[..k] != n[n.len() - k..])
    }

    /// Index of a named capture group.
    pub fn group_index(&self, name: &str) -> Option<usize> {
        match &self.engine {
            Engine::Literal(_) => None,
            Engine::Automata(re) => re.group_info().to_index(PatternID::ZERO, name),
            Engine::Backtrack { re, .. } => re.capture_names().position(|n| n == Some(name)),
        }
    }
}

fn automata(pattern: &str, match_case: bool) -> Result<Engine, String> {
    build_automata(pattern, match_case)
        .map(Engine::Automata)
        .map_err(|err| short_error(&err))
}

fn build_automata(pattern: &str, match_case: bool) -> Result<meta::Regex, String> {
    meta::Regex::builder()
        .syntax(
            syntax::Config::new()
                .case_insensitive(!match_case)
                .multi_line(true)
                .crlf(true),
        )
        .configure(meta::Regex::config().nfa_size_limit(Some(NFA_LIMIT)))
        .build(pattern)
        .map_err(|err| {
            let mut text = err.to_string();
            let mut source = std::error::Error::source(&err);
            while let Some(inner) = source {
                text = inner.to_string();
                source = inner.source();
            }
            text
        })
}

fn regex(pattern: &str, match_case: bool) -> Result<Engine, String> {
    let err = match build_automata(pattern, match_case) {
        Ok(re) => return Ok(Engine::Automata(re)),
        Err(err) => err,
    };
    if !(err.contains("look-around") || err.contains("backreference")) {
        return Err(short_error(&err));
    }
    let re = RegexBuilder::new(pattern)
        .case_insensitive(!match_case)
        .multi_line(true)
        .crlf(true)
        .backtrack_limit(BACKTRACK_LIMIT)
        .build()
        .map_err(|err| short_error(&err.to_string()))?;
    let multiline = pattern.contains(r"\n") || pattern.contains('\n');
    Ok(Engine::Backtrack { re, multiline })
}

/// One line from a multi-line parser error, with its pattern column.
fn short_error(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let message = lines
        .iter()
        .find_map(|l| l.trim_start().strip_prefix("error: "));
    let column = lines
        .iter()
        .find(|l| {
            let t = l.trim();
            !t.is_empty() && t.chars().all(|c| c == '^')
        })
        .and_then(|l| l.find('^'))
        .map(|col| col.saturating_sub(4) + 1);
    let mut out = match message {
        Some(m) => m.to_owned(),
        None => lines
            .iter()
            .map(|l| l.trim())
            .find(|l| !l.is_empty())
            .unwrap_or("Invalid pattern")
            .to_owned(),
    };
    if let Some(first) = out.get(..1) {
        out = first.to_uppercase() + &out[1..];
    }
    match column {
        Some(col) if message.is_some() => format!("{out} at {col}"),
        _ => out,
    }
}

fn is_word(ch: Option<char>) -> bool {
    ch.is_some_and(|c| c.is_alphanumeric() || c == '_')
}

/// Resumable state of an in-order match enumeration over `from..end`
/// ([`Searcher::next_match`]). It can be continued with a later `end`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchIter {
    /// Next search position.
    pub from: u64,
    /// Matches end at or before here.
    pub end: u64,
    /// End of the previous match: an empty match there is skipped.
    last_end: Option<u64>,
}

impl MatchIter {
    pub fn new(range: Range<u64>) -> MatchIter {
        MatchIter {
            from: range.start,
            end: range.end,
            last_end: None,
        }
    }

    /// Continues at `pos` (no match starts between the current position and
    /// `pos`).
    pub fn resume_at(&mut self, pos: u64) {
        self.from = self.from.max(pos);
    }
}

/// Result of [`Searcher::expand_within`].
#[derive(Debug, PartialEq, Eq)]
pub enum Expansion {
    Text(Vec<u8>),
    /// The expansion could need more bytes than allowed; nothing was copied.
    OverBudget,
    /// The groups could not be read: not a match on this text and scope.
    NotAMatch,
}

/// A backtracking window: its text (`None`: invalid UTF-8 or too long), the
/// line or position it was read for.
struct Window {
    range: Range<u64>,
    line_start: u64,
    made_at: u64,
    multiline: bool,
    text: Option<String>,
}

impl Window {
    /// Whether a search from `pos` sees the same text and enough context.
    fn covers(&self, pos: u64, multiline: bool, len: u64) -> bool {
        if self.multiline != multiline || pos >= self.range.end && pos < len {
            return false;
        }
        if multiline {
            pos >= self.made_at && (self.range.end == len || self.range.end - pos >= WINDOW / 2)
        } else {
            pos >= self.line_start
        }
    }
}

/// Searches one snapshot with one matcher. `stop` is polled while scanning;
/// once it returns true every call returns no match and [`Searcher::stopped`]
/// is set.
pub struct Searcher<'a> {
    matcher: &'a Matcher,
    snapshot: &'a Snapshot,
    stop: &'a dyn Fn() -> bool,
    /// Matches end at or before here (the search scope end).
    end: u64,
    bytes: Bytes<'a>,
    cache: Option<meta::Cache>,
    caps: Option<regex_cursor::regex_automata::util::captures::Captures>,
    /// Leaf the literal engine scanned last: start and bytes.
    leaf: (u64, &'a [u8]),
    buf: Vec<u8>,
    window: Option<Window>,
    stopped: bool,
    /// Lines the backtracking engine skipped: invalid UTF-8 or too long.
    pub skipped_lines: u64,
    /// Lines where the backtracking engine hit its limit.
    pub complex_lines: u64,
}

impl<'a> Searcher<'a> {
    pub fn new(matcher: &'a Matcher, snapshot: &'a Snapshot, stop: &'a dyn Fn() -> bool) -> Self {
        let (cache, caps) = match &matcher.engine {
            Engine::Automata(re) => (Some(re.create_cache()), Some(re.create_captures())),
            _ => (None, None),
        };
        Searcher {
            matcher,
            snapshot,
            stop,
            end: snapshot.len(),
            bytes: Bytes::new(snapshot),
            cache,
            caps,
            leaf: (0, &[]),
            buf: Vec::new(),
            window: None,
            stopped: false,
            skipped_lines: 0,
            complex_lines: 0,
        }
    }

    /// Limits every match, including re-matching for captures, to end at or
    /// before `end` (the search scope end).
    pub fn set_end(&mut self, end: u64) {
        self.end = end.min(self.snapshot.len());
    }

    pub fn stopped(&self) -> bool {
        self.stopped
    }

    /// First match inside `from..end` (start at or after `from`, end at or
    /// before `end`).
    pub fn find(&mut self, from: u64, end: u64) -> Option<Range<u64>> {
        let end = end.min(self.end);
        let mut from = from;
        while from <= end && !self.stopped {
            let m = self.raw(from, end)?;
            if self.is_whole_word(&m) {
                return Some(m);
            }
            from = m.start + self.char_len(m.start);
        }
        None
    }

    /// Next match of `it`, advancing it; zero-width matches advance by one
    /// character and are skipped right after a match.
    pub fn next_match(&mut self, it: &mut MatchIter) -> Option<Range<u64>> {
        let end = it.end.min(self.snapshot.len());
        while it.from <= end {
            let m = self.find(it.from, end)?;
            if m.is_empty() && it.last_end == Some(m.start) {
                it.from = m.start + self.char_len(m.start);
                continue;
            }
            it.last_end = Some(m.end);
            it.from = if m.is_empty() {
                m.end + self.char_len(m.end)
            } else {
                m.end
            };
            return Some(m);
        }
        None
    }

    /// Calls `f` for each match inside `range` in order until it returns false.
    pub fn for_each(&mut self, range: Range<u64>, mut f: impl FnMut(Range<u64>) -> bool) {
        let mut it = MatchIter::new(range);
        while let Some(m) = self.next_match(&mut it) {
            if !f(m) {
                return;
            }
        }
    }

    /// Matches inside the sorted `rows`, in order, at most `limit` of them
    /// (highlights). Touching rows are searched as one range.
    pub fn matches_in(&mut self, rows: &[Range<u64>], limit: usize) -> Vec<Range<u64>> {
        let mut out: Vec<Range<u64>> = Vec::new();
        let mut i = 0;
        while i < rows.len() && out.len() < limit {
            let mut range = rows[i].clone();
            i += 1;
            while i < rows.len() && rows[i].start <= range.end {
                range.end = range.end.max(rows[i].end);
                i += 1;
            }
            let from = out.last().map_or(range.start, |m| range.start.max(m.end));
            let mut it = MatchIter::new(from..range.end);
            while out.len() < limit {
                let Some(m) = self.next_match(&mut it) else {
                    break;
                };
                out.push(m);
            }
        }
        out
    }

    /// Last match ending at or before `before` in the order a forward search
    /// from `anchor` finds them. `anchor` must be where that order is known:
    /// the scope start or the start of a match of it. When matches never
    /// overlap, windows growing back from `before` give the same answer
    /// without reading from `anchor`.
    pub fn find_prev(&mut self, before: u64, anchor: u64) -> Option<Range<u64>> {
        let before = before.min(self.end);
        let last_in = |s: &mut Self, lo: u64| {
            let mut last = None;
            s.for_each(lo..before, |m| {
                last = Some(m);
                true
            });
            last
        };
        if !self.matcher.never_overlaps() {
            return last_in(self, anchor);
        }
        let mut size = 64 * 1024;
        loop {
            let lo = before.saturating_sub(size).max(anchor);
            let last = last_in(self, lo);
            if self.stopped {
                return None;
            }
            if last.is_some() || lo == anchor {
                return last;
            }
            size = size.saturating_mul(4);
        }
    }

    /// Capture groups when `m` is exactly a match on this snapshot ending
    /// within the search end (group 0 is `m`), else `None`.
    pub fn captures(&mut self, m: Range<u64>) -> Option<Vec<Option<Range<u64>>>> {
        let end = self.end;
        if m.end > end || !self.is_whole_word(&m) {
            return None;
        }
        let matcher: &'a Matcher = self.matcher;
        match &matcher.engine {
            Engine::Literal(finder) => {
                (self.snapshot.to_vec(m.clone()) == finder.needle()).then(|| vec![Some(m)])
            }
            Engine::Automata(re) => {
                let stop = self.stop;
                let mut cursor = DocCursor::at(self.snapshot, m.start, stop);
                let mut input = Input::new(&mut cursor);
                input.set_range(m.start as usize..end as usize);
                input.set_anchored(Anchored::Yes);
                let caps = self.caps.as_mut().expect("automata captures");
                let cache = self.cache.as_mut().expect("automata cache");
                re.search_captures_with(cache, &mut input, caps);
                let found = caps.get_match()?;
                if found.start() as u64 != m.start || found.end() as u64 != m.end {
                    return None;
                }
                Some(
                    (0..caps.group_len())
                        .map(|i| caps.get_group(i).map(|s| s.start as u64..s.end as u64))
                        .collect(),
                )
            }
            Engine::Backtrack { re, multiline } => {
                let window = self.load_window(m.start, *multiline);
                let text = self.window.as_ref()?.text.as_deref()?;
                let caps = re
                    .captures_from_pos(text, (m.start - window.start) as usize)
                    .ok()??;
                let whole = caps.get(0)?;
                let found = window.start + whole.start() as u64..window.start + whole.end() as u64;
                (found == m).then(|| {
                    (0..caps.len())
                        .map(|i| {
                            caps.get(i).map(|g| {
                                window.start + g.start() as u64..window.start + g.end() as u64
                            })
                        })
                        .collect()
                })
            }
        }
    }

    /// Replacement text for the match `m`, or `None` when `m` is not a match.
    pub fn expand(
        &mut self,
        replacement: &Replacement,
        m: Range<u64>,
        preserve: bool,
    ) -> Option<Vec<u8>> {
        let groups = self.captures(m.clone())?;
        Some(self.expand_with(replacement, m, preserve, &groups))
    }

    /// Like [`Searcher::expand`] for a match `m` this searcher just returned:
    /// it is not verified again unless the replacement reads groups. Refuses
    /// before copying when the expansion and its temporary copies could need
    /// more than `budget` bytes.
    pub fn expand_within(
        &mut self,
        replacement: &Replacement,
        m: Range<u64>,
        preserve: bool,
        budget: u64,
    ) -> Expansion {
        let groups = if replacement.uses_groups() {
            match self.captures(m.clone()) {
                Some(groups) => groups,
                None => return Expansion::NotAMatch,
            }
        } else {
            vec![Some(m.clone())]
        };
        let matcher = self.matcher;
        let mut bound = replacement.bound(&mut |g| {
            let index = match g {
                Group::Index(i) => *i,
                Group::Name(name) => matcher.group_index(name)?,
            };
            groups.get(index)?.as_ref().map(|r| r.end - r.start)
        });
        if preserve {
            bound = bound.saturating_mul(CASE_GROWTH) + (m.end - m.start);
        }
        // The result plus one copied piece or case-converted text at a time.
        if bound.saturating_mul(2) > budget {
            return Expansion::OverBudget;
        }
        Expansion::Text(self.expand_with(replacement, m, preserve, &groups))
    }

    fn expand_with(
        &mut self,
        replacement: &Replacement,
        m: Range<u64>,
        preserve: bool,
        groups: &[Option<Range<u64>>],
    ) -> Vec<u8> {
        let snapshot = self.snapshot;
        let matcher = self.matcher;
        let mut out = replacement.expand(&mut |g| {
            let index = match g {
                Group::Index(i) => *i,
                Group::Name(name) => matcher.group_index(name)?,
            };
            let range = groups.get(index)?.clone()?;
            Some(snapshot.to_vec(range))
        });
        if preserve {
            let matched = snapshot.to_vec(m);
            if let (Ok(matched), Ok(text)) =
                (std::str::from_utf8(&matched), std::str::from_utf8(&out))
            {
                out = preserve_case(matched, text).into_bytes();
            }
        }
        out
    }

    fn poll(&mut self) -> bool {
        if !self.stopped && (self.stop)() {
            self.stopped = true;
        }
        self.stopped
    }

    fn char_len(&mut self, pos: u64) -> u64 {
        self.bytes.char_at(pos).map_or(1, |(_, len)| len)
    }

    fn is_whole_word(&mut self, m: &Range<u64>) -> bool {
        if !self.matcher.whole_word {
            return true;
        }
        let at = |b: &mut Bytes<'_>, p| b.char_at(p).and_then(|(c, _)| c);
        let before = |b: &mut Bytes<'_>, p| b.char_before(p).and_then(|(c, _)| c);
        let b = &mut self.bytes;
        let (first, prev) = (at(b, m.start), before(b, m.start));
        let (last, next) = (before(b, m.end), at(b, m.end));
        !(is_word(prev) && is_word(first)) && !(is_word(last) && is_word(next))
    }

    /// First engine match inside `from..end`, before the whole-word filter.
    fn raw(&mut self, from: u64, end: u64) -> Option<Range<u64>> {
        if self.poll() {
            return None;
        }
        let matcher: &'a Matcher = self.matcher;
        match &matcher.engine {
            Engine::Literal(finder) => self.literal(finder, from, end),
            Engine::Automata(re) => {
                let stop = self.stop;
                let mut cursor = DocCursor::at(self.snapshot, from, stop);
                let found = {
                    let mut input = Input::new(&mut cursor);
                    input.set_range(from as usize..end as usize);
                    let cache = self.cache.as_mut().expect("automata cache");
                    re.search_with(cache, &mut input)
                };
                if cursor.stopped() {
                    self.stopped = true;
                    return None;
                }
                found.map(|m| m.start() as u64..m.end() as u64)
            }
            Engine::Backtrack { re, multiline } => self.backtrack(re, *multiline, from, end),
        }
    }

    fn literal(&mut self, finder: &memmem::Finder<'_>, from: u64, end: u64) -> Option<Range<u64>> {
        let n = finder.needle().len() as u64;
        let snapshot = self.snapshot;
        let mut pos = from;
        while pos + n <= end {
            if self.poll() {
                return None;
            }
            let (leaf_start, leaf) = self.leaf;
            if pos < leaf_start || pos >= leaf_start + leaf.len() as u64 {
                self.leaf = snapshot.chunk_at(pos)?;
            }
            let (leaf_start, leaf) = self.leaf;
            let stop = (leaf_start + leaf.len() as u64).min(end);
            let slice = &leaf[(pos - leaf_start) as usize..(stop - leaf_start) as usize];
            if let Some(i) = finder.find(slice) {
                let start = pos + i as u64;
                return Some(start..start + n);
            }
            if stop >= end {
                return None;
            }
            // A match crossing into the next leaf.
            if n > 1 {
                let lo = stop.saturating_sub(n - 1).max(pos);
                let hi = (stop + n - 1).min(end);
                self.buf.clear();
                for chunk in snapshot.chunks(lo..hi) {
                    self.buf.extend_from_slice(chunk);
                }
                if let Some(i) = finder.find(&self.buf) {
                    let start = lo + i as u64;
                    return Some(start..start + n);
                }
            }
            pos = stop;
        }
        None
    }

    /// Makes the backtracking window for a search from `pos` current (kept
    /// while later searches see the same text and enough context): its line
    /// with its line break and the preceding one (for look-behind), or for
    /// multiline patterns whole lines from up to [`WINDOW`] / 2 bytes before
    /// `pos` to [`WINDOW`] bytes after it. Returns its range.
    fn load_window(&mut self, pos: u64, multiline: bool) -> Range<u64> {
        let s = self.snapshot;
        let len = s.len();
        if let Some(w) = &self.window
            && w.covers(pos, multiline, len)
        {
            return w.range.clone();
        }
        let line = s.line_of(pos);
        let line_start = s.line_start(line).unwrap_or(len);
        let line_end = s.line_start(line + 1).unwrap_or(len);
        let mut start = self.break_before(line_start);
        let mut end = line_end;
        if multiline {
            let back = s
                .line_start(s.line_of(pos.saturating_sub(WINDOW / 2)))
                .unwrap_or(0);
            if pos - back <= WINDOW {
                start = start.min(back);
            }
            let target = (pos + WINDOW).min(len);
            if target > end {
                end = s.line_start(s.line_of(target) + 1).unwrap_or(len);
            }
        }
        let valid = if line_end - line_start > LONG_LINE {
            None
        } else {
            self.read_valid(start..end, line_start..line_end)
        };
        if let Some(old) = self.window.take().and_then(|w| w.text) {
            self.buf = old.into_bytes();
        }
        let (range, text) = match valid {
            Some((range, text)) => (range, Some(text)),
            None => {
                self.skipped_lines += 1;
                (line_start..line_end, None)
            }
        };
        self.window = Some(Window {
            range: range.clone(),
            line_start,
            made_at: pos,
            multiline,
            text,
        });
        range
    }

    /// Start of the line break that ends just before `line_start`.
    fn break_before(&mut self, line_start: u64) -> u64 {
        match line_start {
            0 => 0,
            2.. if self.bytes.get(line_start - 1) == Some(b'\n')
                && self.bytes.get(line_start - 2) == Some(b'\r') =>
            {
                line_start - 2
            }
            _ => line_start - 1,
        }
    }

    /// Reads `range` as text, leaving out whole lines with invalid UTF-8
    /// around the line `own`; `None` when `own` itself is invalid.
    fn read_valid(
        &mut self,
        mut range: Range<u64>,
        own: Range<u64>,
    ) -> Option<(Range<u64>, String)> {
        let s = self.snapshot;
        let mut buf = std::mem::take(&mut self.buf);
        buf.clear();
        for chunk in s.chunks(range.clone()) {
            buf.extend_from_slice(chunk);
        }
        loop {
            match String::from_utf8(buf) {
                Ok(text) => return Some((range, text)),
                Err(err) => {
                    let bad = range.start + err.utf8_error().valid_up_to() as u64;
                    buf = err.into_bytes();
                    if bad < own.start {
                        let next = s.line_start(s.line_of(bad) + 1).unwrap_or(own.start);
                        let next = self.break_before(next).max(bad + 1);
                        buf.drain(..(next - range.start) as usize);
                        range.start = next;
                    } else if bad >= own.end {
                        let cut = s.line_start(s.line_of(bad)).unwrap_or(own.end).max(own.end);
                        buf.truncate((cut - range.start) as usize);
                        range.end = cut;
                    } else {
                        self.buf = buf;
                        return None;
                    }
                }
            }
        }
    }

    fn backtrack(
        &mut self,
        re: &fancy_regex::Regex,
        multiline: bool,
        from: u64,
        end: u64,
    ) -> Option<Range<u64>> {
        let len = self.snapshot.len();
        let mut pos = from;
        while pos <= end {
            if self.poll() {
                return None;
            }
            let window = self.load_window(pos, multiline);
            let next = if window.end >= len {
                u64::MAX
            } else {
                window.end
            };
            let Some(text) = self.window.as_ref().and_then(|w| w.text.as_deref()) else {
                pos = next;
                continue;
            };
            match re.find_from_pos(text, (pos - window.start) as usize) {
                Ok(Some(m)) => {
                    let found = window.start + m.start() as u64..window.start + m.end() as u64;
                    return (found.end <= end).then_some(found);
                }
                Ok(None) => {}
                Err(_) => self.complex_lines += 1,
            }
            pos = next;
        }
        None
    }
}

#[cfg(test)]
mod tests;
