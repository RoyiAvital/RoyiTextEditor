# File System 001

## Task

Notice when another program (an agent, a formatter, `git checkout`) changes
the open file on disk, also while the editor has the focus, and let the user
choose: reload the disk version, or save the editor's version over it.

A simple first step before the 3-way merge of
[Data Safety 001](../Plan/DataSafety001.md).

## Scope

### Included

- **Detection** while a file is open, focused or not: a folder change signal
  from Windows, then a comparison of the file with its baseline.
- **One prompt** for every outside change, with or without unsaved edits:
  `<name> changed on disk.   R: Reload   S: Save Current`. Only these two
  answers; other keys are ignored until one is chosen.
- **Reload** reads the disk version and starts a fresh undo history, as
  opening the file does.
- **Save Current** writes the editor's text over the disk version (the
  current Overwrite path).
- The same prompt replaces the save-time conflict prompt
  (`O: Overwrite   Esc: Cancel`).
- **Deleted or moved away:** a status line notice; the next save recreates
  the file (as now).

### Excluded

- Merging (Data Safety 001), the edit journal and crash recovery.
- Silent reload of clean documents (VS Code does it; the owner chose one
  prompt for every change).
- A Later / `Esc` answer, a setting to turn detection off, polling.
- Content hashing on detection: a file rewritten with identical content and
  a new time also prompts.
- Folders without change notifications (some network shares): no
  detection; the check before saving still protects.
- Changes a notification on the file's folder does not report: the target
  of a file symlink in another folder, and renaming or moving the folder
  that holds the file. The check before saving still applies.

### Compatibility Constraints

- Nothing before the first frame; no periodic work (DEVELOPMENT.md, Design
  Choices).
