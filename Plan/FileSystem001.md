# File System 001

## Task

Notice when another program (an agent, a formatter, `git checkout`) changes
the open file on disk, also while the editor has the focus, and let the user
choose: reload the disk version, or save the editor's version over it.

A simple first step before the 3-way merge of
[Data Safety 001](DataSafety001.md).

## Scope

### Included

- **Detection** while a file is open, focused or not: a folder change signal
  from Windows, then a comparison of the file with its baseline.
- **One prompt** for every outside change, with or without unsaved edits:
  `<name> changed on disk.   R: Reload   S: Save mine`. Only these two
  answers; other keys are ignored until one is chosen.
- **Reload** reads the disk version; it is one undo step.
- **Save mine** writes the editor's text over the disk version (the
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

### Compatibility Constraints

- Nothing before the first frame; no periodic work (DEVELOPMENT.md, Design
  Choices).
- The save sequence of [Architecture](Architecture.md#saving) is unchanged;
  only its conflict prompt changes keys (`O` becomes `S`, `R` is added,
  `Esc` is removed).
- No new dependency or `windows-sys` feature
  (`Win32_Storage_FileSystem`, `Win32_System_Threading` are enabled).
- Single session: the watcher observes the file only, never other sessions.

## Design

### Watcher (`rte_platform`, `rte_app/src/watcher.rs`)

- **Platform** (`rte_platform::files`): `FolderWatch` wraps
  `FindFirstChangeNotificationW` on the file's folder (not recursive;
  `FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_SIZE |
  FILE_NOTIFY_CHANGE_LAST_WRITE`) and a stop event. `wait()` blocks in
  `WaitForMultipleObjects` and returns whether the folder changed or the
  watch was stopped; `FindNextChangeNotification` re-arms it. A `Send`
  stopper sets the event.
- **Thread** (one per open file, small stack): reports the file's stamp
  once when it starts, then after every folder change whose stamp differs
  from the last report: `Option<FileStamp>` (`None`: the file is absent)
  through a channel, plus a wake-up of the window. Unrelated changes in the
  folder cost one stamp read and no wake-up. Stamps come from
  [`FileOps::stamp`](../crates/rte_app/src/save.rs), so tests inject them.
- **Lifetime:** started when the file's load is done and the first frame is
  shown (the command-line file), or when a dropped file's load is done.
  Also for a new file named on the command line (it may appear). Stopped by
  `reset_document` (close, replace by a drop); the UI never joins it.
- **Failure:** `FindFirstChangeNotificationW` fails → no thread, no notice.

### Disk state (UI thread)

- The editor keeps the last reported stamp. A change is pending when it
  differs from `baseline.stamp` (or from "absent" without a baseline).
- Evaluated when a report arrives, a save finishes, a prompt is answered and
  a reload finishes. Skipped while a save, a reload, a close or another
  prompt is running; evaluated again when it ends. Own saves therefore never
  prompt: the save's new baseline equals the stamp the watcher reports.
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

### Reload

- Spawns the loader on the same path (as a drop does), sets the `loading`
  lock and shows `Reloading <name>…`; the current text stays visible.
  Progress snapshots are ignored.
- **Done:** `Document::reload(snapshot)` records one undo step from the
  current text and selections to the loaded ones (`EditKind::Other`, cost =
  the replaced text's length), then sets `snapshot` and `saved` to it.
  Selections keep their offsets, clamped (as a load does); the scroll
  position stays. Encoding, line ending, indentation and the encoding lock
  follow the reloaded file. Syntax restarts; search and spelling caches
  reset (as after a load).
- **Undo** of the reload restores the editor's previous text: the document
  becomes dirty (it differs from disk); a save then overwrites the disk
  version without a prompt (the baseline is the reloaded one).
- **Failed / rejected** (read error, deleted meanwhile): text unchanged,
  notice with the reason; the disk state is evaluated again.

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
- **Fresh undo history on Reload.** A few dozen lines less; loses the way
  back after an accidental `R`. Rejected by the owner: undo restores the
  last state.
- **Hash on detection** to ignore touched files. Up to 64 MiB of reading on
  folder changes; rejected, a needless prompt is harmless.

## Runtime Effects

- **Startup:** none before the first frame. After it: one thread, one
  `FindFirstChangeNotificationW`, one stamp read.
- **Idle:** the thread blocks in `WaitForMultipleObjects`; no timers, no
  wake-ups without folder changes.
- **Folder changes:** one stamp read (open, `GetFileInformationByHandle`)
  on the thread per signal; the window wakes only when the file's stamp
  changed. Busy folders (build output, logs) cost one stamp read per signal.
- **Saves:** the temporary and backup siblings signal the folder; reports
  during a save are deferred, then match the new baseline.
- **Reload:** the cost of opening the file (read, full syntax parse). Old and
  new text coexist while loading; afterwards the undo step keeps the old
  version (its size) until the history drops it (256 MiB budget; the
  history keeps at least the last step, so a larger file's old version is
  dropped at the next edit).
- **Memory:** one thread stack (64 KiB); no buffers.
- **Cancellation:** the stop event ends the wait; a stamp read in progress
  finishes and its report is dropped with the channel.
- **Disabled path:** empty state, or a folder without notifications: no
  thread, nothing runs.

## Tests

- **Platform:** `FolderWatch` signals a file write and a rename in a
  temporary folder; the stopper ends a blocked `wait()`.
- **Watcher thread** (temporary folder, real files): the initial report;
  a write, a temp-then-rename replacement and a deletion are reported; an
  unrelated file in the folder is not; stopping ends the thread (joined in
  the test).
- **Document:** `reload` is one undo step; undo restores text and
  selections and makes the document dirty; redo returns to clean; the cost
  is the replaced length.
- **Editor, headless** (`Ctx::headless`, `FakeFiles`, reports injected
  through the channel):
  - A differing stamp shows the prompt; other keys are ignored.
  - `S` saves with the reported baseline; a newer disk version at save
    time prompts again.
  - `R` reloads (real temporary file): text, clean state, undo and redo.
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
cargo test -p rte_app --bin RoyiTextEditor --locked -- watcher document disk_change save
cargo clippy -p rte_platform -p rte_app --all-targets --locked -- -D warnings
```

## Implementation Steps

1. `FolderWatch` and its stopper in `rte_platform::files`, with tests.
2. `rte_app/src/watcher.rs`: thread, initial report, deduplication, stop;
   tests with a temporary folder.
3. `Document::reload` with its undo step; tests.
4. Editor: watcher lifecycle, disk state evaluation, `Prompt::Changed`
   (replacing `Prompt::Conflict`), `R` / `S`, reload, deleted notice;
   headless tests.
5. Architecture (Threading, Saving: External changes, Runtime Effects: no
   file watchers), README, `docs/src/files-and-saving.md`, CHANGELOG.
6. Focused benchmarks and manual checks.

## Acceptance Criteria

- A change by another program shows the prompt without any key press,
  whether or not the editor has the focus.
- `R` shows the disk version, clean; `Ctrl+Z` brings the previous text back
  (dirty) and `Ctrl+Y` returns to the disk version.
- `S` writes the editor's text; a version not yet seen is never
  overwritten without the prompt.
- The editor's own saves never prompt.
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
