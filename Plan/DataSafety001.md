# Data Safety 001

## Task

Unsaved edits to a file must survive a crash, a killed process, a power loss
or an operating-system failure. Changes made to the file on disk by other
programs (agents, formatters, `git checkout`) must be merged with unsaved
edits instead of being overwritten or silently lost.

Inspired by [Dan](https://github.com/dfallman/dan) (atomic saves, swap files,
live 3-way merge), adapted to this editor: an edit journal instead of periodic
full swap files, and event-driven change detection instead of polling.

## Scope

### Included

- **Edit journal** for every file-backed document with unsaved changes.
- **Recovery hint** when a file is opened and a journal from an improper
  close exists for its path: Recover / Discard.
- **Unsaved hint**: a visible marker whenever the document differs from disk,
  with a distinct state when the edits are not protected by a journal.
- **Journal clean-up command**: lists orphan journals and deletes selected ones.
- **External change detection** with `ReadDirectoryChangesW`, plus the
  existing checks on activation and before save.
- **Live 3-way merge** of external changes with unsaved edits; overlapping
  regions are conflicts resolved by the user.

### Excluded

- Session restore: nothing reopens automatically after a normal or improper
  close.
- Untitled documents are not journaled; their unsaved state is shown as
  not protected.
- No automatic deletion or ageing of journals; only the clean-up command
  and the per-file Discard delete orphans.
- Merging files above 64 MiB; they get a Reload / Keep mine prompt.
- Character-level merge; merge granularity is lines.

### Compatibility Constraints

- Journals live under `UserSettings/Recovery/`; no Registry writes.
- The atomic save sequence of the [Architecture](Architecture.md#saving) is
  kept unchanged, except that a pre-check mismatch merges instead of offering
  Overwrite.
- Journals contain document text and are as sensitive as the documents.
- Supersedes in [Architecture](Architecture.md): Crash recovery (panic-only
  snapshots), "No file watchers", the save conflict prompt, the Recovery row
  of the decisions table and the recovery test. These sections are updated
  when this task is approved.

## Design

### Unsaved hint

The title strip shows a marker after the file name and the status line shows
a state word whenever `current_root != saved_root`:

| State          | Condition                                | Marker and status                  |
| -------------- | ---------------------------------------- | ---------------------------------- |
| Saved          | `current_root == saved_root`             | None                               |
| Unsaved        | Dirty, journal records written           | `●` in accent color                |
| Unsaved (risk) | Dirty and untitled, or journal failed    | `●` in warning color; "at risk"    |

The marker changes only on transactions, saves and journal state changes;
no timers.

### Journal files

- **Location and name.** `UserSettings/Recovery/<path-hash>-<pid>-<start>.journal`
  plus `<same>.base`. `<path-hash>`: 64-bit hash of the final path
  (`GetFinalPathNameByHandleW`, upper-cased). `<start>`: process creation time.
  Names are unique per process; no shared files.
- **Creation.** On the first transaction of a clean, file-backed, editable
  document. No journal for untitled, read-only or never-edited documents.
- **Ownership lock.** The owning process keeps the journal open with
  `FILE_SHARE_READ` only. Windows releases the handle on any termination,
  including `TerminateProcess`. A journal that can be opened for exclusive
  access is an **orphan**.
- **Header.** Magic, format version, full path, encoding, BOM, line ending,
  and the file baseline at load (volume serial, file ID, size, last-write
  time, 64-bit hash when ≤ 64 MiB).
- **Base copy.** The loaded snapshot is written to `.base` on the journal
  thread after creation (temporary name, flush, rename, then a `BaseReady`
  record). Until `BaseReady` is durable, recovery depends on the file still
  matching the header baseline.
- **Records.** `length | kind | payload | CRC-32C`. Kinds:
  - `Edit`: changes (`old_range`, inserted bytes), selections before and
    after, undo-group flag (so coalescing replays identically).
  - `Undo`, `Redo`, `CursorUndo`.
  - `Saved`: new baseline and the revision that was committed.
  - `ExternalMerge`: new baseline of the merged disk version (the merge
    itself is journaled as an `Edit`).
  - `BaseReady`, `Rebase` (see below).
- **Replay** reads `.base`, applies records in order and stops at the first
  truncated or CRC-failing record (torn tail after power loss). The result is
  the text, selections and undo history as of the last durable record.
- **Rebase.** When the journal holds more than the undo retention (10,000
  steps or 256 MiB of inserted bytes), the current snapshot becomes the new
  `.base` (written beside, flushed, renamed) and the journal restarts with a
  `Rebase` header. Recovered undo history therefore matches what the live
  session could still undo.
- **Deletion.** On a successful save that leaves the document clean, on
  "Don't Save" at close, and on Discard. A save during further edits writes
  `Saved` and keeps the journal.

### Durability

- **Writer thread.** Created with the journal; the UI thread sends records
  through a channel and never waits for disk.
- **Process crash or kill.** A record is safe once `WriteFile` returns
  (operating-system cache). The writer writes each record immediately.
  The panic hook drains the channel synchronously with a 100 ms bound.
- **Power loss or OS crash.** `FlushFileBuffers` 200 ms after the last
  record, at most 1 s apart during continuous typing, and immediately on
  focus loss, before the dirty-close prompt and before save. Loss window:
  edits of the last ≤ 1 s.
- **Failure** (disk full, access denied): the state becomes
  Unsaved (risk) with the error in the status line. The next record retries
  by rewriting the journal from the current snapshot (new base). Editing is
  never blocked.

### Recovery hint

- After the first frame, the UI thread asks a worker to list
  `UserSettings/Recovery/<path-hash>-*.journal`, skip live (locked) journals,
  and read the headers of orphans whose full path matches.
- Found orphans → an in-window strip: "This file was not closed properly.
  Unsaved changes from <local time> can be recovered." Buttons: Recover,
  Discard, and with several orphans "Show all" (newest first).
- **Recover** replays on the worker, then applies the result as one
  transaction on the current document; the undo history is the recovered one.
  The recovered journal is adopted: its records are rewritten into this
  process's new journal, then the orphan is deleted.
- **Changed file.** If the disk file differs from the last recorded baseline,
  the recovered text is merged with the disk version as an external change
  (base = last recorded saved version).
- **Unreplayable** (no `BaseReady` and the file changed, or a corrupt base):
  the recovered inserted text is opened in a new untitled instance for manual
  salvage; the orphan is kept until Discard.
- Ignoring the strip keeps the orphan.

### Journal clean-up command

`Recovery: Manage Journals` opens a palette list of orphans (path, time,
size, replayable or not). Enter recovers into a new instance; `Delete`
deletes the selected orphan after confirmation. Live journals are never
listed. No automatic clean-up exists.

### External change detection

- **Watcher thread.** After the first frame, one thread per process calls
  `ReadDirectoryChangesW` on the file's directory (name, size, last-write
  and rename notifications) and filters for the file name. Closing the
  document cancels it with `CancelIoEx`.