- The save sequence of [Architecture](../Plan/Architecture.md#saving) is unchanged;
  only its conflict prompt changes keys (`O` becomes `S`, `R` is added,
  `Esc` is removed).
- No new dependency. One compile-time `windows-sys` feature,
  `Win32_Security`, for the `CreateEventW` binding (its parameter type);
  no new DLL import (`kernel32`).
- Single session: the watcher observes the file only, never other sessions.

## Design

### Watcher (`rte_platform`, `rte_app/src/watcher.rs`)

- **Platform** (`rte_platform::files`):
  - `FolderWatch` owns the notification handle of
    `FindFirstChangeNotificationW` on a folder (not recursive;
    `FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_SIZE |
    FILE_NOTIFY_CHANGE_LAST_WRITE`). Only the watcher thread creates,
    waits on and closes it (`FindCloseChangeNotification`).
  - `Poke` owns an auto-reset event (`CreateEventW`), closed when its last
    owner drops. The editor creates it, with a stop flag, before spawning
    the thread and shares both with it (`Arc`), so the event outlives every
    wait and every poke.
  - `FolderWatch::wait(&Poke)` blocks in `WaitForMultipleObjects` and
    returns `Changed`, `Poked` or an error; after `Changed`,
    `FindNextChangeNotification` re-arms the watch (its failure is an
    error).
- **Thread** (one per open file, 64 KiB stack) owns all of its I/O: it
  resolves the path with `std::path::absolute` (against the working
  directory, which the process never changes; the file need not exist),
  registers the watch on the parent folder and reads stamps through
  [`FileOps::stamp`](../crates/rte_app/src/save.rs). It uses the watch
  through a small trait, so tests inject stamps, signals and failures.
- **Reports** `{ epoch, stamp: Option<FileStamp> }` (`None`: absent)
  through a channel, plus a wake-up of the window:
  - once after registration;
  - after a folder change, when the file's state differs from the last
    report (unrelated changes cost one stamp read and no wake-up);
  - after every poke, unconditionally (a refresh).

  `epoch` is read from a counter shared with the editor before the stamp
  is read. A report is the latest observable state: writes between two
  reads are not distinguished.
- **Errors:**
  - Registration fails (no folder notifications, for example on some
    network shares): the thread ends without a report, silently.
  - A stamp read fails (access denied, network error): no report. It is
    neither a deletion nor a change; the next folder change or poke reads
    again.
  - `WaitForMultipleObjects` or the re-arm fails: the thread ends.
  - An ended watcher is started again after the next own save (a reload
    starts a new one anyway); until then the check before saving protects.
- **Lifetime:** started when the file's load is done and the first frame is
  shown (the command-line file), or when a dropped or reloaded file's load
  is done. Also for a new file named on the command line (it may appear).
  Stopped by `reset_document` (close, replace by a drop, reload) with the
  stop flag and a poke. The thread checks the flag after registration and
  after every wake; a poke before its first wait leaves the event signaled,
  so that wait returns at once. It exits after its current stamp read and
  closes the notification handle; the event is freed with its last owner.
  The UI never joins it.

### Disk state (UI thread)

- The editor keeps the shared `epoch` and the last accepted disk state.
  Reports with an older epoch are dropped.
- **Own saves:** when a save succeeds, the editor increments the epoch,
  sets the disk state to the new baseline's stamp and pokes the watcher.
  Reports read before the save finished are dropped whatever their
  delivery order; the refresh is compared with the new baseline, so a
  version written by another program after the commit is still reported.
  A reload restarts the watcher, so no older report can arrive.
- A change is pending when the disk state differs from `baseline.stamp`
  (or is present without a baseline).
- Evaluated when a report is accepted and when a save, a reload, a close or
  another prompt ends; skipped while one of them runs.
- **Absent:** notice `<name> was deleted on disk; saving recreates it`, once
  per disappearance.
- **Present and different:** `Prompt::Changed(disk)` with
  `disk = Baseline { stamp, hash: None }`. The save-time conflict uses the
  same prompt with the save's `disk` baseline (hash included).

### Prompt answers

| Key | Action                                                                                  |
| --- | --------------------------------------------------------------------------------------- |
| `S` | `start_save(Some(disk))`: overwrite the version just seen; a newer one prompts again    |
| `R` | Reload; during a close (`Alt+F4`, `Ctrl+W`) it also cancels the close                   |

`S` follows the save rules (refused under `malformed UTF-16` or a read
error, with the usual notice; the prompt stays). `--read-only` does not
refuse saving, so both answers work there.

Until answered, keys other than `R` and `S` are ignored, `Alt+F4`
included. A close request from Windows (for example from the taskbar) is
not a key: it drops the prompt and runs the usual quit flow (unsaved
changes ask first).

### Reload

- `R` opens the same path again through the open path of a dropped file,
  without its unsaved-changes refusal. The current text stays, read-only
  (the `loading` lock), until the loader's first answer; then
  `reset_document` and the load replace it,
  exactly as opening the file: a fresh history; the file's encoding,
  options and locks; the search box, palettes, previews, spelling and
  syntax state reset; a new watcher. Session settings that opening keeps
  (zoom, highlighting, the search query, favorites) stay.
- When the load is done, the previous scroll position and the primary
  caret come back: one caret, clamped to the new length and snapped with
  `rte_edit::snap_to_boundary` (never inside a UTF-8 sequence, between CR
  and LF, or before a combining mark).
- **Rejected before reading** (deleted meanwhile), or stopped by a close:
  nothing changes and editing comes back; a notice with the reason; the
  disk state is evaluated again. A read error during the load shows as
  when opening the file.

## Alternatives

- **Check on window activation (`WM_ACTIVATE`).** No thread, but no alert
  while the editor has the focus. Rejected by the owner.
- **`ReadDirectoryChangesW`** (planned in Data Safety 001). File names
  avoid stamp reads for unrelated changes, but need a buffer, record parsing,
  rename pairs and I/O cancellation. Rejected for this step: one stamp read
  per folder change is cheap.
- **Polling.** Periodic wake-ups; against the design choices.
- **VS Code (recursive watcher, polling fallback).** More than one file
  needs.
- **Reload as one undo step.** Keeps a way back after an accidental `R`,
  but needs an encoding and lock policy for undo across a reload (FS2) and
  keeps the old text in memory. Rejected by the owner (2026_11_01): too
  many edge cases; Reload starts a fresh history.
- **Hash on detection** to ignore touched files. Up to 64 MiB of reading on
  folder changes; rejected, a needless prompt is harmless.
- **Stop and refresh by `QueueUserAPC`** with an alertable wait. Needs no
  `Win32_Security` feature, but APC delivery is harder to follow and to
  test than an event; rejected.

## Runtime Effects

- **Startup:** none before the first frame. After it, on the watcher
  thread: one path resolution, one `FindFirstChangeNotificationW`, one
  stamp read. The UI thread does no file I/O for this feature.
- **Idle:** the thread blocks in `WaitForMultipleObjects`; no timers, no
  wake-ups without folder changes.
- **Folder changes:** one stamp read (open, `GetFileInformationByHandle`)
  on the thread per signal; the window wakes only when the file's stamp
  changed. Busy folders (build output, logs) cost one stamp read per signal.
- **Saves:** the temporary and backup siblings signal the folder; reports
  read before the save finished are dropped by their epoch, and one
  refresh follows.
- **Reload:** the cost of opening the file (read, full syntax parse). Old and
  new text coexist while loading; the old text and the history are freed
  when the reload completes.
- **Memory:** one thread stack (64 KiB), the notification handle and one
  event; no buffers.
- **Cancellation:** a stop flag and a poke end the wait; a stamp read in
  progress finishes and its report is dropped with the channel.
- **Disabled path:** empty state, or a folder without notifications: no
  thread, nothing runs.

## Tests

- **Platform:** `FolderWatch` signals a file write and a rename in a
  temporary folder; a poke ends a blocked `wait()`.
- **Watcher thread** (temporary folder, real files): the initial report;
  a write, a temp-then-rename replacement and a deletion are reported; an
  unrelated file in the folder is not; stopping ends the thread (joined in
  the test). A stop before registration completes ends it before its first
  wait; a poke after the thread ended is harmless.
- **Paths:** a bare file name, a relative subfolder, an absolute path and
  a not-yet-created file register on the right folder.
- **Runtime errors** (injected): a stamp error, then success, gives no
  deletion notice and then the change; a failing wait or re-arm ends the
  thread, and the next own save starts a new one.
- **Busy folder:** 1,000 writes to another file give no report and no
  wake-up; stopping with changes pending ends the thread promptly; 100
  start and stop cycles leave the process handle count unchanged
  (`GetProcessHandleCount`).
- **Editor, headless** (`Ctx::headless`, `FakeFiles`, reports injected
  through the channel):
  - A differing stamp shows the prompt; other keys, `Alt+F4` included, are
    ignored; a window close request runs the quit flow.
  - `S` saves with the reported baseline; a newer disk version at save
    time prompts again.
  - `R` reloads (real temporary file) with unsaved edits: the disk text,
    clean, empty history, the file's encoding and locks, a new watcher.
  - Reload caret and scroll: a caret inside a character that became
    multibyte, between CR and LF, past the end of a shorter file; typing
    then keeps valid UTF-8.
  - Reload with the search box, a palette or spelling navigation open:
    they reset as when opening a file. A file deleted before the reload
    reads: nothing changes.
  - Own save with a stale report delivered before and after its
    completion: no prompt; another program's write after the commit: the
    prompt.
  - A report during a save is deferred; the own save's stamp never prompts.
  - Deleted: one notice; reappearing: the prompt.
  - During a dirty close: `S` saves and exits, `R` cancels the close.
  - `--read-only`: both answers; malformed UTF-16: `S` refused, `R` works.
  - The save conflict shows the new prompt (existing conflict tests updated).
- **Performance:** `cargo xtask bench-perf --only L01,L09,L26,E01`,
  alternating with the current build (same session): equal within noise.
- **Manual:** change the file from another editor while the window has the
  focus and while it does not; an agent writing several times in a row;
  `git checkout`; a network share without notifications.

Commands:

```powershell
cargo test -p rte_platform files --locked
cargo test -p rte_app --bin RoyiTextEditor --locked -- watcher disk_change reload save
cargo clippy -p rte_platform -p rte_app --all-targets --locked -- -D warnings
```

## Implementation Steps

1. `FolderWatch` and `Poke` in `rte_platform::files` (`Win32_Security`
   feature), with tests.
2. `rte_app/src/watcher.rs`: thread, path resolution, epoch, initial
   report, deduplication, refresh, error handling, stop; tests with a
   temporary folder and injected failures.
3. Reload through the open path of a drop (no unsaved-changes refusal),
   then the caret and scroll restored; tests.
4. Editor: watcher lifecycle, disk state evaluation, `Prompt::Changed`
   (replacing `Prompt::Conflict`), `R` / `S`, reload, deleted notice;
   headless tests.
5. Architecture (Threading, Saving: External changes, Runtime Effects: no
   file watchers), README, `docs/src/files-and-saving.md`, CHANGELOG.
6. Focused benchmarks and manual checks.

## Acceptance Criteria

- A change by another program shows the prompt without any key press,
  whether or not the editor has the focus.
- `R` shows the disk version, clean, with a fresh undo history. Carets are
  always on valid positions.
- `S` writes the editor's text; a version not yet seen is never
  overwritten without the prompt.
- The editor's own saves and reloads never prompt.
- Files opened by a relative path are watched.
- A deleted file gives one notice and the next save recreates it.
- Startup and frame times are unchanged; no thread runs without a file and
  nothing wakes while the folder is unchanged.

## Reviewers

### 2026_11_01 - Claude Opus 5.5

- Role: Reviewer
- Activity: Design
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Designed at the owner's request as the simple first step before
  Data Safety 001's merge: a folder change signal (alerts while focused,
  no polling), one prompt with only Reload and Save mine for every change,
  also at save time. Open for the owner: Reload as one undo step
  (recommended) or a fresh history. Awaiting review.

