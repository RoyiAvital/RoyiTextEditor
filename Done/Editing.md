# Editing (M3)

## Task

Turn the read-only viewer into an editor: carets, selections, typing,
undo/redo, clipboard, edit-permission locks and safe saving, as specified in
the [Architecture](../Plan/Architecture.md) (Editing, App, Saving, Platform).

## Scope

### Included

- New crate `rte_edit`: selections, movement, pure edit commands, undo history.
- Keys: typing (`WM_CHAR`), `Enter` (keeps indentation), `Tab` / `Shift+Tab`
  (insert or indent / outdent lines), `Backspace` / `Delete` and their `Ctrl`
  word variants, arrows, `Ctrl+Arrows`, `Home` (smart) / `End`,
  `Ctrl+Home` / `Ctrl+End`, `PgUp` / `PgDn`, all with `Shift`; `Ctrl+A`,
  `Ctrl+Z`, `Ctrl+Y` / `Ctrl+Shift+Z`, `Ctrl+C` / `Ctrl+X` / `Ctrl+V`
  (whole line on an empty selection), `Ctrl+S`, `Esc`, `Insert` (overtype),
  `Ctrl+Up` / `Ctrl+Down` (scroll), `Ctrl+W` / `Ctrl+F4`, zoom.
- Mouse: click, drag, double-click (word), triple-click (line),
  `Shift+Click`; I-beam cursor over text.
- Carets and selections drawn; caret kept visible while moving.
- Undo: root restore, word-group coalescing, retention 10,000 steps /
  256 MiB inserted bytes; dirty state by `saved_root`.
- Clipboard: `CF_UNICODETEXT` plus a private format (line mode, pieces).
  Pasted line breaks follow the document style.
- Locks `user`, `loading`, `encoding`, `load_error`; `RO` and reason in the
  status line. The toggle command comes with the command registry (M4).