- **Confirmation.** 100 ms after the last notification, compare identity,
  size and last-write time with the baseline; for files ≤ 64 MiB compare the
  hash when only the time differs. No change → nothing happens.
- **Fallback.** The existing checks on `WM_ACTIVATE` and before save remain,
  for directories where notifications fail (some network shares).
- **Deleted or moved file:** reported once; the document becomes dirty and
  saving recreates the file.

### 3-way merge

- **Inputs** (all immutable snapshots): base = `saved_root` (the last saved
  or loaded version), ours = current snapshot, theirs = the new disk content
  read by the loader.
- **Algorithm** on a worker: line diffs base → ours and base → theirs
  (histogram diff), then a diff3 merge. Non-overlapping hunks combine;
  identical changes on both sides combine; overlapping different hunks are
  conflicts.
- **Result** is applied on the UI thread as one transaction (one undo step),
  mapped through any edits made during the merge; a stale result (revision
  moved past mapping) is recomputed. `saved_root` and the baseline become
  theirs. The document is dirty unless ours equals theirs.
- **Clean document:** theirs replaces the text directly (same path), keeping
  carets by line and the scroll anchor.
- **Conflicts:** ours is kept in conflicting regions and the strip offers
  Keep mine / Take theirs / Decide later per conflict or for all. Deferred
  conflicts stay marked.
- **Marks:** externally changed lines are marked in the gutter (changed and
  conflict colors) until save or `Recovery: Clear Change Marks`. Marks are
  mapped through edits.
- **Save pre-check mismatch:** merge first; the user saves again. So a save
  never overwrites an unseen version. Above 64 MiB: Overwrite / Cancel as now.
- **Encoding change** in theirs (different BOM or UTF-16): no merge; Reload /
  Keep mine prompt.

### Settings

`files.recoveryJournal` (default `true`) and `files.watchChanges` (default
`true`) in `RoyiTextEditor.yaml`.

## Alternatives

- **Periodic full swap file (Dan, Vim).** Rejected: loses up to the interval
  (Dan: 5 s) and rewrites the whole document each time.
- **Panic-only snapshots (current Architecture).** Rejected: no protection
  against kill, power loss or OS failure.
- **SQLite or another embedded database** for the journal. Rejected: a
  dependency for an append-only log.
- **Checkpoints referencing pieces of the original file** instead of a full
  base copy. Rejected for v1: recovery would depend on the original file
  staying unchanged. Possible later optimization for very large files.
- **Polling the file (Dan, every ~0.5 s).** Rejected: periodic wake-ups
  violate the zero-idle-CPU goal. `FindFirstChangeNotification` rejected:
  no file names, so every directory change needs a stat.
