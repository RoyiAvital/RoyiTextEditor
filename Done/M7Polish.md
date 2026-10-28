# M7 Polish

## Task

Finish the single-file editing experience with the VS Code defaults still
missing:

- **Basic Editing commands** of VS Code that fit one file: move, copy,
  delete and insert lines, indent lines, add and remove line comments,
  scroll by page, Go to Line.
- **Long lines:** text beyond 64 KiB of a line is never drawn today (`End`
  on a 200 KiB line shows the caret in an empty row). Draw it.
- **Full screen** (`F11`) and `Ctrl+K Ctrl+S` for Keyboard Shortcuts.
- **Scroll indicator:** nothing shows where the view is in the file today.
  Add a thin slider on the right, at no measurable cost.

Startup and the feel of normal files must not change.

## Scope

### Included

New commands (VS Code IDs and default Windows keys):

| Keys                                | Command (palette label)                   | VS Code ID                                  |
| ----------------------------------- | ----------------------------------------- | ------------------------------------------- |
| `Ctrl+G`                            | Go: Go to Line/Column                     | `workbench.action.gotoLine`                 |
| `Alt+Up` / `Alt+Down`               | Edit: Move Line Up / Down                 | `editor.action.moveLinesUpAction` / `...Down...` |
| `Shift+Alt+Up` / `Shift+Alt+Down`   | Edit: Copy Line Up / Down                 | `editor.action.copyLinesUpAction` / `...Down...` |
| `Ctrl+Shift+K`                      | Edit: Delete Line                         | `editor.action.deleteLines`                 |
| `Ctrl+Enter` / `Ctrl+Shift+Enter`   | Edit: Insert Line Below / Above           | `editor.action.insertLineAfter` / `...Before` |
| `Ctrl+]` / `Ctrl+[`                 | Edit: Indent Line / Outdent Line          | `editor.action.indentLines` / `...outdentLines` |
| `Ctrl+K Ctrl+C` / `Ctrl+K Ctrl+U`   | Edit: Add / Remove Line Comment           | `editor.action.addCommentLine` / `...removeCommentLine` |
| `Alt+PgUp` / `Alt+PgDn`             | View: Scroll Page Up / Down               | `scrollPageUp` / `scrollPageDown`           |
| `F11`                               | View: Toggle Full Screen                  | `workbench.action.toggleFullScreen`         |
| `Ctrl+K Ctrl+S`                     | Application: Keyboard Shortcuts (new key) | existing command                            |

`Edit: Outdent Line` exists (`Shift+Tab`); it gets `Ctrl+[` as well.

Behavior:

- **Lines of a selection** follow the existing rule: the lines it touches,
  without the line where it ends when it ends at that line's start.
  Touching selections merge into blocks (line intervals), computed per
  selection, never per line.
