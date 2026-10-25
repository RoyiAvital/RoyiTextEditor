# Empty State and Drag and Drop

## Task

Started without a file, the editor opens an untitled document that can be
edited but never saved, and closing it asks to discard the edits. This makes
no sense for a single-file editor without Save As.

Replace the untitled document with an **empty state**: no document, nothing
to edit. A file **dragged onto the window** opens in it. Closing a file
returns to the empty state; `Alt+F4` quits. Startup time must not grow.

## Scope

### Included

- **Empty state** when started without a path, and after closing a file:
  - Title strip and window title show the application name only.
  - Body: background and one centered hint, `Drop a file here to open it`.
    No gutter, line numbers, caret or selection.
  - Status line: no position, encoding or language.
  - Allowed commands: Show All Commands, Show Favorite Commands, About,
    Keyboard Shortcuts, Documentation, Quit, Zoom In / Out / Reset, Toggle
    Read-Only, Toggle Syntax Highlighting. All others are refused with
    `No file is open` and dimmed in the palette. Typing, mouse selection,
    the search box and code commands do nothing.
  - File: Close (`Ctrl+W`, `Ctrl+F4`) does nothing (notice `No file is open;
    Alt+F4 quits`), so pressing it twice never quits by accident.
  - Quit and the window close request exit at once.