- **Prompt on every external change** instead of merging. Rejected as the
  default: agents and formatters change files often; non-overlapping changes
  merge safely and stay one undo step.
- **Character-level merge.** Rejected: harder to review; line hunks match
  Git and user expectations.

## Runtime Effects

- **Startup:** none before the first frame. After it: one directory listing
  of `UserSettings/Recovery/` filtered by name, and one blocked watcher
  thread.
- **Typing:** one channel send per transaction. The writer appends records
  of the size of the edit; flushes at most once per second while typing.
- **Base copy:** one write of the document size on the first edit, on the
  journal thread at normal I/O priority; for a 1 GiB file about 1 GiB of
  disk I/O and disk space while dirty.
- **Memory:** the writer reuses one record buffer; merge memory ≈ 3 × line
  table of files ≤ 64 MiB, only while merging.
- **Idle:** writer blocked on the channel (timeout only while a flush is
  pending); watcher blocked in `ReadDirectoryChangesW`; no timers.
- **Cancellation:** the merge job carries a `CancelToken` and a revision;
  stale results are mapped or recomputed. Closing cancels the watcher.
- **Disabled paths:** `files.recoveryJournal: false` → no journal thread,
  no files, status shows Unsaved (risk). `files.watchChanges: false` → no
  watcher thread; activation and save checks remain. Read-only documents
  create neither journal nor merge jobs (watcher still reloads them).

## Tests

- **Unit, journal format:** round trip of every record kind; torn tail at
  every byte offset of the last record; CRC mismatch; unknown version.
- **Property (`proptest`):** random edits, undo, redo and saves; truncate the
  journal at a random record boundary or byte; replay equals the model state
  after the last complete record (text, selections, undo stack).
- **Rebase:** crossing the retention limits produces a new base; replay
  after rebase equals the live state.
- **Ownership:** live journal is locked and never offered; orphan detected
  after the owner thread drops the handle; two orphans for one path listed
  newest first.
- **Integration, kill:** start the release binary on a temporary file, edit
  through posted messages, `TerminateProcess`, relaunch: the hint appears;
  Recover restores text, carets and undo; Discard deletes the orphan.
- **Changed file after crash:** recovery merges with the new disk version;
  without `BaseReady` the salvage path opens.
- **Failure injection:** journal write and flush errors through `FileOps`
  show Unsaved (risk) and recover on the next record.
- **Merge unit:** non-overlapping, identical, overlapping, delete versus
  edit, insert at EOF, no trailing newline, CRLF, empty base.
- **Watcher integration:** in-place write, write to temporary + rename,
  delete, move away and back, burst of writes (debounced to one merge).
- **Save:** pre-check mismatch merges instead of overwriting; above 64 MiB
  the Overwrite / Cancel prompt remains.
- **Hint:** dirty, clean after undo to `saved_root`, untitled, journal
  failure.
- **Performance:** typing benchmark with journaling on/off (UI cost per
  transaction < 5 µs); flush cost; startup gate unchanged (`cargo xtask
  bench-startup`); idle wake-ups 0 with both features enabled.
- **Manual:** power-cut simulation by resetting a Hyper-V VM while typing;
  network-share directory without notifications.

## Implementation Steps

1. Update the Architecture sections listed under Compatibility Constraints.
2. Unsaved hint states in the title strip and status line (with M3 editing).
3. Journal format, writer thread and replay in a pure, tested module with
   `FileOps` injection (with M3, since it hooks transactions and undo).
4. Recovery hint, Recover / Discard, and `Recovery: Manage Journals` (M4,
   with the palette and strips).
5. Line diff and diff3 merge in a pure module with tests.
6. Watcher thread, debounce, merge application, change marks and conflict
   strip (M7).
7. Save pre-check merge; kill and watcher integration tests; benchmarks.

## Acceptance Criteria

- A killed or crashed editor loses no edit whose record was written; after a
  power loss at most the last second of edits is lost.
- Reopening the file shows the recovery hint; Recover restores text,
  selections and undo history; Discard deletes the journal.
- Journals are deleted only by save-to-clean, Don't Save, Discard or the
  clean-up command.
- Untitled or unprotected dirty documents show the warning marker.
- External non-overlapping changes merge as one undo step with marks;
  overlapping ones prompt; a save never overwrites an unseen disk version.
- Startup gate and zero idle wake-ups hold with both features enabled.

## Reviewers

### 2026_10_23 - Claude Opus 5.5

- Role: Reviewer
- Activity: Design
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Design drafted from the user's decisions: journal for file-backed
  documents only, recovery hint on open instead of session restore, no
  automatic journal clean-up, visible unsaved hint. Supersedes the
  Architecture's panic-only recovery and no-watcher rule; awaiting review.