- **No size limits (owner's decision):** no line command refuses because
  of size, like paste (300 MB pastes work today). The design aims at the
  fastest form instead: Move, Copy and Delete Line copy no bytes at all.
- **Move Line Up / Down:** each block swaps with the line above / below;
  selections move with it. The two texts swap places as shared pieces of
  the existing text, so the cost does not depend on the size of either.
  Blocks at the first / last line stay; the others move. The line break
  between the two stays in place, so a last line without a break stays
  without one.
- **Copy Line Up / Down:** duplicates each block above / below; the
  selections end on the copy in the key's direction. The copy shares the
  original's pieces: no bytes are copied and no memory is added until one
  of them is edited. A last line without a break gets the document's line
  break.
- **Delete Line:** deletes each block with its breaks (the last line takes
  the preceding break); the caret goes to the same column of the following
  line. Deletions copy no text: one range per block.
- **Insert Line Below / Above:** a new line with the current line's
  indentation, below or above, at every caret; the caret moves there.
- **Indent / Outdent Line:** one indentation unit on every touched line,
  also for a caret or a one-line selection (unlike `Tab`).
- **Add / Remove Line Comment:** as Toggle Line Comment, but always adds
  (also to commented lines) or only removes. Same language rules and
  embedded-language handling as the toggle.
- **Scroll Page Up / Down:** the view moves by a page; the caret stays.
- **Go to Line/Column:** the palette in a line mode. The query is `N`,
  `N:C`, `N,C` or `-N` (from the end). The row shows `Go to line N,
  column C` or `Type a line number between 1 and M`. While typing, the view
  previews the line; `Enter` places the caret, `Esc` restores the scroll.
- **Long lines.** Beyond the first 64 KiB of a line, every component
  already uses one column rule: the column at the scan end plus one column
  per byte (caret, clicks, selections, `Up` / `Down`, the status line). The
  view now draws the visible part of that region by the same rule, and
  search highlights and syntax colors cover it. Exact for ASCII without
  tabs (minified files, logs). Tabs, wide and multi-byte characters there
  keep consistent positions with extra spacing; the status line shows
  `Col ~N` for a caret beyond the scan end.
- **Character-safe seams.** The scan end is the last character boundary at
  or before 64 KiB (at most 3 bytes earlier), so a character straddling
  64 KiB counts and draws as that character, not as invalid bytes; every
  component uses the same seam. Beyond it, each character is drawn once,
  at the column of its first byte, and takes one column per byte. At the
  viewport's left and right edges the decoder reads up to 3 bytes outside
  the visible columns to complete a character; one that starts before the
  left edge is clipped, not shifted. Zero-width marks attach to the
  preceding character's cell (the existing per-cell mark limit holds); real
  invalid bytes stay one symbol each; the CR of a CRLF ending is not drawn.
- **Full screen:** `F11` makes the window's client area cover its monitor
  exactly (Windows hides the taskbar for it), from a normal or a maximized
  window, and hides the title strip; the status line stays (prompts and
  notices). It follows the window to another monitor or a DPI change.
  `F11` again restores the previous placement and show state, maximized or
  not. Moving, sizing or maximizing the window by the system (keys, the
  system menu, snapping) leaves full screen first. If Windows refuses a
  step, the window stays as it was and a notice says so. Available in the
  empty state.
- **Scroll indicator (owner's choices).** A strip `round(6 × dpi / 96)`
  pixels wide, reserved at the right of the body whenever a file is open:
  the text area ends before it, so nothing is ever covered and the text does
  not move when the file grows. No track is drawn; only the slider, in
  VS Code's dark slider color, brighter while dragged. With `N` lines, a
  page of `P` rows and a strip `H` pixels high, the slider is
  `max(H × P / (N − 1 + P), one text row)` high (at most `H`) at
  `(H − height) × top_line / (N − 1)`: the bottom means the last line at the
  top of the view, as scrolling allows today. Hidden while the whole file
  fits (`N ≤ P` and the view at the top). Pressing the slider drags it; the
  view scrolls in proportion over the slider's travel. Pressing elsewhere
  on the strip moves the slider's center there, then drags. The caret and
  selections never change; the pointer is the arrow; the wheel scrolls as
  over the text. Pressing and releasing the slider each redraw one frame
  (its color changes) even without scrolling; nothing else adds frames.
  Not in the empty state. No marks (matches, selections),
  no fading, no hover effect, no horizontal bar.

### Excluded

- Configuration, key bindings, customization (VS Code defaults only).
- Navigation history (`Alt+Left` / `Alt+Right`, owner's decision): keeping
  recorded places correct through edits, undo and redo touches the edit
  path for a small gain in one file; Notepad++ has no such feature built in.
- `Ctrl+M` (Tab moves focus): no focusable controls.
- A separate large-file mode: loading, counts and interactive work are
  already bounded or asynchronous (owner's decision).
- An exact long-line index (byte → display column checkpoints): only if the
  spacing beyond 64 KiB ever matters.
- External change detection and crash recovery:
  [DataSafety001](../Plan/DataSafety001.md).
- Themes, minimap ([Minimap](../Plan/Minimap.md)), word wrap, folding, edit-location history
  (`Ctrl+K Ctrl+Q`).
- Scroll indicator marks, fading and hover effects, a horizontal scroll bar:
  each needs text scans, timers or extra frames.

### Compatibility Constraints

- No startup work, no new thread, no new DLL import, no new setting.
- Existing keys keep their commands; the new keys are unbound today.
- `rte_text::Edit` changes shape (internal crates only); E01, E02 and M05
  guard the typing path.
- Single session ([Architecture](../Plan/Architecture.md), Principle 8).

## Design

### `rte_text`

- `Edit.text` becomes `Inserted`: `Bytes(Box<[u8]>)` (today's form) or
  `Shared(Vec<Piece>)`, pieces joined in order, each `Piece::Bytes` or
  `Piece::Copy(Range<u64>)` (a range of the snapshot the transaction applies
  to). `Inserted::len()` serves every consumer that needs only the length
  (`ChangeSet`, the document's counters, `rte_edit::code`).
- `Snapshot::apply` builds every shared text from the unchanged base first
  (two `split`s per copied range, `concat` of the pieces: the existing tree
  operations, O(log n) for any size, sharing the leaves' buffers), then
  applies the edits from the end. A copied range may lie after its edit or
  be deleted by another edit of the same transaction (Move Line Down). A
  transaction with shared text skips the batch path.
- Undo history counts only owned bytes toward `MAX_HISTORY_BYTES`.

### `rte_edit` (pure, golden-tested)

- **`lines.rs`:** `line_blocks(snapshot, selections) -> Vec<Range<u64>>`
  (merged line intervals, O(selections × log n), never per line).
  `move_lines(up)`, `copy_lines(up)`, `delete_lines`, `insert_line(below)`,
  `indent_lines`, each `(snapshot, selections, options) -> Coded` (command
  plus selections). Move builds two edits per block (each of the two texts
  replaced by a copy of the other); Copy one shared insert per block (the
  block, plus a line break for a last line without one); Delete one
  deletion per block; Indent one insert per line (as `Tab` on lines today).
- **`covered_lines`** stays for the per-line commands (Indent / Outdent
  Line, comments, `Tab` / `Shift+Tab`), which edit every line anyway.
- **Comments:** `toggle_line_comment` becomes `line_comment(mode)` with
  `Toggle`, `Add`, `Remove`; the toggle keeps its behavior.
- **`columns.rs` seam:** `scan_end(prefix) -> usize`, the prefix length
  without a trailing incomplete UTF-8 sequence (at most 3 bytes). Column
  values do not change: those bytes count one column each whether read as
  invalid bytes before the cut or by the one-column-per-byte rule after it,
  so `walk`, `column_after` and `offset_at_column` stay as they are (clicks
  already snap to character boundaries). The seam decides only where exact
  drawing ends. `column_until(line, until)` returns a line's column or stops
  as soon as a character starts at `until`.

### `rte_app`

- Commands: new `Action`s and registry entries (`mutates` for the edits),
  `VK_OEM_4` / `VK_OEM_6` / `VK_F11`, chords `Ctrl+K Ctrl+C`,
  `Ctrl+K Ctrl+U`, `Ctrl+K Ctrl+S`. `works_without_file` adds Toggle Full
  Screen. Add / Remove Line Comment use the toggle's language checks.
- Go to Line: `palette::Mode::Line` with a parsed target, a preview scroll
  saved and restored on `Esc`.
- Long-line ranges: `visible_ranges` (syntax) and the search box's viewport
  rows add each long row's visible bytes beyond the seam (widened by up to
  3 bytes at each edge to whole characters). The seam's column comes from
  `column_until` over growing windows (reading only up to the visible
  columns, so a view at the line start never scans 64 KiB), cached per
  text, tab width and line start once a full scan was needed.
- Full screen: the platform owns the state; `Ctx::set_full_screen(on)`
  requests it and `Event::FullScreen(Result<bool, _>)` reports the actual
  state, which the layout reads (no title strip, `is_caption` false). A
  refused request shows a notice.
- Scroll indicator: the editor sets the layout's `scroll_strip` from the
  DPI (zero in the empty state); `mouse_down` checks the strip before the
  text and starts `DragMode::Slider { grab }`; `drag_move` scrolls with
  `clamp_scroll`; `MouseUp` (also sent on capture loss) ends it. `Visible`
  gets `slider_active`, so press and release repaint once each and repeated
  events without a change do not.
- Alt line and page commands act only while the text has the focus, not
  from the find box (VS Code `editorTextFocus`).

### `rte_view`

- `layout_line` stops at the seam (`scan_end`) and returns the column it
  reached. When a row's line continues past the seam and the visible
  columns extend past that column, the row decodes only the visible bytes
  (plus up to 3 at each edge to complete characters) and draws each
  character once at `column(seam) + (first byte − seam)`: tabs blank,
  controls and invalid bytes as symbols, zero-width marks on the preceding
  cell within `MAX_MARKS_PER_CELL`. Caret, selection and match rectangles
  use the same rule (`column_after`).
- `Options` gets `title_strip: bool`; `Layout::new` gives the title zero
  height without it.
- `Options.scroll_strip: u32` (pixels; `0` none); `Layout` gets
  `scrollbar: Rect` and the text area ends before it. A pure
  `slider(layout, line_count, top_line, page, min) -> Option<Rect>` (the
  minimum is one text row) is used by the drawing and the editor's hit
  test, so both agree. `Theme` gets `scroll_slider` and
  `scroll_slider_active`; `Frame` gets `slider_active: bool`. Drawing is
  one fill.

### `rte_platform`

- **Full screen state** lives in the window state (`full_screen: Option<
  Saved>` with the `WINDOWPLACEMENT` and window style). Entering: read the
  placement and `GetMonitorInfoW(MonitorFromWindow(...)).rcMonitor`; if
  either fails, stay unchanged and report the failure. Otherwise set the
  state first, then `SetWindowPos` to `rcMonitor` with `SWP_FRAMECHANGED`
  and the corners to `DWMWCP_DONOTROUND`. A maximized window keeps its
  show state for the restore, so the frame code checks the state, not only
  `IsZoomed`.
- **`WM_NCCALCSIZE`:** while full screen, no maximized insets: the whole
  window is client area, equal to `rcMonitor`.
- **`WM_NCHITTEST`:** no resize edges and no caption while full screen.
- **`WM_DPICHANGED` / `WM_DISPLAYCHANGE`:** while full screen, cover the
  window's current monitor again instead of applying the suggested rectangle.
- **`WM_SYSCOMMAND`** `SC_MOVE`, `SC_SIZE`, `SC_MAXIMIZE`, `SC_RESTORE` and
  a `WM_WINDOWPOSCHANGED` not caused by the platform that leaves the
  monitor rectangle: leave full screen (restore style and corners, clear
  the state) before the system action; minimizing keeps it.
- **Exit:** clear the state, `SetWindowPlacement` with the saved placement
  (restores the normal rectangle and maximized or normal show state),
  `SWP_FRAMECHANGED`, round corners. A failed restore still clears the
  state and keeps the window usable (normal frame, current position).
- The editor receives `Event::FullScreen(Ok(on))` before each change of
  the window, or `Err(reason)` when a request failed, so its layout always
  matches the platform. While full screen the window style drops
  `WS_OVERLAPPEDWINDOW` (no caption or sizing frame) and gets it back on
  exit. All functions are in `user32`, `gdi32` and `dwmapi`, already
  imported.

### Threading, persistence, failure

- No new threads; nothing persisted (full screen lasts for the session).
- Refused edits (locks, an edge block) leave text and
  selections unchanged with a notice where useful.

## Alternatives

- **Navigation history.** Dropped by the owner. Following edits needs a
  mapping on every edit and a bounding change per undo step (the edit path);
  storing line numbers instead is simple but lands off after edits above.
- **An exact long-line index now.** Deferred: a background thread, its
  invalidation and an `rte_edit` column interface, to improve only
  non-ASCII spacing beyond 64 KiB.
- **Full screen hiding the status line.** Rejected: prompts and notices
  live there.
- **Move Line rewriting the whole block, or copying the smaller side.**
  Rejected: both copy bytes in proportion to line size; shared pieces copy
  none.
- **A size budget refusing large commands** (review M7R1). Rejected by the
  owner: the user may do anything; the work is to make it fast. With shared
  pieces, Move, Copy and Delete Line take O(log n) for any size.
- **Go to Line as its own input box.** Rejected: the palette already has a
  field, preview area and keys.
- **Scroll indicator overlaid on the text.** Rejected by the owner: it may
  cover the end of a line reaching the edge; hiding it until hovered defeats
  the hint and needs mouse-leave tracking.
- **A 14 px VS Code-wide bar.** Rejected by the owner after comparing
  renderings; 6 px costs one column.
- **Position text on the status line.** Not needed: `Ln`, `Col` and the line
  count are already shown.

## Runtime Effects

- **Startup:** none (static command entries only).
- **Per edit:** typing and other byte edits take the existing path
  unchanged (`Inserted::Bytes`); only transactions with a `Copy` take the
  split-and-concat path.
- **Line commands:** Move, Copy and Delete Line: O(selections × log n),
  independent of line and block size, no bytes copied. Indent / Outdent
  Line and comments: one edit per touched line, as `Tab` today. No limits.
- **Frames:** rows up to the seam unchanged. A long row scrolled past the
  seam adds at most one row of bytes (plus 6 edge bytes) and glyphs; the
  64 KiB column scan runs once per long visible row when the text changes.
- **Full screen:** a few window calls when toggled, on DPI or display
  changes while full screen; none otherwise.
- **Scroll indicator:** per frame a few integer operations (the line count
  is stored) and one fill of about 6 × 20 to 6 × H pixels; no text read, no
  thread or timer. Pressing and releasing the slider each repaint once
  (the only extra frames). The text area is one column narrower at 100 %
  scaling.
- **Memory:** long-row cache ≤ the visible rows.
- **Disabled path:** commands not used cost nothing.

## Tests

Unit (`cargo test -p rte_text`):

- `Copy` edits: results equal the same edits with copied bytes (randomized
  against the byte form, including line counts and summaries); a copied
  range shares the base's buffers (no new allocation for its bytes); mixed
  `Copy` and `Bytes` edits in one transaction; copies at leaf boundaries
  and of the whole text; undo and redo through `Copy` edits.

Unit (`cargo test -p rte_edit`):

- Golden tests per line command: single and multiple selections, adjacent
  blocks merging, selections ending at a line start, first / last line,
  last line without a break, CRLF and mixed breaks, tabs and spaces
  indentation, undo restoring text and selections.
- Scaling: a one-byte line moved past a 65 MiB neighbor, a 65 MiB block
  copied, and one selection over 2,000,000 short lines moved, copied and
  deleted each build at most two edits per block and copy no bytes.
- Seam: 65,535 ASCII bytes followed by a 2-, 3- and 4-byte character, a
  combining mark, a tab, a real invalid byte, and CRLF at the tail: the
  seam is the character's start, columns are continuous across it, and
  `offset_at_column` round-trips.
- Comment modes: add to commented lines, remove from mixed lines, block-only
  languages; the toggle unchanged.

View (`cargo test -p rte_view`):

- `End` on a 200 KiB ASCII line draws the line's last characters with the
  caret after them; a hit test there lands on the drawn character; a
  multi-byte line beyond 64 KiB keeps caret and glyph columns consistent;
  rows below 64 KiB produce the same display list as today.
- Seams: a character straddling 64 KiB, and characters straddling the
  viewport's left and right edges beyond the seam, are drawn once as
  themselves (glyph identity asserted) at the caret's column; combining
  marks stay within the per-cell limit; invalid bytes draw one symbol each.
- No title strip with `title_strip: false`.
- Slider: hidden when the file fits; top, middle and bottom positions;
  minimum height; millions of lines; DPI scaling; the text area ends before
  the strip; none with `scroll_strip: 0`.

Editor (`cargo test -p rte_app`):

- Every new key and palette entry runs its command; read-only refuses the
  edits and allows navigation; the empty state allows only Toggle Full
  Screen and Keyboard Shortcuts among them.
- Go to Line: `N`, `N:C`, `-N`, out of range, preview and `Esc` restore,
  `Enter` places the caret.
- Long lines: search highlights and syntax spans beyond 64 KiB on a long
  JSON line; `Col ~` on the status line.
- Full screen: `F11` requests it; the layout follows `Event::FullScreen`
  (no title strip), also when the platform leaves full screen on its own;
  a refused request shows a notice.
- Scroll indicator: dragging the slider reaches the first and last lines;
  a press on the strip centers the slider there; the caret never changes;
  the pointer is not the text cursor over the strip; no strip in the empty
  state. Press without motion and release without motion each request one
  redraw and change the slider's pixels with scroll, text and selections
  unchanged; a capture loss ends the drag the same way; repeated events
  without a change request none.

Platform (`cargo test -p rte_platform`): the headless `Ctx` records the
request and reports the state. Native (an ignored test run on demand, and
manual): after entering from a normal and from a maximized window, the
client rectangle in screen coordinates equals `rcMonitor`; moving the
window between 100 % and 150 % monitors keeps that; exit restores the saved
placement and show state.

Performance (focused; Benchmark v1 is still open):

- E03 exists (`End,Home` on `line.txt`, both steps timed); its baseline is
  reused unchanged and must stay within noise, now drawing the line's end.
- New E04 (`Alt+Down`, Move Line Down) and E05 (`Shift+Alt+Down`, Copy
  Line Down) on `medium.py`, key to frame.
- `cargo xtask bench-perf --only L01,L26,L04,L16,E01,E02,E03,E04,E05,M05,S02`:
  startup, typing, multi-cursor and scrolling within noise of the previous
  run; E04 and E05 within one frame.
- `cargo xtask check-imports`: the same DLL list.

Manual: `F11` on one and two monitors, maximized and not, at 100% and 150%
scaling, crossing monitors while full screen, `Win+Up` / `Win+Left` and the
system menu while full screen; `Alt+Up` / `Alt+Down` with the system menu (`Alt` alone still
opens it); a minified JSON scrolled to its end; the slider at 100 % and
150 % on `large.md`, dragged to both ends.

## Implementation Steps

1. `rte_text`: shared texts (`Inserted::Shared`) and their apply path;
   unit tests.
2. `rte_edit`: the column seam; `lines.rs` blocks, the line commands,
   comment modes; unit tests.
3. `rte_app`: the registry entries and actions; editor tests; E04, E05.
4. Go to Line palette mode; tests.
5. Long lines: `rte_view` drawing with seams, editor ranges, status `~`;
   view and editor tests; E03.
6. Full screen: `rte_platform` state and messages, layout option, editor;
   tests; native and manual checks.
7. Scroll indicator: `rte_view` layout, slider and drawing, editor drag;
   tests.
8. `Ctrl+K Ctrl+S`; documentation (README, keyboard shortcuts page,
   Architecture command coverage), CHANGELOG.
9. Focused validation and benchmarks.

## Acceptance Criteria

- Every command in the table works as described, through its keys and the
  palette, and edits are one undo step each; no command refuses because of
  size.
- `End` on a long line shows its end; text, caret and clicks agree beyond
  64 KiB, and characters at the seam and the viewport edges draw as
  themselves.
- `F11` covers the monitor exactly from a normal or maximized window,
  survives monitor and DPI changes, and restores the previous placement.
- The slider shows the view's place in the file, never covers text, and
  dragging or pressing the strip scrolls without moving the caret.
- Startup (L01, L26, L04) and the typing, multi-cursor and scrolling
  benchmarks are within noise; no new DLL import; focused tests, Clippy
  and formatting pass.

## Reviewers

### 2026_10_25 - Claude Opus 5.5

- Role: Reviewer
- Activity: Design
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Designed from the owner's decisions: navigation history, the
  VS Code Basic Editing commands relevant to one file, `F11` and
  `Ctrl+K Ctrl+S` (no performance cost), a long-line display fix instead
  of a large-file mode, no configuration, no themes, external change
  detection in DataSafety001, minimap not included. Awaiting review.

### 2026_10_25 - Claude Opus 5.5

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Navigation history removed (owner's decision): following edits
  and undo touches the edit path for a small gain; Notepad++ has none built
  in. M7 no longer changes the edit or undo path. Awaiting review.

### 2026_10_25 - Claude Opus 5.5

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Added the scroll indicator (owner's choices after comparing
  6 px and 14 px renderings): a reserved 6 px strip with only a slider, drag
  and press to jump, no marks, fading or hover. Zero measurable cost: O(1)
  geometry and one fill per frame. No status line change (it already shows
  the position). Awaiting review.

### 2026_10_25 - GPT-6.1 Sol

- Role: Reviewer
- Activity: Review
- Agent: AI assistant using Copilot SDK in VS Code
- Model: GPT-6.1 Sol
- Effort: High
- Context Window: 1M
- Outcome: Direction supported; changes requested for M7R1-M7R4 before
  implementation. Reviewed against the current edit, column, layout,
  repaint and Win32 paths. No implementation or runtime measurements
  authored. The owner's scope and UI choices are unchanged.

#### M7R1 [P1] Line-command construction is not bounded

The proposed reuse of
[`covered_lines`](../crates/rte_edit/src/commands.rs#L231) materializes a
`BTreeSet` entry for every touched line. Moving or deleting one large
selected block would therefore enumerate millions of lines before building
what could be a few range edits. Copy's byte cap does not bound that
enumeration.

Moving only the neighbor also does not bound copied text:
[`Edit::text`](../crates/rte_text/src/change.rs) owns its bytes. Moving a
short line past a 1 GiB neighbor would materialize that neighbor on the UI
thread with the proposed command shape. Copy's per-block 256 MiB limit
also leaves the aggregate of several blocks unbounded.

Use merged line intervals for block operations instead of enumerating
their contents. Define a total construction budget, checked before copying
or enumerating, and a typed refusal with no partial edits. Specify how
large neighbors are shared, deferred or refused; unchanged transaction and
undo machinery alone does not make command construction cheap.

Add focused cases for a tiny line beside a very long neighbor, one
selection over millions of short lines, and several individually permitted
copy blocks whose total exceeds the budget. Check refusal before large
allocation, unchanged text/selections, and accepted-command latency. The
current typing and scrolling benchmarks do not exercise these operations.

#### M7R2 [P2] Full screen needs non-client and DPI handling

The platform design changes hit testing, corners and placement, but the
current
[`WM_NCCALCSIZE`](../crates/rte_platform/src/lib.rs#L853) still subtracts
resize borders whenever `IsZoomed` is true. If entry retains the maximized
state, sizing to `rcMonitor` does not make the entire monitor the client
area. Disabling resize hit tests does not remove these insets.
[`WM_DPICHANGED`](../crates/rte_platform/src/lib.rs#L883) also applies its
suggested rectangle unconditionally.

Define the native full-screen state before frame recalculation. Specify
whether maximization/styles are temporarily changed or the maximized
insets are bypassed; while full screen, DPI/monitor changes must preserve
monitor coverage. Restore the original show state and placement on exit.
Report failed placement/monitor calls without leaving editor and platform
state inconsistent.

Extend validation beyond a headless request: compare the client rectangle
in screen coordinates with `rcMonitor`, entering from both maximized and
normal states, then crossing 100%/150% monitors and exiting. Confirm the
saved placement and maximized state are restored.

#### M7R3 [P2] Long-line byte windows need character-safe seams

The current
[`column walker`](../crates/rte_edit/src/columns.rs) and
[`row layout`](../crates/rte_view/src/lib.rs#L905) decode bounded byte
slices. A 64 KiB cut can split a valid UTF-8 character. For example,
65,535 ASCII bytes followed by U+4E2D place its UTF-8 bytes across the cut:
`... E4 | B8 AD ...`. Decoding the prefix and tail independently produces
invalid-byte symbols, not that character. The left and right edges of a
scrolled visible window have the same problem.

Define a character-safe `(byte offset, column)` seam and bounded decoding
context at window edges. Keep the approximate byte-column rule shared by
drawing, carets, hit tests, selections, search and syntax ranges. Moving
the decoder boundary must not silently move the column origin or draw the
same character twice. Preserve the mark-count and scanned-byte bounds.

Add cases where a 2-, 3- or 4-byte character straddles 64 KiB or a viewport
edge, plus combining marks, tabs, actual invalid bytes and CRLF at the
tail. Assert glyph identity as well as caret/click columns. A multibyte
character starting wholly after 64 KiB does not cover this seam.

#### M7R4 [P2] Slider press and release need their own repaint transition

Putting `slider_active` in `Frame` does not itself schedule a frame.
[`Visible`](../crates/rte_app/src/editor.rs#L767) does not track drag state,
and [repaint requests](../crates/rte_app/src/editor.rs#L2034) depend on its
before/after comparison. Pressing the existing thumb without moving it
changes no scroll, caret or selection. Releasing after a drag likewise
only clears `drag`, so the active color can remain drawn until another
event changes the view.

Include slider-active state in visible-state invalidation, or explicitly
request redraw on its transitions. Document the bounded press/release
frames as an exception to "no extra frame"; no timer or hover tracking is
needed.

Test press-without-motion, release-without-motion and capture loss. The
active/inactive pixels must update while scroll, text and selections stay
unchanged; redundant events must still avoid repainting.

#### Review Validation

- Source inspection and the public Win32 `SetWindowPos` contract only.
  These are design gaps, not reproduced implementation failures.
- No tests, builds, benchmarks or native full-screen experiments were run.
- E03 already exists in `xtask/src/perf.rs`: `End,Home` on the 64 MiB
  line, timing both steps. Reuse that baseline rather than treating it as
  a new test. Record any deliberate procedure change explicitly.
- Only this reviewer record was appended. The design, earlier records and
  pending status are preserved.

### 2026_10_25 - Claude Opus 5.5

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Accepted M7R1-M7R4 after checking them against the code; the
  design is revised as below. The owner's scope and UI choices are
  unchanged. Awaiting re-review.

#### Response M7R1: Accepted

Confirmed: `covered_lines` inserts every touched line into a `BTreeSet`, and
`Edit.text` owns its bytes. Line commands now work on merged line blocks
(per selection, never per line) and check one budget before reading text or
building per-line edits: 64 MiB copied and 1,048,576 lines edited per
command, refused with `Refusal::TooLarge` and no partial edits. Move copies
the smaller side (neighbor or block); Copy counts all blocks together;
Delete copies nothing. The same block helper and line budget replace
`covered_lines` for `Tab` / `Shift+Tab` and the comment toggle. Sharing
snapshot bytes in an `Edit` is recorded as a rejected alternative. Tests
cover the tiny line beside a 65 MiB neighbor, 2,000,000 lines in one
selection, aggregate copies over the budget, and refusal before reading
text; E04 and E05 time Move and Copy Line.

#### Response M7R2: Accepted

Confirmed: `WM_NCCALCSIZE` subtracts the resize borders whenever
`IsZoomed`, and `WM_DPICHANGED` applies the suggested rectangle. The
platform now owns the full-screen state (saved placement and style), set
before the frame recalculation; the frame code skips the maximized insets
while it is set; DPI and display changes re-cover the current monitor;
system move, size, maximize and restore leave full screen first; failed
placement or monitor calls leave the window unchanged with a notice; the
editor follows `Event::FullScreen`. Validation adds an on-demand native
test comparing the client rectangle with `rcMonitor` from normal and
maximized windows, across 100 %/150 % monitors, and the restored placement
and show state.

#### Response M7R3: Accepted

Confirmed: `walk` decodes `line[..64 KiB]` with `utf8_chunks`, so a
character split by the cut counts as invalid bytes. `rte_edit` gets one
`scan_end` seam (the last character boundary at or before 64 KiB) used by
every component; beyond it each character draws once at its first byte's
column, the decoder completes characters across viewport edges by up to
3 bytes, zero-width marks keep the per-cell limit. Tests add 2-, 3- and
4-byte characters straddling 64 KiB and the viewport edges, combining marks,
tabs, real invalid bytes and CRLF, asserting glyph identity and columns.

#### Response M7R4: Accepted

Confirmed: `Visible` has no drag state. It gets `slider_active`, so press
and release repaint once each; capture loss already reports `MouseUp`. The
design now states these two frames as the only extra frames. Tests cover
press and release without motion, capture loss, and no repaint for
repeated events.

#### Response Validation

- Source inspection of `commands.rs`, `change.rs`, `columns.rs`,
  `rte_platform` (`WM_NCCALCSIZE`, `WM_DPICHANGED`, `WM_CAPTURECHANGED`),
  `editor.rs` (`Visible`) and `perf.rs`.
- E03 already exists (`End,Home` on `line.txt`); the design reuses it and
  adds E04 and E05 instead.
- No code, tests or benchmarks run (design only).

### 2026_10_25 - Claude Opus 5.5

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Owner's decision: no limit on any selection or copy (pasting
  300 MB works today). The M7R1 budget and refusals are removed, and the
  existing `Tab` / comment commands stay unchanged. Kept from M7R1: blocks
  found per selection (Move, Copy, Delete never enumerate lines) and Move
  copying the smaller side, so cost follows the bytes copied. Awaiting
  re-review.

### 2026_10_25 - Claude Opus 5.5

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Owner's direction: the user may do anything; make it as fast as
  possible. Move, Copy and Delete Line now copy no bytes: `rte_text` gains
  `Inserted::Copy(range)`, applied with the tree's existing `split` and
  `concat` in O(log n) for any size, sharing buffers. Typing keeps the
  byte path. Tests and steps updated. Awaiting re-review.

## Implementer

### 2026_10_25 - Claude Opus 5.5

- Role: Implementer
- Activity: Implementation
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Implemented at the owner's request before the revised design was
  re-reviewed. All steps are in: shared texts in `rte_text`, the line
  commands and comment modes, Go to Line, long lines with seams, `F11`,
  the scroll slider, `Ctrl+K Ctrl+S`, E04 and E05, documentation and
  CHANGELOG. Deviations, recorded in the design above: `Inserted::Shared`
  pieces instead of a single copied range (a last line needs a copy plus a
  line break); Move Line swaps the two texts; column functions keep their
  values (the seam only ends exact drawing); the slider's minimum height is
  one text row; Alt line and page commands do not act from the find box.
  Focused tests, Clippy, formatting, `check-imports` and the focused
  benchmarks (within noise of 0.4.2) pass. Native full-screen checks and the
  manual checks are still open (see Validation Results), so the task stays
  pending.

## Validation Results

- `cargo test -p rte_text -p rte_edit -p rte_view -p rte_render -p rte_platform -p rte_app -p xtask`:
  all pass (rte_text 21 + 2, rte_edit 36, rte_view 35, rte_render 9,
  rte_platform 6 and 1 ignored, rte_app 144 and 1 ignored, xtask 10). New
  tests cover shared copies (randomized against copied bytes, buffer
  sharing, out-of-range sources), every line command and comment mode
  (including a 65 MiB line and a 2,000,000-line selection), seams at
  64 KiB and at the viewport edges, the slider geometry and drawing, the
  title-strip option, the new keys, Go to Line, full screen events, the
  slider drag and its repaints, and the find-box focus rule.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` and
  `cargo fmt --all --check`: pass.
- `cargo xtask check-imports`: pass; the DLL list is unchanged.
- `cargo xtask bench-perf --only L01,L26,L04,L16,E01,E02,E03,E04,E05,M05,S02`:
  A/B against 0.4.2 on a machine other than the published results (numbers
  not published). Both builds ran from temporary worktrees: run from
  `target\release`, the new executable is deleted by the machine's security
  tool after each run and M05 went over its 1 s cap there, while the same
  code in a worktree ran M05 as fast as 0.4.2 (an in-process probe of M05
  also measured equal times). Startup (L01, L26, L04, L16), typing (E01,
  E02), E03, M05 and S02 are within run-to-run noise of 0.4.2; L01, L26 and
  L04 repeated twice with alternating builds differ by at most about 2 % in
  first paint and first frame. The new E04 and E05 run well within a frame;
  their caps are 2 s (E05 needed just over 1 s, mostly load and
  highlighting).
- Not run: the native full-screen check (client rectangle equals
  `rcMonitor` from normal and maximized windows, across 100 % / 150 %
  monitors, restored placement) and the manual checks listed in Tests.

## Implementation Review

### 2026_10_25 - GPT-6.1 Sol

- Role: Reviewer
- Activity: Review
- Agent: AI assistant using Copilot SDK in VS Code
- Model: GPT-6.1 Sol
- Effort: High
- Context Window: 1M
- Outcome: Changes requested for M7I1-M7I7. The focused tests, Clippy and
  formatting pass. Isolated probes reproduce six behavior defects; M7I7 is
  a source-confirmed native failure-handling gap. No application fixes
  authored. The task stays pending and previous records are preserved.

### M7I1 [P1] Shared copies can overflow the logical document size

Locations:
[`Snapshot::apply`](../crates/rte_text/src/snapshot.rs#L271),
[`Summary::add`](../crates/rte_text/src/tree.rs#L41), and
[`ChangeSet`](../crates/rte_text/src/change.rs).

Shared copies make previously impractical logical sizes reachable without
allocating their text. Starting with one byte, append a shared copy of the
whole snapshot 64 times. The debug core probe panics in `Summary::add`.
With the release libraries, copy 64 succeeds but reports a length of zero:
the `u64` sum wrapped. The fixture does not allocate an enormous input.

Validation checks destination/source ranges, not the resulting length or
signed position-mapping arithmetic. Check these before constructing shared
nodes and return an explicit error without changing the document. Propagate
that error through command application rather than an `expect`. This is a
representation-safety check, not a policy size limit; the owner's decision
against arbitrary selection/copy limits remains unchanged.

Add small shared-tree tests around the supported numeric boundaries,
including compound pieces and transactions whose intermediate
construction could overflow. Assert failure before wrapped metadata or
invalid selection mapping is produced.

### M7I2 [P2] Moving selected LF text onto CRLF splits the selection boundary

Location:
[`move_lines` selection mapping](../crates/rte_edit/src/lines.rs#L101).

For `a\nb\r\nc`, select bytes `0..2` (the first line including its LF) and
move down. The text correctly becomes `b\na\r\nc`, but the selection becomes
`2..4`, not `2..5`: its head is between CR and LF. The isolated probe
confirms `snap_to_boundary(head) == 3` while `head == 4`.

The mapping preserves the old break's byte count with
`min(next_break)`. A position after an LF must instead map after the whole
replacement CRLF. Preserve boundary affinity, not a partial break offset.
Cover both directions, LF/CRLF transitions, reversed/full-line selections
and a subsequent typing/delete operation. No produced caret or selection
endpoint may split CRLF.

### M7I3 [P2] File reset leaves the new seam cache and preview alive

Locations:
[`reset_document`](../crates/rte_app/src/editor/lifecycle.rs#L16),
[`seam`](../crates/rte_app/src/editor.rs#L599), and
[`Go to Line preview`](../crates/rte_app/src/editor/goto.rs).

The new per-file `seams` and `line_preview` fields are not reset by the
existing lifecycle.

- Search a 128 KiB single-line file, then Close. The editor is empty, but
  `seams.0` still owns the old snapshot. That reference can pin the entire
  closed file until another long-line request replaces the cache.
- While a dropped file is pending, open Go to Line on the current file.
  When the drop completes, reset closes the popup, then
  `end_line_preview` restores the old file's scroll into the new file. The
  headless probe restores top line 100 into a two-line document, leaving
  its text outside the viewport.

Clear both fields as part of the document transition, before any preview
restoration can run. Preserve session settings such as fullscreen and zoom.
Test Close, replacement by a short file, and a pending open completing
during a preview. Check release of the old snapshot and a valid new
viewport. No process-memory measurement is claimed by the cache probe.

### M7I4 [P2] Alt line shortcuts edit through an open palette

Location: [Alt-key dispatch](../crates/rte_app/src/editor.rs#L1150).

The text-focus guard checks only `find_focused()`. Alt keys are dispatched
before popup handling, so the command palette, language picker and panels
do not protect the document from the new line-edit shortcuts.

The headless probe opens the Command Palette over `one\ntwo`, then sends
`Alt+Down`. The document becomes `two\none` while the palette has focus.
Gate these shortcuts on actual editor-text focus, including the absence
of a popup. Keep explicit execution of a line command from the palette
working. Cover Move/Copy Line and page-scroll keys across editor, find,
palette and panel contexts.

### M7I5 [P2] Extreme Go to Line columns wrap instead of clamping

Locations:
[`go_to_line`](../crates/rte_app/src/editor/goto.rs#L87) and
[`offset_at_column`](../crates/rte_edit/src/columns.rs#L136).

On a line containing two U+00E9 characters (four UTF-8 bytes), enter
`1:18446744073709551615` in Go to Line. The parser accepts this `u64`
column. The debug editor probe panics in the offset calculation; the
release helper returns byte 0 instead of the line end at byte 4.

The final `reached + ...` can overflow before `offset_in_line` clamps the
result. Bound or check the arithmetic before addition, then clamp to the
content boundary. Test the actual query/Enter path on ASCII, multibyte,
wide and combining text. The ordinary out-of-range column control
`1:999999` already clamps correctly.

### M7I6 [P2] Combining marks disappear at the long-line seam

Locations:
[`long-row drawing`](../crates/rte_view/src/lib.rs) and
[`layout_beyond`](../crates/rte_view/src/lib.rs#L1107).

Render 65,536 ASCII `a` bytes followed by U+0301 and `z`, with the view
spanning the seam. The last `a` is drawn, but its combining mark is absent
from the display list. The exact-prefix layout knows the previous visible
cell; the tail layout starts with `last = None` and drops the mark.

Carry the preceding visible cell and its mark count across the seam (or
use an equivalent shared layout state). Preserve `MAX_MARKS_PER_CELL`
across both paths. Test marks starting at and straddling the seam,
including four marks already drawn before it. The current test starts the
base character inside the tail and therefore misses this transition.

### M7I7 [P2] Fullscreen reports success before native calls succeed

Locations:
[`set_full_screen`](../crates/rte_platform/src/lib.rs#L776),
[`cover_monitor`](../crates/rte_platform/src/lib.rs#L753), and
[`leave_full_screen`](../crates/rte_platform/src/lib.rs#L810).

Entry stores the saved state and sends `FullScreen(Ok(true))` before the
style and positioning calls. `cover_monitor` silently returns on a failed
monitor lookup and ignores `SetWindowPos`'s result. Exit removes the saved
state and sends `Ok(false)` before checking restoration; its essential
native results are also ignored.

Thus the promised failure notice and unchanged state are not implemented
when Windows refuses a transition. Check essential style, monitor and
placement operations; retain the recovery state until success and report
failure/rollback accurately. This does not require treating optional corner
styling as fatal. Add controlled failure checks for entry and exit, not
only a headless request flag. No native failure was injected in this review.

### Review Validation Results

| Command or procedure | Result |
| -------------------- | ------ |
| `cargo test -p rte_text -p rte_edit -p rte_view -p rte_render -p rte_platform -p rte_app -p xtask --locked` | 263 passed, 2 ignored |
| `cargo clippy -p rte_text -p rte_edit -p rte_view -p rte_render -p rte_platform -p rte_app -p xtask --all-targets --locked -- -D warnings` | Pass |
| `cargo fmt --all --check` | Pass |
| Core debug harness: `review_ --nocapture --test-threads=1` | 4 assertions failed: M7I1, M7I2, M7I5, M7I6 |
| Editor debug harness: `review_m7_ --nocapture --test-threads=1` | 4 assertions failed: both M7I3 cases, M7I4, M7I5; ordinary column control passed |
| Core release harness: `review_release_ --nocapture --test-threads=1` | 2 assertions failed: M7I1 wraps length to 0; M7I5 returns offset 0 |

The disposable harnesses were compiled from stdin against Cargo-reported
artifacts. The editor harness adjusted module/include paths and added tests
in memory; production files were unchanged. Core and editor harnesses exited
101. Failed assertions demonstrate defects, not passing regression checks.

Dependency/artifact resolution used the focused `cargo test --no-run
--locked --message-format=json` commands: `rte_text`, `rte_edit` and
`rte_view` for the debug core harness; `rte_app` for the editor harness; and
`rte_text`, `rte_edit` with `--release` for the release harness. The latter
did not run the release crate suites. Release probe codegen matched
production: opt-level 3, fat LTO, one codegen unit and x86-64-v3. An initial
link attempt without matching LTO was corrected before running the probes.
Executables and the failed-link object files were removed.

Not run: the full workspace tests, performance suites, startup A/B, a new
release-import check, native fullscreen/monitor experiments or manual
checks. Existing implementation measurements are not claimed as fresh
review validation. No user file was saved, no system clipboard was changed
and no normal application window was inspected or controlled.

## Implementation Review Response

### 2026_10_25 - Claude Opus 5.5

- Role: Implementer
- Activity: Implementation
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Fixed M7I1-M7I7, each with a regression test. None of the
  findings affects speed; M7I3 also released a closed file's text kept by
  the long-line cache. The A/B against 0.4.2 is within noise. Native
  full-screen and manual checks remain open; the task stays pending.

#### Response M7I1: Accepted

`Transaction::validate` bounds the base length plus every inserted byte
(owned and shared, with checked sums) by `MAX_LEN` (`i64::MAX`, the range of
position deltas) and returns `EditError::TooLarge` before any tree or
mapping is built. That sum also bounds every intermediate tree. The
document reports it as a refused edit instead of the former `expect`. Not a
size policy: copies stop only where positions cannot represent the text.
Tests: 62 doublings succeed and the 63rd is refused with the text
unchanged; pieces whose sum overflows; a transaction that passes the bound
only together.

#### Response M7I2: Accepted

A position after a moved block's line break now maps after the whole break
that follows the block afterwards (CRLF or LF), never into it. Tests: both
directions, LF to CRLF and back, a reversed selection, and typing over the
moved selection.

#### Response M7I3: Accepted

`reset_document` clears the seam cache (releasing its snapshot) and the Go
to Line preview, so nothing of the old file is restored into the new one;
full screen and zoom stay. Tests: Close after scanning a 128 KiB line, and
a dropped file opening during a preview (scroll at the top, popup closed).

#### Response M7I4: Accepted

Alt line and page commands need the text focus: no focused find box and no
popup. The palette still runs them by name. Tests: palette and Keyboard
Shortcuts panel with Move Line, Copy Line and Scroll Page keys, then Move
Line Down from the palette.

#### Response M7I5: Accepted

`offset_at_column` saturates past the scanned bytes and `offset_in_line`
adds with saturation before clamping to the line end. Tests: the helpers
with `u64::MAX`, and the Go to Line query and `Enter` on multibyte, wide,
combining and ASCII lines.

#### Response M7I6: Accepted

`layout_line` returns `LineEnd` with the last drawn cell and its mark
count; `layout_beyond` continues from it when its bytes start at the seam,
keeping `MAX_MARKS_PER_CELL`. Tests: a mark right at the seam, one
straddling it, and a fifth mark after four drawn before the seam.

#### Response M7I7: Accepted

Full screen runs through a `Screen` trait (the native window, or a fake in
tests). Entry: the saved state is set before the style change, a style that
did not take is rolled back with `Err`, `Ok(true)` follows the style
change, and a failed monitor sizing leaves full screen again and reports
it. Exit: a frame that cannot be restored keeps full screen and reports
`Err`; otherwise `Ok(false)`, then a failed placement restore is reported.
A monitor or DPI change that cannot be covered ends full screen. Corner
styling stays optional. Tests make each step fail and check the events and
state.

#### Response Validation

- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass.
- `cargo test -p rte_text -p rte_edit -p rte_view -p rte_render -p rte_platform -p rte_app -p xtask --locked`:
  all pass (rte_text 22 + 2, rte_edit 36, rte_view 36, rte_render 9,
  rte_platform 7 and 1 ignored, rte_app 147 and 1 ignored, xtask 10).
- `cargo xtask check-imports`: pass; the DLL list is unchanged.
- `cargo xtask bench-perf --only L01,L26,L04,L16,E01,E02,E03,E04,E05,M05,S02`
  against 0.4.2, both from temporary worktrees, on a machine other than the
  published results (numbers not published): within run-to-run noise. A
  single slow L04 first frame did not repeat in two alternating reruns. E01
  varies with the overlap of typing and the background parse of the 8 MiB
  file on both builds; ten alternating runs each give medians within about
  0.1 ms.
- Not run: native full-screen checks and the manual checks listed in Tests.
- Completion: marked done by the owner on 2026_10_28 and moved to `Done/`.
  The native full-screen and manual checks were not run by the implementer.