- **Quit keys.** `Application: Quit` gets `Ctrl+Shift+W` beside `Alt+F4`
  (VS Code's Close Window); both ask first when there are unsaved changes.
- **Drag and drop** of files from Explorer (or any `CF_HDROP` source):
  - Empty state: the file opens, as from the command line.
  - A file is open without unsaved changes (and no save running): the
    dropped file replaces it.
  - Unsaved changes or a save running: nothing opens; notice
    `<name> has unsaved changes; save or close it to open <dropped>`.
  - Several files: the first opens; notice `Only one file opens at a time;
    <n> more ignored`.
  - A folder: notice `Folders cannot be opened`; nothing changes.
  - While a prompt is shown: ignored. While a popup is open: the popup
    closes, then the rules above apply.
- **Close returns to the empty state.** File: Close on an open file:
  clean → empty state; unsaved → the existing prompt (Save / Don't Save /
  Cancel), then the empty state; during a save → waits for it (as closing
  does today), then the same.
- **Per-file settings.** `--read-only` applies to every file opened in the
  process; a read-only toggle is reset to it when another file opens.
  `--syntax <id>` applies to the command-line file only; dropped files are
  detected by name and content. A Change Language Mode choice ends with its
  file.
- **Load cancellation** (Architecture, Text Engine): closing or replacing a
  file while it loads stops its loader between blocks.
- Removed: the untitled scratch document, its at-risk marker, the
  `Discard the untitled document?` prompt and the `Untitled documents cannot
  be saved` notice.
- Documentation: README (usage, keys), docs pages (CLI, keyboard shortcuts,
  introduction), Architecture (process model, dirty close, new files,
  decisions, `Ctrl+N` row), CHANGELOG, TODO.

### Excluded

- File: Open (`Ctrl+O`) and a file dialog: a later task.
- Drops onto an elevated (administrator) window: Windows blocks them from a
  non-elevated Explorer (UIPI); not worked around.
- Drag feedback beyond the system copy cursor (no highlight while hovering).
- Dropped text (not files), dropping folders to browse them, several
  documents at once, tabs, recent files.

### Compatibility Constraints

- Startup with a path is unchanged; startup without one does less work.
- No new DLL imports at startup (`shell32` loads on the first drop).
- A path on the command line that does not exist still opens an empty
  document that the first save creates.
- Command IDs, the command line and the configuration file are unchanged.
- Single session ([Architecture](../Plan/Architecture.md), Principle 8): opening,
  replacing and closing files happen inside this session; nothing launches,
  closes or contacts another session.
- Supersedes in [Architecture](../Plan/Architecture.md): "One document per process
  lifetime; no in-place reload of another file", the untitled document
  under Saving (New files) and Dirty close, the `Ctrl+N` untitled instance,
  and the Process and New files rows of the decisions table. These are
  updated when this task is implemented.
- [DataSafety001](../Plan/DataSafety001.md) mentions untitled documents; its next
  review must follow this change.

## Design

### Platform (`rte_platform`)

- `CreateWindowExW` gets `WS_EX_ACCEPTFILES` (a style bit; no call, no
  import). Explorer then sends `WM_DROPFILES` with an `HDROP`.
- `WM_DROPFILES`: `take_drop` reads the count and the first path with
  `DragQueryFileW` and always releases the drop with `DragFinish` before
  the application sees anything, so folder rejection, prompts and failures
  never leak it. Both functions are resolved together from `shell32.dll`
  on the first drop (`library_proc`, cached). The window becomes the
  foreground window, then the application receives
  `Event::FilesDropped(Result<Dropped { path, others }, String>)`. `Event`
  loses `Copy` (it keeps `Clone`).
- A missing `shell32` entry point (the drop cannot be released) or an
  unreadable name produces `Err` with a message the editor shows once.

### Application (`rte_app`)

- **File lifecycle** (`editor/lifecycle.rs`). `Editor.path == None` is the
  empty state; the untitled document no longer exists.
  - `files_dropped`: refuses with a notice when the current file has
    unsaved changes, a save running, a close pending or a prompt (prompts
    ignore drops silently); otherwise starts a loader for the dropped file
    (missing files are not created) and keeps it as `PendingOpen`. A newer
    drop replaces and stops it.
  - `drain_pending`: on the pending loader's first event, a
    `LoadEvent::Rejected` (folder, missing) shows its reason and changes
    nothing. Otherwise the refusal check runs again (edits made while it
    loaded keep the file); then `reset_document(Some(path))` and the
    pending load becomes the document's load.
  - `close_file`: in the empty state a notice; otherwise prompt when dirty,
    wait for a running save, then `reset_document(None)`.
  - `reset_document` resets: `Document` (text, history, saved, selections,
    locks with `user` back to `--read-only`, `no_file`), `info`, `baseline`,
    load status and load (dropping it stops the loader), scroll, language
    and choice, `Syntax` (dropping it retires its trees on its thread),
    popups, prompt, notice, chords, cursor history, column state,
    expansions, symbol list, bracket and caret caches, trace marks. The
    early syntax slot is closed explicitly. `reset_search` closes the
    search box, drops its scope (the query and options stay), bumps its
    generation and the operation generation, cancels both lanes and frees
    the count checkpoints.
- **Empty lock.** `Locks.no_file` refuses edits and saves with `no file is
  open`. `works_without_file(action)` decides the other commands; the
  palette dims the rest; typing, editing keys and clicks do nothing.
- **Loader.** `loader::spawn(path, wake, on_done, create_missing)` returns
  a `Load` (events and a stop flag; dropping it sets the flag). The loader
  checks the flag before each block read, after it, and before the
  `on_done` hook and `Done`; a stopped loader publishes nothing more (an
  event already being sent is dropped with the receiver). A folder (Windows
  reports it as not found or access denied) and, for drops, a missing file
  give `LoadEvent::Rejected` before any other event; the check runs only
  when opening fails, so a normal open does no extra I/O. A folder named on
  the command line starts the empty state with the notice.
- **Early syntax start** stays for the command-line file only. A dropped
  file starts its syntax thread after `Done`; `rte_syntax::warm` compiles
  its query while it is read.
- **Close and quit.** `Prompt::DirtyClose(Exit)` and `closing:
  Option<Exit>` carry what follows: `Exit::CloseFile` (`File: Close`) ends
  in the empty state, `Exit::Quit` (`Application: Quit`, the window close
  request) exits. `Ctrl+Shift+W` is a second key of Quit.
- **Window title** follows the path (`Royi Text Editor` alone in the empty
  state) and the unsaved marker.

### View (`rte_view`)

- `Frame` gets `empty: Option<&str>`: when set, the body is the background
  plus the hint centered in the text area; no line numbers, rows,
  selections, caret or highlights are drawn. The search thread gets
  `release()`, and a count that finds itself cancelled right after
  recording checkpoints clears them.

### Threading, persistence, failure

- No new threads. The loader of a dropped file is the existing loader
  thread. Nothing new is written to disk.
- A dropped file that cannot be read becomes the existing read-error state
  (read-only, `Read error: ...`); Close returns to the empty state.

## Alternatives

- **OLE drag and drop (`RegisterDragDrop`, `IDropTarget`).** Rejected: needs
  `OleInitialize` (COM) before a drag can arrive, adding milliseconds to
  startup, for hover feedback only.
- **Keep the untitled document and add Save As.** Rejected by the owner:
  one file per session, launched from a file manager.
- **Dropping onto an open file spawns a new instance.** Rejected by the
  owner in favor of replacing a clean file, and by the single-session
  principle.
- **Close quits (as today).** Rejected: the empty state is the natural
  place to drop the next file; `Alt+F4` quits.
- **Close in the empty state quits (VS Code).** Rejected: two `Ctrl+W`
  presses would quit by accident.
- **Opening all dropped files in new instances.** Rejected by the owner for
  simplicity.

## Runtime Effects

- **Startup with a path:** unchanged (one extra window style bit).
- **Startup without a path:** no `Document` edits possible, no syntax work,
  a frame of one fill and one hint line; equal or faster than today.
- **Drop:** loads `shell32.dll` once (about 1-3 ms, after the first frame,
  only when a file is dropped), then the normal load path.
- **Memory:** closing a file frees its text, history, syntax trees (on the
  syntax thread) and search checkpoints; the empty state holds no document
  data. Glyph caches stay.
- **Idle:** no timers or wake-ups; the empty state has no workers.
- **Cancellation:** the loader stops at the next block (at most one 16 MiB
  read); stale syntax and search replies are rejected by their existing
  sequence and generation checks.
- **Disabled path:** without drops, the feature costs one style bit.

## Tests

Unit and headless editor (`cargo test -p rte_app`, `cargo test -p rte_view`,
`cargo test -p rte_platform`):

- Starting without a path: empty state, edits and saves refused, allowed
  commands run, others dimmed and refused, Find does not open, Close shows
  the notice and keeps running, Quit closes, status and title texts.
- Drop in the empty state: the file loads and is editable; the window title
  follows.
- Drop on a clean file: replaced; undo history, cursor history, search box
  and language reset; the syntax thread restarts for the new language.
- Drop on a dirty file and during a save: refused, text unchanged, notice.
- Several files: the first opens, the notice counts the rest.
- A folder: notice, state unchanged.
- Close: clean → empty; dirty → prompt; Save → empty after the save;
  Don't Save → empty; Esc → stays.
- `--read-only` then a drop: read-only; a toggle is reset on the next file.
- `--syntax json` then a drop of a `.py`: Python.
- Stale results: a syntax view and a search reply of the closed file are
  ignored after a drop.
- Loader: the stop flag ends a multi-block load between blocks with no
  further events; a dropped `Load` stops its thread.
- View: the empty frame has no gutter, caret or selection fills and draws
  the hint inside the text area at small sizes.

Performance (focused, not the full suite):

- `cargo xtask bench-perf --only L01,L26,L04`: L26 (added to Benchmark v1,
  owner's request) starts without a file next to L01 (an empty file).
- Startup without a file, A/B against 0.4.1 in one session (30 interleaved
  launches, `--trace-startup --exit-after-first-frame`): first frame and
  peak private memory within noise or better.
- `cargo xtask bench-perf --only L01,L04`: unchanged.
- `cargo xtask check-imports`: same DLL list.

Manual:

- Drag from Explorer onto the empty window, a clean file, a dirty file;
  several files; a folder; a file that cannot be read.
- Close a 256 MiB file while it loads, then drop another file.
- An elevated window rejects the drop (expected).

## Implementation Steps

1. `rte_platform`: `WS_EX_ACCEPTFILES`, `WM_DROPFILES`, lazy `shell32`,
   `Event::FilesDropped`.
2. `loader`: `Load` with a stop flag; cancellation test.
3. `rte_view`: empty frame; view tests.
4. `rte_app`: lifecycle (`open`, `close`, reset), `Locks.no_file`, the
   allow-list, drop rules, Close to the empty state; untitled code removed;
   editor tests.
5. Startup A/B, focused benchmarks, `check-imports`, manual drop checks.
6. Documentation: README, docs pages, Architecture, CHANGELOG, TODO.

## Acceptance Criteria

- Started without a file, the window shows the empty state and nothing can
  be edited; closing never asks.
- Dropping a file opens it; dropping onto a clean file replaces it; onto a
  file with unsaved changes it is refused with a notice.
- File: Close returns to the empty state (asking first when there are
  unsaved changes); `Alt+F4` and `Ctrl+Shift+W` quit.
- No untitled document, at-risk marker or discard prompt remains.
- Startup without a file is not slower than 0.4.1; startup with a file and
  the import list are unchanged.
- Focused tests pass; documentation and changelog describe the behavior.

## Reviewers

### 2026_10_25 - Claude Opus 5.5

- Role: Reviewer
- Activity: Design
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Designed from the owner's decisions: empty state instead of the
  untitled document; drop replaces a clean file and is refused with a
  notice when there are unsaved changes; the first of several files opens;
  Close returns to the empty state and `Alt+F4` quits; no elevated-window
  drops; no `Ctrl+O` in this task. Designer's choices to confirm: Close in
  the empty state does nothing, `--read-only` applies to every file and
  `--syntax` only to the command-line file, the command allow-list.
  Awaiting review.

### 2026_10_25 - GPT-6.1 Sol

- Role: Reviewer
- Activity: Review
- Agent: AI assistant using Copilot SDK in VS Code
- Model: GPT-6.1 Sol
- Effort: High
- Context Window: 1M
- Outcome: Direction supported; changes requested for ESD1-ESD4 before
  implementation. Reviewed against the current editor, loader, search,
  syntax and Win32 ownership paths. Source inspection only; no application
  changes or runtime validation. The designer's three owner decisions
  remain unconfirmed.

#### ESD1 [P1] Search generations must outlive each document

The plan discards the search box but keeps its threads. Today
[`FindBox::new`](../crates/rte_app/src/editor/find.rs) starts its generation
at 1, while [`SearchThread::send` and `cancel`](../crates/rte_app/src/search.rs)
only raise the worker's generation with `fetch_max`. After searching file A
at generation 100, closing it and constructing a new box for B produces
jobs below the worker's generation. Counts, navigation and regex highlights
are silently discarded until B catches up. Incrementing A's discarded box
does not fix this.

Give both search lanes process-lifetime generation ownership, outside the
per-document reset; seed new requests above the cancellation generation.
Specify cancellation of both lanes, stale-reply draining and checkpoint
release, rather than only closing the widget. Test A with many query
changes and an in-flight count/replace, then close or replace it with B:
B's first search and occurrence operation must complete, A's replies must
not apply, and old snapshots must be released after cancellation settles.

#### ESD2 [P2] A pre-read stop check does not suppress late completion work

Cancellation may happen during the last block read. The current
[`loader`](../crates/rte_app/src/loader.rs) then builds the final snapshot,
calls `on_done` and sends `Done` without another block iteration. Checking
only before reads therefore does not establish "no further events".
Dropping the receiver protects the next document, but does not prevent the
command-line file's early syntax hook from starting work after Close.

Check cancellation after reads and before publication/finalization hooks.
Explicitly close the shared
[`EarlySyntax` slot](../crates/rte_app/src/syntax.rs) during the lifecycle
reset; dropping an `Arc` is not the same as setting its `closed` flag.
State the allowed race for an event already being published and rely on the
discarded receiver for that case. Test cancellation while the final read
is gated, immediately before the hook, and after an early worker exists.
The abandoned load must start no new syntax worker and publish no usable
result.

#### ESD3 [P2] Folder rejection needs a boundary before document reset

Scope requires a folder drop to leave the current file unchanged, but
`open(path)` resets it before the loader runs. The current
[`load events`](../crates/rte_app/src/loader.rs) distinguish success and
read failure, not a rejected directory. Following the reset-first path
would replace the current file with a read-error document. Adding
`path.is_dir()` to the window handler instead puts filesystem I/O on the UI
thread, including slow network paths.

Define directory classification on the loader side, before committing the
document switch. Preserve the old state on directory rejection; recheck
the current document and dirty/save/prompt conditions before accepting a
delayed classification. Keep ordinary file read failures as scoped. Test a
folder drop on an open file with history and scroll state, plus a delayed
classification while the user edits or closes that file.

#### ESD4 [P2] Native drop failures must not be silent

The platform design explicitly ignores a failed DLL load, symbol lookup or
path extraction. A user then cannot distinguish a failed feature from an
ignored drop, and the document specifies no native failure check.

Return a visible drop error without changing the current document. Define
ownership of the `HDROP` and cleanup on every path, including folder
rejection, ignored prompts and failed extraction. Resolve the required
entry points together, with a usable cleanup path before extracting the
file. Add injected loader/query failures and assert one notification,
unchanged editor state and exactly-once cleanup when cleanup is available.
The intentional rejection of elevated-window drops remains excluded.

#### Review Validation

- Inspected the proposed transitions against the current source; findings
  describe design gaps, not failures reproduced in an implementation.
- No tests, builds, benchmarks or native drop experiments were run.
- Only this reviewer record was appended. The design, pending status and
  existing records are unchanged.

### 2026_10_25 - Claude Opus 5.5

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Added the single-session constraint (Architecture Principle 8);
  the design already opens, replaces and closes files inside the session.
  ESD1-ESD4 of the GPT-6.1 Sol review are not yet addressed.

### 2026_10_25 - Claude Opus 5.5

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Accepted ESD1-ESD4 of the GPT-6.1 Sol review and revised the
  design (Design section). The owner confirmed `Ctrl+Shift+W` as Quit,
  approved implementation and added benchmark L26 (Start empty session).

#### Response ESD1: Accepted

The search box is kept across files; `reset_search` closes it, drops its
scope, bumps its generation and the operation generation, cancels both
lanes and frees the count checkpoints (`SearchThread::release`). A count
cancelled just after recording checkpoints clears them itself. Test:
`a_new_file_searches_after_a_busy_search_of_the_old_one` (31 query changes
on 200 KB, replace, no checkpoints left, the new file's count completes).

#### Response ESD2: Accepted

The loader checks its stop flag before each read, after it, and before the
`on_done` hook and `Done`; `reset_document` closes the early syntax slot.
A send already under way is dropped with its receiver. Tests:
`a_stopped_load_publishes_nothing_more_and_runs_no_hook` (stopped before
the start and after the first prefix: no further event, no hook),
`dropping_a_load_stops_its_loader`.

#### Response ESD3: Accepted

Classification is on the loader thread (`LoadEvent::Rejected`, checked
only when opening fails, so no extra I/O on the normal path). A dropped
file stays pending until its first event; the current file is reset only
then, after rechecking unsaved changes, saves, closes and prompts. Tests:
`folders_failed_drops_and_prompts_leave_the_file_as_it_is` (scroll and
selections kept), `drops_onto_unsaved_changes_or_a_running_save_are_refused`
(edit while the dropped file loads keeps the file).

#### Response ESD4: Accepted

`take_drop` releases the drop exactly once whenever `DragFinish` is
available, before the application sees it, and reports failures as
`Event::FilesDropped(Err(..))`; the editor shows one notice and changes
nothing. Tests: `drops_name_their_first_file_and_are_released_once`
(injected API: success, unreadable name, missing shell32) and the failed
drop case of the folder test.

## Implementer

### 2026_10_25 - Claude Opus 5.5

- Role: Implementer
- Activity: Implementation
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Implemented as designed: empty state, drag and drop, Close to
  the empty state, `Ctrl+Shift+W` Quit, loader cancellation and rejection,
  per-file reset, untitled document removed, L26 benchmark, documentation.
  Focused tests, formatting and Clippy pass; the startup A/B against 0.4.1
  could not be completed (see Validation Results).

## Validation Results

| Command or check                                                                                | Outcome                                                        |
| ----------------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| `cargo fmt --all --check`                                                                       | Clean                                                          |
| `cargo clippy -p rte_platform -p rte_view -p rte_render -p rte_app -p xtask --all-targets -- -D warnings` | Clean                                                  |
| `cargo test -p rte_platform -p rte_view -p rte_render -p rte_app -p xtask`                      | 5 (1 ignored), 29, 8, 136 (1 ignored), 10 passed               |
| `cargo xtask bench-perf --only L01,L26,L04`                                                     | Pass; medians below                                            |

| ID  | Test                  | First paint [ms] | First frame [ms] | Peak private [MiB] |
| --- | --------------------- | ---------------- | ---------------- | ------------------ |
| L01 | Empty file            | 24.5             | 38.3             | 5.0                |
| L04 | Python, 64 KiB        | 24.0             | 40.8             | 11.1               |
| L26 | Start empty session   | 23.0             | 25.4             | 4.9                |

Local machine (Intel64 Family 6 Model 183), not the reference machine; one
run. Starting without a file paints no later and holds less memory than an
empty file.

Not run or not completed:

- Startup A/B against 0.4.1 without a file: v0.4.1 was built from its tag,
  but the current release executable was replaced on disk by a 22-byte text
  file ("WAS VIRUS!!! DESTROYED") before the comparison ran, apparently by
  a security tool on this machine. The comparison and `cargo xtask
  check-imports` (which reads that executable) were not run; the import list
  is unchanged by design (`shell32` is resolved with `LoadLibraryW`).
- Manual drag and drop from Explorer (empty, clean, dirty, several files,
  a folder, an elevated window), and closing a 256 MiB file while it loads.
- The full test and performance suites (not requested).

## Implementation Review

### 2026_10_25 - GPT-6.1 Sol

- Role: Reviewer
- Activity: Review
- Agent: AI assistant using Copilot SDK in VS Code
- Model: GPT-6.1 Sol
- Effort: High
- Context Window: 1M
- Outcome: Changes requested for ESI1. The focused tests, Clippy and
  formatting pass; an isolated headless assertion reproduces syntax work
  without a file. Startup A/B, release imports and native drag-and-drop
  checks remain unverified. No application fixes authored; historical
  records and the task's location are preserved.

### ESI1 [P2] An unused CLI language can start syntax in the empty state

Locations:
[`choose_initial_language`](../crates/rte_app/src/editor.rs#L414),
[`restart_syntax`](../crates/rte_app/src/editor.rs#L448), and
[startup query warm-up](../crates/rte_app/src/main.rs#L167).

Start without a file using `--syntax json`. The constructor still records
JSON as the document language. Toggle Syntax Highlighting off, then on:
`restart_syntax` checks the language, highlighting flag and load status,
but not whether a file exists. It starts a syntax worker on the empty
snapshot while `no_file()` remains true. Separately, startup already warms
the override's query without a path, because `chosen` bypasses the
filename-detection branch.

This violates the empty state's no-syntax-work contract and the rule that
the override belongs to the command-line file. It is unnecessary parsing,
query allocation and a hidden worker, not a reproduced text-loss defect.
No startup-latency regression is claimed from this probe.

Apply the CLI language choice only to an actual command-line file. Gate
both startup warm-up and parser creation on a file being open. Keep the
empty state's highlighting toggle as a preference for the next file.
Add regression coverage for no path plus a known `--syntax` ID, including
off/on toggles and a subsequent drop whose language is detected normally.

### Review Validation

| Command | Result |
| ------- | ------ |
| `cargo test -p rte_app -p rte_platform -p rte_view -p rte_render -p xtask --locked` | 188 passed, 2 ignored |
| `cargo clippy -p rte_platform -p rte_view -p rte_render -p rte_app -p xtask --all-targets --locked -- -D warnings` | Pass |
| `cargo fmt --all --check` | Pass |
| `cargo test -p rte_app --no-run --locked --message-format=json` | Pass; exact library and build metadata for the isolated probe |
| Isolated harness, `review_empty_ --nocapture --test-threads=1` | 2 controls passed; ESI1 assertion failed |

The stdin-built harness kept the production bodies unchanged, adjusted
module/include paths and added three tests in memory. Existing tests were
filtered out. Reproduction:

1. Construct an editor with no path and `Settings.syntax = Some("json")`.
2. Deliver `Shown`, then perform `ToggleSyntax` twice.
3. Assert that no file is open and no syntax worker exists. The second
   assertion fails.

Controls pass: the default empty session stays parser-free after the same
toggles, and closing a loading file closes its early-syntax slot so a late
loader hook starts no worker. The failed assertion is evidence of the
defect, not a passing regression check. The disposable executable was
removed; no repository test or application source was edited.

The initial harness compile needed its relative manifest include adjusted
for stdin. The GUI-subsystem executable also required an explicit process
wait; its initially printed native exit code is not validation evidence.
The test report above is the decisive result.

Not run:

- Full workspace tests and the full performance suite (not requested).
- Startup A/B and release import validation: the current release artifact
  is 22 bytes and lacks an `MZ` header. The cause was not independently
  verified; it was not executed or bypassed.
- Native Explorer drops, elevated-window behavior, and the 256 MiB
  close-during-load smoke check. Headless/fake-API tests do not certify
  these paths.

## Implementation Review Response

### 2026_10_25 - Claude Opus 5.5

- Role: Implementer
- Activity: Implementation
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Accepted and fixed ESI1 with a regression test; focused tests,
  Clippy and formatting pass.

#### Response ESI1: Accepted

- `Editor::new` applies `--syntax` only when a file is named on the command
  line; without one the language stays unset.
- `restart_syntax` returns in the empty state before any parser starts,
  whatever the language (defense in depth; Change Language Mode is refused
  there).
- `main` warms a highlight query only with a command-line path (the
  `--syntax` language, or the one its name suggests).
- The highlighting toggle stays a session preference for the next file.

Test `a_syntax_choice_without_a_file_starts_no_syntax_work`: no path with
`--syntax json`, `Shown`, Toggle Syntax Highlighting off and on: no file,
no syntax thread, no language; a dropped `.py` is then detected as Python
and highlighted. With both guards removed it fails at its first assertion.
The startup warm-up in `main` has no unit test (it runs before any
window); the condition is a direct `if let Some(path)`.

While adding it, three drop tests shared one temporary folder name and
could delete each other's file when run in parallel; `TempFile` now uses a
unique folder per call.

#### Response Validation

| Command                                                            | Result                                     |
| ------------------------------------------------------------------ | ------------------------------------------ |
| `cargo test -p rte_app a_syntax_choice_without_a_file`, guards removed | Fails at `--syntax waits for a command-line file` |
| `cargo test -p rte_app` (3 runs)                                   | 137 passed, 1 ignored, each run            |
| `cargo clippy -p rte_app --all-targets -- -D warnings`             | Clean                                      |
| `cargo fmt --all --check`                                          | Clean                                      |

Not run: as before, the startup A/B, `check-imports` and native drops
(the release executable is still unusable on this machine).