- Saving per [Saving](../Plan/Architecture.md#saving) with `FileOps` fault injection;
  conflict strip (Overwrite / Cancel); dirty-close strip (Save / Don't Save /
  Cancel; untitled: Discard / Cancel); close during a save.
- Unsaved marker `●` in the title strip: accent color, warning color for
  untitled documents.
- Application manifest: supported OS, per-monitor v2 DPI (replaces the
  runtime call), UTF-8 code page, long paths; segment heap only if measured
  better.

### Excluded

- Command registry, palette, configurable keymap, Basic Editing table
  commands (move/copy/delete line, comments, brackets): M4.
- Multi-cursor creation (`Alt+Click`, `Ctrl+Alt+Up/Down`), cursor undo,
  box selection: M5. The selection model and commands already handle
  several selections.
- Edit journal and recovery ([DataSafety001](../Plan/DataSafety001.md), not yet
  reviewed); external change detection: M7.
- Save As and new files from inside the editor (Architecture decision).

### Compatibility Constraints

- Bytes the user did not edit are preserved, including invalid UTF-8,
  lone CR and mixed line endings.
- No Registry writes; no new startup DLL imports.

## Design

- **`rte_edit`** depends on `rte_text` and `unicode-width`. `Selection
  { anchor, head, goal }`; `Selections` sorted, merged, non-empty, with a
  primary index. Movement treats CRLF and a character followed by zero-width
  marks as one step. Columns follow the view's cell rules; beyond 64 KiB
  into a line, columns are approximated by bytes (long-line indexing is M7).
  Commands are pure: `(snapshot, selections, options) → Edit` (transaction,
  new selections, kind).
- **History** stores `{before, after, sels_before, sels_after}` snapshots;
  undo restores roots. Typing coalesces until a word starts after a
  non-word character; deletions coalesce by direction.
- **`rte_view`** draws selection fills (clipped to the text area), the line
  break cell of selected line ends, and carets (bar; underline in
  overtype) in an overlay layer drawn after glyphs. `hit_test` maps a
  point to a byte offset with the same column rules.
- **`rte_app`**: `Document` (snapshot, selections, history, locks, saved
  snapshot, baseline) is the only mutation entry (`apply`). `save.rs` runs
  the save sequence over a `FileOps` trait on one save thread at a time;
  results return through a channel and the waker. Prompts replace the
  status line until answered.
- **`rte_platform`**: mouse events with capture and click counting, I-beam
  cursor, clipboard, file stamps (`FILE_ID_INFO`, size, last-write time),
  `ReplaceFileW`, `MoveFileExW` without replace. Content hashes: XXH3-64.
- **Manifest** compiled by MinGW `windres` from `rte_app/build.rs` and linked
  into the binary only.

## Alternatives

- `unicode-segmentation` for grapheme movement: deferred; zero-width
  marks attached to the previous character match the view's drawing.
- Save As dialog: dropped by decision (new files only from the command line).
- `embed-resource` crate for the manifest: rejected; `windres` is already
  required by the GNU toolchain setup and needs no extra dependency.
- Own hash for baselines: rejected for XXH3 (`xxhash-rust`, no
  dependencies, stable across versions for the later journal).

## Runtime Effects

- Startup: no new work before the first frame; the manifest replaces the
  DPI call.
- Typing: one transaction, one history step (or coalescing), visible rows
  re-laid out.
- Saving: one thread per save, file size I/O plus hashing of files
  ≤ 64 MiB (target before and backup after the commit).
- Memory: history holds shared snapshots within the retention caps.
- Idle: no timers; mouse moves are delivered only while a button is held.

## Tests

- `rte_edit`: movement (CRLF, marks, tabs, wide characters, words, smart
  Home, goal column), commands (typing, overtype, newline indentation,
  Backspace over CRLF, word deletion, indent/outdent, cut/copy/paste
  with line mode and distribution), history (coalescing, retention, redo
  cleared), property: random commands then undo all restores the original
  tree, redo all restores the final text.
- `rte_view`: selection and caret fills; hit test versus columns; column
  rules versus `layout_line`.
- `rte_app`: locks refuse every mutation; save refused under `encoding` and
  `load_error` and for untitled documents; fake `FileOps`: success, conflict
  before commit, race after commit (backup kept), absent target with a file
  appearing, 1175/1176/1177/unknown; queued save resolves at start; edits
  during a save keep the document dirty; close during a save. Real files:
  round trip byte identity for LF, CRLF, mixed, lone CR, BOM, UTF-16 LE/BE,
  invalid UTF-8, no trailing newline.
- `rte_platform`: stamps change on rewrite; replace and move-without-replace
  on real files.
- Release smoke: type, close, dirty prompt, Don't Save; startup gate.
- Manifest: imports unchanged; segment heap measured (startup, idle
  `PrivateUsage`).

## Implementation Steps

1. `rte_edit` with tests.
2. View: selections, carets, hit test.
3. Platform: mouse, cursor, clipboard, file operations.
4. App: document, keys, mouse, clipboard, locks, prompts.
5. Saving with `FileOps` and tests.
6. Manifest and segment-heap measurement.
7. Validation, documentation, changelog.

## Acceptance Criteria

- All included keys and mouse actions edit as specified; undo/redo restore
  exact text and selections.
- Saving follows the Architecture sequence; no tested failure path deletes
  the only copy of any version.
- Locks refuse edits and saves as specified.
- Startup gate passes; release imports unchanged.

## Reviewers

### 2026_10_23 - Claude Opus 5.5

- Role: Reviewer
- Activity: Design
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Task defined from the approved Architecture M3 scope and the
  user's decision that new files are created only from the command line.

## Implementer

### 2026_10_23 - Claude Opus 5.5

- Role: Implementer
- Activity: Implementation
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: M3 delivered as designed. Deviations: the read-only toggle is not
  bound (it needs the M4 command registry); Save As does not exist (user
  decision). Segment heap measured without gain and not enabled. 88 tests,
  Clippy, release smoke tests and the startup gate pass.

## Validation Results

Machine: development PC of M2 (Ryzen 9 9950X, Windows 11), release build.

| Command or check                                                 | Outcome                                       |
| ---------------------------------------------------------------- | --------------------------------------------- |
| `cargo fmt --all -- --check`                                     | Pass                                          |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Pass                                          |
| `cargo test --workspace --locked`                                | 88 tests pass                                 |
| `cargo xtask bench-startup` (50 runs, 5 warm-up)                 | Gate PASS, `first_frame` p95 38.80 ms         |
| `cargo bench -p rte_render --bench frame` (median)               | 1080p 0.39 ms, 4K 1.25 ms, marks 3.45 ms      |
| `cargo xtask package`                                            | ZIP created; no MinGW imports; manifest found |
| DPI awareness of the running release (manifest only)             | Per-monitor v2 (`GetProcessDpiAwareness` = 2) |
| Smoke: type, close, `D` (Don't Save)                             | Prompt shown; exit 0; file unchanged          |
| Smoke: type, close, `Enter` (Save)                               | Exit 0; file saved; no temporary files left   |
| Smoke: new path from the command line, type, close, `Enter`      | File created with the typed text              |

The frame benchmark now selects part of every visible row and draws a caret.
Startup medians varied between runs (25.5 ms and 37.6 ms for `first_frame`),
because the first frame waits for DWM composition (one refresh at 59 Hz).

Segment heap (30 startup runs, 7 idle memory samples, `RTE_SEGMENT_HEAP=1`
versus default):

| Build        | `first_frame` median / p95 | Idle private / working set |
| ------------ | -------------------------- | -------------------------- |
| Segment heap | 26.06 / 27.36 ms           | 4.70 / 13.01 MiB           |
| Default      | 25.51 / 27.20 ms           | 4.66 / 13.00 MiB           |

No gain, so the segment heap stays disabled.

Not run: GitHub-hosted CI and release workflows (their `windres` comes from
the runner image's GNU Binutils), clipboard exchange with other applications
(the clipboard was tested in memory only), manual mouse and IME checks,
4K and multi-monitor DPI moves, cold start after reboot.

## Implementation Review

### 2026_10_23 - GPT-6.1 Sol

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: GPT-6.1 Sol
- Effort: High
- Context Window: 1M
- Outcome: Changes requested for M3R1-M3R6 below. Sixty-eight existing focused
  tests, formatting, Clippy, startup and package checks pass. Save/discard UI
  smoke passes, but five disposable correctness probes fail and a release
  movement probe exceeds interactive budgets. No implementation fixes were
  authored. Historical records and the task's location are preserved; this
  review does not approve M3.

### Scope

Reviewed `rte_edit`, the document and editor integration, clipboard formats,
file baselines, saving, selection/caret layout and rendering, the manifest
build, and package checks. Journal/recovery and external-change merging in
[DataSafety001](../Plan/DataSafety001.md) are not approved M3 deliverables.
The findings below concern the implemented save guarantees and editing paths,
not missing recovery, Save As, command-palette or multi-cursor features.

### M3R1 [P1] Save completion can adopt an unseen external version

Location: [new_baseline](../crates/rte_app/src/save.rs#L236).
After committing, the code stamps the current path rather than establishing
that it still holds the committed version. If another writer replaces or
rewrites it with different-size content before that stamp, `new_baseline`
adopts the writer's stamp and silently sets `hash: None`. The save still
reports Saved. A subsequent save accepts that unseen version as its baseline,
overwrites it and deletes its backup as if it were a known previous version.

A `FileOps` probe writes external content immediately after a successful
replace. The returned baseline contains the external size with no hash; a
second save returns Saved instead of Conflict, leaving only the editor's text.
Do not adopt a stamp whose size/content disagrees with the committed snapshot
or silently downgrade its hash guarantee. Preserve conflict/recovery evidence
when the post-commit target cannot be verified. Add a two-save regression with
a writer between commit and baseline collection.

### M3R2 [P1] Failed temporary creation deletes a file it does not own

Locations: [temporary-name search](../crates/rte_app/src/save.rs#L167) and
[write failure cleanup](../crates/rte_app/src/save.rs#L269).
The name search is a check, not a reservation. If a file appears before
`create_new`, creation correctly fails with AlreadyExists, but `save` then
unconditionally deletes that path. It may be another actor's only copy.
The probe creates unrelated content in that interval and returns AlreadyExists;
the unrelated file is deleted while the original target remains unchanged.
Track ownership of a successfully created temporary file, or perform cleanup
inside the layer that owns its handle. Never unlink a name solely because it
was selected earlier. Add collision and pre-creation permission-failure tests
that assert unowned files survive.

### M3R3 [P1] Approximate columns produce invalid UTF-8 caret positions

Locations: [offset_at_column](../crates/rte_edit/src/columns.rs#L69),
[vertical movement](../crates/rte_edit/src/motion.rs#L128), and
[hit testing](../crates/rte_view/src/lib.rs#L501).
Beyond the 64 KiB scanned prefix, one column becomes one byte without finding
a character boundary in the destination snapshot. Approximate display columns
are allowed by M3, but splitting a valid encoded character is not.

The probe moves from column 65,537 of an ASCII line to a line containing
65,536 ASCII bytes followed by U+4E2D characters. The resulting head is one
byte inside a three-byte character; inserting `X` makes valid UTF-8 invalid.
Snap estimated positions to safe UTF-8/control-cluster boundaries, including
prefix cuts through a character; keep any approximation explicitly separate
from edit coordinates. Add vertical-movement and mouse-hit tests around the
scan limit, then type/delete and verify valid text remains valid.

### M3R4 [P1] Word and cluster movement blocks the UI on long runs

Locations: [character reads](../crates/rte_edit/src/motion.rs#L10),
[next_boundary](../crates/rte_edit/src/motion.rs#L49), and
[word_right](../crates/rte_edit/src/motion.rs#L181).
Word scanning repeatedly allocates small range copies and seeks through the
tree for each character. Word navigation, word deletion and double-click
selection run these scans synchronously in editor event handling, without a
work/time bound. Cluster movement likewise walks every attached mark.

Release measurements on the development machine: Ctrl+Right across one ASCII
word took 10.86 ms at 64 KiB, 225.61 ms at 1 MiB and 918.31 ms at 4 MiB.
Right across 200,000 combining marks took 13.56 ms. These exclude layout and
presentation. Use chunk cursors instead of per-character allocations and add
a bounded execution/pending-result policy for long scans, with stale-result
rejection. Add long-word, long-whitespace and combining-run latency tests;
short-file startup measurements do not cover this risk.

### M3R5 [P2] Typing during a save coalesces across its undo boundary

Locations: [start_save](../crates/rte_app/src/editor.rs#L449) and
[save completion](../crates/rte_app/src/editor.rs#L482).
History is sealed only when the save completes. Typing `a`, starting a save,
then typing `b` while it runs can coalesce both characters into one step.
After `a` is committed, undo jumps to the original empty root rather than the
saved `a` root, so the expected clean savepoint is not reachable through undo.
The document/history probe reproduces this exact state transition. Seal
coalescing when capturing the save snapshot, not just after its result, and
add a blocked-save editor test that types during the save and then undoes to
the committed text and a clean marker. Include failed saves and queued saves
when defining the grouping policy.

### M3R6 [P2] An untouched untitled document starts dirty

Location: [Document::new](../crates/rte_app/src/document.rs#L51).
`snapshot` and `saved` are independently allocated empty roots. Since dirty
state compares root identity, a new document reports dirty before the first
edit, shows an unsaved marker, and prompts to discard an untouched buffer.
The direct constructor probe fails `!document.is_dirty()`. Initialize the
saved root from the same empty snapshot. Add no-argument startup/close and
type-then-undo tests confirming that the pristine untitled state is clean.

### Review Validation Results

Successful focused commands:

```powershell
cargo test -p rte_edit -p rte_app -p rte_platform -p rte_view --locked
cargo test -p rte_render -p xtask --locked
cargo build -p rte_app --release --locked
cargo clippy -p rte_edit -p rte_app -p rte_platform -p rte_view -p rte_render -p xtask --all-targets --locked -- -D warnings
cargo fmt --all --check
cargo xtask bench-startup --runs 50 --warmup 5
cargo xtask package
```

- Existing focused tests: 68 pass (13 editing, 31 application, 3 platform,
  13 view, 5 renderer, 3 harness). No full workspace suite was run.
- Application-test output: the Windows-subsystem test executable hides its
  console output. A disposable console copy of the unchanged application test
  root, with module paths preserved, ran
  `target/m3_app_visible_tests.exe --nocapture`: all 31 tests pass. This does
  not add another 31 distinct tests to the total.
- Correctness: original document, save and loader modules plus five review
  tests compiled from stdin into ignored `target/m3_review_probe.exe`.
  Run `target/m3_review_probe.exe review_ --nocapture --test-threads=1`:
  all five review assertions fail, confirming M3R1-M3R3 and M3R5-M3R6. The
  save races use `FileOps` and in-memory files; no external user files are
  touched. These failures are findings, not passing regression coverage.
- UTF-8 fixture: the final probe uses ASCII Rust Unicode escapes so shell
  encoding cannot replace its multibyte characters. The initial literal-based
  movement result was discarded after detecting that transport issue.
- Latency: `cargo build -p rte_edit --release --locked --message-format=json`
  selects the exact matching release artifacts, then a stdin probe linked
  with `opt-level=3`, fat LTO and `x86-64-v3` runs as
  `target/m3_motion_review_probe.exe`. The reported movement times are narrow
  local samples, not frozen-hardware percentile certification.
- Startup: 50 measured launches, 5 warm-ups, generated C file; first-frame
  median 35.91 ms and p95 38.78 ms, gate PASS.
- Native smoke: two review-owned release processes on disposable files.
  Send bounded `WM_CHAR('X')`, `WM_CLOSE`, then `WM_KEYDOWN('D')` or Enter to
  each child's own window. Both exit 0 within the bound. Don't Save leaves
  `old`; Save writes `Xold`; no temporary/backup artifacts remain. Fixtures
  are removed and every child is joined or terminated in cleanup. No global
  keyboard injection or system-clipboard access is used.
- Packaging: ZIP produced; forbidden MinGW imports absent; the package
  manifest-marker check passes. This is not a hosted release run.
- Recording checks: preserve the original task-document bytes, verify one
  new reviewer record and M3R1-M3R6, resolve new file/line links, and confirm
  unchanged hashes for 35 reviewed source/build/design files.
- Not run: GitHub-hosted CI/release, native inter-application clipboard
  exchange, manual mouse/AltGr/dead-key checks, cold start after reboot,
  multi-monitor DPI moves, 4K/144 Hz display checks, or data-journal recovery.

No application code, build configuration, changelog, task index or earlier
reviewer/implementer record was changed by this review.

## Efficiency Review

### 2026_10_23 - GPT-6.1 Sol

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: GPT-6.1 Sol
- Effort: High
- Context Window: 1M
- Outcome: Identified six CPU/memory opportunities, with allocation probes
  for E1-E5. Prioritize eliminating redundant work and duplicate storage over
  reducing useful caches or undo retention. No optimizations were implemented;
  M3R1-M3R6 remain open and this addendum does not approve M3.

### Measurement Limits

A disposable release probe wraps Rust's system allocator and reports requested
live bytes, peak extra bytes, and cumulative allocation/reallocation requests.
These are not process `PrivateUsage`: allocator overhead/fragmentation, DIBs,
Win32 clipboard blocks, mapped images/fonts and thread stacks are excluded.
The allocator's internal temporary overlap during `realloc` is not visible.
Cumulative requests measure churn, not resident memory or actual bytes copied.
Instrumentation affects CPU time; use the earlier uninstrumented latency
results for M3R4, not timings from this allocation probe.

### E1 High: Stop reading after EOF was already observed

Location: [loader loop](../crates/rte_app/src/loader.rs#L113).
`fill` reads until full or EOF. After a short nonempty block, the loop still
allocates the next 16 MiB buffer and reads again merely to rediscover EOF.
Loading a 512-byte fixture reached 16,778,418 peak extra heap bytes, although
only 984 bytes remained after completion. The existing `more` flag already
identifies a short block. Stop after processing it, then finish decoding and
publish Done normally. This removes an allocation and read without adding
work; it should reduce the small-file load peak to the first-buffer scale.
Check empty, short, exact-block, UTF-16-tail and read-error cases. The proposed
post-change peak has not been measured because production code is unchanged.

### E2 High: Borrow chunks instead of repeatedly materializing text

Locations: [character reads](../crates/rte_edit/src/motion.rs#L10),
[column prefixes](../crates/rte_edit/src/motion.rs#L110), and
[viewport prefix copy](../crates/rte_view/src/lib.rs#L334).
The 64 KiB word scan makes 393,220 allocation/reallocation calls, requesting
19.5 MiB cumulatively while peaking at only 104 extra bytes. This is primarily
a CPU/churn problem, not a large retained-memory problem. A borrowed forward/
backward chunk cursor, with a small stack buffer only at UTF-8 boundaries,
can remove per-character allocation and repeated tree seeks (M3R4).

Even `column_at(..., 10, ...)` copies the full 64 KiB prefix of a long line.
One hundred calls request 11.7 MiB cumulatively and peak around 64 KiB.
Stop scanning/copying when the required column or visible end is known; reuse
caret location results across reveal/status/layout for the same snapshot
identity, caret and tab settings. Preserve byte/time limits and safe character
boundaries (M3R3). Avoid a large whole-document layout cache.

### E3 High: Share glyph bitmaps by glyph identity

Location: [non-ASCII glyph cache](../crates/rte_render/src/lib.rs#L126).
Different characters mapping to the same font glyph currently get separate
bitmap allocations. At 28 px, 100,000 unsupported characters, all mapping to
glyph 0, retain 33,973,968 extra bytes (32.4 MiB) and generate about 2.05 GiB
of cumulative allocation requests. This exceeds the architectural 16 MiB
glyph-cache budget without containing 100,000 distinct glyph images.

Share coverage masks by face/size/glyph ID, especially the missing-glyph box,
while preserving the ASCII fast path and cheap warm character lookup. A
control aliasing those inputs to one representative character produces
identical pixels and retains only 466 extra bytes. This proves duplicate
storage is unnecessary; it is not an implemented cache or a production timing
comparison. Deduplication avoids eviction/rerasterization and is the preferred
first step. A bounded LRU is a separate memory/performance tradeoff to benchmark.

### E4 High: Reduce clipboard staging copies

Locations: [encoding](../crates/rte_app/src/clip.rs#L15),
[paste preparation](../crates/rte_edit/src/commands.rs#L429), and
[native text conversion](../crates/rte_platform/src/clipboard.rs#L103).
For one valid 8 MiB ASCII piece, encoding adds a 16 MiB heap peak above the
input, although only the final 8 MiB string needs to survive. Paste preparation
also adds a 16 MiB peak and requests 24 MiB cumulatively. Valid text is cloned,
joined, normalized and cloned again; distributed paste constructs an unused
whole joined string before processing its individual pieces.

Borrow valid UTF-8 with `Cow`, reserve known final lengths, normalize/join only
when necessary, and move the finished buffer into the transaction rather than
clone it. Skip the whole-string path when distributing pieces. Native UTF-16
staging for that 8 MiB text reserves 22,369,640 bytes for 16,777,218 bytes of
output, before another copy into the Win32 global block. Exact sizing/direct
encoding into the owned global block can avoid that spare capacity and copy.
On read, decode a bounded locked UTF-16 slice instead of copying both raw bytes
and a second unit vector first. Preserve malformed-text handling, termination,
private metadata and allocation-failure ownership. Benchmark conversion cost;
do not claim exact total-process savings from these Rust-only counters.

### E5 Medium: Remove full-block ownership conversion copies

Locations: [loaded block conversion](../crates/rte_app/src/loader.rs#L142) and
[Chunk ownership](../crates/rte_text/src/tree.rs#L55).
Converting a full 16 MiB `Vec<u8>` into `Arc<[u8]>` temporarily needs roughly
32 MiB: both allocations coexist. A comparison retaining its boxed allocation
under `Arc<Box<[u8]>>` peaks at roughly 16 MiB, without copying the payload.
Consider an immutable shared block owner with that property. Right-size partial
buffers: retaining a truncated 16 MiB vector for a tiny tail would negate the
saving. This is a peak-memory/bandwidth opportunity, not a demonstrated reduction
in steady-state document memory. An extra indirection and different tree layout
need load/edit/query benchmarks; a thinner chunk handle alone does not ensure
a smaller node when another enum variant still determines its size.

### E6 Low: Skip no-op invalidation and font resets

Locations: [unconditional redraw](../crates/rte_app/src/editor.rs#L916) and
[font-size reset](../crates/rte_render/src/lib.rs#L101).
Unhandled keys, ignored control characters, unchanged selections and wheel
remainders can request a full repaint without changing visible state. Mark
redraw only when needed, leaving Win32 paint coalescing intact. Also return
early on unchanged pixel size once font metrics have been initialized, rather
than clear caches. First-time initialization in `Renderer::new` must still run.
These are source-level opportunities; savings have not been measured here.
Verify paint counts, focus/resize behavior and warm-cache glyph counts before
claiming an improvement.

### Memory Tradeoffs To Avoid

- Do not shrink reusable frame buffers on every paint or undo history on every
  keystroke. Reallocation/rerasterization can cost more than the saved capacity.
- Tiny pieces retaining large source blocks need a policy, not eager copying.
  Saved snapshots and undo may intentionally retain the same buffers; copying
  current pieces then would add memory without freeing the originals. Consider
  compaction only after accounting for ownership and measuring its cost.
- Keep bundled font data borrowed and snapshots shared. Their current design
  already avoids per-frame font copies and full-document undo copies.

### Efficiency Validation

- Built the current artifacts with
  `cargo build -p rte_app --release --locked --message-format=json`; used
  Cargo's reported library paths, not timestamps of cached artifacts.
- Compiled the stdin-only probe with `opt-level=3`, fat LTO,
  `codegen-units=1`, self-contained linking and `x86-64-v3`. Ran
  `target/m3_efficiency_review_probe.exe`: all result, length, load-completion
  and pixel-equality assertions pass. No application source was modified.
- Glyph inputs, clipboard payloads and existing snapshots are allocated before
  their measurement phases, so reported deltas exclude the fixture storage.
  The glyph control changes only disposable input aliases, not the renderer.
- The loader uses a temporary 512-byte file, removed after the probe. Clipboard
  checks operate only on memory; no system clipboard, user file or installed
  application was inspected or changed.
- E5 is an isolated ownership-conversion comparison, not a text-engine rewrite.
  E6 is source inspection only. No claimed optimization has passed full
  application regression/performance gates because none was implemented.
- The earlier 68-test and startup results remain historical evidence; the
  complete suite was not rerun for this documentation-only addendum.
- Recording checks preserve the preceding document bytes, verify E1-E6 and
  the new reviewer attribution, resolve local links, and compare 14 source
  hashes. Existing implementation-review findings are unchanged.

## Review Response

### 2026_10_23 - Claude Opus 5.5

- Role: Implementer
- Activity: Implementation
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: M3R1-M3R6 fixed with regression tests; E1-E6 implemented and
  measured. Worst-case movement ≤ 1.1 ms (was up to 918 ms); small-file
  peak private memory 17.39 → 4.88 MiB; 512 MiB load peak 541.10 → 527.07 MiB.

### Response M3R1

Fixed. [`committed_baseline`](../crates/rte_app/src/save.rs#L248) adopts the
post-commit stamp only when the file still holds the committed text: same
size, and the same XXH3 hash (≤ 64 MiB) or the temporary file's identity
(above). Otherwise it keeps the committed version's stamp (taken from the
temporary file before the commit) with its hash, reports "changed again right
after the save", and the next save returns Conflict. Test
`writer_right_after_the_commit_is_never_adopted`: replaced and created
targets, different-size and same-size external writes, then a second save.

### Response M3R2

Fixed. `save` no longer deletes the temporary name after a failed
[`write_new`](../crates/rte_app/src/save.rs#L72); the implementation removes
only a file it created itself (after its handle is closed). A name taken by
another file is never deleted. Tests:
`temporary_names_taken_by_others_are_never_deleted` (collision, and
permission failure before creation) and
`system_write_new_removes_only_what_it_created` (real files).

### Response M3R3

Fixed. [`snap_to_boundary`](../crates/rte_edit/src/motion.rs#L68) moves an
estimated position out of a UTF-8 sequence, from between CR and LF, and before
zero-width marks of a character on the same line. Vertical movement and
`hit_test` both go through `offset_in_line`, which snaps. Tests:
`estimated_columns_never_split_characters` (columns 65,530-65,559 over a
line of 65,536 ASCII bytes plus U+4E2D, then type, Backspace and Delete keep
valid UTF-8), `snapping_avoids_split_characters_crlf_and_marks`, and the view
test `hits_beyond_the_scan_limit_land_on_character_boundaries`.

### Response M3R4

Fixed with a bounded synchronous policy instead of pending results.
Movements read through a borrowed leaf cursor
([`Bytes`](../crates/rte_edit/src/cursor.rs#L6), new
`Snapshot::chunk_at`), with no allocation per character and one tree seek
per leaf. Each movement scans at most
[`MOTION_SCAN_LIMIT`](../crates/rte_edit/src/motion.rs#L13) (64 KiB); longer
words, whitespace and mark runs are crossed by repeating the key. No
background state can go stale, so no rejection logic is needed. Benchmark
`cargo bench -p rte_edit --bench motion` (median): `Ctrl+Right` in a 4 MiB
word 0.36 ms, `Ctrl+Left` 0.70 ms, double-click word 1.06 ms, 4 MiB
whitespace 0.33 ms, Right+Left over 200,000 marks 0.45 ms, `Down` between
4 MiB lines 0.14 ms. Test `long_runs_are_crossed_in_bounded_steps`.

### Response M3R5

Fixed. [`start_save`](../crates/rte_app/src/editor.rs#L523) seals undo
coalescing when the save snapshot is captured; queued and retried saves pass
through it as well, and failed saves keep the boundary. Test
`typing_during_a_save_stays_a_separate_undo_step`.

### Response M3R6

Fixed. [`Document::new`](../crates/rte_app/src/document.rs#L51) shares one
empty snapshot as text and saved root. Tests `a_new_document_is_clean` and
`a_pristine_untitled_document_closes_without_asking`; release smoke: an
untitled window closes on `WM_CLOSE` without a prompt.

### Response E1

Done. Reads are sized by the remaining length (at least a 64 KiB
[probe](../crates/rte_app/src/loader.rs#L115)), and a short read ends the
loop. A file that grew is still read through the probe. Small-file peak
private memory: 17.39 → 4.88 MiB. Test
`empty_and_exact_block_files_load_completely`.

### Response E2

Done. Movement uses the borrowed cursor (M3R4); `column_at` copies only up to
the caret; `offset_in_line` reads a column-sized window that grows only when
needed. The editor caches the caret column per snapshot, caret and tab width
([`caret_column`](../crates/rte_app/src/editor.rs#L389)), shared by reveal and
the status line. View rows inside one leaf are
[borrowed](../crates/rte_view/src/lib.rs#L335) instead of copied. Benchmark:
column 10 of a 4 MiB line 63 ns.

### Response E3

Done. Non-ASCII [bitmaps](../crates/rte_render/src/lib.rs#L137) are keyed by
glyph ID; a character-to-glyph map keeps warm lookups cheap. Test
`missing_characters_share_one_bitmap_and_sizes_keep_caches`: 1,000 missing
characters hold one bitmap. A bounded LRU is not added.

### Response E4

Done. `clip::encode` borrows valid pieces and builds the text in one exactly
sized allocation; `decode` moves the clipboard string into a single piece.
`paste` builds no joined string when distributing pieces, normalizes line
breaks only when needed (exactly sized), and moves the last buffer into the
transaction. Native text is encoded straight into the global block and
decoded straight from it ([`global_text`](../crates/rte_platform/src/clipboard.rs#L72)).
Allocation peaks were not re-measured with an allocator probe. The native
round trip is an ignored test (`native_round_trip`) because it replaces the
user's clipboard; it was not run.

### Response E5

Done without changing the text engine: each block is allocated as its
`Arc<[u8]>` (`repeat_n(0).collect()`, no intermediate vector) and filled
in place through `Arc::get_mut`, so full blocks are never copied. The last
block is right-sized by the remaining length; short or BOM-trimmed blocks
get a right-sized copy. 512 MiB load peak private memory 541.10 → 527.07 MiB
(steady 525.6-525.9 MiB).

### Response E6

Done. The editor compares what a frame shows before and after each event and
requests a repaint only on a change (test
`only_visible_changes_request_a_repaint`). `Renderer::set_size`
([source](../crates/rte_render/src/lib.rs#L105)) keeps caches when the size is
unchanged (zoom at its limits).

### Review Response Validation Results

| Command or check                                                 | Outcome                                       |
| ---------------------------------------------------------------- | --------------------------------------------- |
| `cargo fmt --all -- --check`                                     | Pass                                          |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Pass                                          |
| `cargo test --workspace --locked`                                | 102 pass, 1 ignored (`native_round_trip`)     |
| `cargo xtask bench-startup` (50 runs, 5 warm-up)                 | Gate PASS, `first_frame` p95 38.60 ms         |
| `cargo bench -p rte_render --bench frame` (median)               | 1080p 0.38 ms, 4K 1.28 ms, marks 3.47 ms      |
| `cargo bench -p rte_edit --bench motion` (median)                | Worst case 1.06 ms (double-click word)        |
| Peak private memory, 512-byte file (5 runs, median)              | 4.88 MiB (before: 17.39 MiB)                  |
| Peak private memory, 512 MiB file (5 runs, median)               | 527.07 MiB (before: 541.10 MiB)               |
| Smoke: type, close, `Enter` (Save)                               | Exit 0; `xhello`; no temporary files          |
| Smoke: untitled window, close                                    | Closes without a prompt                       |

Peak memory is `PeakPagefileUsage` (.NET `PeakPagedMemorySize64`) of the
release build 3 s after start; "before" was measured on the unchanged build
the same session.

Not run: `native_round_trip` (system clipboard), allocator probes for E4,
GitHub-hosted CI, manual mouse and IME checks, 4K and multi-monitor DPI moves.
