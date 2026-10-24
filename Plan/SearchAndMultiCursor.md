# Search and Multi-Cursor (M5)

## Task

Find and replace inside the open file, with VS Code keys and behavior, and the
VS Code multi-cursor and column-selection commands, as specified in the
[Architecture](Architecture.md#search-rte_search). Search must stay responsive
on files of any size and must not slow down startup.

Mockup: [SearchMockup.png](../Design/SearchMockup.png) (source
[SearchMockup.html](../Design/SearchMockup.html)).

## Scope

### Included

- **Search modes** (exclusive): Text (literal, found anywhere), Glob and RegEx.
  Options for every mode: Match Case (`Alt+C`) and Whole Word (`Alt+W`).
  - Glob (`Alt+G`, not in VS Code): `?` one character, `*` any text within the
    line (shortest match; a trailing `*` runs to the end of the line),
    `[abc]`, `[!abc]`, `\` escapes the next character.
  - RegEx (`Alt+R`): VS Code dialect, including lookaround and backreferences;
    `^` / `$` per line; `.` does not match a line break; patterns containing
    `\n` match across lines.
- **Search box** at the top right of the text area (see the mockup): field,
  mode and option toggles, `N of M` counter, previous / next, Find in
  Selection (`Alt+L`), close; replace row with Preserve Case (`Alt+P`),
  Replace and Replace All.
  - `Ctrl+F` opens it, `Ctrl+H` opens it with replace; the field starts with a
    single-line selection or the word at the caret (escaped in RegEx mode).
  - Results update while typing and move to the nearest match after the caret.
  - `Enter` / `Shift+Enter` and `F3` / `Shift+F3`: next / previous (wraps).
  - `Ctrl+Enter`: a line break in the field. `Alt+Enter`: select all matches.
  - Replace field: `Enter` or `Ctrl+Shift+1` replaces one,
    `Ctrl+Alt+Enter` replaces all.
  - `Esc` closes it; the current match stays selected.
  - The field has the palette's editing keys (`Ctrl+A`, `Ctrl+V`, word moves).
  - Errors (invalid pattern, unsupported construct, too complex, skipped
    lines) are shown under the field; `No results` in the counter.
- **Highlights:** visible matches, the current match and the find-in-selection
  range, with new dark-theme colors.
- **Replacement syntax** (VS Code): `$1`, `$<name>`, `$&`, `$$`, `\n`, `\t`,
  `\u` `\l` `\U` `\L`; Preserve Case. In Text and Glob modes the replacement is
  inserted as typed.
- **Occurrence commands:** `Ctrl+D` (add next occurrence; from an empty
  selection it selects the word and then matches whole words),
  `Ctrl+K Ctrl+D` (move last selection to next), `Ctrl+Shift+L` (all
  occurrences of the selection), `Ctrl+F2` (all occurrences of the word),
  `Alt+Enter` in the search box.
- **Multi-cursor (VS Code):** `Alt+Click` (add a cursor; on an existing cursor,
  remove it), `Ctrl+Alt+Up` / `Ctrl+Alt+Down`, `Shift+Alt+I`, `Ctrl+L`
  (select line, repeat to extend), `Ctrl+U` (cursor undo), `Esc` (collapse to
  the primary cursor, existing).
- **Column selection:** `Shift+Alt+Drag`, middle-button drag,
  `Ctrl+Shift+Alt+Arrows`, `Ctrl+Shift+Alt+PgUp` / `PgDn`. Short lines get a
  caret at their end; columns follow tabs and wide characters.
- **Limits:** select-all stops at `editor.multiCursorLimit` (default 10,000)
  and refuses instead of a partial selection; Replace All above
  `search.replaceAllMaxMiB` (default 512) makes no edit.
- Configuration keys `editor.multiCursorLimit`, `search.replaceAllMaxMiB`.
- Palette entries for every new command (`Edit:`, `Selection:`, `Go:` families).
- Read-only: Replace and Replace All are refused and dimmed like other edits.
- Benchmarks M01-M08 and new search tests F01-F03 in `cargo xtask bench-perf`.

### Excluded

- Find history (`Up` / `Down` in the field), search across files, search
  editor, overview-ruler marks, `Ctrl+F3` family: later.
- Persisting search state; nothing new is written to `UserSettings`.
- Expand / shrink selection (`Shift+Alt+Right/Left`): M6 (syntax tree).
- Word wrap interaction: wrap ships after v1.

### Compatibility Constraints

- Startup unchanged: no search state, thread, pattern or table is created
  before the first search action. L01 stays within noise of 0.2.0 in an A/B
  run in the same session.
- Matches never change unedited bytes; invalid UTF-8 and line endings are
  preserved by replace.
- No new DLL imports; `check-imports` passes.

## Design

- **`rte_search` (new crate, no UI):**
  - `Query { text, mode: Text | Glob | Regex, match_case, whole_word }`
    compiles into a `Matcher`.
  - Text, case-sensitive: `memchr::memmem` over chunks, carrying a
    pattern-length overlap across chunk boundaries. Case-insensitive text is
    compiled as an escaped pattern for the automata engine.
  - Glob is translated to an automata pattern (`?` → `[^\n]`, `*` →
    `[^\n]*?`, trailing `*` → `[^\n]*`, `[!x]` → `[^x\n]`, the rest escaped).
  - RegEx: compile with the automata engine (`regex-cursor` over a chunk
    cursor of the snapshot; linear, multiline, invalid-UTF-8 safe). When the
    pattern needs lookaround or backreferences, use `fancy-regex` per line
    (multiline patterns in a 1 MiB window) with `backtrack_limit`; lines with
    invalid UTF-8 or longer than 16 MiB are skipped and counted. Size limits
    on compiled programs.
  - Whole word: a post-filter with the editor's word definition (same as
    `Ctrl+Right` and double-click), for all modes.
  - `Replacement::parse(template)` and `expand(captures, preserve_case)`.
  - API: `find_next / find_prev(snapshot, from, range, cancel)`,
    `matches_in(snapshot, range, out)`, `count(snapshot, cancel, progress)`.
    Zero-width matches advance by one character.
- **Search thread (`rte_app::search`):** created on the first job that is not
  a short literal viewport search, then idle on its channel. Jobs carry
  `JobTicket { kind: Search, generation, rev }` and a `CancelToken`; a new
  query or option bumps the generation and cancels the running job. Jobs:
  compile (error text back), viewport matches, streaming count (progress at
  most every 50 ms), next / previous beyond the UI budget, select all, Replace
  All. Results wake the UI with the existing `WM_APP` queue.
- **UI work bounds:** case-sensitive Text without whole word runs on the UI for
  the visible rows (byte bound per row) and for next / previous within 0.5 ms;
  everything else goes to the search thread. The UI never compiles a pattern.
- **Edit authority:** Replace re-matches the selection in place on the current
  snapshot; Replace All and select-all commit only when `rev` and generation
  still match, otherwise re-run once, then abort with a message. Replace All
  is one `Transaction` (one undo step) built on the search thread; its memory
  estimate is checked against the cap before the edit is created.
- **Search box state** (`Option<Box<FindState>>` in the editor, created on
  first `Ctrl+F` / `Ctrl+H`): query, replace text, options, focus, counter,
  message, generation. It reuses the palette's text field (`Query`). The box is
  a popup layer item in `rte_view::popup` (`FindGeometry` shared by drawing and
  hit tests); it keeps its size on zoom, as the palette. Opening the palette
  hides the search box; closing it shows it again.
- **Highlights:** `rte_view` takes match ranges for the visible rows, drawn as
  fills under the glyphs (other matches, current match with a border,
  find-in-selection range). Results for an older `rev` are mapped through
  `ChangeSet`s for display only.
- **Multi-cursor (`rte_edit`, pure):** `add_cursor_vertical(dir)` (keeps
  `goal`), `cursors_at_line_ends`, `select_line`, `box_select(anchor_cell,
  head_cell)` (tabs and wide characters by cell column), `next_occurrence`
  helpers for literal text. Cursor undo is a bounded stack (64 states) of
  `Selections` in the document, pushed by cursor-creating commands only.
- **Mouse (`rte_platform`):** `Alt` state on button events, middle-button
  down / move / up, captured drags.
- **Theme:** `search_match` `#603317`, `search_current` `#515C6A` with
  border `#8FA3BA`, `search_range` `#2A2D33`, `error` `#F48771` on `#5A1D1D`
  (border `#E05252`); warnings reuse `prompt_background` / `at_risk`; toggles
  on use `selection` with a `popup_match` border. Box icons (`▸ ▾ ↑ ↓ ≡ ✕`)
  come from the bundled face or are drawn as shapes when missing.
- **Persistence:** none. **Failure behavior:** pattern errors, limits and
  stale results produce messages, never partial edits.

## Alternatives

- **Glob by its own matcher:** rejected; translation to the automata engine
  gives the same speed and chunk handling without a third engine.
- **Greedy `*` in Glob:** `foo*bar` would span to the last `bar` on the line;
  shortest matches what users usually mean. A trailing `*` is greedy so that
  `TODO*` selects the rest of the line.
- **Literal and Substring as separate modes:** merged (user decision); Whole
  Word covers the exact-token case.
- **One regex engine without lookaround:** rejected (Architecture A9, VS Code
  parity). **PCRE2:** rejected (C build, larger binary, no chunked input).
- **Search on the syntax worker:** rejected (Architecture A6); regex scans
  cannot yield.
- **Deferral by a separate DLL:** rejected for now; unused code pages of the
  executable are not read at startup. Kept as the fallback if L01 regresses.
- **Bottom find bar (Notepad++, Sublime):** rejected for VS Code parity.
- **Stored match lists:** rejected; memory would grow with the file.

## Runtime Effects

- **Startup:** none. No allocation, thread, pattern or Unicode table. The
  executable grows by an estimated 1-1.5 MiB (regex crates); untouched pages
  are not loaded. Checked by L01 and exe size, A/B in one session.
- **First search:** a short literal search runs on the UI; the first RegEx,
  Glob, case-insensitive or count job starts the search thread (stack
  reserved, committed on use).
- **Steady state:** idle search thread blocked on its channel, no timers, no
  CPU. Compiled patterns are bounded by size limits; lazy-DFA cache bounded.
- **Memory:** no match lists; viewport spans only. Select-all holds at most
  `editor.multiCursorLimit` ranges. Replace All ≤ `search.replaceAllMaxMiB`.
  Cursor undo 64 states.
- **I/O:** none.
- **Cancellation:** every job has a `CancelToken` checked between chunks,
  lines and matches; stale results are rejected by ticket.
- **Disabled path:** search box closed → no viewport search, no jobs.

## Tests

- **Unit (`rte_search`):** literal across chunk boundaries and CRLF;
  case-insensitive Unicode; whole word at chunk edges; the Glob translation
  table (including trailing `*` and `[!x]`); automata matches across chunks
  and lines; `foo(?=bar)`, `(?<!x)y`, `(\w+)\s+\1`; `(a+)+$` hits
  `backtrack_limit` and reports; invalid UTF-8 lines skipped and counted;
  zero-width advance; replacement expansion (`$1`, `$<name>`, `$&`, `$$`,
  `\n`, `\t`, case modifiers, Preserve Case); unsupported JS syntax reported.
- **Property:** literal and Glob search versus a naive model over random
  text and random chunk sizes.
- **Golden (`rte_edit`):** add cursor above / below (goal column, short
  lines), `Shift+Alt+I`, `Ctrl+L` repeat, box selection with tabs, wide
  characters and short lines, cursor undo.
- **Integration (`rte_app`):** stale result after a query change with the same
  `rev` is rejected; `cat` → `dog` while a `cat` job is in flight; an edit
  during Replace All construction aborts or re-runs, never edits stale text;
  Replace All is one undo step; select-all above the limit refuses; Replace
  All above the cap makes no edit; read-only refuses replace; toggles are
  exclusive (`Alt+G` turns RegEx off and the reverse); seed text; `Esc`
  keeps the match selected; scripted keys open, search and close the box.
- **Deferral:** a new editor has no search state and no search thread; a
  literal viewport search does not start the thread.
- **Bounds (ignored by default, run on request):** 1 GiB of `a` searched for
  `a` and for an empty-width regex: memory flat, count streams, cancel works.
- **Performance (`bench-perf`, focused):** L01 and exe size A/B with 0.2.0;
  M01-M08; F01 find next in `large.log` (key to frame), F02 count in
  `large.log` (time to final count), F03 RegEx viewport highlight in
  `medium.py` (key to frame).
- **Manual:** `Alt+Click`, `Shift+Alt+Drag`, middle drag, `Ctrl+Alt+Up/Down`
  (graphics-driver hotkeys may take these), the search box at small window
  sizes and high DPI.

## Implementation Steps

1. `rte_search`: query, Text, Glob, whole word, case; unit and property tests.
2. `rte_search`: RegEx engines, chunk cursor, replacement; unit tests.
3. `rte_edit`: multi-cursor and box-selection commands; golden tests.
4. `rte_app::search`: search thread, tickets, jobs; integration tests.
5. Search box: `rte_view` geometry and drawing, theme colors, highlights;
   `rte_app` input and state.
6. Commands, keys and palette entries; replace, Replace All, occurrence
   commands; configuration keys.
7. Mouse: `Alt+Click`, `Shift+Alt+Drag`, middle drag.
8. `bench-perf`: M01-M08, F01-F03; L01 and exe size A/B.
9. Docs: README, docs site shortcuts, CHANGELOG, Architecture coverage
   (`Alt+G` added), `Perfromance.md` (F01-F03 specification and results).

## Acceptance Criteria

- Every key in the Search and Replace and Multi-cursor and Selection tables of
  the Architecture (except Expand / Shrink Selection) works as in VS Code;
  `Alt+G` toggles Glob.
- The search box matches the mockup in layout and states.
- Search never blocks the UI: during a count over a 256 MiB file, typing and
  scrolling frames stay within their budgets.
- Stale results never edit; Replace All is one undo step; limits refuse
  without partial results.
- L01 and peak memory of an empty file are within noise of 0.2.0 in a
  same-session A/B; the search thread does not exist before the first search.
- Focused tests and clippy pass; M01-M08 and F01-F03 are recorded.

## Reviewers

### 2026_10_24 - Claude Opus 5.5

- Role: Reviewer
- Activity: Design
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Design drafted with a PNG mockup. Text merges Literal and
  Substring (user approved). Open for user confirmation: `Alt+G` for Glob and
  shortest-match `*` with a greedy trailing `*`.

### 2026_10_24 - Claude Opus 5.5

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: User accepted `Alt+G` and the Glob `*` rule. Replace keys confirmed
  as VS Code (`Ctrl+H`, `Enter` / `Ctrl+Shift+1`, `Ctrl+Alt+Enter`) and shown
  in the mockup. Implementation awaits approval.

## Implementer

### 2026_10_24 - Claude Opus 5.5

- Role: Implementer
- Activity: Implementation
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Steps 1-7 and 9 implemented; step 8 partly (M01-M04, F01-F03).
  Focused tests, clippy, `check-imports` and the L01 A/B pass. Open: M05-M08
  harness (typed characters and clipboard in the scripted input mode), the
  UI-responsiveness-during-count check, the 1 GiB bounds test and the manual
  checks. The task stays in `Plan/` until these are closed or waived.

### 2026_10_24 - Claude Opus 5.5

- Role: Implementer
- Activity: Implementation
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Step 8 completed. Scripted input accepts quoted text, typed one
  character per step (`Ctx::post_char`); M05-M08 added to `bench-perf`, with
  a deterministic 10,000-selection setup. Fixed: an invalid pattern left the
  navigation pending, so the box stayed busy. Open: the responsiveness check,
  the 1 GiB bounds test and the manual checks.

### 2026_10_24 - Claude Opus 5.5

- Role: Implementer
- Activity: Implementation
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Addressed review findings M5R1-M5R11 (see Review Response at the
  end). Focused tests (200), clippy and fmt pass; focused benchmarks pass
  except M07, blocked by a system clipboard that no process could open
  during validation. Open as before: the responsiveness check, the 1 GiB
  bounds test and the manual checks.

### 2026_10_24 - Claude Opus 5.5

- Role: Implementer
- Activity: Implementation
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Addressed GPT-6 Astra findings M5A1-M5A7 (see Astra Review
  Response at the end). Focused tests (208, 3 ignored), clippy and fmt pass;
  all focused benchmarks including M07 pass. Open as before: the
  responsiveness check, the 1 GiB bounds test and the manual checks.

## Implementation Notes

- New crate [rte_search](../crates/rte_search/src/lib.rs); multi-cursor in
  [rte_edit/src/multi.rs](../crates/rte_edit/src/multi.rs); search box in
  [rte_view/src/find.rs](../crates/rte_view/src/find.rs); search thread in
  [rte_app/src/search.rs](../crates/rte_app/src/search.rs); editor state in
  [editor/find.rs](../crates/rte_app/src/editor/find.rs) and
  [editor/cursors.rs](../crates/rte_app/src/editor/cursors.rs).
- Deviations from the design:
  - The UI bound for case-sensitive Text is a 1 MiB scan (with wrap), not a
    0.5 ms time budget.
  - After an edit, regex highlights hide until the new result arrives instead
    of being mapped through `ChangeSet`s.
  - Line-break symbol in the field is `¶` (`↵` is missing from the font).
  - The executable grows by 1.9 MiB, not the estimated 1-1.5 MiB.
- Known limitations: see the Review Response for the remaining count and
  highlight limits; the search field has no own undo (`Ctrl+Z`
  / `Ctrl+Y` are ignored in it); find history is excluded (see Scope).

## Validation Results

| Command                                                         | Result              |
| --------------------------------------------------------------- | ------------------- |
| `cargo test -p rte_search`                                      | 23 passed           |
| `cargo test -p rte_edit`                                        | 20 passed           |
| `cargo test -p rte_view`                                        | 26 passed           |
| `cargo test -p rte_render`                                      | 8 passed            |
| `cargo test -p rte_app`                                         | 95 passed           |
| `cargo test -p rte_platform`                                    | 4 passed, 1 ignored |
| `cargo test -p xtask`                                           | 7 passed            |
| `cargo clippy --workspace --all-targets -- -D warnings`         | Clean               |
| `cargo fmt --all -- --check`                                    | Clean               |
| `cargo xtask check-imports`                                     | No new DLL imports  |
| `cargo xtask bench-perf --only L01,M01,M02,M03,M04,F01,F02,F03` | Pass, total 3.4 s   |
| `cargo xtask bench-perf --only M05,M06,M07,M08`                 | Pass, total 0.8 s   |

Focused benchmark (medians, key to frame):

| ID  | Test                          | Median [ms] | p95 [ms] |
| --- | ----------------------------- | ----------- | -------- |
| M01 | Add next occurrence           | 0.43        | 6.16     |
| M02 | Select all occurrences        | 1.67        | 5.55     |
| M03 | Column selection, 1,000 lines | 1.16        | 1.21     |
| M04 | Insert cursor below           | 0.41        | 0.57     |
| M05 | Type at 10,000 carets         | 4.42        | 9.38     |
| M06 | Backspace at 10,000 carets    | 5.22        | 9.97     |
| M07 | Paste 10,000 pieces           | 5.15        | 5.29     |
| M08 | Undo and redo, 10,000 carets  | 0.46        | 0.49     |
| F01 | Find next, 256 MiB            | 0.54        | 0.69     |
| F02 | Count matches, 256 MiB        | 230.67      | 235.84   |
| F03 | Toggle RegEx, 8 MiB           | 3.05        | 3.19     |

L01 A/B against v0.2.0 (built from the tag in `target/ab/v020`), 30
interleaved launches each of `--trace-startup --exit-after-load` on
`target/perf/v1/empty.txt`:

| Build  | First paint median [ms] | p95 [ms] | Peak private [MiB] | Executable [bytes] |
| ------ | ----------------------- | -------- | ------------------ | ------------------ |
| v0.2.0 | 21.30                   | 23.60    | 4.98               | 2,142,720          |
| M5     | 21.02                   | 24.03    | 4.99               | 4,153,856          |

The M05-M08 setup pattern `sum_\d{1,4}\(` matches exactly 10,000 times in
`medium.c` (checked with .NET `Regex.Matches`).

Not run: the responsiveness check during a 256 MiB count, the 1 GiB bounds
test (not written), manual mouse and DPI checks, the full test and
performance suites (not requested).

## Implementation Review

### 2026_10_24 - GPT-6.1 Sol

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: GPT-6.1 Sol
- Effort: High
- Context Window: 1M
- Outcome: Changes requested for M5R1-M5R11 below. Existing focused tests
  pass, but seven correctness assertions fail and release probes violate
  UI-work and memory bounds. No application fixes were authored. Keep this
  task in Plan; recorded implementation results do not establish acceptance.

### Scope

Reviewed the matcher engines, replacement expansion, search-thread jobs and
tickets, find-box integration, occurrence/cursor commands, limits and nearby
tests. Existing task notes already acknowledge multiline counting gaps; this
review reproduces that limitation and identifies additional failure paths.
The probes do not certify native presentation, startup parity or 1 GiB bounds.

### M5R1 [P1] A changed query does not revoke replacement authority

Locations: [query_changed](../crates/rte_app/src/editor/find.rs#L270) and
[op_reply](../crates/rte_app/src/editor/find.rs#L1039).
Find-query generations and operation generations are independent. A pending
replacement checks text/root and selections, but not the find query/options
that authorized it. Reproduction: start replacing `cat` with `dog` in
`cat cat`, change the query to `missing` without changing the document or
selections, then deliver the old reply. It still commits `dog dog`.
Capture and validate the query/options generation for replacements and
find-driven select-all, and cancel their work when that input changes. A
text-change retry must not silently adopt a different query. Add delayed
Replace One/All and select-all tests for query, scope and option changes.

### M5R2 [P1] Cursor undo can restore coordinates inside UTF-8

Location: [cursor_undo](../crates/rte_app/src/editor/cursors.rs#L54).
History stores raw byte offsets; restore only clamps them to the current
length. Insert a multibyte character before old cursor positions, then undo
the cursor operation: a restored head can sit inside that character. In the
probe, saved carets at 0 and 1 are restored after inserting U+00E9 at 0;
typing `X` produces bytes `58 C3 58 A9 61 62`, invalid UTF-8.
Map historical coordinates through edits or reject obsolete states under an
explicit policy, and validate safe boundaries on restore. Avoid eagerly
remapping all 64 large selection sets on every keystroke without measuring
the cost; revision-tagged/lazy mappings are preferable. Test insertions,
deletions, CRLF, combining sequences and edits before historical carets.

### M5R3 [P1] Replace All allocates every match before checking its cap

Location: [ReplaceAll work](../crates/rte_app/src/search.rs#L391).
All match ranges are collected into `ranges` before any expansion or memory
check. On a 1 MiB document of `a`, replacing `a` with `x` under the supported
1 MiB cap peaks at 17,325,964 extra requested heap bytes (16.5 MiB), then
refuses. The scan and allocation already happened. At billion-match scale
the unbounded range list can exhaust memory before refusal or cancellation.
Stream matches and expansion through one cursor, account for all live range,
edit/capture and replacement storage before growth, and poll cancellation
during expansion too. Prefer bounded construction that reuses unchanged tree
pieces. Add dense and zero-width cap tests that assert peak memory, no partial
edit, bounded cancellation and one-step undo for successful work.

### M5R4 [P1] A bounded byte range still blocks the UI and stores huge spans

Locations: [literal viewport scan](../crates/rte_app/src/editor/find.rs#L489),
[literal matcher](../crates/rte_search/src/lib.rs#L414), and
[close_find](../crates/rte_app/src/editor/find.rs#L255).
A single 1 MiB line of `a` searched for `a` generates 1,048,576 ranges on
the UI thread. `sync_find` takes about 98-101 ms and retains 16 MiB of range
capacity, although drawing scans only a bounded visible prefix. Escape keeps
all those spans/capacity in the closed find box. A byte limit is neither a
frame-time limit nor a match-storage limit.

The literal iterator also restarts from the next match, recopies the remainder
of its leaf, and allocates a new chunk iterator each time. Enumerating that
1 MiB takes about 93 ms and 1,048,577 allocation/reallocation calls; the same
automata pattern takes about 50 ms with no allocations in the measured phase.
Use a persistent borrowed scan cursor, a work/time budget and bounded visible
results. Coalesce adjacent display intervals where valid; keep counts and edit
authority separate. Release oversized transient buffers on close. Test dense
long lines, horizontal scrolling and zero-width patterns against UI and heap
budgets, not only sparse log/code fixtures.

### M5R5 [P1] Cursor limits are checked after expensive full construction

Locations: [line-end cursors](../crates/rte_edit/src/multi.rs#L37),
[box_select](../crates/rte_edit/src/multi.rs#L69), and
[place_column](../crates/rte_app/src/editor/cursors.rs#L261).
The helpers materialize and normalize every selection before the editor tests
the configured limit. With a limit of two, a 50,000-line selection still takes
about 560 ms to reject for both line-end cursors and a box selection. Peak
extra heap is about 3.9 MiB and 4.2 MiB respectively; the latter makes 200,006
allocation/reallocation calls. The final selection is unchanged, but the UI
has already stalled and memory has already grown.
Check known line counts before construction and enforce a limit while building
when deduplication/merging is needed. Preserve atomic refusal. Add large
selection/drag tests with a deliberately small limit and verify early refusal,
not just the final selection count. Give cursor history a byte budget as well
as a state-count limit if large configurable cursor counts are supported.

### M5R6 [P2] Backward windows truncate variable-length regex matches

Location: [find_prev](../crates/rte_search/src/lib.rs#L266).
Each growing window is searched as an independent starting range. Searching
backwards for `a+` in 100,000 `a` bytes returns `34464..100000`, the suffix
starting at the first 64 KiB window, instead of the document's match
`0..100000`. The window is accepted as soon as it contains any match, without
establishing that the match began before its boundary. Preserve the canonical
match sequence/context when accelerating backward search. Add long greedy
matches, multiline matches and previous/next symmetry tests at window edges.

### M5R7 [P2] Lookbehind replacement loses the context used by Find

Locations: [capture re-match](../crates/rte_search/src/lib.rs#L314) and
[backtracking window](../crates/rte_search/src/lib.rs#L438).
On `a\nb`, `(?<=a\n)b` is found at `2..3`, but expansion returns None. Capture
re-matching builds a new window starting at the match's own line, discarding
the preceding line needed by lookbehind. Replace One cannot replace the
visible match; Replace All silently skips it. Retain sufficient verified
context or captures from enumeration; never treat context loss as a valid
empty replacement result. Test positive/negative multiline lookbehind and
backreferences through Find, Replace One and Replace All.

### M5R8 [P2] Next does not advance from a zero-width current match

Location: [find_next](../crates/rte_app/src/editor/find.rs#L375).
Forward navigation starts at `primary.end()`, which equals its start for an
empty match. With query `^` on `a\nb`, the second F3 again selects `0..0`
instead of advancing to `2..2`. Iterator zero-width tests do not cover editor
navigation. Track whether the caret is the current empty match and advance by
a valid character before the next search, with correct EOF wrap behavior.
Test F3/Shift+F3, Enter/Shift+Enter and zero-width replacement navigation.

### M5R9 [P2] Viewport highlights ignore Find in Selection

Location: [sync_find](../crates/rte_app/src/editor/find.rs#L473).
Viewport jobs and the UI literal path use the viewport range without
intersecting the search scope. On `cat cat` with scope `0..3`, highlights
contain both `0..3` and `4..7`. Counts/navigation scoped elsewhere can then
disagree with the displayed matches. Intersect visible work with scope for
every engine and reject old viewport replies by their requested scope/range.
Add literal, case-insensitive and regex scope tests, including scrolling.

### M5R10 [P2] Plain multiline paste does not distribute across cursors

Locations: [paste](../crates/rte_edit/src/commands.rs#L429) and
[plain clipboard decoding](../crates/rte_app/src/clip.rs#L38).
Distribution checks private piece count, but plain text from another program
is decoded as one piece regardless of its line count. Pasting `X\nY` at the
two carets before `a` and `b` in `a b` yields `X\nYa X\nYb`, not `Xa Yb`.
Implement the default VS Code line-spreading behavior when plain clipboard
line count matches cursor count, while keeping private metadata, line-mode
paste and newline normalization correct. Add LF/CRLF, trailing-break,
unequal-count and private-versus-plain clipboard cases. The probe uses the
headless clipboard only; no system clipboard was changed.

### M5R11 [P2] Count steps are neither complete streaming nor fixed work

Locations: [count_step](../crates/rte_app/src/search.rs#L442),
[step_end](../crates/rte_app/src/search.rs#L550), and
[checkpoint index](../crates/rte_app/src/search.rs#L420).
This confirms the existing known limitation: a 4 MiB prefix followed by
`a\nb\n`, searched for `a\nb`, counts zero while the full matcher finds one.
The step boundary splits the match and loses it. Conversely, a long line
extends a nominal 4 MiB step to its line end, potentially the entire file;
the worker cannot serve same-generation navigation/viewport jobs until the
step returns. Even a dense 1 MiB step takes about 92 ms in the release probe.
Generation cancellation is not preemption for those same-generation jobs.

Checkpoints also grow without a byte cap: 1,024 entries/16 KiB for one MiB
of single-byte matches, implying about 16 MiB at a billion-match scale before
other storage. That scale was not executed. Preserve matcher continuation
across steps, use an actual bounded scheduling policy, and cap/adapt checkpoint
storage. Add cross-step multiline/zero-width counts and queued-navigation
latency tests. Do not present an undercount as a completed exact count.

### Review Validation Results

Successful focused commands:

```powershell
cargo test -p rte_search -p rte_edit --locked
cargo test -p rte_view -p rte_render -p rte_platform --locked
cargo build -p rte_app --locked --message-format=json
cargo build -p rte_app --release --locked --target-dir target/m5-review-build --message-format=json
cargo clippy -p rte_search -p rte_edit -p rte_app -p rte_view -p rte_render -p rte_platform --all-targets --locked -- -D warnings
cargo fmt -p rte_search -p rte_edit -p rte_app -p rte_view -p rte_render -p rte_platform --check
```

- Existing focused tests: 176 pass (23 search, 20 edit, 95 application,
  26 view, 8 renderer, 4 platform). One native clipboard test is intentionally
  ignored because it replaces the user's clipboard. No workspace-wide suite.
- Application tests run in a disposable console harness because the normal
  Windows-subsystem test executable hides console output:
  `.\target\m5_review_probe.exe --skip review_`. Original modules are retained;
  only two test helpers are exposed to the injected child-module tests.
- `.\target\m5_review_probe.exe review_ --nocapture --test-threads=1`:
  seven assertions fail, reproducing M5R1-M5R2 and M5R6-M5R10. Exit 101 is
  defect evidence, not passing regression coverage. Replacement replies are
  delivered deterministically after changing query input; no disk edits.
- `.\target\m5_efficiency_alloc.exe --exact m5_efficiency_profile --nocapture --test-threads=1`
  and `.\target\m5_efficiency_timing.exe --exact m5_efficiency_profile --nocapture --test-threads=1`
  reproduce M5R3-M5R5 and M5R11. These profile controls assert observed limits,
  counts and atomic refusals, including the known incorrect cross-step count;
  their successful exit does not establish correct search behavior.
- Release probes use opt-level 3, fat LTO, one codegen unit and x86-64-v3,
  with exact dependency paths from Cargo's JSON artifacts. Timing and allocator
  instrumentation are separate executables. Quoted timings are uninstrumented
  local samples, not frozen-hardware percentile certification.
- Rust allocator counters exclude fixture snapshots created before each phase,
  allocator overhead/fragmentation, internal realloc overlap, mapped assets,
  native allocations and thread stacks. Peak requested heap is not process
  PrivateUsage; cumulative requests measure churn, not actual bytes copied.
- The UI scan is measured with its worker Find lane invalidated so background
  count work cannot inflate the CPU/heap sample. The synchronous highlight
  code is unchanged. The final Replace All probe uses the supported 1 MiB
  configuration value; an initial 1 KiB API-only probe is not the cited result.
- Probes are compiled from stdin into ignored `target/`; application source,
  settings and system clipboard are unchanged. Normal executables/processes
  were not inspected or stopped. Source/control hashes are checked for 31
  files when recording this append; the original task bytes are preserved.
- Not run: native manual mouse/DPI checks, the requested 256 MiB mixed-input
  responsiveness scenario, the 1 GiB bounds case, new startup A/B, hosted CI,
  or the full performance suite. Existing A/B and benchmark tables above are
  historical implementation evidence, not fresh reviewer results.

### Optimization Priorities

Fix edit authority and safe coordinates first (M5R1-M5R2). Then make limits
construction-time guarantees (M5R3-M5R5), keep scans resumable and match buffers
bounded, and reuse borrowed cursors/captures instead of allocating per match.
Preserve exact matching and undo guarantees; do not trade correctness for
misleading small-file medians. The current results do not support calling the
implementation the fastest editor or approving M5's runtime/memory criteria.

### Review Response

2026_10_24, Claude Opus 5.5 (Implementer). All findings accepted.

- **M5R1:** search-box operations (Replace One/All, Select All Matches)
  carry the query, options, replacement and scope that started them. Any
  change cancels the operation (checked after every event and again on the
  reply); a Replace All retry after a text change keeps the same input.
  Test: `changed_search_input_revokes_pending_replacements_and_selections`.
- **M5R2:** cursor-undo states keep the text they were recorded on; states of
  an older text are dropped (VS Code clears them on edits). History is
  bounded by 64 states and 262,144 selections.
  Test: `cursor_undo_never_restores_offsets_of_an_older_text`.
- **M5R3:** Replace All streams matching, expansion and edit storage through
  one enumeration; every growth of the result is checked against the cap
  before it is allocated. A match whose groups cannot be read aborts with no
  edit instead of being skipped.
  Test: `replace_all_checks_its_cap_while_building`.
- **M5R4:** highlights search per visible row only what the view lays out
  (64 KiB), at most 1 MiB in all, and keep at most 65,536 ranges. The literal
  engine scans leaves in place with a cached leaf (no copies, no per-match
  iterator). Large buffers are released on close. The view computes match
  columns in one pass per row and stops past the visible columns.
  Tests: `dense_long_line_highlights_are_bounded_and_released_on_close`,
  `dense_matches_on_a_long_row_draw_only_visible_columns`.
- **M5R5:** `cursors_at_line_ends` and `box_select` take the limit and refuse
  before building. Tests: `line_end_and_column_limits_refuse_before_building`,
  `line_ends_and_column_drags_refuse_above_the_limit_early`.
- **M5R6:** `find_prev` accepts a window's last match only when a window four
  times larger agrees, or the window reaches the start.
  Test: `find_prev_keeps_a_long_match_that_begins_before_its_window`.
- **M5R7:** backtracking windows include the preceding line break
  (single-line patterns) or up to 512 KiB of preceding lines (multiline
  patterns), in Find and in capture re-matching alike.
  Test: `look_behind_sees_previous_lines_in_find_and_replace`.
- **M5R8:** next and previous step over an empty current match by one
  character, wrapping at the scope ends; its index is shown.
  Test: `next_and_previous_step_over_an_empty_current_match`.
- **M5R9:** viewport rows are intersected with the Find in Selection scope for
  every engine; replies for other rows are ignored.
  Test: `highlights_stay_inside_find_in_selection`.
- **M5R10:** plain text with as many lines as cursors (one trailing break
  removed) pastes one line per cursor; private pieces and line mode keep
  their behavior. Test: `plain_paste_spreads_lines_over_as_many_cursors`.
- **M5R11:** counts continue one match enumeration: a step starts matches in
  4 MiB, reads 1 MiB further (doubled while one match does not fit) and
  counts at most 65,536 matches. Checkpoints thin to at most 4,096.
  Tests: `counts_continue_matches_across_steps`,
  `dense_counts_take_bounded_steps_and_keep_bounded_checkpoints`.

Remaining limits: a count can still miss a match longer than the read-ahead
whose truncated prefix does not match (more than 1 MiB long); highlights stop
after 65,536 ranges; matches past 64 KiB of a line are not highlighted (the
view does not draw them either).

Response validation (release-mode focused benchmark, same session):

| ID  | Test                          | Median [ms] | p95 [ms] |
| --- | ----------------------------- | ----------- | -------- |
| L01 | Empty file, first paint       | 14.8        | -        |
| M01 | Add next occurrence           | 0.42        | 6.01     |
| M02 | Select all occurrences        | 0.81        | 6.31     |
| M03 | Column selection, 1,000 lines | 1.15        | 1.67     |
| M04 | Insert cursor below           | 0.45        | 6.08     |
| M05 | Type at 10,000 carets         | 3.75        | 4.72     |
| M06 | Backspace at 10,000 carets    | 3.53        | 4.84     |
| M08 | Undo and redo, 10,000 carets  | 0.56        | 5.73     |
| F01 | Find next, 256 MiB            | 0.57        | 5.71     |
| F02 | Count matches, 256 MiB        | 234.18      | 235.27   |
| F03 | Toggle RegEx, 8 MiB           | 4.91        | 7.00     |

F02 and F03 reruns: 250.78 / 222.55 ms and 3.40 / 3.24 ms. M07 could not run:
the system clipboard could not be opened by any process (PowerShell
`Get-Clipboard` failed the same way), so its `Ctrl+C` setup copied nothing.

Commands: `cargo test -p rte_search` (27), `-p rte_edit` (22), `-p rte_view`
(27), `-p rte_render` (8), `-p rte_platform` (4, 1 ignored), `-p rte_app`
(104), `-p xtask` (8); `cargo clippy --workspace --all-targets -- -D warnings`;
`cargo fmt --all -- --check`;
`cargo xtask bench-perf --only L01,M01,M02,M03,M04,M05,M06,M08,F01,F02,F03`.

### 2026_10_24 - GPT-6 Astra

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: GPT-6 Astra
- Effort: Extra High
- Context Window: 1M
- Outcome: Changes requested. Seven correctness assertions fail on the updated
  implementation. M5R3, M5R6 and M5R11 are not fully resolved; additional
  capture-scope, reply-identity, occurrence and dense-regex issues follow.
  Existing focused tests and ordinary-workload benchmarks pass. No application
  fixes were authored; M5 remains pending.

#### M5A1 [P1] Capture expansion precedes the replacement memory check

Location: [replace_all](../crates/rte_app/src/search.rs#L544).
The new streaming builder fixes the unlimited match list, but calls
`expand_found` before testing the cap. Replacing a single 16 MiB `a+` match
with `$&$&` under a 1 MiB cap requests a 32 MiB allocation before refusing,
in addition to capture-copy storage. The same path can exhaust memory on
larger matches. Compute checked expansion sizes before allocating, bound
temporary captures and case conversion too, and poll cancellation during
expansion. Probe: `review_capture_expansion_obeys_cap_before_allocation`.
This reopens the remaining part of M5R3; refusing after allocation is too late.

#### M5A2 [P1] Count windows change regex meaning and can stall navigation

Location: [count_step](../crates/rte_app/src/search.rs#L465).
On 6 MiB of `a` followed by `Z`, `\Aa+Z` is found by a full search but its
completed count is zero. A second fixture, with `X` at byte 1024, counts two
matches for `a.*Z|X` although forward search has one whole-document match.
Truncating the input can choose a lower-priority alternative; growing only
when a returned match touches the read end cannot detect this. `a+Z|a` on
the first fixture also failed to complete in 30 seconds: the truncated
search repeatedly scans ahead before accepting one character, without
serving queued same-generation work during that step.
Use genuine matcher continuation or an exact cancellable scan isolated from
interactive requests. Until exact completion is supported, do not label a
partial count as final. Probes: `review_count_does_not_lose_long_anchored_match`,
`review_count_preserves_alternative_priority`, and the opt-in
`review_count_does_not_commit_truncated_alternative`. M5R11 remains open.

#### M5A3 [P1] Dense lookaround reprocesses the whole line per match

Locations: [window](../crates/rte_search/src/lib.rs#L579) and
[backtrack](../crates/rte_search/src/lib.rs#L613).
Each match copies the same line into `buf` and validates all its UTF-8 again.
For `(?=a)` on single lines of 16, 32 and 64 KiB of `a`, release medians
were 8.69, 29.86 and 105.65 ms. These are simple successful matches, not
catastrophic backtracking; `backtrack_limit` does not bound this repeated
copy/validation cost. Cache the decoded window and retain a position within
it; reuse captures during replacement. Add dense single-line scaling and
same-generation navigation-latency tests. Probe:
`review_profile_backtracking_dense_line` (timings, not a pass/fail speed gate).

#### M5A4 [P2] Agreement between backward windows is not proof of a match

Location: [find_prev](../crates/rte_search/src/lib.rs#L346).
For literal `aa` in 300,001 `a` bytes, forward enumeration ends at
`299998..300000`; backward search returns `299999..300001`. Both suffix
windows start at odd offsets and agree on the wrong partition. Thus the
larger-window check still violates the function's canonical forward-order
contract. Preserve a verified match boundary/continuation, or enumerate from
the actual scope start when correctness cannot be established. Probe:
`review_backward_literal_preserves_forward_match_partition`. M5R6 remains open.

#### M5A5 [P2] Capture replacement does not preserve the search scope

Location: [captures](../crates/rte_search/src/lib.rs#L370).
In `aaaa`, Find in Selection over `0..2` finds `a+` at `0..2`, but replacing
with `[$&]` fails. Capture re-matching runs to the full document end, finds
`0..4`, then rejects it. A literal replacement without groups succeeds through
the trusted expansion path, so replacement syntax changes whether the same
selection is editable. Carry the original search bounds or verified captures
through enumeration and replacement, retaining required lookaround context.
Probe: `review_capture_replace_uses_same_scope_as_find`.

#### M5A6 [P2] Delayed match indexes attach to the wrong selection

Location: [find_reply](../crates/rte_app/src/editor/find.rs#L681).
`Outcome::Index` omits the requested position. If an index for the first match
arrives after the caret selects the third match, it labels the third match
`1 of 3`; `sync_find` then sees an index for the current range and need not
correct it. Document/query generations cannot distinguish these requests.
Return the requested match position or a navigation request ID and reject
replies for another selection. Probe:
`review_old_index_reply_does_not_label_new_selection`.

#### M5A7 [P2] Add Next Occurrence stops at an already-selected hit

Location: [apply_occurrence](../crates/rte_app/src/editor/cursors.rs#L201).
With the first two `cat` ranges selected in `cat cat cat` and the first
selection primary, Ctrl+D finds the second selection and returns without
adding the third. Repeating the command cannot make progress. Continue past
already-selected ranges with a wrap/visited bound; retain early limit checks
and atomic selection updates. Probe:
`review_next_occurrence_skips_already_selected_match`.

#### Validation and Performance

- 188 existing focused tests passed: search 27, edit 22, application 104,
  view 27 and renderer 8. No workspace-wide test run.
- Seven new correctness assertions fail. Two profiling controls pass their
  fixture/result checks; this does not establish performance acceptance.
  The separate 30-second count timeout was reproduced once and is now opt-in.
- Literal viewport improvement verified: a sized 800 x 600 headless editor
  on a dense 1 MiB line scans/retains 65,536 ranges in 0.779 ms median
  (0.877 ms maximum, nine runs). The first unsized fixture scanned no rows
  and was discarded, not reported as an optimization result.
- Focused native benchmark medians in ms: M01 0.40, M03 1.19, M05 3.61,
  M06 3.82, M08 0.47, F01 0.58, F02 234.21, F03 3.85. M06 p95 was
  9.93 ms and F03 p95 10.20 ms. These are local samples, not a startup A/B,
  comparative editor ranking or reference-hardware certification.
- Probes live only in ignored `target/`. The console application harness is
  compiled from an in-memory copy of current source: only the subsystem
  attribute, module/include paths, and test access to `editor_on` and
  `find_reply` change. Real matcher, worker, editor and transaction logic
  are used. Fake files/headless clipboard only; no user settings or system
  clipboard access. The allocation probe reports the largest individual Rust
  allocation request, not peak process memory; fixture storage is excluded.
- Release probes use opt-level 3, fat LTO, one codegen unit, x86-64-v3 and
  Cargo's exact release dependency artifacts. Initial harness link errors
  (LTO and duplicate host/target artifacts) were corrected before execution.

Commands run:

```powershell
cargo build -p rte_app --release --locked --message-format=json
cargo test -p rte_search -p rte_edit --locked
cargo test -p rte_view -p rte_render --locked
./target/m5-astra-app.exe --skip astra_review --test-threads=1
./target/m5-astra-app.exe editor::astra_review --nocapture --test-threads=1
./target/m5-astra-review.exe review_ --nocapture --test-threads=1
cargo xtask bench-perf --only M01,M03,M05,M06,M08,F01,F02,F03
```

The two `review_` invocations intentionally exit 101 for the assertions above;
these are reproduced defects, not passing regression coverage. Append-only
task preservation, local links, source fingerprints and whitespace are checked
with PowerShell and `git diff --check` (no-index for this untracked task).
Not run: M07/system clipboard, new startup A/B, 1 GiB bounds, mixed-input
256 MiB responsiveness, manual DPI/mouse checks, full suites or hosted CI.
The remaining findings and these acceptance gaps prevent M5 approval.

### Astra Review Response

2026_10_24, Claude Opus 5.5 (Implementer). All findings accepted.

- **M5A1:** `Searcher::expand_within` computes the expansion size from the
  group ranges (case operators and Preserve Case counted as tripling) and
  refuses before copying when it and one temporary copy exceed the remaining
  Replace All budget. Tests: `expansion_is_refused_before_copying_beyond_its_budget`,
  `replace_all_checks_its_cap_while_building`.
- **M5A2:** counts run on their own thread as one exact forward enumeration
  of the whole scope; navigation, highlights and indexes stay on the search
  thread and are answered while a count runs. The windowed steps are gone.
  Tests: `counts_are_exact_across_long_inputs` (`a\nb` across 4 MiB,
  `\Aa+Z`, `a+Z|a`, `a.*Z|X`), `dense_counts_run_beside_navigation_and_keep_bounded_checkpoints`.
- **M5A3:** the backtracking window is read and validated once and kept while
  later searches see the same text and context; lines with invalid UTF-8 are
  left out of multiline windows instead of skipping the window. Tests:
  `dense_look_around_reads_each_line_once`, profile `profile_dense_cases`.
- **M5A4:** `find_prev` takes a canonical anchor (the scope start or a count
  checkpoint, which are match starts) and enumerates forward from it; only
  literals that cannot overlap themselves use backward windows (exact for
  them). The UI scans backwards only for such literals. Checkpoints are
  spaced by 1,024 matches or 4 MiB, at most 4,096. Tests:
  `backward_search_keeps_the_forward_partition`,
  `backward_search_keeps_the_forward_match_partition`.
- **M5A5:** `Searcher::set_end` limits matches and capture re-matching to the
  scope end; Replace One and Replace All pass their scope. Tests:
  `captures_stay_inside_the_search_end`, `replace_one_verifies_captures_inside_the_scope`.
- **M5A6:** index replies carry the requested match start and apply only to
  a primary selection starting there.
  Test: `an_old_index_reply_does_not_label_a_new_selection`.
- **M5A7:** Add Next Occurrence passes already-selected matches, at most once
  per selection, on the UI and through the search thread; with every
  occurrence selected it reports "No more occurrences".
  Test: `add_next_occurrence_passes_already_selected_matches`.

Remaining limits: highlights stop after 65,536 ranges; matches past 64 KiB of
a line are not highlighted (the view does not draw them); a backward search
without count checkpoints reads from the scope start.

Release profiles (`cargo test --release -p rte_search -- --ignored
--nocapture profile`, same for `-p rte_app`), medians of 9 runs:

| Case                                     | Review [ms] | Now [ms] |
| ---------------------------------------- | ----------- | -------- |
| `(?=a)` on a 16 KiB line                 | 8.69        | 1.02     |
| `(?=a)` on a 32 KiB line                 | 29.86       | 2.12     |
| `(?=a)` on a 64 KiB line                 | 105.65      | 4.24     |
| Previous `aa` in 300,001 `a`             | wrong match | 1.51     |
| Previous `a{2}` in 300,001 `a`           | wrong match | 6.55     |
| Count `\Aa+Z`, 6 MiB (one run)           | count 0     | 7.7      |
| Count `a+Z\|a`, 6 MiB (one run)          | > 30 s      | 19.1     |
| Replace All `$&$&` on a 16 MiB match     | 32 MiB copy | 81.0     |

The Replace All case is refused with no copy; the time is the match scan.

Focused benchmark (`cargo xtask bench-perf --only
L01,M01,M02,M03,M04,M05,M06,M07,M08,F01,F02,F03`), medians:

| ID  | Test                          | Median [ms] | p95 [ms] |
| --- | ----------------------------- | ----------- | -------- |
| L01 | Empty file, first paint       | 21.6        | -        |
| M01 | Add next occurrence           | 0.42        | 2.94     |
| M02 | Select all occurrences        | 0.82        | 3.21     |
| M03 | Column selection, 1,000 lines | 1.26        | 8.40     |
| M04 | Insert cursor below           | 0.46        | 9.65     |
| M05 | Type at 10,000 carets         | 4.18        | 4.67     |
| M06 | Backspace at 10,000 carets    | 3.76        | 4.99     |
| M07 | Paste 10,000 pieces           | 5.09        | 5.18     |
| M08 | Undo and redo, 10,000 carets  | 0.50        | 0.59     |
| F01 | Find next, 256 MiB            | 0.59        | 0.69     |
| F02 | Count matches, 256 MiB        | 219.97      | 220.21   |
| F03 | Toggle RegEx, 8 MiB           | 3.20        | 3.85     |

L01 varied from 14.7 to 22.3 ms across this session without code changes;
it is not an A/B. Commands: `cargo test -p rte_search` (31, 1 ignored),
`-p rte_edit` (22), `-p rte_view` (27), `-p rte_render` (8), `-p rte_platform`
(4, 1 ignored), `-p rte_app` (108, 1 ignored), `-p xtask` (8);
`cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all`.
