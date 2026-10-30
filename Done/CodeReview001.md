# Code Review 001

Status: Fixed (2026_10_30). See [Final Status](#final-status).

## Task

Review the whole code base for bugs, mishandled edge cases and
opportunities to make the editor faster. Several reviewers record their
findings independently in this document; fixes are decided afterwards.

## Scope

- Included: every crate under `crates/` (`rte_text`, `rte_edit`,
  `rte_search`, `rte_syntax`, `rte_spell`, `rte_view`, `rte_render`,
  `rte_platform`, `rte_assets`, `rte_app`) and `xtask`.
- Excluded: documentation, CI workflows and helper scripts under `tools/`.
- Operator rules apply: single user on a single computer; events closer
  than a person can act (about 10 ms) are not findings. Command IDs,
  configuration keys and the command line stay compatible.
- No code changes are part of this task. Each fix gets its own approval
  (and a task document when it is a feature or design change).

## Design

- Each reviewer adds a `Findings - <Reviewer>` section with IDs
  `CR001-<initial><n>` (for example `CR001-C1`), so findings of different
  reviewers never collide.
- A finding states its category (Bug, Edge case, Performance), severity
  (High, Medium, Low), the code location, a concrete trigger, the actual
  and the expected result, a suggested fix and the evidence (code trace,
  probe or test).
- Candidates that were investigated and found correct are listed briefly,
  so later reviewers can skip or challenge them.
- Reviewers may confirm, dispute or extend earlier findings under their own
  section; earlier sections are not edited.

## Alternatives

- One reviewer only: faster, but misses what a second reading catches.
- Fix while reviewing: mixes review and implementation and bypasses the
  approval of each change.

## Runtime Effects

Not applicable: a review changes no code. Each accepted fix states its own
runtime effects.

## Tests

Not applicable to the review itself. Each accepted finding gets a focused
regression test with its fix (the suggested test is given per finding).

## Implementation Steps

1. Reviewers record findings (this step).
2. The operator triages the findings: accept, reject or defer.
3. Accepted findings are fixed in small, independently tested changes.

## Acceptance Criteria

- Every planned reviewer has recorded findings and a reviewer record.
- Every finding has a triage decision.

## Findings - Claude Opus 5.5

Method: a full read of the crates split by area (text, editing, search,
syntax, spelling, view and rendering, load and save, palette and
configuration), each area reviewed twice, then every candidate traced in
the code by hand. A disposable probe in `%TEMP%` checked `ReplaceFileW`
behavior. Most first-pass candidates did not survive the trace; they are
listed under Checked and Correct.

| ID       | Category    | Severity | Title                                                           |
| -------- | ----------- | -------- | --------------------------------------------------------------- |
| CR001-C1 | Bug         | Medium   | Saving a file with the read-only attribute leaves a full copy   |
| CR001-C2 | Edge case   | Medium   | The first spelling suggestion is not the most likely fix        |
| CR001-C3 | Edge case   | Low      | Files with CR-only line breaks show as one line                 |
| CR001-C4 | Edge case   | Low      | A scrolled panel keeps empty rows after the window grows        |
| CR001-C5 | Performance | Low      | Undo history does not count the memory of its selections        |
| CR001-C6 | Edge case   | Low      | Singular counts read `1 lines`                                  |

### CR001-C1 - Saving a file with the read-only attribute leaves a full copy

- Category: Bug. Severity: Medium. Confidence: High.
- Location: [save.rs](../crates/rte_app/src/save.rs) `save` (step 3,
  `ops.replace`) and `replace_failed` (the `_ =>` branch); the loader and
  [editor.rs](../crates/rte_app/src/editor.rs) never look at the attribute.
- Trigger: open a file whose read-only attribute is set (Explorer,
  Properties, Read-only), edit it and press `Ctrl+S` a few times.
- Actual: the file opens editable. Every save writes the whole text to a
  new sibling `<name>.~rte-<pid>[-n].tmp`, then `ReplaceFileW` fails with
  error 5 (access denied; confirmed by a probe: `File.Replace` onto a
  read-only target fails, a hidden target keeps its attribute). The
  `_ =>` branch keeps the temporary file and reports
  `Cannot replace <path> (error 5). Kept: <path>, <tmp>`. `free_sibling`
  picks a new name each time, so each attempt leaves one more full copy.
- Expected: a clear message (the file is read-only) and no leftovers; the
  target is unchanged, so the temporary file can be deleted.
- Suggested fix: at load, open such a file with the read-only lock (as
  `--read-only` and malformed UTF-16 do) and say why on the status line;
  in `replace_failed`, treat `ERROR_ACCESS_DENIED` with `target_intact()`
  like the other intact-target codes (delete the temporary file) and name
  the cause.
- Test: `FakeFiles` failing `replace` with code 5 leaves no temporary
  file and reports the cause; a loader test for the attribute.

### CR001-C2 - The first spelling suggestion is not the most likely fix

- Category: Edge case (user visible). Severity: Medium. Confidence: High.
- Location: [suggest.rs](../crates/rte_spell/src/suggest.rs) `edits`.
- Trigger: `mispelled` (seen in the documentation screenshot session),
  then `Ctrl+.`, `Enter`.
- Actual: `dispelled` is first and `Enter` inserts it; `misspelled` comes
  second. Distance-1 candidates are ordered transpositions, replacements,
  insertions, deletions, so a replacement (`m` to `d`) beats the missing
  letter.
- Expected: the most common typos first. A doubled letter typed once
  (insertion) and swapped letters are more frequent than a wrong first
  letter.
- Suggested fix: order distance-1 edits as transpositions, insertions,
  deletions, then replacements; optionally rank replacements of the first
  letter last. Keep the cost unchanged (same candidate set).
- Test: `candidates("mispelled", ..)` starts with `misspelled`; existing
  suggestion tests still pass.

### CR001-C3 - Files with CR-only line breaks show as one line

- Category: Edge case. Severity: Low. Confidence: High.
- Location: [tree.rs](../crates/rte_text/src/tree.rs) (line counts use
  `memchr_iter(b'\n', ..)`) and `LineEnding::detect` in
  [encoding.rs](../crates/rte_text/src/encoding.rs).
- Trigger: open a classic Mac text file (`a\rb\rc`).
- Actual: one line of 5 bytes; `Enter` inserts `\r\n` (no `\n` in the
  sample, so CRLF is chosen), mixing styles.
- Expected: either show the lines, or keep the file as is but tell the
  user (status line) and insert `\r`.
- Suggested fix: decide the policy. The cheapest is a status-line notice
  and `LineEnding::Cr` for new breaks; splitting lines on lone `\r` touches
  the text engine and every line query.
- Test: loader test for a CR-only sample.

### CR001-C4 - A scrolled panel keeps empty rows after the window grows

- Category: Edge case. Severity: Low. Confidence: Medium.
- Location: [editor.rs](../crates/rte_app/src/editor.rs) `draw_popup`
  (`let first = panel.first.min(panel.lines.len())`); `panel.first` is
  clamped to `lines - page` only by keys and the wheel.
- Trigger: open `Application: Keyboard Shortcuts` in a short window,
  scroll to the end, enlarge the window.
- Actual: the panel stays at the old first line, leaving empty rows below
  the last line until a key or the wheel scrolls it.
- Expected: the last page fills the panel.
- Suggested fix: clamp `first` to `lines - page` when drawing.
- Test: headless editor test resizing with a scrolled panel.

### CR001-C5 - Undo history does not count the memory of its selections

- Category: Performance (memory). Severity: Low. Confidence: Medium.
- Location: [history.rs](../crates/rte_edit/src/history.rs) `Step`,
  `trim`.
- Trigger: many cursors (up to `editor.multiCursorLimit`, 10,000 by
  default) and many separate steps (each typed word after a non-word
  character starts one).
- Actual: each step keeps `sels_before` and `sels_after`; only inserted
  bytes count toward `MAX_HISTORY_BYTES`. With 10,000 cursors a step holds
  about 2 x 10,000 selections, so 10,000 steps can hold gigabytes.
- Expected: the history bound covers what it holds.
- Suggested fix: add the selection bytes of a step to `inserted` (or a
  separate budget) so `trim` drops old steps.
- Test: history unit test with large selection sets and a small byte
  budget.

### CR001-C6 - Singular counts read `1 lines`

- Category: Edge case (text). Severity: Low. Confidence: High.
- Location: [editor.rs](../crates/rte_app/src/editor.rs) status line
  (`format!("{} lines", ..)`) and
  [find.rs](../crates/rte_app/src/editor/find.rs) `find_reply`
  (`{n} lines skipped`).
- Trigger: an empty or one-line file; a search skipping one line.
- Actual: `1 lines`, `1 lines skipped: ...`.
- Expected: `1 line`.
- Suggested fix: a small plural helper used by both.
- Test: status text for a one-line document.

### Checked and Correct

Candidates traced and found correct, with the reason:

- `merge_leaves` underflow (`rte_text` tree): `total > COALESCE_MAX >=
  MIN_LEAF`, so `ca.len() - (MIN_LEAF - cb.len())` cannot underflow.
- Zero-width match at the document end: `it.from` becomes `len + 1` and
  the loop ends.
- Literal search across leaf boundaries: the overlap buffer is reused and
  at most `2n - 2` bytes per boundary.
- Forward `Find` past the scope end wraps: `s.find(scope.start, scope.end)`.
- `Replace` after an edit: `op_reply` requires the reply and operation
  snapshots to equal the current one.
- Find in Selection keeps its scope when the selection changes: matches
  VS Code (the scope is fixed until toggled).
- Go to Line on an empty file: `line_count()` is at least 1.
- Palette selection after filtering: `refresh` resets `first` and
  `selected`.
- Popup font after a DPI change: `sync_ui_renderer` runs after every
  event.
- Seam cache after an edit: `same_tree` compares root pointers, so any
  edit resets it.
- Growing past `syntax.maxFileMiB`: `sync_syntax` restarts highlighting
  on both crossings.
- Markdown mask request when the syntax thread stops: `MaskGuard` answers
  `None` on drop, the job replies `Unavailable`.
- Spelling verdict cache after Add or Ignore: `add_user` and `ignore`
  clear it.
- Word offsets with multi-byte letters and `'s`: `start` is advanced past
  trimmed apostrophes; the end uses the stem length from the same start.
- All-capital misspellings: never marked (acronyms are skipped), so their
  suggestion casing does not matter.
- Adding `Teh` does not accept `teh`: documented behavior (user words
  match as written or lowercase).
- `Backspace` between a pair at the file end: `to_vec` returns one byte,
  the pair test fails and `delete_pair` returns `None`.
- Hidden attribute on save: kept by `ReplaceFileW` (probe).
- Favorites and dictionary writes: on their worker threads, never on the
  UI thread.

## Reviewers

### 2026_10_30 - Claude Opus 5.5

- Role: Reviewer
- Activity: Design
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Review task created and first full review recorded: six
  verified findings (CR001-C1 to C6; C1 and C2 Medium) and the candidates
  found correct. No code changed. Waiting for the other reviewers.

### 2026_10_30 - GPT-6.1 Sol

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: GPT-6.1 Sol
- Effort: High
- Context Window: 1M
- Outcome: Independent code review across all crate areas and `xtask`.
  Recorded six new findings, CR001-S1 to S6, with bounded correctness or
  timing probes. Five probe assertions fail and three timing controls pass;
  existing focused checks and clippy pass. No production fixes or triage
  decisions made. Waiting for the other reviewers and operator triage.

### 2026_10_30 - GitHub CoPilot Fusion

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot SDK in VS Code
- Model: GitHub CoPilot Fusion
- Effort: High
- Context Window: NA
- Outcome: Independent performance-focused pass confirmed a separate
  same-row selection-fill cost adjacent to S6. No production code or triage
  decisions changed; finding awaits operator triage.

### 2026_10_30 - GitHub CoPilot Fusion

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot SDK in VS Code
- Model: GitHub CoPilot Fusion
- Effort: High
- Context Window: NA
- Outcome: Expanded the performance review across the crates and `xtask`,
  timed the original finding, and recorded eight additional findings
  (CR001-F2 to F9). Focused existing tests passed. No production fixes or
  triage decisions made; waiting for the other reviewers and operator triage.

### 2026_10_30 - Claude Opus 5.5

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Summarized all 21 findings for the operator and recorded the
  operator's triage in the Triage section: 15 accepted (S5 partly), C3
  notice only, S2 documentation only, F2, F3 and F7 no fix. F2 is a final
  decision. No code changed.

## Findings - GPT-6.1 Sol

Method: inspect the ownership and control paths of the text/editing core,
search, syntax/spelling, view/renderer, platform, assets/build, application
lifecycle/save/palettes/configuration and packaging. Follow candidates to
their computing code and use disposable probes for the findings below.
This is a review pass, not an exhaustive correctness proof or a claim that
every execution path was tested. Documentation, workflows and `tools/`
were excluded as requested. Sub-10 ms races and cross-session coordination
are not findings. No fixes are proposed for implementation without approval.

### CR001-S1 - Enter infers the wrong opener past the line scan limit

- Category: Bug. Severity: Medium. Confidence: High. Triage: Pending.
- Location: [`newline_indent`](../crates/rte_edit/src/code.rs#L452),
  particularly the prefix ending at `content.start + COLUMN_SCAN_LIMIT`.
- Trigger: in a code file, place the caret after `{` following 65,536
  ASCII characters on one line and press Enter. Conversely, use `{`,
  65,536 spaces, then `x`, and press Enter after `x`.
- Actual: the first case does not add the promised indentation; the second
  adds it despite the actual preceding non-space character being `x`.
  The code reads the last non-space byte of the first 64 KiB, not of the
  text before the caret. A 65,535-character control handles `{` correctly.
- Expected: the trigger is determined near the actual caret independently
  of the separately bounded leading-indentation scan.
- Suggested fix: obtain the preceding non-space token from a bounded
  backwards walk near the caret; keep leading indentation lookup separate.
  If relevant context exceeds a bound, use an explicit conservative path,
  not an unrelated byte before the cutoff.
- Evidence: two failing pure-edit assertions,
  `review_enter_uses_the_opener_at_the_actual_caret_after_the_scan_limit`
  and `review_enter_does_not_use_an_opener_far_before_the_caret`.
- Regression: both sides of the 64 KiB cutoff, opener/non-opener cases,
  and multiline selections with the closing delimiter after the selection.

### CR001-S2 - Multiline lookaround misses equivalent dotall and whitespace patterns

- Category: Bug. Severity: Medium. Confidence: High. Triage: Pending.
- Location: [`regex`](../crates/rte_search/src/lib.rs#L157), where
  backtracking is considered multiline only for textual `\n` or a literal
  newline in the pattern.
- Trigger: search `start\nend!` using RegEx. The explicit-newline pattern
  `start\nend(?=!)` finds `0..9`, but `(?s)start.*end(?=!)` and
  `start\s+(?=end)` find nothing.
- Actual: valid patterns that cross a line without spelling `\n` are sent
  through the per-line fallback. Their necessary following context never
  enters the backtracking window.
- Expected: dotall, whitespace classes and equivalent newline-capable
  patterns behave consistently within the documented backtracking limits.
- Suggested fix: determine newline reachability from parsed regex
  semantics, or use the bounded multiline fallback conservatively when it
  cannot be ruled out. Retain the existing backtracking/window caps.
- Evidence: `review_multiline_backtracking_recognizes_dotall_and_whitespace`
  fails both alternative forms while its explicit-newline control passes.
- Regression: these cases plus newline-capable character classes,
  scoped flags, lookbehind and scope-edge matches.

### CR001-S3 - Visible spelling marks disappear beyond the long-line seam

- Category: Bug. Severity: Medium. Confidence: High. Triage: Pending.
- Location: [`RowRanges::waves`](../crates/rte_view/src/lib.rs#L781).
- Trigger: horizontally scroll a plain-text line past byte 65,536, with a
  misspelled word wholly in the visible tail. The spelling request path
  includes that tail and can return its mark.
- Actual: the text is drawn, but `waves` stops when `word.end` exceeds the
  exactly scanned prefix. A display-list probe with a visible `teh` and a
  supplied authoritative mark creates zero waves.
- Expected: every retained misspelling in the drawn tail has the same blue
  wavy mark as one before the seam, under the documented approximate
  one-column-per-byte tail rule.
- Suggested fix: calculate tail mark columns with the same seam/tail
  mapping used for glyphs and range fills; clip to the viewport rather
  than rejecting every mark beyond the prefix.
- Evidence: `review_spelling_wave_is_drawn_for_a_visible_long_line_tail`
  verifies the marked word's glyph is present, then fails the wave check.
- Regression: words before, across and after the seam, horizontally
  scrolled views, and multibyte text at the cut.

### CR001-S4 - Whole Word treats a combining accent as a word boundary

- Category: Edge case. Severity: Low. Confidence: High. Triage: Pending.
- Location: [`is_word`](../crates/rte_search/src/lib.rs#L193) and its use by
  the whole-word filter; compare `rte_edit::word_at`/caret boundaries.
- Trigger: Whole Word literal search for `cafe` in `cafe\u{301}` (a
  decomposed accented word).
- Actual: search returns `0..4`, ending before the accent, while the
  editor's word selection spans `0..6`. Replacement of that apparent whole
  word can leave the combining accent attached to different replacement
  text. Plain `cafe ` and `cafe_name` controls behave as expected.
- Expected: a consistent Unicode word-boundary policy that does not split
  an attached combining mark from a whole-word match.
- Suggested fix: account for combining marks in the boundary predicate,
  preferably sharing an explicit word-boundary policy with editing. This
  is not a request for accent-insensitive matching.
- Evidence: `review_whole_word_does_not_end_before_a_combining_accent`.
- Regression: decomposed/precomposed accents, match boundaries adjacent
  to combining marks, underscores and ordinary punctuation.

### CR001-S5 - Tiny Markdown masks perform work over unseen document text

- Category: Performance. Severity: Medium. Confidence: High. Triage: Pending.
- Location: [`MarkdownMask::walk`](../crates/rte_syntax/src/mask.rs#L72)
  and [`inline`](../crates/rte_syntax/src/mask.rs#L98).
- Trigger: spelling asks for a small visible range in a Markdown document
  with many paragraphs, or in one large paragraph.
- Actual: `walk` iterates all children before each recursive call rejects
  out-of-range siblings. An intersecting inline node is copied and parsed
  whole, even for an 80-byte request. It has no cancellation callback.
  Release medians with the production tree-sitter allocator:
  a fixed six-byte range takes 0.079 / 0.809 / 10.11 ms at
  1,000 / 10,000 / 50,000 paragraphs; a fixed 80-byte range takes
  0.50 / 5.14 / 54.85 ms at paragraph sizes 5,001 / 50,001 / 500,001 bytes.
  Host parsing and fixture construction are outside these timings.
- Repeat control: the 500,001-byte paragraph takes 56.09 ms and the
  50,000-paragraph case 9.73 ms. These repeated samples support the scaling
  finding; they are not hard timing guarantees.
- Expected: bounded mask work that does not hold up the syntax worker for
  unseen siblings or repeatedly parse a large unchanged inline region.
- Suggested fix: byte-seek to intersecting children and stop at the range
  end, as injection discovery already does. Cache authoritative inline
  parses/exclusion intervals for the snapshot, invalidate them on edits,
  and poll cancellation during discovery/parsing. Do not lose Markdown
  context by blindly parsing a context-free substring.
- Evidence: passing scaling controls
  `review_profile_markdown_mask_unseen_siblings` and
  `review_profile_markdown_mask_unseen_paragraph_tail`. These establish
  avoidable worker latency, not a measured UI-frame stall or peak heap.
- Regression/performance check: fixed view as document/paragraph grows,
  repeated requests without edits, cancellation, and inline delimiters
  outside the requested substring.

### CR001-S6 - Same-row carets repeatedly rescan the entire preceding text

- Category: Performance. Severity: Medium. Confidence: High. Triage: Pending.
- Location: [`caret_row`](../crates/rte_view/src/lib.rs#L918); inspect
  `selection_row` for the equivalent repeated-column pattern.
- Trigger: use many cursors on one long line (for example Select All
  Occurrences), within the default 10,000-cursor limit, then paint.
- Actual: each caret calls `column_of` from byte zero before checking
  whether it is visible. The same row prefix is scanned repeatedly,
  including for thousands of offscreen carets. A release display-list
  control takes 1.37-2.11 / 33.46-47.39 / 134.08-190.01 ms across two
  runs at 1,000 / 5,000 / 10,000 same-row carets in a fixed 800 x 600
  view. The respective hidden-caret controls take 0.001-0.002 /
  0.006-0.010 / 0.015-0.020 ms. Renderer rasterization and fixture creation
  are excluded; builder buffers are reused.
- Expected: one advancing column walk over sorted carets plus early
  rejection after the right edge, as `RowRanges::matches` already does.
- Suggested fix: use `column_after` with monotonic offset/column state;
  stop once remaining sorted carets cannot be visible. Check the same
  optimization for sorted selection fills without changing overlapping
  selection semantics.
- Evidence: `review_profile_many_carets_on_one_row`, a passing timing
  control rather than a flaky wall-clock assertion.
- Regression/performance check: same-row cursor scaling, tabs, zero-width
  marks, reversed selection orientation and horizontal scrolling.

### Checked and Correct - Sol

- Core storage: shared copy ranges are validated against the base
  snapshot; transactions validate ordering before mutation. UTF-16
  streaming retains incomplete units between chunks and refuses malformed
  UTF-16 save paths. Existing pure-core and large-tree tests pass.
- Clipboard cut: the edit lock is checked before work, and deletion runs
  only after a successful clipboard write. Clipboard metadata split uses
  checked lengths and UTF-8 string boundaries. No native clipboard test
  was run or clipboard content replaced.
- Save/lifecycle: dirty files and running saves block replacement by a
  pending drop; close cancels a pending open; dropped missing files and
  folders are refused. Existing save and drop filters pass. C1 is a
  separate existing finding, not duplicated here.
- Search: replacement authority is snapshot-scoped; counting checkpoints
  and viewport results have explicit caps. The new S2/S4 edge cases are
  separate from the earlier zero-width and capture-context fixes.
- Syntax: incomplete windows are explicitly provisional; ordinary layer
  discovery uses byte seeking. S5 concerns the newly separate Markdown
  mask walk, not a reversal of the injection-discovery fix.
- Spelling: the three implementation findings already recorded in
  `Done/Spelling001.md` are not copied as new independent discoveries.
  The present core suite includes the smart-apostrophe override regression.
  Cross-session persistence races remain outside the operator's scope.
- Palette/configuration: query length, selected rows, setting ranges and
  Hunspell relative-path validation have existing focused checks. No new
  defect confirmed in this pass. C4 remains the first reviewer's finding.
- Platform/render/assets: file moves refuse to overwrite, replacement
  preserves a backup, fonts are embedded and glyph caches use glyph IDs.
  The inspected event path handles UTF-16 surrogate pairs and DWM writes
  opaque pixels. No new confirmed native-API safety finding is asserted.
- Packaging: the archive root is the metadata file name, while its ZIP
  filename stays versioned. The archive-layout regression passes; this
  pass does not rebuild or certify a release package.
- C5 independently remains a source-supported memory concern: history's
  inserted-byte budget does not account for selection arrays. It is not
  assigned a duplicate S identifier.

### Validation - Sol

- Existing focused checks: 141 pure text/edit/search/view/render tests,
  43 syntax/spelling-core tests, 82 application test executions across
  the selected filters (some overlap), three platform file tests and one
  archive-layout test pass. One existing search test is ignored.
  Clippy passes for all reviewed crates and `xtask`.
- Eight isolated probes: five correctness assertions fail for S1-S4
  (two assertions for S1), three performance controls pass for S5/S6.
  Failure exit 101 records reproduction, not passing regression coverage.
- Probes are ignored `target/code-review-sol-core.rs` and `.exe`, compiled
  against exact Cargo JSON release artifacts with opt-level 3, fat LTO,
  one codegen unit, x86-64-v3 and self-contained GNU linking. Markdown
  controls install the production tree-sitter allocator before any use.
  Timings are local medians, not comparative-editor or reference-hardware
  certification. No peak-memory numbers are inferred from them.
- Original review-document bytes and existing contributor records are
  preserved. Source fingerprints are compared with the start of this pass;
  manual edits are limited to this review and ignored probes.
- Not run: the full workspace test suite, full performance suite, native
  clipboard test, package build, hosted CI, installed-app/settings
  inspection, or helper-script/workflow review. No implementation fixes,
  commits or task completion/triage changes are made.

Commands:

```powershell
cargo test -p rte_text -p rte_edit -p rte_search -p rte_view -p rte_render -p rte_assets --locked
cargo test -p rte_syntax -p rte_spell --locked
foreach ($filter in @('save', 'palette', 'config', 'drop', 'spell')) {
    cargo test -p rte_app $filter --locked | Out-String
}
cargo test -p rte_platform files --locked
cargo test -p xtask package_archive --locked
cargo clippy -p rte_edit -p rte_search -p rte_syntax -p rte_view -p rte_render -p rte_app -p rte_platform -p rte_assets -p rte_text -p rte_spell -p xtask --all-targets --locked -- -D warnings
./target/code-review-sol-core.exe review_ --nocapture --test-threads=1
```

All six findings are Pending operator triage. This review is complete for
this contributor; wait for the other reviewers before choosing fixes.

## Findings - GitHub CoPilot Fusion

Method: trace the UI paint path and inspect bounded work, allocation and
iteration patterns in the editor/view/search hot paths. This pass confirms
the selection-fill optimization candidate explicitly left open by S6; it is
separate from S6's caret loop. No source changes or tests were run.

### CR001-F1 - Selection fills repeatedly rescan the row prefix

- Category: Performance. Severity: Medium. Confidence: High. Triage: Pending.
- Location: [`selection_row`](../crates/rte_view/src/lib.rs#L871),
  called once per drawn row from [`ViewBuilder::build`](../crates/rte_view/src/lib.rs#L434);
  [`column_of`](../crates/rte_edit/src/columns.rs#L61) starts each scan at
  column zero.
- Trigger: use Alt+Enter to select up to the configured match limit on one
  long line, then repaint. The default limit is 10,000; configuration permits
  up to 1,000,000 matches. Non-touching matches (for example, `a` separated
  by spaces) remain distinct selections.
- Actual: `selection_row` visits each visible-row selection and independently
  calls `column_of` for both endpoints. Since `column_of` walks from byte
  zero, a 64 KiB row with 10,000 evenly distributed matches can scan on the
  order of hundreds of millions of prefix bytes per frame. The loop also
  computes columns for selections beyond the horizontal viewport before
  discarding their fills.
- Expected: process sorted selection endpoints with one advancing
  offset/column state and stop once later selections are beyond the visible
  columns, without changing range merging or reversed-selection behavior.
- Evidence: source trace from `Editor::paint` through `ViewBuilder::build`
  and `selection_row`; `ROW_SCAN_LIMIT` bounds the scanned row at 64 KiB,
  while `multiCursorLimit` bounds selected matches at 10,000 by default and
  1,000,000 maximum. Unlike S6, this cost comes from the non-empty selection
  fill loop, not the caret loop.
- Regression/performance check: same-row, non-touching selections near the
  default limit on a 64 KiB line; compare display-list fills and verify
  approximately linear scaling. Include tabs, multibyte characters,
  horizontal scrolling, offscreen ranges, and reversed selections.

#### Follow-up confirmation

A release-mode probe used `"a "` repeated on one row, selecting each `a`,
with an 800-by-600 layout, 10-by-20 cells and the caret hidden so the
measurement excludes S6's caret loop. Median warm selection-only builds
took 2.2 ms for 1,000 selections, 53.7 ms for 5,000, and 214.4 ms for
10,000. These are local measurements, not end-to-end frame timings.

### CR001-F2 - An expensive backtracking call cannot promptly cancel

- Category: Performance. Severity: High. Confidence: High. Triage: Pending.
- Location: [`Searcher::backtrack`](../crates/rte_search/src/lib.rs#L740)
  polls before calling `fancy_regex::Regex::find_from_pos`, but cannot poll
  while that call runs. Search and count workers process their respective
  queues serially in [`search.rs`](../crates/rte_app/src/search.rs#L291).
- Trigger: regex `(?=a)(a+)+b` on a long line containing only `a`; then
  change or close the search after the scan starts. Lines up to 16 MiB
  are eligible for this engine.
- Actual: cancellation and complexity reporting wait for one engine call
  to return. A stale search can keep a worker busy, delaying new search
  requests and consuming CPU after the query changes. The count worker is
  separate, but can independently be blocked by the same pattern.
- Expected: cancellation or a real work budget bounds latency within an
  expensive match, and any complexity limit is reported without silently
  changing normal lookaround or backreference semantics.
- Evidence: a release probe set the stop callback after 20 ms. Single-line
  inputs of 8,192, 16,384, and 32,768 bytes took 50, 202, and 801 ms
  respectively; `stopped()` remained false and `complex_lines` was zero.
  A 1 MiB case was still running after 120 seconds and its isolated process
  was stopped. Existing cancellation tests do not cover a single costly
  engine call. This is separate from S2's multiline matching correctness.
- Suggested fix: use an interruptible backtracking implementation or a
  semantics-preserving bounded execution strategy, checking cancellation
  during expensive matching and reporting exhausted work explicitly.
- Regression/performance check: cancel this pattern during its first match,
  assert a bounded stop and that a newer queued query is serviced, and
  retain lookaround, backreference, multiline and complexity-limit tests.

### CR001-F3 - One leaf-crossing edit disables batching for a whole transaction

- Category: Performance. Severity: Medium. Confidence: High. Triage: Pending.
- Location: [`tree::apply_batch`](../crates/rte_text/src/tree.rs#L356),
  [`Snapshot::apply`](../crates/rte_text/src/snapshot.rs#L271), and
  [`Editor::apply_replaced_all`](../crates/rte_app/src/editor/find.rs#L844).
- Trigger: Replace All produces many small replacements, one of which
  crosses a text-tree leaf boundary.
- Actual: a single crossing makes the batch traversal return `None`; the
  fallback applies *every* edit individually on the UI thread, not just
  the crossing edit.
- Expected: retain batched processing for unrelated local edits even when
  a few edits cross leaves.
- Evidence: in a 1,600,000-byte snapshot, 100,000 two-byte replacements
  took 3.0 ms when all were local versus 70.2 ms when exactly one crossed
  the 8 KiB boundary. With 10,000 replacements the same comparison was
  0.34 versus 5.84 ms. The fixture repeated `aa` followed by 14 dots,
  replacing each `aa` with `bb`; the crossing case shifted the match at
  byte 8,192 one byte earlier. Replace All is not limited by the
  multicursor cap.
- Suggested fix: incorporate crossing edits into a batch or partition the
  transaction so a crossing does not force all unrelated edits onto the
  per-edit path. Preserve base offsets, change mapping and shared copies.
- Regression/performance check: compare results and position mappings with
  a reference application for mixed local and boundary-crossing edits;
  include large Replace All transactions and shared-copy cases.

### CR001-F4 - Delete Lines searches all earlier selections again per block

- Category: Performance. Severity: Medium. Confidence: High. Triage: Pending.
- Location: [`delete_lines`](../crates/rte_edit/src/lines.rs#L164).
- Trigger: place selections on alternating lines, for example with
  Select All Matches, and use Ctrl+Shift+K.
- Actual: for each line block, `.find` restarts at the beginning of the
  sorted selection list and repeatedly calls `Snapshot::line_of`. The
  command-building work grows approximately quadratically in the number
  of separate blocks.
- Expected: locate each block's first selection in one advancing pass or
  by an appropriate indexed search.
- Evidence: on `"a\nb\n"` repeated, with one caret on each `a` line,
  `delete_lines` alone took 22.4 ms for 1,000 blocks and 102.1 ms for
  2,000; constructing `line_blocks` alone took 0.06 and 0.13 ms.
  Transaction application and drawing were not included.
- Suggested fix: advance through the already sorted selections while
  visiting blocks, retaining the current primary-caret and column rules.
- Regression/performance check: alternating and adjacent blocks, reversed
  selections, final-line deletion and primary selection; verify near-linear
  scaling for thousands of blocks.

### CR001-F5 - Vertical movement and Tab rescan prefixes for every caret

- Category: Performance. Severity: Medium. Confidence: High. Triage: Pending.
- Location: [`column_at` and `offset_in_line`](../crates/rte_edit/src/motion.rs#L129),
  [`vertical`](../crates/rte_edit/src/motion.rs#L154), and
  [`tab`](../crates/rte_edit/src/commands.rs#L254); vertical movement is
  invoked for each selection on the
  [UI thread](../crates/rte_app/src/editor.rs#L1378).
- Trigger: create many distinct carets on a long row, then move them to
  another long row or press Tab.
- Actual: each caret copies and walks its own source-line prefix; vertical
  movement also walks the target-line prefix independently. Cached goal
  columns can avoid the source scan on later moves, but not the repeated
  target scans. Tab independently computes each caret's source column.
- Expected: share ordered per-row column and offset walks among carets,
  while preserving goal columns, tabs, Unicode widths and selections.
- Evidence: with `"a "` and `"b "` repeated into two 20,000-byte rows and
  10,000 evenly spaced carets, the isolated vertical calculations took
  241.7 ms and Tab command construction took 111.6 ms. At 5,000 carets
  they took 61.4 and 28.4 ms. This excludes transaction application and
  painting, and is distinct from F1's selection fills and S6's caret drawing.
- Suggested fix: batch the computations by source and target row using
  advancing column/offset state instead of rescanning each prefix.
- Regression/performance check: compare single- and multi-caret motion
  and insertion results with tabs, multibyte/wide characters, cached goals,
  short target lines and long-line scan limits; verify scaling.

### CR001-F6 - Enter duplicates prefix work on the ordinary code-mode path

- Category: Performance. Severity: Medium. Confidence: High. Triage: Pending.
- Location: [`newline_indent`](../crates/rte_edit/src/code.rs#L452),
  [`newline`](../crates/rte_edit/src/commands.rs#L144), and their
  [UI fallback](../crates/rte_app/src/editor.rs#L1444).
- Trigger: press Enter with many carets on a long code line when no caret
  follows an indentation opener.
- Actual: `newline_indent` copies and scans each prefix and constructs
  candidate edits, then returns `None`; the ordinary `newline` fallback
  copies and scans those prefixes again. Both functions copy the whole
  prefix even when its leading indentation ends immediately.
- Expected: reuse or avoid per-line prefix work on the no-special-indent
  path, without changing code-aware indentation or ordinary Enter.
- Evidence: on `"a     "` repeated to 60,000 bytes, with 10,000 carets,
  the code-aware no-op took 9.6 ms and ordinary Enter construction another
  8.0 ms, excluding edit application and paint. This is below the scan
  limit and is separate from S1's beyond-limit opener correctness issue.
- Suggested fix: share indentation information per line and avoid copying
  unnecessary prefixes or rebuilding work in the fallback.
- Regression/performance check: many same-line carets with no opener and
  mixed opener/non-opener cases; preserve split-pair, colon, tab and Unicode
  behavior and compare construction scaling.

### CR001-F7 - Symbol palette fuzzy matching can stall query input

- Category: Performance. Severity: Medium. Confidence: High. Triage: Pending.
- Location: [`Palette::push_choices`](../crates/rte_app/src/palette.rs#L315),
  [`Matcher::score`](../crates/rte_app/src/fuzzy.rs#L43), and the
  [symbol limit](../crates/rte_app/src/syntax.rs#L34).
- Trigger: Go to Symbol in a file with many long, valid symbol names,
  then type a matching query. The list permits 20,000 symbols, names are
  read up to 256 bytes, and the query permits 256 characters.
- Actual: every query update synchronously scores every label using a
  query-length by label-length dynamic-programming matrix, then builds and
  orders the rows on the UI thread.
- Expected: keep filtering responsive without changing ranking, matched
  character positions, ordering or selection behavior.
- Evidence: scoring alone took 19.6 ms for 20,000 45-character labels and
  the query `document_edit_event_then_repaint`. The labels were
  `handle_document_edit_event_then_repaint_` plus a five-digit suffix.
  With labels made from `data_conversion_` repeated eight times plus a
  five-digit suffix, and that prefix repeated four times as the query,
  scoring took 33.8 ms for 5,000 labels and 133.8 ms for 20,000. Row
  creation, sorting and paint were excluded.
- Suggested fix: use a semantics-preserving faster matcher or move large
  filtering jobs to cancellable worker snapshots with generation checks.
- Regression/performance check: compare scores, positions and stable order
  with the current matcher, including Unicode; exercise fast query changes
  and stale-result rejection with large symbol lists.

### CR001-F8 - Single-selection copy makes an avoidable full-size text copy

- Category: Performance. Severity: Medium. Confidence: High. Triage: Pending.
- Location: [`Editor::copy`](../crates/rte_app/src/editor.rs#L1002),
  [`edit::copy`](../crates/rte_edit/src/commands.rs#L349), and
  [`clip::encode`](../crates/rte_app/src/clip.rs#L18).
- Trigger: copy or cut one large, valid UTF-8 selection; the performance
  harness explicitly includes a 64 MiB copy.
- Actual: `edit::copy` first allocates the selected bytes, then `encode`
  borrows that single piece and copies it into a second full-size `String`.
  Both buffers coexist before the platform prepares UTF-16 clipboard
  data. This work runs synchronously on the UI thread.
- Expected: avoid the second full-size copy when a single owned, valid
  UTF-8 piece can become the encoded text, while preserving metadata,
  invalid-byte replacement, line mode and multi-piece behavior.
- Evidence: without touching the native clipboard, a local 64 MiB ASCII
  selection took 15.6 ms to copy from the snapshot and another 19.7 ms
  to encode; counting UTF-16 units took 8.9 ms more. OS clipboard
  allocation and writing were not timed.
- Suggested fix: consider consuming the owned clip and transferring its
  single valid piece into the result; retain the existing fallback for
  invalid UTF-8 and cases requiring separators.
- Regression/performance check: large single-piece copy and cut, invalid
  UTF-8, empty and multi-piece selections, line mode and CRLF; verify
  clipboard text and private metadata as well as time and peak memory.

### CR001-F9 - Clipboard retries sleep on the UI thread

- Category: Performance. Severity: Low. Confidence: High. Triage: Pending.
- Location: [`clipboard::open`](../crates/rte_platform/src/clipboard.rs#L19),
  called synchronously by clipboard
  [read and write](../crates/rte_platform/src/clipboard.rs#L87).
- Trigger: copy, cut or paste while another application holds the Windows
  clipboard for the duration of the retries.
- Actual: the UI thread sleeps for 5 ms after each of ten failed open
  attempts, including the final failed attempt: at least 50 ms without
  processing window input, before reporting failure on copy or cut.
- Expected: retries should not block window input, and no sleep is needed
  after the last attempt.
- Evidence: direct call trace from the editor to the Win32 clipboard
  helpers. No native clipboard was opened or modified for this review.
- Suggested fix: schedule bounded retries without sleeping on the UI thread
  and preserve cut's rule that deletion occurs only after a successful
  clipboard write.
- Regression/performance check: simulate a clipboard held across several
  retries and through exhaustion; verify UI events remain serviceable,
  copy/cut failure notifications and existing paste behavior are preserved,
  and a failed cut does not delete text.

### Checked or not established

- A case-sensitive literal viewport scan returned 65,536 matches in
  0.87 ms locally; that isolated scan did not establish a performance
  finding.
- Constructing a progressive `TreeBuilder` snapshot with 1 GiB of
  synthetic shared blocks took 1.65 ms locally. This measures snapshot
  construction only, not file I/O or the complete load.
- Cloning and comparing two selection snapshots took a median 8.51 ms for
  1,000,000 carets locally; this isolated measurement did not establish
  an additional user-visible delay or an actionable comparative baseline.
- A suspected syntax-worker idle spin was not confirmed: pending spans
  and symbols are processed before waiting. The native clipboard was not
  exercised and no system clipboard contents were changed.

### Follow-up validation

The timings are local release-mode microprobes using the current worktree,
not fixed performance guarantees or end-to-end UI timings. The probes
checked their fixture sizes and results; they did not change production
files. Focused existing tests passed:

```powershell
cargo test --locked -p rte_edit -p rte_text -p rte_search -p rte_view --lib -- vertical_keeps_goal_column columns_follow_cell_rules tab_indent_and_outdent typing_overtype_and_newline enter_indents_after_an_opening_bracket delete_lines_keep_the_column_on_the_next_line copy_cut_paste batch_apply lookaround_and_backreferences backtracking_skips_invalid_lines_and_reports_complex_ones stop_ends_a_scan selection_fills_include_selected_line_breaks --test-threads=2
cargo test --locked -p rte_app --bin RoyiTextEditor -- fuzzy::tests clip::tests palette::tests::choices_filter_and_select palette::tests::all_commands_filter_by_fuzzy_score editor::tests::line_copy_and_paste_through_the_clipboard --test-threads=2
```

All Fusion findings are Pending operator triage. No fixes were made; wait
for the other reviewers.

## Triage

Operator decisions, 2026_10_30. Principles behind them: follow VS Code
behavior; no protection against misuse (users know their input, for
example a regex); single user, nothing for events closer than about 10 ms;
no startup or periodic checks for rare cases (check at the action).

| ID       | Decision    | Fix or reason                                                                |
| -------- | ----------- | ---------------------------------------------------------------------------- |
| CR001-C1 | Accepted    | Read-only check at save, orange dot; never change attributes. See below.     |
| CR001-C2 | Accepted    | Order distance-1 edits: transpositions, insertions, deletions, replacements. |
| CR001-C3 | Notice only | Status-line notice for CR-only line breaks; the bytes stay unchanged.        |
| CR001-C4 | Accepted    | Clamp the panel scroll when drawing.                                         |
| CR001-C5 | Accepted    | Count selection memory in the history budget.                                |
| CR001-C6 | Accepted    | Singular `1 line`.                                                           |
| CR001-S1 | Accepted    | Find the opener near the caret, not before the 64 KiB scan limit.            |
| CR001-S2 | Docs only   | Document: with look-around or back-references, write line breaks as `\n`.    |
| CR001-S3 | Accepted    | Draw spelling waves in the long-line tail.                                   |
| CR001-S4 | Accepted    | Follow VS Code: combining marks are word characters (no new dependency).     |
| CR001-S5 | Partial     | Seek to the intersecting children only; no inline cache or cancellation.     |
| CR001-S6 | Accepted    | One advancing column walk per row, shared with F1, F4, F5 and F6.            |
| CR001-F1 | Accepted    | As S6.                                                                       |
| CR001-F2 | No fix      | Final; see below.                                                            |
| CR001-F3 | No fix      | Extreme edge case.                                                           |
| CR001-F4 | Accepted    | As S6.                                                                       |
| CR001-F5 | Accepted    | As S6.                                                                       |
| CR001-F6 | Accepted    | As S6.                                                                       |
| CR001-F7 | No fix      | Extreme edge case.                                                           |
| CR001-F8 | Accepted    | Reuse the single owned piece instead of a second full copy.                  |
| CR001-F9 | Accepted    | No sleep after the last attempt, shorter retries; still synchronous.         |

### CR001-C1 Design

- The save thread checks the target's read-only attribute (one
  `GetFileAttributesW`) before writing anything. If set, the save returns
  a read-only outcome: no temporary file, nothing on disk changes.
- The editor shows `<name> is read-only. Clear its Read-only attribute and
  save again.` and draws the unsaved dot in `Theme::warning` (orange) until
  the next save attempt, which checks again. The text stays editable.
- The editor never changes file attributes or permissions.
- An access-denied or sharing failure of `ReplaceFileW` with the target
  verified unchanged deletes the temporary file and names the cause.
- No check at startup or load, no periodic check.

### CR001-F2 Decision (final)

No fix; do not raise it again. A look-around or back-reference regex that
is costly on a very long line keeps the search thread busy until the regex
library returns, and `Esc` cannot interrupt that single call (the
configured `BACKTRACK_LIMIT` does not count its retries at each start
position). VS Code has no protection either, and users are expected to know
their regex. Reopen only if the operator asks.

## Implementer

### 2026_10_30 - Claude Opus 5.5

- Role: Implementer
- Activity: Implementation
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Implemented every accepted finding as triaged; F2, F3 and F7
  unchanged. C1: `FileOps::read_only` (`GetFileAttributesW`) before any
  write, `SaveOutcome::ReadOnly`, orange dot (`Theme::warning`) until the
  next save attempt, and a temporary file deleted after any replace failure
  with the target intact. C2: distance-1 suggestions ranked swapped,
  missing, extra, then wrong letters; generation and distance-2 order
  unchanged (reordering the generation cost about 1 ms on P05). C3:
  `FileInfo::cr_breaks` from the existing line-ending sample, shown on the
  status line. C4: `panel_first` clamps every use. C5: history cost counts
  selections. C6: `plural`. S1/F6: `indent_before` reads only the
  indentation, `last_non_blank_before` the bytes before the caret. S3:
  waves past the seam use the selection column rule. S4: zero-width marks
  are word characters (`unicode-width`, already in the workspace). S5:
  `MarkdownMask::walk` seeks to the intersecting children. S6/F1:
  `ColumnWalk` for carets, selections, matches and waves, stopping past the
  right edge. F4: one forward pass over the selections. F5: `LineWalk` and
  `Vertical` (Up/Down, Ctrl+Alt+Up/Down, Tab). F8: `clip::encode` takes the
  clip and moves a single valid piece. F9: five attempts, no sleep after
  the last. S2: documentation only.

## Validation Results

- `cargo fmt --all`; `cargo clippy --workspace --all-targets -- -D warnings`:
  clean.
- Focused tests, all passing: `rte_edit` 38, `rte_view` 42, `rte_search` 31
  (1 ignored, existing), `rte_spell` 11, `rte_syntax` 33, `rte_app` 184
  (1 ignored, existing), `rte_platform files` 4, `xtask docs` 2. New tests:
  `read_only_files_are_refused_before_writing`, the access-denied case of
  `replace_failures_keep_every_version`, `read_only_attribute`,
  `read_only_file_save_is_refused_with_an_orange_dot`,
  `status_line_counts_lines_and_flags_cr_breaks`,
  `cr_only_line_breaks_are_flagged`, the panel clamp in
  `scroll_page_shortcuts_panel_and_the_empty_state`, `mispelled` in
  `builtin_suggests_close_words_in_the_case_of_the_word`,
  `history_budget_counts_selections`, the scan-limit cases of
  `enter_indents_after_an_opening_bracket`,
  `line_walks_agree_with_single_lookups`, the accent case of
  `whole_word_uses_word_characters`,
  `many_same_row_selections_and_carets_draw_at_their_columns`,
  `misspelled_words_past_the_long_line_seam_get_a_wave`,
  `markdown_mask_of_a_small_range_matches_the_whole_answer`, and the
  invalid single piece in `clip::tests::round_trip_and_rejection`.
- Release probes (temporary, removed): 10,000 same-row carets draw in
  0.024 ms and selections in 0.023 ms (S6/F1 measured 134-190 and 214 ms);
  Vertical for 10,000 carets 15 ms (241.7), Tab 1.7 ms (111.6), Enter
  no-op plus fallback 4.2 ms (17.6). Delete Lines is linear in blocks
  (2,000 / 4,000: 67 / 134 ms); its remaining cost is line lookups in
  leaves dense with line breaks (2-byte lines), not the selection search.
- `cargo xtask bench-perf --only E01,E02,E03,M01,M02,M03,M04,M05,M06,M08,K02,K09`
  and `--only S01,S02,P01,P02,P03,P04,P05,P06`: no regression against
  `Performance.md` 0.4.0 and the Spelling001 results (for example E01 0.43,
  M05 3.72, K02 1.15, P01 3.72 / 4.55, P05 5.1 ms). M05 peak private varied
  139.8-147.1 MiB across three runs (no same-session baseline).
- Not run: the full workspace test suite, the full performance suite, and
  the clipboard benchmarks C01-C06 and M07 (they replace the system
  clipboard); F8 was checked by the round-trip test only.

## Implementation Review - GPT-6.1 Sol

### 2026_10_30 - GPT-6.1 Sol

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: GPT-6.1 Sol
- Effort: High
- Context Window: 1M
- Outcome: Verified the accepted implementation against operator triage.
  Focused existing tests, differential probes and performance controls pass;
  one Low boundary defect remains, CR001-S7. No production fixes authored.
  S2 remains documentation-only, S5 remains child seeking only, and the
  final no-fix decisions for F2/F3/F7 are not reopened. Existing completion,
  triage and contributor records are preserved.

### CR001-S7 - CRLF split at the sample boundary produces a false CR-only notice

- Category: Edge case. Severity: Low. Confidence: High. Triage: Pending.
- Location: [`loader::read`](../crates/rte_app/src/loader.rs#L298), the
  `cr_breaks` calculation over the first `LINE_ENDING_SAMPLE` bytes.
- Trigger: load a valid CRLF file whose first line has 1,048,575 ASCII
  bytes, followed by `\r\nnext\r\n`. The 1 MiB sample ends at the CR;
  its paired LF is just outside the sample.
- Actual: `cr_breaks` is true, so the status line claims CR-only line
  breaks although the file is CRLF. Adjacent controls with 1,048,574 and
  1,048,576 prefix bytes correctly leave the flag false. Loaded bytes are
  unchanged in every case.
- Expected: a complete CRLF pair cut by sampling must not become evidence
  of a lone CR. This does not request full CR-only line-ending support.
- Suggested fix: account for the existing snapshot byte just beyond a
  sample ending in CR, or classify such partial evidence conservatively.
  Retain the approved sampling approach; no full-file scan, extra startup
  I/O or periodic check is needed.
- Evidence: `review_crlf_crossing_sample_end_is_not_cr_only` uses the real
  loader with disposable files, verifies every loaded byte, and fails only
  for the middle prefix. This is a deterministic input boundary, not a
  concurrent-operation or sub-10 ms race.
- Suggested regression: both sides of the sample boundary, a CRLF pair
  straddling it, and a genuine lone CR at end of file.

### Accepted-Fix Verification

- **C1:** read-only refusal occurs on the save worker before staging text;
  the target's attributes are never changed. Existing save and native
  attribute tests pass. The warning state is cleared on retry and document
  reset, and replace failures retain the intact-target cleanup path.
- **C2:** distance-1 checks now visit transpositions, insertions, deletions
  and replacements in the approved order without changing distance-2
  generation. The `mispelled` regression passes.
- **C3/C4/C5/C6:** CR-only notice, panel clamping, history selection cost
  and singular status wording have passing focused regressions. C3's
  sample-cut edge case is S7 above, not a request to change its triage.
- **S1/F6:** original long-line opener cases pass. New indentation and
  preceding-token helpers avoid using the first 64 KiB as caret context.
- **S3/S4:** the original visible-tail wave and combining-accent
  whole-word probes now pass.
- **S5:** verified only the approved byte seeking. A fixed six-byte mask
  request has release medians 0.0015 / 0.0015 / 0.0018 ms at
  1,000 / 10,000 / 50,000 paragraphs, versus about 0.079 / 0.809 / 10 ms
  in the original review. No inline cache or cancellation was required or
  treated as missing work.
- **S6/F1:** 10,000 same-row carets and selections build their display
  lists in release medians 0.0150 and 0.0118 ms, respectively. Their
  fixture setup is outside the measurement and builder buffers are reused.
  This reproduces the intended improvement over the original 134-190 ms
  caret path. These are local controls, not end-to-end frame percentiles.
- **F4/F5:** the forward selection walk and command-local line walkers
  replace repeated prefix work. Fresh-versus-cached comparisons pass for
  increasing/decreasing columns, tabs, combining marks, CJK and emoji;
  existing line-edit tests pass. No cross-command cache validity is assumed.
- **F8/F9:** the single owned valid UTF-8 piece becomes the clipboard
  string without a second copy, with invalid/multiple-piece round trips
  preserved. Clipboard retry code has five attempts and four sleeps as
  approved. No native clipboard mutation or clipboard benchmark was run.
- **S2/F2/F3/F7:** reviewed as explicit scope decisions, not implementation
  failures. In particular the old multiline lookaround probe is not an
  approval test for this documentation-only S2 decision.

### Implementation Review Validation

- Existing library tests: editing 38, view 42, search 31 (one pre-existing
  ignore), spelling 11 and syntax 33, all passing: 155 tests total.
- Application filters below: 29 passing executions, with overlapping
  save/read-only coverage. Native platform file tests: four passed.
- Six isolated implementation controls pass: accepted Enter behavior,
  cached/fresh line-walk comparisons, accent boundaries, visible tail
  waves, same-row drawing timing and Markdown seeking timing.
- One isolated real-loader boundary assertion fails for S7, exit 101.
  The loader harness provides only its data-only `Baseline` dependency;
  the actual loader/source and platform file calls are used. Fixtures are
  temporary files, with no user files, settings or clipboard access.
- Probes are under ignored `target/`, use exact Cargo JSON release
  artifacts, opt-level 3, fat LTO, one codegen unit, x86-64-v3 and
  self-contained GNU linking. Markdown controls install the production
  tree-sitter allocator before use.
- Focused clippy passes. Production source fingerprints and both task
  byte prefixes are checked after the append. Only review documents and
  ignored probes were edited by this reviewer.
- Not run: full workspace tests, full or native performance benchmarks,
  package rebuild, hosted CI, installed-app/settings inspection or system
  clipboard tests. No production fix, triage change or task move is made.

Commands:

```powershell
cargo test -p rte_edit -p rte_view -p rte_search -p rte_spell -p rte_syntax --locked
foreach ($filter in @('save', 'read_only', 'cr_only', 'scroll_page_shortcuts', 'clip::tests', 'status_line_counts')) {
    cargo test -p rte_app $filter --locked | Out-String
}
cargo test -p rte_platform files --locked
cargo clippy -p rte_edit -p rte_view -p rte_search -p rte_spell -p rte_syntax -p rte_app -p rte_platform --all-targets --locked -- -D warnings
./target/cr001-implementation-core.exe --nocapture --test-threads=1
./target/cr001-loader-review.ps1
```

## Implementation Review - GitHub CoPilot Fusion

### 2026_10_30 - GitHub CoPilot Fusion

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot SDK in VS Code
- Model: GitHub CoPilot Fusion
- Effort: High
- Context Window: NA
- Outcome: Independently reviewed the accepted implementation and ran
  focused tests and release probes. Most accepted behavior is preserved,
  but two multicursor hot paths retain user-visible latency cliffs (F10,
  F11). Confirmed that S7 remains present. No production changes or triage
  decisions made.

### CR001-F10 - Code-aware Enter repeats indentation work for every caret

- Category: Performance. Severity: Medium. Confidence: High. Triage: Pending.
- Location: [`indent_before` and `last_non_blank_before`](../crates/rte_edit/src/commands.rs#L145),
  called for each caret by [`newline_indent`](../crates/rte_edit/src/code.rs#L455),
  before the plain-Enter fallback.
- Trigger: on a code-language file, put many distinct carets on a long,
  all-space line and press Enter. The default multi-cursor cap allows
  10,000 carets; each caret below the 64 KiB scan limit has a different
  indentation prefix.
- Actual: `last_non_blank_before` scans back through the prefix in 256-byte
  chunks for every caret. `indent_before` also copies each complete leading
  whitespace prefix. `newline_indent` builds candidate edits for every
  caret before returning `None` when no opener applies, so this repeated
  work is discarded before plain Enter constructs its required insertions.
- Expected: determine the no-special-indent case without independently
  rescanning and allocating every prefix; share per-line indentation and
  prior-token information while preserving each caret's intended inserted
  indentation.
- Evidence: a release-mode probe used a 65,536-byte all-space line and
  evenly spaced carets. The code-aware no-op took 32.8 ms for 1,000,
  158.0 ms for 5,000, and 312.7 ms for 10,000; plain Enter construction
  then took 13.9, 70.8, and 152.1 ms. The latter creates the requested
  inserted indentation; the avoidable candidate scan/build occurs before
  it. The probe used a constant no-op bracket/colon rules callback; editor
  syntax lookup, edit application and painting were excluded. This extends
  F6: the earlier `"a     "` fixture has almost no leading indentation
  and does not exercise this repeated-prefix case.
- Suggested fix: share a forward scan per line (or defer candidate edit
  construction until a special indentation case is established), and avoid
  copying per-caret prefixes merely to find the previous nonblank byte.
- Regression/performance check: no-op and mixed opener/non-opener cases
  with thousands of carets on blank, deeply indented, and long code lines;
  verify the produced edit/selection mapping, including tabs and the scan
  boundary, and measure no-op scaling separately from unavoidable output
  construction.

### CR001-F11 - Delete Lines still spends tens of milliseconds on line lookups

- Category: Performance. Severity: Medium. Confidence: High. Triage: Pending.
- Location: [`line_blocks`](../crates/rte_edit/src/lines.rs#L19),
  [`delete_lines`](../crates/rte_edit/src/lines.rs#L164), and
  [`Snapshot::line_of`](../crates/rte_text/src/snapshot.rs#L118).
- Trigger: use many distinct matches/carets on alternating lines in a
  dense small file and invoke Ctrl+Shift+K.
- Actual: the accepted forward pass avoids restarting selection search,
  but block construction and command generation still repeatedly call
  `line_of` for each selection/block. `line_of` counts newlines from the
  beginning of the leaf to each offset, so dense short lines within a
  leaf repeatedly scan the same prefixes. The command remains noticeably
  slow despite the selection-search change.
- Expected: derive line numbers in an advancing byte/line walk, or reuse
  each selection's computed line while finding block heads and destinations.
- Evidence: release-mode measurements on `"a\nb\n".repeat(n)` with a
  caret on each `a` line: `delete_lines` took 17.0 ms for 1,000 blocks,
  67.0 ms for 2,000, and 134.0 ms for 4,000. `line_blocks` alone took
  0.06, 0.15, and 0.30 ms, respectively; the remaining cost is after
  block collection. Edit application and painting were excluded. This is
  a residual latency finding, not a claim that the accepted
  forward-selection fix was omitted.
- Suggested fix: carry the current byte offset and line number through the
  sorted selections/blocks and calculate destination columns without
  repeating prefix line counts.
- Regression/performance check: preserve primary-block, reversed-range,
  adjacent-block and final-line behavior; compare generated edits and
  carets with the current implementation, then benchmark 1,000–4,000
  alternating blocks in one and multiple leaves.

### Existing boundary finding

CR001-S7 remains reproducible: [`loader::read`](../crates/rte_app/src/loader.rs#L298)
classifies CR-only input solely from the first 1 MiB sample. A CRLF pair
whose CR is the sample's last byte and LF is the next byte is therefore
reported as CR-only. The sample-boundary fix has not been applied.

### Validation

- Passed: the focused `rte_edit`, `rte_view`, `rte_search`, `rte_syntax`,
  and `rte_spell` library suites (155 passed; one existing search test
  ignored), eight named application regressions, and four
  `rte_platform files` tests.
- Release probes used the current `rte_edit`/`rte_text` release libraries;
  measured `newline_indent` separately from plain Enter and `delete_lines`
  separately from `line_blocks`. No system clipboard or user files were
  touched.
- Not run: full workspace tests, full performance suite, package build,
  native clipboard tests, or the real-loader S7 probe. No fixes or triage
  changes made.

## Triage of the Implementation Reviews

Operator decisions, 2026_10_30.

| ID        | Decision | Fix or reason                                                                |
| --------- | -------- | ---------------------------------------------------------------------------- |
| CR001-S7  | Accepted | A CR ending the sample counts as lone only when the next byte is not LF.     |
| CR001-F10 | No fix   | Extreme case; most of the cost is the indentation plain Enter must insert.   |
| CR001-F11 | No fix   | Extreme case: thousands of blocks on 1-2 byte lines (dense leaf lookups).    |

## Implementer - Implementation Reviews

### 2026_10_30 - Claude Opus 5.5

- Role: Implementer
- Activity: Implementation
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Fixed CR001-S7 in `loader::read`: without an LF in the sample,
  the first CR is lone unless it is the sample's last byte and the next
  snapshot byte is LF (no extra I/O). F10 and F11 unchanged per triage.
  Removed the stale `Plan/CodeReview001.md` copy (its text is contained in
  this document).

### Validation Results

- `cargo test -p rte_app loader`: 15 passed. `cr_only_line_breaks_are_flagged`
  now covers a CRLF pair before, across and after the 1 MiB sample end and a
  lone CR at the end of a file.
- `cargo clippy -p rte_app --all-targets -- -D warnings` and
  `cargo fmt --all -- --check`: clean.

## Final Status

Fixed. Every accepted finding (C1-C6, S1, S3-S7, F1, F4-F6, F8, F9) is
implemented and validated; S2 is documented; F2, F3, F7, F10 and F11 are
final no-fix decisions.
