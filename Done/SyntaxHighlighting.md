# Syntax Highlighting and Code Awareness (M6)

## Task

Highlight code with built-in, open-source tree-sitter grammars and queries,
fast enough for every keystroke and without any cost at startup, and add the
code-aware commands of the
[Architecture](../Plan/Architecture.md#syntax-and-code-awareness-rte_syntax):
comments, brackets, auto-closing pairs, auto-indent, expand and shrink
selection, Go to Symbol and Change Language Mode.

## Scope

### Included

- `rte_syntax` crate: static language registry (id, name, extensions, file
  names, interpreters, aliases, comment tokens, brackets, auto-closing pairs)
  for Bash, C, C#, C++, CSS, Go, HTML, Java, JavaScript, JSON, Julia, Lua,
  Markdown, MATLAB, Octave, PowerShell, Python, Rust, SQL, TOML, TypeScript,
  TSX, XML and YAML. `.m` is MATLAB unless the content shows Octave markers;
  `.ipynb` is not detected.
- Trimmed `highlights.scm` and `tags.scm` per grammar under
  `crates/rte_syntax/queries/`, generated from the queries of the grammar
  crates (`cargo xtask syntax-queries`); written `tags.scm` where upstream
  has none. Sources and licenses in
  [queries/README.md](../crates/rte_syntax/queries/README.md).
- Dark theme colors for `keyword`, `string`, `comment`, `number`, `type`,
  `function`, `constant`, `operator`, `punctuation`, `property`, `tag`,
  `attribute`.
- Parsing on a syntax thread: incremental, chunked input from the piece tree,
  cancellable. The visible rows are highlighted on the UI thread within 1 ms;
  rows over budget are highlighted by the thread.
- Viewport-first parsing above 512 KiB; `syntax.maxFileMiB` (default 32).
- Injections (`syntax.injections`, default on): Markdown inline, fenced code
  by info string, YAML and TOML front matter, HTML blocks; HTML `<script>`
  (JavaScript, JSON, TypeScript by `type`) and `<style>`.
- Change Language Mode (`Ctrl+K M`), `--syntax <id>` with ids and aliases,
  `View: Toggle Syntax Highlighting`; the status line shows the language.
- Code awareness: Toggle Line Comment (`Ctrl+/`; block tokens per line where
  a language has no line comment), Toggle Block Comment (`Shift+Alt+A`),
  bracket pair mark and Go to Bracket (`Ctrl+Shift+\`), auto-closing pairs
  (surround, step over, pair delete), indentation after `Enter`, Expand and
  Shrink Selection (`Shift+Alt+Right` / `Left`), Go to Symbol in Editor
  (`Ctrl+Shift+O`).
- Benchmarks: L09 Fully highlighted, S01-S03 Jump to highlighted;
  `--exit-after-highlight`; a short-report row.

### Excluded

- Light theme, theme files, outside grammars or queries, folding, semantic
  tokens, word occurrence highlight, CSV (plain text).

### Compatibility Constraints

- First paint never waits for syntax work (user decision, option A).
- Plain text, untitled documents without a language and files above the
  limit start no syntax thread and record no edit journal.
- No new DLL imports; command and plug-in behavior unchanged otherwise.

## Design

- **Registry** (`registry.rs`): `&'static` table; detection by file name,
  extension, then `#!` interpreter; `by_id` (ids and aliases) and `by_fence`.
- **Highlighter** (`highlight.rs`): per grammar, compiled on first use on the
  syntax thread and shared (`OnceLock<Arc<_>>`); capture index → style;
  `@none` paints plain text over outer captures. Spans are flattened by
  painting captures by layer, then size (outer first), then pattern (first
  pattern last, so it wins).
- **Trimming** (`generate.rs`, feature `generate`): patterns split with
  `start_byte_for_pattern` / `end_byte_for_pattern`; kept when they have a
  themed capture and no general predicate; comments removed; ordered by
  specificity (named and anonymous nodes, predicates), then by upstream
  precedence (most grammars let the last pattern win; Go, JSON, CSS, HTML,
  XML, PowerShell and MATLAB let the first win). Rust numbers are renamed to
  `@number`.
- **Session** (`session.rs`, syntax thread): host tree; window parse with
  `set_included_ranges` (document positions) and a separate resumable full
  parser for large files; edits applied to every tree and to the pending
  full parse; injected layers parsed for the interest range, reused when
  untouched; `current_range` and `is_complete` describe how final the trees
  are.
- **UI** (`rte_app/src/syntax.rs`): sends `(before, after, ChangeSet)` steps
  (from the document journal, or `Snapshot::diff` for undo, redo and gaps);
  the thread computes exact `InputEdit`s. The UI view is edited with
  `input_edits_near`: exact edits near the visible rows and one point edit
  for all changes before them (cost independent of 10,000 carets). Received
  views are edited forward with the edits sent since. Spans are cached per
  (view generation, edit sequence, range).
- **View** (`rte_view`): `Highlights::syntax` spans colored per character via
  byte offsets from `layout_line`; `Highlights::brackets` filled cells.
- **Code awareness**: text edits in `rte_edit::code` return the command and
  the final selections (`Document::apply_selecting`); trees answer brackets
  (sibling tokens; strings and comments excluded), enclosing nodes and
  symbols (`aware.rs`), with a bounded text scan and word/line/document
  fallbacks.
- **Failure behavior:** a grammar or query that fails, or a stopped thread,
  shows `Syntax highlighting off: <reason>` and draws plain text.

## Alternatives

- **syntect / TextMate grammars:** slower regex-based highlighting; rejected
  in the Architecture.
- **Runtime trimming:** rejected; generated files are small, reviewable and
  checked by a test.
- **Grammar DLLs loaded on demand:** rejected (built-in grammars requested);
  the measured startup cost of the 31 MiB executable is within noise.
- **Dropping regex predicates:** rejected after review of the queries: the
  regex engine is linked by tree-sitter anyway and uppercase constructor and
  constant rules improve results.
- **Editing the UI tree with all edits:** rejected; 10,000-caret typing took
  56 ms per key from tree-sitter edit costs.

## Runtime Effects

- **Startup:** none for plain text; for a code file one detection and a
  thread start after the window exists. A/B with 0.3.0 (25 runs, first
  frame): `.txt` 26.5 → 27.1 ms, `.rs` 27.3 → 27.7 ms. Benchmark L01 first
  paint 21.4 ms (0.3.0: 21.3 ms).
- **Executable:** 4,153,856 → 32,086,016 bytes (grammar tables, paged in only
  when used). DLL imports unchanged.
- **Steady state:** idle syntax thread; per frame a 1 ms-bounded query over the
  visible rows (cached while nothing changes); per edit an incremental parse
  on the thread.
- **Memory:** syntax trees take about 50 bytes per file byte (L09, 8 MiB
  Python: 422 MiB peak once fully parsed); bounded by `syntax.maxFileMiB`.
- **Cancellation:** every message to the thread sets a flag checked by the
  parse and query progress callbacks; the full parse of a large file resumes
  after the interruption.

## Tests

- `rte_syntax`: queries generated and compiling (all grammars, tags),
  detection, a highlight case per language, injections (Markdown inline,
  fences, front matter, HTML blocks, HTML script/style, JSON scripts),
  incremental parse equal to a fresh parse, near edits equal to exact edits
  in their range, cancellation and resume, window then full parse, budget,
  flattening, brackets (tree, strings, text scan), enclosing nodes, symbols.
- `rte_text`: `Snapshot::diff` property test.
- `rte_edit`: line and block comments, auto-closing pairs, pair delete,
  indentation after `Enter`.
- `rte_view`: syntax colors per character.
- `rte_app`: highlighted file, edits, undo, toggle; plain text without
  syntax work and the size limit; Change Language Mode, auto-close, Enter,
  pair delete, comments, Go to Bracket, Expand/Shrink, Go to Symbol, bracket
  mark; palette choices; configuration keys; bench driver highlight timing.
- Benchmarks: focused subset of v1 on highlighted files (below).

## Implementation Steps

1. `rte_syntax`: grammars, registry, query generator and trimmed queries.
2. Parsing session, highlighting, injections, window and full parse.
3. View colors, the app syntax thread, status line, configuration.
4. Language mode, `--syntax`, toggle.
5. Code awareness in `rte_edit`, `rte_syntax` and the editor.
6. Benchmarks and harness metrics, documentation, CHANGELOG.

## Acceptance Criteria

- All listed languages highlight; injections work in Markdown and HTML. Met.
- L01 within noise of 0.3.0; executable size recorded. Met.
- Typing in an 8 MiB Python file below one frame (E01 0.40 ms). Met.
- Focused tests and clippy pass; focused benchmarks recorded. Met (the full
  benchmark is left for the release run).

## Reviewers

### 2026_10_24 - Claude Opus 5.5

- Role: Reviewer
- Activity: Design
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Design drafted from the Architecture. User decisions: full M6
  scope, first frame never waits (option A), all planned languages, dark
  theme only. Implementation approved.

## Implementer

### 2026_10_24 - Claude Opus 5.5

- Role: Implementer
- Activity: Implementation
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Implemented as designed with three deliberate changes: regex
  predicates kept, specificity ordering of patterns, and near-visible edits of
  the UI trees (found by M05). Focused tests, clippy, formatting, imports and
  focused benchmarks pass. Follow-ups: undo of multi-cursor edits diffs to one
  large change (the thread re-parses that range); word occurrence highlight
  and a light theme remain.

## Validation Results

| Check                                                                         | Result                                   |
| ----------------------------------------------------------------------------- | ---------------------------------------- |
| `cargo test -p rte_syntax`                                                    | 14 passed                                |
| `cargo test -p rte_app -p rte_edit -p rte_view -p rte_text -p xtask`          | All passed (114, 26, 28, 22, 9)          |
| `cargo clippy --workspace --all-targets -- -D warnings`                       | Clean                                    |
| `cargo fmt --all -- --check`                                                  | Clean                                    |
| `cargo xtask syntax-queries`                                                  | Files up to date (test)                  |
| `cargo xtask check-imports`                                                   | Same DLLs as 0.3.0                       |
| Startup A/B with 0.3.0, 25 runs                                               | Within 0.6 ms                            |
| Screenshots of Rust and Markdown samples (disposable probe)                   | Highlighted as expected                  |

Focused benchmark (`cargo xtask bench-perf --only` the load, edit,
multi-cursor, search and scroll tests on highlighted files; medians):

| ID  | Test                            | Measure                  | Result    |
| --- | ------------------------------- | ------------------------ | --------- |
| L01 | Empty file                      | First paint [ms]         | 21.4      |
| L04 | Python, 64 KiB                  | First paint [ms]         | 21.5      |
| L07 | Markdown, 64 KiB                | First paint [ms]         | 21.4      |
| L09 | Python, 8 MiB                   | First paint [ms]         | 22.2      |
| L09 | Python, 8 MiB                   | Fully highlighted [ms]   | 944.7     |
| L09 | Python, 8 MiB                   | Peak private [MiB]       | 422.8     |
| L12 | Markdown, 8 MiB                 | First paint [ms]         | 22.6      |
| E01 | Type, Python 8 MiB              | Key to frame [ms]        | 0.40      |
| C05 | Paste 1 MiB, Python 8 MiB       | Key to frame [ms]        | 2.07      |
| M05 | Type at 10,000 carets           | Key to frame [ms]        | 4.35      |
| M08 | Undo and redo, 10,000 carets    | Key to frame [ms]        | 1.14      |
| S01 | Scroll and jump, MD 64 KiB      | Jump to highlighted [ms] | 1.25      |
| S02 | Scroll and jump, MD 8 MiB       | Jump to highlighted [ms] | 4.89      |
| S02 | Scroll and jump, MD 8 MiB       | Missed refreshes [%]     | 0.0       |

Not run: the full test suite and the full benchmark (not requested); a
frozen-package smoke test.

## Implementation Review

### 2026_10_24 - GPT-6 Astra

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: GPT-6 Astra
- Effort: Extra High
- Context Window: 1M
- Outcome: Changes requested for M6A1-M6A10. Nine focused assertions fail;
  three scaling controls establish additional latency costs. Existing focused
  tests and clippy pass. No application fixes authored. The historical
  completion record and task location are preserved; this review does not
  approve the current performance and correctness guarantees.

### M6A1 [P1] Long-line viewport parsing repeats the entire parse

Locations: [parse_host](../crates/rte_syntax/src/session.rs#L232) and
[window_around](../crates/rte_syntax/src/session.rs#L428).
The window expands to whole lines, but the decision to create a separate full
parser uses file size before checking the resulting range. An 800,003-byte
single-line JSON array takes 226 ms to parse the entire document as a window,
then another 226 ms to parse it again from scratch. Both trees can coexist.
When the effective window covers the document, promote that tree to the full
host and discard the unnecessary full-parse job. Separately, whole-line
expansion is not a bounded viewport parse for minified files. Probe:
`review_profile_single_line_duplicate_parse` fails its completion assertion.

### M6A2 [P1] The UI highlight budget excludes expensive work

Locations: [captures](../crates/rte_syntax/src/highlight.rs#L166),
[flatten](../crates/rte_syntax/src/highlight.rs#L226), and
[visible_bytes](../crates/rte_app/src/editor.rs#L465).
The query progress callback does not bound Rust text-predicate evaluation,
capture sorting, per-byte painting or span conversion. A 1 KiB range over an
8 MiB Rust identifier takes 12.7 ms and requests an allocation near 16 MiB,
yet returns `complete=true` for a 1 ms deadline. Painting a 16 MiB JSON scalar
takes 5.5 ms with the same incorrect completion claim. The application also
requests 6,881,280 bytes for one long line in a 1100 x 2160 client although
the view draws at most 65,536 bytes of that line.
Pass actual drawn row ranges, not aggregate row-count capacity. Bound token
materialization and capture count; move oversized predicates to cancellable
worker work. Flatten intervals directly instead of allocating one paint byte
per input byte, and budget the complete operation. Probes:
`review_small_highlight_range_bounds_predicate_work`,
`review_highlight_deadline_includes_flattening`, and
`review_visible_range_excludes_undrawn_long_line_tail`.

### M6A3 [P1] Bracket marking performs an unbounded sibling walk per frame

Locations: [tree_partner](../crates/rte_syntax/src/aware.rs#L68) and the
[paint path](../crates/rte_app/src/editor.rs#L1958).
At an opening brace, every following sibling is visited to find its closing
token. Release medians for blocks containing 1,000, 10,000 and 50,000
statements are 0.19, 2.27 and 18.10 ms. This runs on the UI during every
paint, even when the caret and parsed tree are unchanged; the text fallback's
64 KiB cap does not bound this tree path. Cache by tree revision and caret,
use enclosing-node boundary/field lookups where valid, and bound unresolved
walks with deferred completion. Probe: `review_profile_bracket_siblings`.

### M6A4 [P1] Go to Symbol scans and allocates on the UI

Locations: [open_symbols](../crates/rte_app/src/editor/code.rs#L267) and
[symbols](../crates/rte_syntax/src/aware.rs#L152).
The command lazily compiles tags, queries the entire host tree, allocates all
names, sorts them, then builds another set of palette labels synchronously.
Even with tags already compiled, 1,000 / 10,000 / 50,000 Python definitions
take 0.76 / 8.25 / 60.64 ms for collection alone. Use a revision-tagged worker
index, reuse unchanged definitions, and bound/paginate palette materialization.
Report pending or partial coverage when only a window tree exists. Probe:
`review_profile_symbols`; these timings exclude palette work and first-use
query compilation, so they are lower bounds on command latency.

### M6A5 [P1] Edits bypass the syntax file-size limit

Locations: [restart_syntax](../crates/rte_app/src/editor.rs#L413) and
[sync_syntax](../crates/rte_app/src/editor.rs#L440).
The size guard runs only when syntax starts. A document initially below a
1 MiB limit keeps its syntax thread and journal after inserting 2 MiB. This
allows later growth to bypass the resource safeguard entirely. Recheck size
before journaling/submitting edits, cancel and retire syntax when over budget,
and define re-enabling after shrinking. Also use an explicit syntax-memory
policy: the fresh 8 MiB Python benchmark peaks at 422.8 MiB private memory,
so an input-byte limit alone is a coarse safeguard. Probe:
`review_syntax_limit_is_rechecked_after_growth`.

### M6A6 [P2] Injection discovery scales with invisible siblings

Location: [regions](../crates/rte_syntax/src/session.rs#L533).
Every visited parent first collects all children into a vector; out-of-range
siblings are rejected only when popped. Moving a fixed 80-byte interest range
in HTML with 1,000 / 10,000 / 50,000 top-level elements costs 0.032 / 0.490 /
2.426 ms. The last case requests a 1.6 MiB Rust allocation although there are
no script/style injections. Seek to the first intersecting child with a tree
cursor, stop at the interest end, cache region boundaries and poll cancellation
during discovery. Probe: `review_profile_injection_discovery`.

### M6A7 [P2] Context-free window colors are reported as final

Location: [current_range](../crates/rte_syntax/src/session.rs#L126).
A C viewport starting inside a long block comment is parsed without its
opening delimiter. The first window highlights `int`, operators and numbers
as code and reports the range as current/final; the full parse correctly
colors the same bytes as one comment. This also weakens highlighted-frame
measurements and tree-based commands. Treat unverified windows as provisional,
or restart at a verified lexical/parser boundary; do not use them as complete
code-awareness authority. Probe: `review_window_comment_colors_are_not_final`.

### M6A8 [P2] Code awareness ignores injected trees

Locations: [matching_bracket](../crates/rte_syntax/src/aware.rs#L24) and
[language-aware commands](../crates/rte_app/src/editor/code.rs#L101).
Bracket matching asks only `View::tree()` (the host). In
`<script>function f() { return 1; }</script>`, the brace is inside the host's
`raw_text` node and returns no partner despite a parsed JavaScript layer.
Comment and indentation dispatch likewise use the host language. Resolve the
innermost applicable layer/language at each selection, while preserving string
and comment exclusions and handling selections crossing layer boundaries.
Probe: `review_brackets_use_injected_javascript_tree`; add equivalent Markdown
fence and mixed-language multi-cursor command coverage.

### M6A9 [P2] Block-comment wrapping copies all selected text

Location: [toggle_block_comment](../crates/rte_edit/src/code.rs#L166).
The command copies the selection to inspect its whitespace and delimiters.
Wrapping 16 MiB of non-whitespace text requests 16 MiB and takes about 4.1 ms
before returning just two small insertions. Cost grows with the document even
when the answer is available at its edges. Inspect borrowed forward/backward
chunks and delimiter-sized slices; bound or defer pathological whitespace
scans. Probe: `review_block_comment_does_not_copy_selection`.

### M6A10 [P2] Enter misreads the suffix of a multiline selection

Location: [newline_indent](../crates/rte_edit/src/code.rs#L373).
After selecting the contents of `{\n  foo\n}`, Enter produces
`{\n    }` instead of `{\n    \n}`. The suffix lookup uses the start line's
content end, which precedes the selection end, so the closing brace is not
seen. Read the bounded suffix from the line containing `sel.end()` and retain
the opening line's indentation. Probe:
`review_enter_replacing_multiline_body_splits_pair`.

### Optimization Order

1. Remove the duplicate full parse (M6A1) and enforce the size/memory guard
   (M6A5). These avoid unnecessary parser CPU and large simultaneous trees.
2. Remove UI stalls: cache/bound bracket lookup, move symbols off-thread,
   and make the highlight deadline cover predicates and flattening (M6A2-A4).
3. Prune injection traversal before allocation (M6A6). Keep worker span
   requests coalesced and cancellable; the current fallback paints with no
   deadline. Tag cached worker spans with tree generation as well as edit
   sequence/range before preferring them to repeated UI queries.
4. Invalidate span caches by changed ranges instead of every new view; retain
   bounded per-row spans and reusable scratch storage. Measure this separately
   from parser time; correctness needs lexical changes to invalidate dependents.
5. Measure an explicit native syntax-memory budget and consider demand-driven
   full parsing after establishing context-correct windows. Do not assume that
   replacing the rasterizer will remove these measured CPU/allocation costs.

### Review Validation

- Existing focused tests: 182 passed (syntax 14, edit 26, app 114, view 28);
  one existing search profiling test ignored. Focused clippy is clean.
- Isolated review probes: nine failing assertions and three passing scaling
  controls. Exit 101 records reproduced findings, not passing regression tests.
- Native focused benchmark: L09 first paint 22.7 ms, first frame 38.0 ms,
  fully highlighted 933.8 ms, peak private 422.8 MiB. E01 median/p95
  0.45/7.75 ms; M05 4.34/4.67 ms. S02 p95 0.73 ms, max 0.86 ms,
  jump-to-highlight 2.70 ms, reported missed refreshes 0%.
- E01/M05 start once the viewport is highlighted, not once the full parse
  finishes. Their main timing ends at the first rendered response, which can
  use edited old trees. The harness discards highlight samples overtaken by
  the next step and closes after the last raw frame. Add warm full-tree runs,
  key-to-current-highlight percentiles, missing-result counts and parser
  backlog; the existing fast medians do not establish syntax-update latency.
- Probes are ignored files under `target/`, compiled against exact release
  artifacts with opt-level 3, fat LTO, one codegen unit and x86-64-v3. Parsing
  and fixture setup are outside individual query/command timings. Timings are
  local samples, not comparative-editor or reference-hardware certification.
- The allocation probe tracks individual Rust allocation requests, not peak
  heap, native tree-sitter allocations or process memory. Only the native
  benchmark's 422.8 MiB value is peak private memory.
- The headless console harness uses current application source, changing only
  module/include paths, the Windows subsystem attribute and visibility of the
  existing `editor_on` test helper. It uses fake file operations; no user
  settings or system clipboard were accessed. Application source and prior
  task bytes are fingerprinted and checked before finishing the append.

Commands run:

```powershell
cargo build -p rte_app --release --locked --message-format=json
cargo test -p rte_syntax -p rte_edit --locked
cargo test -p rte_view --locked
./target/m6-astra-app.exe --skip astra_m6_review --test-threads=1
./target/m6-astra-app.exe astra_m6_review --nocapture --test-threads=1
./target/m6-astra-review.exe review_ --nocapture --test-threads=1
cargo clippy -p rte_syntax -p rte_app -p rte_edit -p rte_view --all-targets --locked -- -D warnings
cargo xtask bench-perf --only L09,E01,M05,S02
```

Documentation validation: PowerShell byte-prefix, source/query fingerprint,
finding-ID and local-link checks, plus a no-index Git whitespace check on this
untracked task. No full test/performance suite, startup A/B, package smoke,
native manual IME/DPI checks or 1 GiB syntax stress run was performed. No
implementation changes or changelog edits were made during this review.

## Review Fixes

### 2026_10_24 - Claude Opus 5.5

- Role: Implementer
- Activity: Implementation
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: M6A1-M6A10 addressed with regression tests; focused tests, clippy
  and the focused benchmark pass. Deferred from the Optimization Order: span
  cache invalidation by changed ranges and a native syntax-memory budget.

| ID    | Fix                                                                                                        | Regression test                                                                                                                               |
| ----- | ---------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| M6A1  | Bounded windows; a window covering half the file or more is one full parse.                                | `windows_are_bounded_and_whole_file_windows_are_full_parses`                                                                                  |
| M6A2  | Predicates clipped to 1 KiB, stop checks in captures, interval flattening, drawn row ranges only.          | `highlight_cost_does_not_grow_with_token_length`, `ranges_skip_undrawn_bytes`, `highlighting_reads_only_the_drawn_part_of_long_lines`         |
| M6A3  | Partner from the parent's first or last child, then at most 256 siblings; cached by text, caret and trees. | `brackets_match_by_tree_and_by_text` (50,000-statement block)                                                                                 |
| M6A4  | Symbols collected on the syntax thread (limit 20,000, cancellable); partial results noted.                 | `change_language_mode_and_code_commands`                                                                                                      |
| M6A5  | Size limit checked on every sync: growth stops highlighting, shrinking restarts it.                        | `the_syntax_size_limit_follows_edits`                                                                                                         |
| M6A6  | Injection discovery seeks the interest range with a tree cursor and polls cancellation.                    | `injection_discovery_seeks_the_interest_range`                                                                                                |
| M6A7  | A window not at the file start is provisional for highlighted frames and code awareness.                   | `windows_after_the_file_start_are_provisional`                                                                                                |
| M6A8  | Brackets use the innermost layer; comments, pairs and indentation use the language at each position.       | `brackets_match_by_tree_and_by_text` (HTML script), `code_commands_follow_the_embedded_language`, `comments_follow_the_language_at_each_line` |
| M6A9  | Block comments read only the selection edges (bounded whitespace scans).                                   | `block_comment_edges_of_a_large_selection`                                                                                                    |
| M6A10 | The suffix is read from the line containing the selection end.                                             | `enter_indents_after_an_opening_bracket` (multiline body)                                                                                     |

Worker span requests are coalesced (the latest replaces an unanswered one)
and painted with cancellation.

### Validation Results

| Check                                                                              | Result                                         |
| ---------------------------------------------------------------------------------- | ---------------------------------------------- |
| `cargo test -p rte_app -p rte_syntax -p rte_edit -p rte_view -p rte_text -p xtask` | All passed (117, 19, 28, 28, 22, 9); 1 ignored |
| `cargo clippy --workspace --all-targets -- -D warnings`                            | Clean                                          |
| `cargo fmt --all`                                                                  | Applied                                        |

Focused benchmark (`cargo xtask bench-perf --only
L01,L04,L07,L09,L10,L11,L12,L17,E01,C04,C05,M01-M08,S01,S02,S03`; medians):

| ID  | Test                          | Measure                  | Result |
| --- | ----------------------------- | ------------------------ | ------ |
| L01 | Empty file                    | First paint [ms]         | 21.2   |
| L09 | Python, 8 MiB                 | First paint [ms]         | 22.5   |
| L09 | Python, 8 MiB                 | Fully highlighted [ms]   | 935.5  |
| L09 | Python, 8 MiB                 | Peak private [MiB]       | 422.7  |
| L17 | Single line JSON, 8 MiB       | First paint [ms]         | 23.1   |
| E01 | Type, Python 8 MiB            | Key to frame [ms]        | 0.47   |
| C05 | Paste 1 MiB, Python 8 MiB     | Key to frame [ms]        | 2.47   |
| M03 | Column selection, 1,000 lines | Key to frame [ms]        | 1.28   |
| M05 | Type at 10,000 carets         | Key to frame [ms]        | 4.14   |
| M08 | Undo and redo, 10,000 carets  | Key to frame [ms]        | 1.31   |
| S01 | Scroll and jump, MD 64 KiB    | Jump to highlighted [ms] | 1.63   |
| S02 | Scroll and jump, MD 8 MiB     | Jump to highlighted [ms] | 4.48   |
| S02 | Scroll and jump, MD 8 MiB     | Missed refreshes [%]     | 0.0    |

All caps passed. One earlier M03 run took 2.15 s (cap 1 s) while background
builds ran; direct and xtask reruns took 0.1-0.3 s. The review's harness
additions (full-tree runs, key-to-current-highlight percentiles, missing
counts) were not added. Not run: the full test suite and full benchmark (not
requested), the review's probe executables (built against the old APIs).

## Performance Follow-up

### 2026_10_24 - Claude Opus 5.5

- Role: Implementer
- Activity: Implementation
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Target confirmed with the user: everything instant for code files
  up to about 4 MiB (typically 10-10,000 lines); large JSON and minified
  files must not fail; no added architecture. Added 50,000-line benchmarks
  and highlight metrics; fixed a stuck highlight; Go to Symbol reuses the
  list of unchanged text. A per-row span cache was dropped: the UI stays
  within a few milliseconds without it.

Changes:

- Fix: after the full parse replaced the window of a large file, rows over
  the UI budget were never highlighted again (the repeated worker request
  was suppressed); single-line JSON never reached `fully_highlighted`.
  Regression test `long_rows_are_fully_highlighted_after_the_window`.
- Go to Symbol shows the list of unchanged text at once (complete parses
  only).
- Benchmark: `--bench-full`; fixtures `code.py` and `code.c` (50,000 lines);
  tests L20-L22 and K01-K05; paced tests report highlighted p95 and keys not
  highlighted, and wait for the last key's highlighted frame.

Measured (release, medians and p95 of one run):

| Case                                 | Result                                          |
| ------------------------------------ | ----------------------------------------------- |
| Open 50,000 lines (Python, C)        | First paint 23 ms; full highlight 226 / 370 ms  |
| Typing, key to frame (K01, K03)      | p95 5.2 / 2.4 ms                                |
| Typing, key to highlighted (K01)     | p95 6.4 ms                                      |
| Scroll and jump (K04)                | p95 2.6 ms, no missed refresh                   |
| Single-line JSON, 8 MiB (L22)        | Full highlight 830 ms, peak 439 MiB             |
| Go to Symbol, 50,000 lines (K05)     | 86 ms (collection on the syntax thread: 77 ms)  |

Tree-sitter's incremental reparse sets the highlight delay after an edit
(plain tree-sitter, same numbers outside the editor):

| File   | 2,000 lines | 10,000 lines | 50,000 lines |
| ------ | ----------- | ------------ | ------------ |
| Python | 0.1-3 ms    | 0.3-15 ms    | 1.5-92 ms    |
| C      | 1.2 ms      | 6-9 ms       | 41-51 ms     |

Python is fast except edits that reshape the tree (an unclosed quote, some
`Enter`/`Backspace` pairs); C costs about 1 µs per top-level item on every
edit. Symbol collection walks the whole tree (about 70 ns per node). Both
are within one frame up to 10,000 lines.

### Validation Results

| Check                                                               | Result                             |
| ------------------------------------------------------------------- | ---------------------------------- |
| `cargo test -p rte_app -p rte_syntax -p xtask`                      | All passed (119, 19, 9); 1 ignored |
| `cargo clippy --workspace --all-targets -- -D warnings`             | Clean                              |
| `cargo fmt --all`                                                   | Applied                            |
| `cargo xtask bench-perf --only L20,L21,L22,K01,K02,K03,K04,K05,S02` | All caps passed                    |

Not run: the full test suite and full benchmark (not requested).

## Typical File Follow-up

### 2026_10_24 - Claude Opus 5.5

- Role: Implementer
- Activity: Implementation
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Added 10,000-line benchmarks (Python, C, TypeScript) and a first
  highlighted frame metric. Files of 10,000 lines showed 2-4 frames of plain
  text before their first highlight; the first window is now 32 KiB above
  128 KiB (was 128 KiB above 512 KiB), so the second frame is highlighted.

Load, 10 processes each (medians, ms from process start):

| Test                     | First frame | Highlighted, before | Highlighted, after | Fully highlighted |
| ------------------------ | ----------- | ------------------- | ------------------ | ----------------- |
| Python, 10,000 lines     | 32.9        | 61.1                | 34.5               | 55.4              |
| C, 10,000 lines          | 32.7        | 88.8                | 35.5               | 82.5              |
| TypeScript, 10,000 lines | 28.1        | 48.5                | 29.1               | 51.4              |

Editing a fully highlighted 10,000-line file (paced, p95 in ms):

| Test                          | Key to frame | Key to highlighted |
| ----------------------------- | ------------ | ------------------ |
| Type, Python (K06)            | 4.8          | 6.3                |
| Type, C (K07)                 | 4.5          | 10.1               |
| Type, TypeScript (K08)        | 4.9          | 6.2                |
| Enter, Backspace, TS (K09)    | 5.3          | 6.3                |
| Go to Symbol, Python (K10)    | 18.1 median  | -                  |

| Check                                                                             | Result                             |
| --------------------------------------------------------------------------------- | ---------------------------------- |
| `cargo test -p rte_app -p rte_syntax -p xtask`                                    | All passed (119, 19, 9); 1 ignored |
| `cargo clippy --workspace --all-targets -- -D warnings`                           | Clean                              |
| `bench-perf --only` L04, L09, L20-L25, K01, K03-K10, S02, E01                     | All caps passed                    |
| `bench-perf --only` L01, L07, L10-L12, L17, L18, C04, C05, M01-M08, F03, S01, S03 | All caps passed                    |

## Below 50 ms for 10K LoC

### 2026_10_24 - Claude Opus 5.5

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Analysis only, no application change. Goal: a 10,000-line file of
  any language is fully highlighted and ready for work within 50 ms of
  process start. Reachable for 19 of 24 languages with three small changes
  (option A); all 24 need a parallel parse (option B).

#### Where the Time Goes

Measured timeline (release, ms from process start; temporary trace marks):

| Stage                         | `10k.py` (337 KiB) | `10k.c` (454 KiB) | `10k.ts` (277 KiB) |
| ----------------------------- | ------------------ | ----------------- | ------------------ |
| `main` entered                | 6.1                | 6.2               | 6.2                |
| File read (loader thread)     | 6.6                | 7.0               | 6.8                |
| Syntax thread starts          | 10.0               | 10.2              | 10.6               |
| Highlight query compiled      | 13.6               | 12.6              | 22.4               |
| View window parsed            | 17.4               | 17.4              | 25.4               |
| Whole file parsed             | 55.0               | 84.2              | 53.2               |
| Fully highlighted frame       | 56.7               | 85.5              | 54.9               |

The whole-file parse dominates. Before it come process start (6 ms), the
wait for the window and editor (4 ms), the query (2-41 ms by language) and
the view window (4 ms); they run one after the other on one thread.

#### Per Language

Synthetic 10,000-line files (a typical block per language repeated), release
build, ms. Query: cold compile of the highlight query. Parse: a fresh full
parse, the file split at block boundaries into 1, 4 or 8 parts parsed on
that many threads (32 hardware threads). Now: projected with today's
pipeline, `10.5 + query + 4 + parse + 1.5` (matches the three measured files
within 3 ms). A: start parsing when the file is read, compile the query on
another thread, run the window and the full parse at once:
`7 + max(query, parse) + 1.5`. B: A with a 4-thread parse. Bold: 50 ms or
more.

| Language   | KiB | Query | Parse, 1 thread | Parse, 4 threads | Parse, 8 threads | Now     | A       | B  |
| ---------- | --- | ----- | --------------- | ---------------- | ---------------- | ------- | ------- | -- |
| Bash       | 184 | 3.6   | 26.8            | 7.8              | 5.3              | 46      | 35      | 16 |
| C          | 267 | 2.3   | 37.5            | 10.9             | 6.7              | **56**  | 46      | 19 |
| C++        | 194 | 16.4  | 32.0            | 9.0              | 5.4              | **64**  | 40      | 25 |
| C#         | 290 | 20.4  | 60.1            | 17.6             | 11.0             | **96**  | **69**  | 29 |
| CSS        | 264 | 0.3   | 29.1            | 8.3              | 5.2              | 45      | 38      | 17 |
| Go         | 169 | 1.1   | 15.8            | 5.0              | 3.4              | 33      | 24      | 14 |
| HTML       | 277 | 0.1   | 23.5            | 6.9              | 4.6              | 40      | 32      | 15 |
| Java       | 208 | 3.2   | 13.2            | 4.2              | 3.0              | 32      | 22      | 13 |
| JavaScript | 289 | 5.3   | 28.5            | 9.1              | 6.4              | **50**  | 37      | 18 |
| JSON       | 754 | 0.1   | 82.4            | 21.9             | 13.9             | **98**  | **91**  | 30 |
| Julia      | 218 | 40.8  | 51.5            | 13.3             | 8.8              | **108** | **60**  | 49 |
| Lua        | 211 | 1.0   | 18.1            | 4.9              | 3.5              | 35      | 27      | 13 |
| Markdown   | 128 | 0.7   | 22.2            | 5.9              | 3.7              | 39      | 31      | 14 |
| MATLAB     | 178 | 26.3  | 25.1            | 7.0              | 5.0              | **67**  | 35      | 35 |
| Octave     | 188 | 26.3  | 19.4            | 5.4              | 3.6              | **62**  | 35      | 35 |
| PowerShell | 326 | 1.8   | 139.9           | 35.5             | 17.7             | **158** | **148** | 44 |
| Python     | 244 | 3.4   | 20.3            | 5.8              | 3.3              | 40      | 29      | 14 |
| Rust       | 181 | 9.9   | 14.7            | 4.2              | 2.5              | 41      | 23      | 18 |
| SQL        | 286 | 9.8   | 30.9            | 8.2              | 4.2              | **57**  | 39      | 18 |
| TOML       | 154 | 0.1   | 9.4             | 2.9              | 1.9              | 26      | 18      | 11 |
| TSX        | 272 | 12.2  | 21.6            | 6.1              | 3.5              | **50**  | 30      | 21 |
| TypeScript | 266 | 10.9  | 20.5            | 6.3              | 3.2              | 47      | 29      | 19 |
| XML        | 250 | 0.3   | 24.7            | 7.7              | 4.0              | 41      | 33      | 16 |
| YAML       | 134 | 2.8   | 10.4            | 2.9              | 1.7              | 29      | 19      | 11 |

Parse time follows bytes more than lines (about 3-12 MB/s; PowerShell 2.3
MB/s, C# 4.8 MB/s). The JSON sample has long lines (754 KiB); 10,000 lines
of one value per line are about 250 KiB, about 27 ms. The `10k.c` fixture is
dense (454 KiB, 67 ms parse): A gives 76 ms for it, B about 26 ms. Parsing
scales almost linearly with threads (4 threads: 3.4-4.0 times faster).
First-screen highlighting costs 0.1-0.9 ms and is not a factor.

#### Ideas

1. Start the syntax session when the loader has the text (about 7 ms),
   not after the window and editor are created (about 10.5 ms). Saves 3-4
   ms. Small change in startup wiring.
2. Compile the highlight query on its own thread while the file parses; it
   can start from the file name before the text is read. Hides 2-41 ms
   (Julia, MATLAB, C#, C++, TSX, TypeScript, Rust, SQL). Small change.
3. Run the view-window parse and the full parse concurrently (two parsers on
   two threads) instead of one after the other. Saves about 4 ms on files
   above 128 KiB; the second frame stays highlighted.
4. Parallel segment parse (option B): split the file at top-level lines
   (column 0 after a blank line) into about 4 segments, parse them on 4
   threads with `set_included_ranges`, and accept a segment only when its
   tree has no error at its edges; otherwise merge it with its neighbour.
   This generalises today's single view window to several windows, and the
   single full tree still replaces them when it is done (in the background).
   Gets every language under 50 ms. Costs: more threads at load, memory for
   the segments until the full tree arrives, and a split heuristic; files
   with one big indented block (a C# or Java class) find few split points and
   fall back to A. Needs a task document before implementation.
5. A faster allocator for tree-sitter (`tree_sitter::set_allocator`): an
   upper bound with a bump allocator made parses 0-28% faster (JSON 82 to 59
   ms, C# 60 to 47 ms, PowerShell 140 to 129 ms, most others 5-10%). A real
   allocator (mimalloc) would give part of that and adds a dependency.
6. Trim the slowest queries (Julia 41 ms, MATLAB 26 ms): only matters with A
   when the query is longer than the parse (Julia under B is at 49 ms).

Rejected:

- Building the grammars with `-march=x86-64-v3`: the C code crashed
  (`0xC0000005`) under MinGW GCC (AVX stack alignment on Windows).
- Precompiled queries: tree-sitter has no query serialization.
- Caching spans on disk: does not help a first open and adds I/O.

#### Recommendation

Do ideas 1-3 first (simple, no new architecture): 19 of 24 languages under
50 ms, typical 10,000-line code files at 20-46 ms. C#, Julia, PowerShell,
long-line JSON and dense C need idea 4; decide on it after 1-3 are measured
with the L23-L25 benchmarks.

Data sources: a temporary probe (cold query compile per process, parse
medians of 3 runs, threaded parses) and temporary trace output in the
editor; both were removed. Synthetic files are not real code; timings are
local samples on the reference machine.

#### Allocator Test

Idea 5 measured in the editor with `mimalloc` 0.1.52 (`libmimalloc-sys`
0.1.49). Variants: the system heap (today), mimalloc for everything (Rust
`#[global_allocator]` and tree-sitter `set_allocator`), and mimalloc for
tree-sitter only. Medians of the focused benchmarks; system heap and
"all" are the mean of two runs, "tree-sitter" one run. Bold: worse.

| Test                         | Measure                     | System heap | mimalloc, all | mimalloc, tree-sitter |
| ---------------------------- | --------------------------- | ----------- | ------------- | --------------------- |
| L01 Empty file               | First paint [ms]            | 21.7        | 21.9          | 21.9                  |
| L01 Empty file               | Peak private [MiB]          | 5.1         | **9.4**       | 5.1                   |
| L09 Python, 8 MiB            | Fully highlighted [ms]      | 911.6       | 789.7         | 803.0                 |
| L09 Python, 8 MiB            | Peak private [MiB]          | 417.5       | 407.9         | 390.6                 |
| L20 Python, 50,000 lines     | Fully highlighted [ms]      | 209.0       | 176.9         | 185.3                 |
| L21 C, 50,000 lines          | Fully highlighted [ms]      | 344.2       | 318.6         | 316.2                 |
| L22 1-line JSON, 8 MiB       | Fully highlighted [ms]      | 834.5       | 701.1         | 700.5                 |
| L22 1-line JSON, 8 MiB       | Peak private [MiB]          | 439.3       | 453.0         | 413.9                 |
| L23 Python, 10,000 lines     | Fully highlighted [ms]      | 56.7        | 50.7          | 51.1                  |
| L23 Python, 10,000 lines     | Peak private [MiB]          | 24.1        | **45.4**      | 24.8                  |
| L24 C, 10,000 lines          | Fully highlighted [ms]      | 84.4        | 79.9          | 79.4                  |
| L24 C, 10,000 lines          | Peak private [MiB]          | 21.8        | **39.8**      | 22.5                  |
| L25 TypeScript, 10,000 lines | Fully highlighted [ms]      | 53.0        | 48.3          | 47.8                  |
| L25 TypeScript, 10,000 lines | Peak private [MiB]          | 19.1        | **40.9**      | 19.6                  |
| K03 Type, C 50k lines        | Key to highlighted p95 [ms] | 48.0        | 42.1          | 40.3                  |
| K07 Type, C 10k lines        | Key to highlighted p95 [ms] | 10.0        | 8.7           | 8.5                   |
| K05 Go to Symbol, 50k lines  | Key to frame [ms]           | 85.6        | 64.0          | 66.2                  |
| K10 Go to Symbol, 10k lines  | Key to frame [ms]           | 18.1        | 13.9          | 14.4                  |
| C05 Paste 1 MiB              | Key to frame [ms]           | 2.35        | 3.01          | 2.33                  |
| C05 Paste 1 MiB              | Peak private [MiB]          | 38.0        | **65.7**      | 35.9                  |
| M05 Type at 10,000 carets    | Key to frame [ms]           | 4.00        | 4.39          | 4.47                  |
| M05 Type at 10,000 carets    | Peak private [MiB]          | 119.9       | **168.4**     | 118.9                 |
| M06 Backspace, 10,000 carets | Key to frame [ms]           | 4.32        | 4.86          | 4.73                  |
| M07 Paste 10,000 pieces      | Key to frame [ms]           | 5.41        | 5.25          | 5.21                  |
| M07 Paste 10,000 pieces      | Peak private [MiB]          | 69.1        | **127.2**     | 76.2                  |
| M08 Undo and redo, 10,000    | Key to frame [ms]           | 0.72        | 0.70          | 0.73                  |
| M08 Undo and redo, 10,000    | Peak private [MiB]          | 45.1        | **102.0**     | 48.7                  |

Findings:

- The gain is tree-sitter's: full highlights 6-16% faster, edits re-parse
  about 13-16% faster, Go to Symbol 20-25% faster. Rust's allocations gain
  nothing measurable (typing, scrolling, multi-cursor edits are within noise
  or slightly slower).
- mimalloc for Rust as well roughly doubles peak private memory for small
  files and edits (empty file 5.1 to 9.4 MiB, 10,000-line files about 20 to
  40-45 MiB, M08 45 to 102 MiB). The options `MIMALLOC_ARENA_EAGER_COMMIT=0`
  and `MIMALLOC_PURGE_DELAY=0` did not change it.
- mimalloc for tree-sitter only keeps memory at today's level (lower for the
  8 MiB files) and startup unchanged.
- Executable: 32,116,224 to 32,348,672 bytes (+227 KiB, "all" variant).
- For the 50 ms goal the gain is too small to move any language under it
  on its own (10,000 lines of Python 56.7 to 51.1 ms, TypeScript 53.0 to
  47.8 ms, C 84.4 to 79.4 ms); with ideas 1-3 it adds about 5-15% margin.

If adopted: mimalloc for tree-sitter only, one `set_allocator` call at
startup (one `unsafe` exception to the workspace lint), `libmimalloc-sys`
without default features plus `extended` (for `mi_calloc`). The experiment
was reverted; no application change remains.

#### Ideas 1-3 Test

Implemented without the allocator change (adopted with it, see below):

1. The loader calls a hook with the complete text just before its `Done`
   event; the hook detects the language like the editor and starts the
   syntax thread. The editor adopts it when language and text match
   (`Settings::early_syntax`), otherwise starts its own. The configuration is
   now read before the loader starts.
2. `Session::new` no longer compiles the highlight query: `warm` compiles it
   on a short-lived thread (also started from the file name at `main`), and
   the first view waits for it. A query error still turns highlighting off.
3. The whole-file parse of a file above 128 KiB runs on its own thread from
   the start, concurrently with the view window; `Work::Waiting` reports it,
   a `Msg::Parsed` wakes the worker, and a dropped `Syntax` now sends
   `Msg::Stop`. If that thread cannot run, the worker parses the whole file
   itself.

Results, focused benchmarks, means of two runs each (before: the two system
heap runs of the allocator test). Bold: under 50 ms.

| Test                          | Measure                     | Before | Ideas 1-3 |
| ----------------------------- | --------------------------- | ------ | --------- |
| L01 Empty file                | First paint [ms]            | 21.7   | 21.6      |
| L01 Empty file                | Peak private [MiB]          | 5.1    | 5.1       |
| L23 Python, 10,000 lines      | Fully highlighted [ms]      | 56.7   | **47.0**  |
| L24 C, 10,000 lines           | Fully highlighted [ms]      | 84.4   | 74.5      |
| L25 TypeScript, 10,000 lines  | Fully highlighted [ms]      | 53.0   | **35.3**  |
| L20 Python, 50,000 lines      | Fully highlighted [ms]      | 209.0  | 198.0     |
| L21 C, 50,000 lines           | Fully highlighted [ms]      | 344.2  | 337.7     |
| L09 Python, 8 MiB             | Fully highlighted [ms]      | 911.6  | 925.1     |
| L22 1-line JSON, 8 MiB        | Fully highlighted [ms]      | 834.5  | 825.9     |
| L23 Python, 10,000 lines      | Peak private [MiB]          | 24.1   | 24.7      |
| E01 Type, Python 8 MiB        | Key to frame [ms]           | 0.45   | 0.44      |
| C05 Paste 1 MiB               | Key to frame [ms]           | 2.35   | 2.30      |
| C05 Paste 1 MiB               | Peak private [MiB]          | 38.0   | 51.3      |
| M05 Type at 10,000 carets     | Key to frame [ms]           | 4.00   | 4.29      |
| M05 Type at 10,000 carets     | Peak private [MiB]          | 119.9  | 139.8     |
| M08 Undo and redo, 10,000     | Peak private [MiB]          | 45.1   | 48.8      |
| K03 Type, C 50k lines         | Key to highlighted p95 [ms] | 48.0   | 49.8      |
| K07 Type, C 10k lines         | Key to highlighted p95 [ms] | 10.0   | 9.9       |
| K10 Go to Symbol, 10k lines   | Key to frame [ms]           | 18.1   | 18.2      |
| S02 Scroll and jump, MD 8 MiB | p95 [ms]                    | 0.89   | 0.94      |

Findings:

- 10,000-line files gain 10-18 ms: Python 47.0 ms and TypeScript 35.3 ms are
  under 50 ms; the dense C fixture (454 KiB) is at 74.5 ms, bound by its
  67 ms parse. The model `7 + max(query, parse) + 1.5` predicted 46.5, 36.5
  and 75.5 ms, so the per-language A column above holds: 19 of 24 languages
  under 50 ms for typical 10,000-line files.
- Large files gain little (the window parse was already short); 8 MiB files
  are within noise.
- Typing, scrolling and startup are unchanged.
- Peak private memory of two 8 MiB C tests rose 13-20 MiB (C05, M05),
  probably because the whole-file parse now overlaps the edit work instead of
  pausing for it; 10,000-line files are unchanged.

Validation: `cargo test -p rte_app -p rte_syntax` (121 and 20 passed,
1 ignored), including new tests for the loader hook, the adoption of an
early syntax thread and the whole-file parse thread with an edit meanwhile;
clippy and formatting clean.

#### Ideas 1-3 and 5

##### 2026_10_24 - Claude Opus 5.5

- Role: Implementer
- Activity: Implementation
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Adopted ideas 1-3 with mimalloc for tree-sitter only
  (`rte_syntax::install_allocator`, called first in `main`;
  `libmimalloc-sys` 0.1.49 with `extended`, MIT). Rust keeps the system
  heap. Python and TypeScript 10,000-line files are fully highlighted under
  50 ms; dense C and the five slow languages need idea 4.

Means of two runs each (before and ideas 1-3 as above). Bold: under 50 ms.

| Test                         | Measure                     | Before | Ideas 1-3 | Ideas 1-3 and 5 |
| ---------------------------- | --------------------------- | ------ | --------- | --------------- |
| L01 Empty file               | First paint [ms]            | 21.7   | 21.6      | 21.8            |
| L01 Empty file               | Peak private [MiB]          | 5.1    | 5.1       | 5.1             |
| L23 Python, 10,000 lines     | Fully highlighted [ms]      | 56.7   | 47.0      | **42.6**        |
| L24 C, 10,000 lines          | Fully highlighted [ms]      | 84.4   | 74.5      | 70.3            |
| L25 TypeScript, 10,000 lines | Fully highlighted [ms]      | 53.0   | 35.3      | **35.7**        |
| L20 Python, 50,000 lines     | Fully highlighted [ms]      | 209.0  | 198.0     | 176.4           |
| L21 C, 50,000 lines          | Fully highlighted [ms]      | 344.2  | 337.7     | 312.9           |
| L09 Python, 8 MiB            | Fully highlighted [ms]      | 911.6  | 925.1     | 817.3           |
| L22 1-line JSON, 8 MiB       | Fully highlighted [ms]      | 834.5  | 825.9     | 688.7           |
| L04 Python, 64 KiB           | Peak private [MiB]          | 9.1    | 9.2       | 11.5            |
| L23 Python, 10,000 lines     | Peak private [MiB]          | 24.1   | 24.7      | 26.7            |
| L25 TypeScript, 10,000 lines | Peak private [MiB]          | 19.1   | 19.5      | 22.1            |
| L09 Python, 8 MiB            | Peak private [MiB]          | 417.5  | 417.9     | 393.2           |
| L22 1-line JSON, 8 MiB       | Peak private [MiB]          | 439.3  | 439.6     | 415.9           |
| C05 Paste 1 MiB              | Peak private [MiB]          | 38.0   | 51.3      | 52.6            |
| M05 Type at 10,000 carets    | Peak private [MiB]          | 119.9  | 139.8     | 144.8           |
| E01 Type, Python 8 MiB       | Key to frame [ms]           | 0.45   | 0.44      | 0.45            |
| M05 Type at 10,000 carets    | Key to frame [ms]           | 4.00   | 4.29      | 4.30            |
| K03 Type, C 50k lines        | Key to highlighted p95 [ms] | 48.0   | 49.8      | 41.3            |
| K07 Type, C 10k lines        | Key to highlighted p95 [ms] | 10.0   | 9.9       | 8.9             |
| K05 Go to Symbol, 50k lines  | Key to frame [ms]           | 85.6   | 87.3      | 65.8            |
| K10 Go to Symbol, 10k lines  | Key to frame [ms]           | 18.1   | 18.2      | 13.6            |

Findings:

- The gains add up: Python 10,000 lines 56.7 to 42.6 ms, C 84.4 to 70.3
  ms, 8 MiB files 10-17% faster, Go to Symbol 25% faster.
- TypeScript did not gain from the allocator: it is fully highlighted in
  the first highlighted frame, so it is bound by the first frame (about
  34 ms after process start), not by parsing.
- Memory: 2-3 MiB more for small and 10,000-line files (mimalloc pages of
  the short-lived threads), 24 MiB less for the 8 MiB files; startup memory
  unchanged.
- Executable: 32,116,224 to 32,409,088 bytes (+286 KiB).

Validation: `cargo test -p rte_app -p rte_syntax` (121 and 20 passed,
1 ignored); clippy and formatting clean; the focused benchmark
`bench-perf --only` L01, L04, L09, L20-L25, E01, C05, M05, M08, K01, K03,
K05-K08, K10, S02 run twice, all caps passed. The allocator has no unit
test: it is process-wide and must be installed before any tree-sitter use,
which the shared test process cannot guarantee; every benchmark run
exercises it.

The performance work from "Performance Follow-up" onward is collected, with
its design, all result tables and review notes, in
[Done/SyntaxPerformance.md](SyntaxPerformance.md).

## Sol Implementation Review

### 2026_10_24 - GPT-6.1 Sol

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: GPT-6.1 Sol
- Effort: High
- Context Window: 1M
- Outcome: Reviewed current M6 and the performance follow-up after the Astra
  fixes. Changes requested for M6S1-M6S4: a disabled early worker remains
  owned, reduced tree edits can authorize the wrong embedded-language edit,
  trailing-whitespace comment scans exceed a frame, and truncated predicate
  text changes highlighting. Findings, measurements and exact focused checks
  are recorded in the canonical
  [performance review](SyntaxPerformance.md#sol-implementation-review).
  No application fixes authored; historical completion and task locations
  are unchanged.
