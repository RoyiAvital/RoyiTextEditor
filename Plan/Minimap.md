# Minimap

## Task

Show a VS Code style minimap on the right of the text: an outline of the
lines around the view, each character a small block in its color, with a
slider marking the visible rows. Clicking or dragging it scrolls. It gives
an overview of the code's shape and fast scrolling.

Startup and the feel of typing and scrolling must not change.

## Scope

### Included

- **Shown** whenever a file is open and the window is wide enough (VS Code
  default `editor.minimap.enabled: true`). Not in the empty state.
- **View: Toggle Minimap** in the palette (VS Code
  `editor.action.toggleMinimap`, no default key). Lasts for the session;
  not persisted (no settings).
- **Geometry (VS Code defaults `scale: 1`, `maxColumn: 120`,
  `size: proportional`):** a unit `u = max(1, round(dpi / 96))` pixels;
  each line is `2u` pixels high, each column `u` pixels wide; the minimap
  is `120u` pixels plus a `2u` gap, the full height of the body, taken from
  the right of the text area. Font zoom does not change it. Hidden while
  the text area would keep fewer than 60 columns.
- **Content:** the first 120 display columns of each line (tabs expanded,
  wide characters two columns), at most 512 bytes read per row. Whitespace
  is blank; each run of non-space characters of one color is one block of
  that color blended toward the background (about 60 %). Colors are the
  syntax colors where known, the text color otherwise. Lines touched by a
  selection get the selection color behind their blocks.
- **Scrolling map:** with `N` lines and `R` minimap rows, the minimap shows
  lines from `0` when `N ≤ R`, otherwise from
  `round(top_line × (N − R) / (N − 1))`, so the first and last lines are
  reachable. The slider covers the visible rows; it is always drawn
  (faint; brighter while dragged).
- **Mouse:** a press on the slider drags it: the view scrolls in proportion
  to the pointer's movement over the slider's travel. A press elsewhere
  centers the clicked line in the view, then drags from there. The caret
  and selections never change. The wheel over the minimap scrolls as over
  the text; the pointer is the arrow.
- Works while loading (the loaded part), read-only, and for 256 MiB files
  and long lines (only 512 bytes per row are read).

### Excluded

- Rendering tiny characters (VS Code `renderCharacters: true`): blocks look
  nearly the same at this size without a second glyph cache.
- Search match marks: only the matches of the visible rows exist today;
  marking the minimap needs a search job per edit (a later task).
- Slider shown only on mouse-over (VS Code `showSlider: mouseover`): needs
  hover tracking; the slider is the scroll hint here.
- Settings (side, scale, columns, size modes), persisted toggle.
- Error, diff and bookmark marks (no LSP, Git or bookmarks).
- Other scroll hints: the scroll indicator is part of
  [M7Polish](../Done/M7Polish.md); the minimap sits to the left of its strip.

### Compatibility Constraints

- No startup work, no new thread, no new DLL import, no new setting.
- No change to the syntax thread's parsing or interest; the minimap only
  asks for spans of text it already parses.
- Single session ([Architecture](Architecture.md), Principle 8).

## Design

### `rte_view`

- `Options.minimap: u32`: the unit in pixels; `0` hides it. `Layout`
  gets `minimap: Rect` (zero width when hidden) and the text area ends
  before it.
- `minimap.rs` (pure): `MinimapGeometry { rect, unit, first_line, rows,
  slider: Rect }` from `(layout, line_count, scroll, page)`; used by the
  drawing and by the editor's hit testing, so both agree.
- `draw_minimap(snapshot, geometry, colors, selections, out)` pushes fills
  only (no glyphs) before the text glyphs: the background, selection rows,
  the blocks, the slider. Bytes are read with `Snapshot::chunks` into the
  builder's reused buffer, at most 512 per row.
- `Highlights.minimap_syntax: &[SyntaxSpan]`: spans for the minimap rows.

### `rte_app`

- The editor sets `Options.minimap` from the DPI and the toggle.
- **Syntax colors, off the UI thread only:** `Syntax::minimap_spans(ranges)`
  returns the last spans the worker sent for the minimap and, when the text
  or the ranges changed, asks again. A separate `Msg::MinimapSpans` slot in
  the worker is answered after the visible spans, only when its parse is
  current, and any new message interrupts it (as the visible spans). The
  last reply is drawn even when a little stale (after an edit, positions
  may be a few bytes off until the next reply); rows outside it use the
  text color. The UI never paints minimap spans itself, so the visible
  rows keep the whole 1 ms budget. A reply bumps the syntax generation, so
  the frame redraws.