### 2026_11_01 - Claude Opus 5.5

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: Medium
- Context Window: 872K
- Outcome: Owner's decision: undo after Reload restores the last state
  (Reload is one undo step). No open decisions remain; awaiting approval
  for implementation.

### 2026_11_01 - GPT-6 Astra

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: GPT-6 Astra
- Effort: Extra High
- Context Window: 1M
- Outcome: Changes requested for FS1-FS5 before implementation. The event-driven
  approach fits the scope; the Reload / Save mine-only prompt, stamp-only
  detection and no-polling decisions are retained. This record reviews the
  design and does not change it or authorize implementation.

#### FS1 [P1] Reload must restore valid caret boundaries

Location: [Reload](#reload).
Clamping an old byte offset to the new length does not make it a valid
position in the new text. A caret at byte 1 followed by reload of U+00E9
(`C3 A9`) remains between its two bytes; inserting `X` produces invalid UTF-8
(`C3 58 A9`). The existing `Document::load` only clamps, so copying that
behavior is insufficient. Snap both ends to the editor's valid boundaries,
then normalize selections; preserve selection direction where possible.
Add reload-and-type, Backspace, undo and redo cases for multibyte characters,
CRLF, combining marks, shorter files and multiple carets.

#### FS2 [P1] Reload undo needs an encoding and lock policy

Location: [Reload](#reload), with
[Document](../crates/rte_app/src/document.rs) and
[History](../crates/rte_edit/src/history.rs).
Current history steps restore only text and selections, while `Document::undo`
refuses every edit lock. Reloading malformed UTF-16 therefore sets the encoding
lock and prevents Ctrl+Z from restoring the previous valid text, contradicting
the promised escape from an accidental Reload. Encoding, BOM and edit options
also remain those of the new file after an ordinary text-only undo; restored
invalid UTF-8 can no longer be saved when the reload changed encoding to UTF-16.
Define what reload undo/redo restores, and how restoring a valid pre-reload
state works without enabling ordinary edits/saves of malformed input. Keep the
disk baseline tied to the reloaded disk version. Test valid/malformed reloads,
UTF-8/UTF-16 transitions, BOM changes and failed loads with prior unsaved edits.

#### FS3 [P2] Cached watcher state can prompt after an own save

Location: [Disk state](#disk-state-ui-thread).
Suppose the watcher last reported A, then the application's save commits B.
When save completion updates the baseline to B, immediately evaluating the
cached A reports an outside change before the watcher has delivered B.
Deferring evaluation while saving does not establish freshness afterwards;
reload completion has the same issue. This needs only one ordinary user
operation, not rapid conflicting actions. Define observation ordering or a
post-operation refresh on the existing worker, and reject older reports before
comparing against the new baseline. No timer is needed. Inject both delivery
orders for own saves/reloads and confirm that a genuinely newer external
version is still reported.

#### FS4 [P2] Watch registration must handle relative file paths

Location: [Watcher](#watcher-rte_platform-rte_appsrcwatcherrs).
The editor retains command-line paths as supplied. For `notes.txt`, the parent
can be empty; for `sub/notes.txt`, it is relative.
[FindFirstChangeNotificationW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-findfirstchangenotificationw)
requires a full, non-empty directory path. Without an explicit resolution step,
normal relative-path launches silently lose detection under the proposed
failure policy. Resolve the watch path against the launch working directory
without requiring the target file to exist. Test a bare filename, a relative
subdirectory, an absolute path and a not-yet-created named file.

#### FS5 [P2] Runtime watch and stamp errors are not represented

Location: [Watcher](#watcher-rte_platform-rte_appsrcwatcherrs).
`FileOps::stamp` returns `Result<Option<FileStamp>>`, but the proposed channel
only represents present/absent. Define access/metadata errors separately from
deletion, plus `WaitForMultipleObjects` and `FindNextChangeNotification` failure
behavior. Otherwise an error can become a false deletion, a silently stale
state or a retry loop on a failed handle. Preserve the last known baseline and
choose an explicit unavailable/terminal state and event-driven recovery policy.
Test a stamp error followed by success, a re-arm failure and stopping while
folder changes are pending. Keep the intentional silent initial-registration
failure policy separate from these runtime cases.

#### Performance and Implementation Checks

- Make watch registration and stamp reads worker-owned; moving them after the
  first frame alone does not prevent UI stalls on slow storage.
- Add one busy-folder check: unrelated writes produce no window wake-up and
  stopping remains prompt. The proposed load/typing benchmarks do not exercise
  this new work path. Measure thread/handle cleanup across close and drop.
- Confirm the stop-event binding under the enabled `windows-sys` features;
  `CreateEventW` takes `SECURITY_ATTRIBUTES`, while the platform manifest does
  not enable `Win32_Security`. Resolve that before promising no feature changes.
- Define notification coalescing as latest observable disk state, not a promise
  to distinguish every intermediate write. This need not introduce protection
  against user actions faster than a person can act.

#### Review Validation

Reviewed the plan against current loading, saving, document locks, history,
file lifecycle, platform stamps and DEVELOPMENT design choices. Checked the
Win32 notification contracts. Small in-memory PowerShell models reproduced
FS1's byte-boundary example and FS3's stale-cache predicate; these are design
counterexamples, not application regression tests. Tests listed per finding
are proposed implementation checks.

Attempted focused commands:

```powershell
cargo test -p rte_platform files --locked
cargo test -p rte_app --bin RoyiTextEditor --locked -- document save
```

Both were blocked before execution: `cargo` was unavailable on PATH;
`Get-Command` also found neither `rustc` nor `rustup`. No tests, builds,
benchmarks, native watcher probes or dependency installations ran. The task
append is checked for preservation of its original bytes, valid local links,
reviewer metadata and whitespace. No application code, index, changelog or
earlier reviewer records were changed.

### 2026_11_01 - Claude Opus 5.5

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: FS1-FS5 and the implementation checks accepted; the design is
  revised (Watcher, Disk state, Reload, Alternatives, Runtime Effects,
  Tests, Implementation Steps, Acceptance Criteria). The owner's decisions
  stay: two answers only, stamp-only detection, no polling, Reload
  undoable. One exception: a reload that changes the encoding starts a
  fresh history. Awaiting owner approval for implementation.

#### Response FS1: Accepted

Reload clamps each selection end, snaps it with the existing
`rte_edit::snap_to_boundary` (UTF-8 sequences, CRLF, combining marks) and
normalizes; the direction is kept unless a selection collapses. Tests cover
the listed cases with typing, `Backspace`, undo and redo.

#### Response FS2: Accepted

An undo step never crosses an encoding: a reload whose encoding or BOM
differs, or with malformed UTF-16 before or after, starts a fresh history
with a notice and adopts the file's encoding and locks. Same-encoding reloads stay one
undo step restoring text and selections only, so undo never bypasses a
lock. The baseline stays the reloaded version. A failed reload changes
nothing. Undo steps carrying encodings were rejected as a `History` and
lock-rule change for a rare case.

#### Response FS3: Accepted

Reports carry an epoch read before the stamp. A successful save or a
completed reload increments it, sets the disk state to the new baseline
and pokes the watcher for an unconditional refresh; older reports are
dropped in either delivery order, and a newer outside version still
prompts. No timer.

#### Response FS4: Accepted

The watcher thread resolves the path with `std::path::absolute` (no file
access; the working directory never changes) before registering its
parent folder. Tests: bare name, relative subfolder, absolute path,
not-yet-created file.

#### Response FS5: Accepted

A failed stamp read is not reported: neither deletion nor change, and the
next signal or poke reads again. A failed wait or re-arm ends the thread;
the editor starts a new one after its next own save or reload. The silent
registration failure stays a separate case. Tests inject each failure
through a small watch trait.

#### Response to the implementation checks

- Registration, path resolution and stamp reads run on the watcher thread;
  the UI does no file I/O for the feature.
- A busy-folder test (no report, no wake-up), prompt stop with changes
  pending and a handle-count check over start/stop cycles were added.
- `CreateEventW` needs the `Win32_Security` feature in `windows-sys` 0.61
  (checked in the registry source); the constraint now allows this one
  compile-time feature. The `QueueUserAPC` alternative is recorded.
- Coalescing is defined as the latest observable state.

### 2026_11_01 - Claude Opus 5.5

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: Medium
- Context Window: 872K
- Outcome: Owner's decision, superseding the earlier undo decision: Reload
  starts a fresh history, as opening the file (no edge cases of undo
  across encodings or locks; FS2 no longer applies). Reload, Alternatives,
  Runtime Effects, Tests, Implementation Steps and Acceptance Criteria
  updated; FS1 snapping, FS3-FS5 and the `Win32_Security` feature remain.
  Awaiting owner approval for implementation.

### 2026_11_01 - GPT-6.1 Sol

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: GPT-6.1 Sol
- Effort: High
- Context Window: 1M
- Outcome: Reviewed the revised design after FS1-FS5 responses and the
  owner's fresh-history decision. Two remaining design gaps, FS6-FS7,
  concern handle ownership and notification coverage. Reload integration
  checks are recorded below. The two-answer prompt, stamp-only detection,
  no polling and no UI joins remain unchanged. No implementation authored.

#### FS6 [P2] The poker needs explicit ownership and startup/shutdown ordering

Location: [Watcher](#watcher-rte_platform-rte_appsrcwatcherrs).
`FolderWatch` owns two handles, while a separate `Send` poker is used by
the UI. Define which object keeps the event handle alive after the worker
fails or exits, and how a worker-created poker is published to the UI.
Closing both handles when the worker's watch drops is not enough if a
retained poker still holds only the numeric event handle. The next save,
reload or close can then call `SetEvent` on an invalid or reused handle.
This does not require two rapid user operations: the worker can fail long
before the next action.

Use shared owned event state or another explicitly owned-handle arrangement;
keep the notification handle worker-owned and close it with
`FindCloseChangeNotification`. The event must outlive every waiter and
poker that can use its handle. Microsoft's
[wait contract](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitformultipleobjects)
states that closing a waited handle while the wait is pending has undefined
behavior. Stop remains flag + poke, not UI-side handle closure or a join.

Also define close before registration/readiness finishes: publish or retain
the stop flag before starting work, check it before entering the first wait,
and ensure late initialization cannot leave an orphaned blocked thread.
Tests: stop before readiness; runtime failure followed by a retained poke;
drop the UI handle while a stamp read runs; release all owners and verify
handle cleanup. These are ownership-contract checks, not a request for
session coordination, periodic checks or a handle-reuse stress experiment.

#### FS7 [P2] Directory notification coverage is narrower than the open-file promise

Location: [Scope](#scope) and
[Watcher](#watcher-rte_platform-rte_appsrcwatcherrs).
An absolute path resolves relative-path registration, but it does not
resolve a file symlink's target. A watch on the containing folder does not
report edits to that symlink's target in another folder. Microsoft
[documents this limitation](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-findfirstchangenotificationw),
and also states that the function does not report changes to the watched
directory itself. Renaming or moving that directory can therefore invalidate
the stored path without providing the claimed deletion/move notice.

Choose an explicit v1 policy: support these cases through an appropriate
resolved-target/watch lifecycle, or declare them outside notification
coverage and retain the save-time guard. Do not silently claim that every
change to an open file is detected by one non-recursive parent watch.
Directory symlinks and file symlinks are different cases: the documented
watch follows a symlink used as its directory path, but does not watch every
target of symlink files inside that directory.

Tests/manual checks: a file symlink targeting a different folder, changing
the link target, and moving/renaming the containing folder while the editor
stays open. Keep no polling, no new startup checks and the existing silent
unsupported-filesystem policy; any broader support needs an explicit design
decision rather than an accidental recursive watcher.

#### Reload Integration Checks

Make the successful reload one UI commit of text, clean root, history,
encoding/options/locks, snapped selections and baseline/disk epoch. Reset
the old document edit journals and per-text operation state explicitly:
`Document::load` currently only replaces text/clean state and clamps
selections; the syntax and spelling observers consume separate journals.
Cancel or invalidate pending navigation, replacements, suggestions, palette
targets and line previews before they can act on the reloaded text.
Preserve session settings and session-only ignored/user words as intended;
resetting text caches need not discard the spelling service's overrides.

On failure, restore the previous loading lock and leave the old text,
history, options and baseline intact. Exercise a reload begun while a
search, spelling navigation or palette request is outstanding, and a failed
reload followed by typing/saving. Verify the two-answer modal policy through
both keyboard input and `Event::CloseRequested`, since the current window
close path does not go through `prompt_key`. These are implementation
acceptance checks, not additional confirmed defects in unimplemented code.

#### Review Validation - Sol

- Checked the revised plan against current document locks, save/close
  handling, syntax/spelling journals and platform file stamps. Verified
  the handle and notification contracts in the linked Microsoft Learn
  documentation.
- Original task/reviewer bytes are preserved; appended metadata, links,
  whitespace and nearby production fingerprints are checked after editing.
- No production changes, dependency installs, native watcher experiments,
  application tests or performance measurements were made. This is a
  design-only review; FS6-FS7 require explicit decisions before claiming
  their contracts are implemented.

### 2026_11_01 - Claude Opus 5.5

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: Medium
- Context Window: 872K
- Outcome: FS6 accepted; FS7 decided as an explicit exclusion; the reload
  checks are met by reusing the open path. Decisions follow the owner's
  guidelines (DEVELOPMENT.md, Design Choices): reasonable behavior, no
  protection for edge cases. Awaiting owner approval for implementation.

#### Response FS6: Accepted

The editor creates the `Poke` (an auto-reset event closed with its last
owner) and the stop flag before spawning the thread and shares both with
it, so the event outlives every wait and poke. The notification handle is
the thread's alone, closed with `FindCloseChangeNotification`. The thread
checks the flag after registration and after every wake; a poke before the
first wait leaves the event signaled, so no thread is orphaned. Tests: stop
before registration, a poke after the thread ended, the handle count.

#### Response FS7: Excluded

A file symlink's target in another folder and renaming or moving the
folder that holds the file are outside the notification coverage, stated
under Excluded. The check before saving remains. No resolved-target watch,
no recursive watch, no tests for these cases.

#### Response to the Reload Integration Checks

Reload is now the open path of a dropped file on the same path, without
its unsaved-changes refusal: `reset_document` already resets the history,
journals, search, palettes, previews, spelling and syntax state, and
restarts the watcher (so reloads need no epoch). Nothing changes before
the loader's first answer, so a file gone by then leaves everything as it
was; a read error during the load shows as when opening. Afterwards one
snapped caret and the scroll position are restored. A window close request
during the prompt drops it and runs the usual quit flow; keys other than
`R` and `S`, `Alt+F4` included, are ignored. `Document::reload` is no
longer needed.

## Implementer

### 2026_11_01 - Claude Opus 5.5

- Role: Implementer
- Activity: Implementation
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Implemented as designed. `rte_platform::files`: `Poke`
  (auto-reset event, `Win32_Security` binding feature) and `FolderWatch`
  (`FindFirstChangeNotificationW`, re-armed after each change).
  `rte_app/src/watcher.rs`: the thread resolves the path, registers the
  watch, reports the file's stamp once and then on each change of it or
  poke, tagged with an epoch; stamp errors are skipped, wait failures end
  it. `FileOps::watch` lets tests inject watches; `FakeFiles` has none.
  Editor: `Watching` (idle, running, ended), `Prompt::Changed` replaces
  `Prompt::Conflict` (also at save time), `R` reopens through the drop path
  (`start_open` with a restore of scroll and snapped caret), `S` saves over
  the reported version; own saves bump the epoch and refresh, an ended
  watcher restarts after a save. Per event the editor only compares a few
  fields (`sync_watcher`, `check_disk`). Startup, typing, palette and
  scrolling are unchanged in a same-session A/B; no new DLL import.

## Validation Results

- `cargo test -p rte_platform --offline`: 11 passed, 1 ignored (existing,
  replaces the clipboard); 3 runs. New: `folder_watch_signals_changes_and_pokes`,
  `watches_and_pokes_close_their_handles`.
- `cargo test -p rte_app --bin RoyiTextEditor --offline`: 195 passed,
  1 ignored (existing). New: 6 watcher tests (`writes_replacements_and_deletions_are_reported`,
  `other_files_wake_nothing_and_stopping_is_prompt`,
  `relative_and_missing_files_are_watched_in_their_folder`,
  `a_refresh_drops_older_reports`,
  `stamp_errors_are_skipped_and_failures_end_the_thread`,
  `stopping_before_the_first_wait_and_poking_an_ended_thread_are_safe`) and
  4 editor tests (`disk_change_prompts_reload_or_save_mine`,
  `disk_change_reports_older_than_an_own_save_are_dropped`,
  `disk_change_prompt_and_closing`, `reload_starts_fresh_and_keeps_the_place`);
  `conflict_prompt_overwrites_only_on_request` updated. The watcher and
  editor filters passed 3 more runs.
- `cargo fmt --all --check`; `cargo clippy -p rte_platform -p rte_app
  --all-targets -- -D warnings`: clean.
- `cargo xtask check-imports`: the same DLLs as v0.6.1. Release executable
  +17,920 bytes (34,644,992).
- Same-session A/B against v0.6.1 (`target/ab-061`, alternating):
  `--only L01,L02,L09,L26,U01,E01,S02` 3 rounds, `--only U01,U02,E01,M04`
  4 rounds. Medians:

  | Metric                         | v0.6.1             | This change        |
  | ------------------------------ | ------------------ | ------------------ |
  | L01 First paint [ms]           | 15.9 / 15.0 / 14.6 | 15.3 / 15.0 / 15.0 |
  | L01 Peak private [MiB]         | 5.0 / 5.1 / 5.1    | 5.0 / 5.0 / 5.0    |
  | L09 First paint [ms]           | 16.3 / 16.3 / 16.1 | 16.6 / 16.1 / 16.5 |
  | L26 First paint [ms]           | 15.2 / 15.4 / 14.9 | 15.1 / 14.9 / 14.9 |
  | U01 Key to frame [ms]          | 0.38 - 0.41        | 0.37 - 0.40        |
  | E01 Key to frame [ms]          | 0.41 - 0.47        | 0.41 - 0.42        |
  | M04 Key to frame [ms]          | 0.49 - 0.50        | 0.49 - 0.50        |
  | S02 Key to frame, p95 [ms]     | 0.86 - 0.91        | 0.87 - 0.99        |
  | S02 Missed refreshes [%]       | 0.0                | 0.0                |

  p95 of U01, U02 and E01 has single 2-11 ms outliers in both builds.
- Manual (release build, real notifications): a file rewritten by another
  program while the editor had the focus showed the prompt; `R` showed the
  new text; typing and `Ctrl+S` saved without a prompt.
- Not run: the full workspace test suite and performance suite; changes
  from another editor while unfocused, agent bursts and `git checkout`
  (covered by the same notification path); network shares (no
  notifications, excluded); symlinks and folder moves (excluded).

## Implementation Review - GPT-6.1 Sol

### 2026_11_01 - GPT-6.1 Sol

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: GPT-6.1 Sol
- Effort: High
- Context Window: 1M
- Outcome: Changes requested for FSI1: reload can discard edits accepted
  after the user chose Reload. Existing focused tests and clippy pass;
  one headless assertion reproduces the loss and a later-version Save
  mine control passes. The owner's prompt, fresh-history, stamp-only,
  no-polling and notification-coverage decisions remain unchanged.
  No production fixes authored; historical completion is preserved.

### FSI1 [P1] Reload discards edits accepted after the reload request

- Location: [`Editor::reload`](../crates/rte_app/src/editor.rs#L1304),
  [`start_open`](../crates/rte_app/src/editor/lifecycle.rs#L120), and
  [`drain_pending`](../crates/rte_app/src/editor/lifecycle.rs#L148).
- Trigger: answer `R` in the Changed prompt while the loader's first
  response is delayed, then type into the still-editable old document.
  A slow read or delayed worker can leave this interval open long enough
  for ordinary user input; no fast competing session or write is needed.
- Actual: `R` clears the prompt but does not lock editing. The edit is
  accepted and recorded. When the first reload response arrives,
  `pending.restore.is_some()` bypasses the unsaved-change check, and
  `reset_document` discards the edited root and its history. The probe
  accepts `X` after the request, then finishes with `theirs` and zero undo
  steps. The newly accepted edit cannot be recovered by Undo.
- Expected: the discard decision covers the text at the time of `R`,
  not future edits accepted while its asynchronous load is pending.
  Prevent editing while that reload waits, or preserve/cancel the reload
  if the document changed after the request.
- Suggested fix: set a pending-reload loading/busy state before launching
  the loader and restore it correctly on rejection/cancellation, or carry
  the initiating snapshot and refuse the later commit if it changed. Keep
  the chosen fresh undo history after a successful reload and the existing
  failure policy; no undo-across-encoding feature, polling or UI I/O is
  required.
- Evidence: `fs001_review_edits_after_reload_request_are_not_discarded`
  replays the real `R` command and event/commit paths, with the loader's
  first reply held in a test channel. This models an arbitrarily delayed
  read, not a claim that users act within a few milliseconds.
- Required regression: pending reload followed by typing, paste or undo;
  a normal successful reload with no later edit; and rejection/close while
  the reload waits. Either reject the input while busy or keep accepted
  edits recoverable; do not silently replace them.

### Checked and Correct

- Shared `Poke` ownership outlives the UI and worker references; the
  notification handle is worker-owned and uses its own close function.
  Native handle-count and stop-before-registration checks pass.
- Watch creation follows `Event::Shown`, which the platform dispatches
  after painting, presenting and un-cloaking the first frame. Folder
  registration and stamp reads remain on the worker, without polling.
- Relative and not-yet-created file paths, stamp-read failure, ended
  watchers, deduplication and refresh epochs have passing focused tests.
- Old reports around an own save are rejected. The new headless control
  `fs001_review_save_mine_reprompts_for_a_later_disk_version` also verifies
  that a second outside version observed while the first prompt is open
  is not overwritten by the first `S`; it prompts again before saving.
- The modal `R`/`S` behavior, Alt+F4 key suppression, Windows close-request
  quit flow, deleted notice and caret snapping pass the existing checks.
- File symlink targets elsewhere and containing-folder changes are explicit
  exclusions, not implementation failures. Fresh history and one restored
  primary caret are the recorded design, not missing undo/multi-cursor work.

### Review Validation

- 40 existing focused test executions pass: watcher 6, disk-change 3,
  reload 2, save 23, native platform files 6; some application filters
  overlap. Clippy for `rte_app` and `rte_platform` passes.
- Two current-source headless probes: FSI1 fails, the Save mine control
  passes. Exit 101 is the expected reproduced review failure, not passing
  regression coverage. No production source or existing tests were edited.
- The console harness expands application modules and rebases includes in
  memory, removes only the Windows subsystem attribute, and adds probes to
  the existing editor test module. It uses fake file operations and controlled
  temporary files, no user settings or system clipboard. Exact Cargo JSON
  release artifacts are used with opt-level 3, fat LTO, one codegen unit,
  x86-64-v3 and self-contained GNU linking.
- Portable Rust tool homes were supplied only for command execution and
  restored afterwards; no install or project-toolchain change was made.
- Original task/reviewer bytes, local links, metadata, whitespace and the
  eight directly reviewed production-source fingerprints are checked after
  the append.
- Not run: full test/performance suites, a fresh startup or frame A/B,
  native delayed-storage UI reproduction, hosted CI, package rebuild or
  excluded symlink/folder-move experiments. The earlier implementer timing
  tables are not new reviewer measurements.

Commands:

```powershell
foreach ($filter in @('watcher', 'disk_change', 'reload', 'save')) {
    cargo test -p rte_app $filter --locked | Out-String
}
cargo test -p rte_platform files --locked
cargo clippy -p rte_app -p rte_platform --all-targets --locked -- -D warnings
./target/fs001-review.ps1
```

## Implementation Review Response

### 2026_11_02 - Claude Opus 5.5

- Role: Implementer
- Activity: Implementation
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: Medium
- Context Window: 872K
- Outcome: FSI1 accepted and fixed. `R` sets the `loading` lock when the
  reload starts (`start_open`), so the text stays read-only until the
  reload replaces it; typing, paste and undo meanwhile are refused, not
  dropped. A reload that does not replace the text (file not found, loader
  gone, a newer drop, `Ctrl+W`, a close request) gives the editing back
  through `cancel_pending` / `end_reload`; the lock then follows the real
  load state. Fresh history after a successful reload, the failure policy
  and every other decision are unchanged. No hot path changed; no new
  benchmark needed.

#### Validation - FSI1

- New `reload_keeps_the_text_read_only_until_it_is_replaced`: typing,
  `Ctrl+V` and `Ctrl+Z` after `R` and before the loader's answer leave the
  text unchanged, then the disk version loads with an empty history; a
  reload of a deleted file gives the editing back; `Ctrl+W` while a reload
  waits stops it and the dirty-close save is not refused. With the lock
  disabled (temporary switch, removed) the test fails with `xymine`, the
  reported loss.
- `cargo test -p rte_app -p rte_platform --offline`: 196 passed and 11
  passed, 1 ignored each (existing). `cargo fmt --all --check` and
  `cargo clippy -p rte_platform -p rte_app --all-targets -- -D warnings`:
  clean.
- Not run: the full test and performance suites; Sol's probe harness.