- Plain text, a stopped or failed syntax thread: text color only, no
  requests.
- **Mouse:** `mouse_down` checks the minimap before the text;
  `DragMode::Minimap { grab_y, top }` scrolls in `drag_move`; `MouseUp`
  ends it. Scrolling uses `scroll_lines` / `clamp_scroll`.
- **Command:** `Action::ToggleMinimap`, a registry entry without keys,
  not available in the empty state.

### `rte_platform`

Not applicable: mouse, wheel and cursor shape already reach the editor.

### Threading, persistence, failure

- No new thread; the syntax worker answers the minimap after its other work.
- Nothing persisted.
- Failure: no syntax colors means text-colored blocks; nothing else depends
  on the minimap.

## Alternatives

- **A cached minimap bitmap updated incrementally (Sublime Text).** Faster
  frames, but invalidation on edits, scrolling, syntax replies and resizes.
  Deferred: build per frame first and cache only if the benchmarks show a
  cost.
- **Syntax spans for the minimap on the UI thread with the 1 ms budget.**
  Rejected: competes with the visible rows on every keystroke.
- **The whole document scaled into the minimap.** Rejected: needs a scan of
  the whole file per change; not bounded by the view.
- **Tiny characters.** See Excluded.

## Runtime Effects

- **Startup:** no extra work before the first frame; the first frame draws
  text-colored blocks (syntax arrives later). Expected well under 1 ms;
  measured by L01, L26 and L04.
- **Per frame:** about `R` rows (about 500 on a 1,000-pixel-high body at
  any DPI), at most 512 bytes each, usually 30 to 60 KB read; a few thousand
  fills of a few pixels each.
- **Syntax thread:** one extra span job after an edit or scroll, cancelled
  by newer work.
- **Memory:** the minimap spans (about ten times the visible rows' spans).
- **Disabled path** (toggled off, too narrow, empty state): no reading, no
  span requests, no fills.

## Tests

View (`cargo test -p rte_view`):

- Geometry: `N ≤ R`; `N > R` at the top, middle and bottom; slider position
  and size; a document of millions of lines; hidden below 60 text columns;
  the text area narrows by the minimap width.
- Drawing: ASCII, tabs, wide characters, CRLF; at most 512 bytes read on the
  64 MiB single line; span colors and blending; selection rows; nothing in
  the empty state or with `minimap: 0`.

Syntax (`cargo test -p rte_app syntax`):

- Minimap spans arrive from the worker, are kept when stale, do not touch the
  visible spans' cache, and are not requested without syntax.

Editor (`cargo test -p rte_app`):

- A click centers the clicked line without moving the caret; slider drags
  reach the first and last lines; the wheel over the minimap scrolls; the
  toggle hides and shows it; read-only works; no minimap in the empty
  state; the hit test agrees with the drawn geometry.

Performance (focused; Benchmark v1):

- `cargo xtask bench-perf --only L01,L26,L04,E01,E02,M05,S01,S02,S03`:
  within noise of the run before the change.
- `cargo bench -p rte_render`: the frame benchmark with the minimap.
- `cargo xtask check-imports`: the same DLL list.

Manual: 100 %, 150 % and 200 % scaling; resizing below and above the width
limit; `large.md` and `line.txt`; a code file while its syntax arrives.

## Implementation Steps

1. `rte_view`: options, layout, `minimap.rs` geometry and drawing; tests.
2. `rte_app` syntax: the minimap span request and reply; tests.
3. Editor: options from DPI, paint, mouse, toggle command; tests.
4. Documentation (README, docs pages, Architecture), CHANGELOG.
5. Focused validation and benchmarks.

## Acceptance Criteria

- With a file open, the minimap shows the lines around the view in their
  colors, with the slider on the visible rows.
- Click and drag scroll as described; the caret never changes.
- View: Toggle Minimap hides and shows it for the session.
- Startup (L01, L26, L04), typing (E01, E02, M05) and scrolling (S01 to S03)
  are within noise; no new DLL import; focused tests, Clippy and formatting
  pass.

## Reviewers

### 2026_10_25 - Claude Opus 5.5

- Role: Reviewer
- Activity: Design
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Designed at the owner's request with VS Code's defaults: always
  on with a session-only palette toggle, character blocks instead of tiny
  characters, an always visible slider, syntax colors only from the syntax
  thread, no search marks. Awaiting the owner's confirmation of these
  choices and review.

### 2026_10_25 - Claude Opus 5.5

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: The scroll indicator moved to M7; the minimap sits to the left of
  its strip. No other change.
