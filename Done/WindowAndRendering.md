# Window and Rendering

## Task

Milestone M2 of [Architecture](../Plan/Architecture.md): the first running
application. A frameless window shows a file read-only with line numbers,
scrolls, and measures startup. The gate decides whether the CPU renderer is
fast enough or a Direct3D 11 presenter is needed.

## Scope

### Included

- `rte_view`: integer cell layout (title strip, gutter, text, status line),
  visible-row layout (tabs, wide, zero-width, control and invalid bytes,
  CRLF as one break, 64 KiB per-row scan bound) into a display list.
- `rte_render`: `swash` glyph rasterization (hinted, grayscale), glyph cache
  (ASCII warmed at startup), CPU drawing of fills and glyphs.
- `rte_platform`: Win32 frameless window (caption removed, DWM shadow, rounded
  corners on Windows 11, edge/corner resizing, maximize insets), per-monitor
  DPI v2, cloaked first paint, DIB section + `BitBlt`, keyboard, wheel, wake-ups,
  startup timing.
- `rte_app` (`RoyiTextEditor.exe`): parallel startup (loader and glyph
  threads, window creation), progressive block loading, read-only scrolling
  view, zoom, `--read-only`, `--trace-startup`, `--exit-after-first-frame`,
  `--exit-after-load`.
- Bundled JetBrains Mono (OFL); Regular embedded in the executable.
- `xtask`: `bench-startup`, `check-imports`, `package`.
- Release workflow rewritten for Rust; inherited Pages workflow set to manual.
- Code generation for AVX2-class CPUs (`x86-64-v3`).

### Excluded

- Editing, carets, selections, saving (M3); commands, palette, configuration
  (M4); search (M5); syntax (M6).
- Damage tracking (not needed by measurement, see Validation Results).
- The other four font families (files added in a later task).
- Application icon and version resources.

### Compatibility Constraints

- CPUs without AVX2 are not supported.
- No Registry writes; no files written by the application.

## Design

- **Startup.** `main` → DPI awareness → spawn loader (first 1 MiB block) and
  glyph thread (embedded font, ASCII) → create hidden window → wait for the
  first snapshot and glyphs → cloak, show, paint, uncloak, `DwmFlush` →
  `Shown` (trace mark `first_frame`) → message loop.
- **Loader.** 1 MiB first block, then 16 MiB blocks; BOM detection, UTF-16
  decoding, prefix snapshots at most every 50 ms; `Done` carries encoding,
  dominant line ending and the malformed flag. Messages travel over a channel;
  a `Waker` posts `WM_APP + 1`.
- **Platform calls into the app** through `App` (`event`, `paint`,
  `is_caption`). Requests (close, redraw, title) are deferred until the call
  returns; re-entrant messages never re-enter the app.
- **Rendering.** Full redraw per paint into the DIB, written opaque.

## Alternatives

- **`winit` + `softbuffer`:** rejected in the architecture (startup control,
  frameless behavior).
- **Font files only:** the default Regular face is embedded instead, so
  startup reads no font file (~274 KB in the executable).
- **Damage tracking now:** deferred; full frames measured at 0.36 ms (1080p)
  and 1.16 ms (4K).

## Runtime Effects

- Startup: two helper threads, one file read stream, no COM, no GPU device.
- Idle: no timers, no wake-ups.
- Memory: idle private memory 4.8 MiB (empty document, 1100 × 750 client).
- Large files: loader thread reads blocks until done; UI stays responsive.

## Tests

- `cargo test --workspace --locked`: view layout (tabs, wide, combining,
  control, invalid, clipping, CRLF), renderer (metrics, glyph inside its cell,
  clipping), loader (BOM, UTF-16, malformed, prefix-first publication, missing
  file), plus all `rte_text` tests.
- `cargo xtask bench-startup` (gate: p95 `first_frame` < 100 ms).
- `cargo bench -p rte_render` (full-frame cost at 1080p and 4K).
- `cargo xtask package` (import check, ZIP).
- Manual: launch, screenshot, scrolling, resize, close.

## Implementation Steps

1. `rte_view` layout and tests.
2. `rte_render` rasterizer and tests.
3. `rte_platform` window.
4. `rte_app` startup, loader, view.
5. `xtask` tasks, frame bench, workflows.
6. Measurements and documentation.

## Acceptance Criteria

- `RoyiTextEditor.exe <file>` shows the file in a frameless window with line
  numbers, scrolls with wheel and keys, closes with `Ctrl+W`/`Alt+F4`.
- Startup gate passes; frame budgets hold at 1080p and 4K.
- The package imports no MinGW runtime DLL.

## Reviewers

### 2026_10_23 - Claude Opus 5.5

- Role: Reviewer
- Activity: Design
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Task defined from the approved architecture (M2) and the user's
  instruction to reach a running application with AVX2 code generation.

## Implementer

### 2026_10_23 - Claude Opus 5.5

- Role: Implementer
- Activity: Implementation
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Running application delivered. Startup gate passes (p95 38 ms);
  full frames cost 0.36 ms (1080p) and 1.16 ms (4K), so no D3D11 presenter is
  needed. Inherited `release.yml` replaced (a copy of the old file was kept
  outside the repository during the edit); `pages.yml` set to manual.

## Validation Results

Machine (development, not yet the frozen reference): AMD Ryzen 9 9950X,
94 GiB RAM, NVIDIA RTX 4000 Ada, 2560 × 1600 at 59 Hz, Windows 11 Pro for
Workstations, release build, warm cache.

| Command or check                                        | Outcome                                     |
| ------------------------------------------------------- | ------------------------------------------- |
| `cargo fmt --all --check`                               | Pass                                        |
| `cargo clippy --workspace --all-targets --locked`       | Pass (`-D warnings`)                        |
| `cargo test --workspace --locked`                       | 33 tests pass                               |
| `cargo xtask bench-startup` (50 runs, 5 warm-up)        | Gate PASS, see below                        |
| `cargo bench -p rte_render`                             | 1080p 0.36 ms, 4K 1.16 ms (median)          |
| `cargo xtask package`                                   | ZIP created; no MinGW runtime imports       |
| 512 MiB log file, 3 runs                                | First frame 24-30 ms, loaded 195-213 ms     |
| Idle memory (empty document)                            | Private 4.8 MiB, working set 15.2 MiB       |
| Screenshot of `rte_view/src/lib.rs` with `--read-only`  | Text, gutter, title, status rendered        |

Startup stages on a generated 300-line C file (ms since process creation):

| Stage            | Min   | Median | p95   | Max   |
| ---------------- | ----- | ------ | ----- | ----- |
| `main`           | 4.66  | 4.98   | 5.50  | 5.57  |
| `window_created` | 7.05  | 7.46   | 8.18  | 8.24  |
| `first_snapshot` | 7.05  | 7.46   | 8.18  | 8.24  |
| `glyphs_ready`   | 7.05  | 7.46   | 8.19  | 8.24  |
| `first_frame`    | 22.24 | 23.87  | 38.04 | 38.21 |

`first_frame` includes waiting for DWM composition (`DwmFlush`, up to one
refresh interval). Imports: `kernel32`, `user32`, `gdi32`, `dwmapi`, `ntdll`,
`msvcrt`, `bcryptprimitives`, `api-ms-win-core-synch-l1-2-0` (the last four
from the Rust standard library).

Not run: GitHub-hosted CI and release workflows, cold start after reboot,
4K and 144 Hz displays, multi-monitor DPI moves.

## Implementation Review

### 2026_10_23 - GPT-6.1 Sol

- Role: Reviewer
- Activity: Review
- Agent: GitHub Copilot
- Model: GPT-6.1 Sol
- Effort: High
- Context Window: 1M
- Outcome: Changes requested for M2R1-M2R7 below. The ordinary startup gate,
  focused tests, formatting, Clippy and packaging pass. Boundary probes expose
  stuck loading, UTF-16 expansion failure, excessive frame work and layout
  defects; an isolated harness probe also produces a false-positive gate.
  No application or build fixes were authored. Historical completion records
  are preserved; this review does not approve M2.

### M2R1 [P1] Early loader completion loses its wake-up

Locations: [loader launch](../crates/rte_app/src/main.rs#L66),
[waker installation](../crates/rte_app/src/main.rs#L95), and
[queue drain](../crates/rte_app/src/editor.rs#L272).
Notifications sent before the `OnceLock<Waker>` is initialized are discarded.
Startup receives only one event. If that is `Progress` and `Done` is already
queued, no later wake drains it, so the view stays at the prefix in Loading.
Eight real release launches with a generated, exactly 1 MiB fixture and
`--trace-startup --exit-after-load` all required intervention after 1.5 s;
each child was then closed, with no lingering process. Arrange an initial
queue drain or pending-notification handshake when installing the waker.
Add a deterministic test that completes the loader before waker installation,
then requires the final snapshot, Ready state and normal exit-after-load.

### M2R2 [P1] Valid UTF-16 expansion panics the loader

Location: [decoded block publication](../crates/rte_app/src/loader.rs#L103).
A 16 MiB UTF-16 block can expand to 24 MiB of UTF-8, but `push_block` permits
only `rte_text::BUFFER_MAX` (16 MiB). A valid 18 MiB plus BOM file of U+4E2D
reproduces `block exceeds BUFFER_MAX` in both LE and BE. Only a prefix event
survives; no Done or Failed event is produced. Split decoded output into
allowed-size shared blocks, and convert worker failure/disconnection into a
visible failure instead of leaving Loading indefinitely. Add LE/BE regression
tests spanning the second input block with three-byte UTF-8 output.

### M2R3 [P1] The row byte bound does not bound frame work

Locations: [row scan](../crates/rte_view/src/lib.rs#L265),
[zero-width emission](../crates/rte_view/src/lib.rs#L347), and
[renderer](../crates/rte_render/src/lib.rs#L177).
A base character followed by combining marks stays in one visible cell, but
every mark enters the display list and is rasterized. At 1920 x 1080, rows of
`a` plus 32,760 U+0301 marks generated 1,670,904 glyph entries. Warm release
layout plus drawing took about 50.3 ms per frame, versus 0.25 ms for short
ASCII and 1.2-1.3 ms for 64 KiB ASCII rows. These timings exclude presentation
and already exceed the 4 ms frame budget. Bound emitted glyphs/work per row
and frame, with an explicit truncation or deferred-rendering policy. Add this
case to the frame benchmarks; short ASCII alone does not justify the gate.

### M2R4 [P2] Startup gate ignores failed launches and missing samples

Location: [child launch and trace collection](../xtask/src/main.rs#L224).
The harness checks only whether spawning succeeded, not the child's exit
status, and pools whatever trace lines arrive. It requires neither one valid
first-frame sample per run nor successful warm-ups. An isolated copy of the
unchanged harness ran three controlled children, all exiting 7; only the
first printed `startup.first_frame_ms=1.000`. The harness nevertheless reported
PASS and exited 0. Require successful child status and complete, finite,
nonnegative samples for every measured run. Bound child duration and collect
diagnostics on failure; the current `output()` can wait indefinitely. Add
tests for failed, silent, malformed, duplicate and timed-out child results.

### M2R5 [P2] Tiny paint dimensions underflow chrome layout

Locations: [title centering](../crates/rte_view/src/lib.rs#L218),
[status centering](../crates/rte_view/src/lib.rs#L228), and
[zero-size paint clamp](../crates/rte_platform/src/lib.rs#L661).
The platform turns zero-sized paints into 1 x 1. `ViewBuilder::build` then
subtracts a full cell height from shorter title/status rectangles using `u32`.
A headless 1 x 1 debug paint panics with subtraction overflow; release builds
wrap the geometry instead. Use saturating or signed centering and skip empty
areas. Add zero/tiny-size view tests and a native minimize/restore regression.
The native minimize crash itself was not executed in this review.

### M2R6 [P2] Partial body rows draw over the status line

Locations: [partial-row count](../crates/rte_view/src/lib.rs#L130) and
[framebuffer-only glyph clipping](../crates/rte_render/src/lib.rs#L217).
The layout includes a partial last row, but glyphs carry no text/gutter clip
and the renderer clips only to the framebuffer. Body glyphs are drawn after
status glyphs and can overwrite them. Rendering `M` rows with empty chrome
at width 400 and heights 200-219 reproduced status-strip contamination at
14 heights, including 119 unexpected pixels at height 201. Carry region clips
through the display list and rasterizer, including wide glyphs at horizontal
edges. Add pixel tests preserving an empty status strip at partial-row sizes.

### M2R7 [P2] A final lone carriage return is hidden

Location: [CRLF handling](../crates/rte_view/src/lib.rs#L271).
`line_range` excludes LF, but the view strips any trailing CR without checking
whether LF actually follows. For bytes `a\r`, the glyph list contains only
the line number and `a`, losing the required control glyph. Strip CR only
when it belongs to a real CRLF. Add final lone-CR, CRLF, and repeated-CR tests;
the existing CRLF fixture does not cover EOF without LF.

### Review Validation Results

Successful focused commands:

```powershell
cargo test -p rte_view -p rte_render -p rte_platform -p rte_app -p rte_assets -p xtask --locked
cargo build -p rte_app --release --locked
cargo clippy -p rte_view -p rte_render -p rte_platform -p rte_app -p rte_assets -p xtask --all-targets --locked -- -D warnings
cargo fmt --all --check
cargo xtask bench-startup --runs 50 --warmup 5
cargo xtask package
```

- Existing M2 tests: 4 loader, 6 view and 4 render tests pass. The platform,
  assets and harness have no unit tests. The loader checks were also run with
  `target/m2_review_probe.exe loader::tests:: --nocapture` to expose their
  output outside the Windows-subsystem test executable. No full workspace
  suite was run.
- Startup: 50 measured launches and 5 warm-ups on the generated C file;
  first-frame median 27.88 ms, p95 38.47 ms. This successful normal workload
  does not invalidate the gate failure-handling finding.
- Package: ZIP produced; the import checker found no forbidden MinGW runtime
  DLLs. No release was published and no freeze/clean command was run.
- Disposable regression harness: original loader source included by path,
  linked against debug text/view libraries, source supplied through stdin.
  `target/m2_review_probe.exe review_ --nocapture` failed all three checks:
  UTF-16 expansion (both byte orders), lone-CR visibility and tiny-size layout.
  Exit 101 is defect evidence, not a passing regression result.
- Wake race: eight release processes over a generated 1 MiB fixture, each
  with `--trace-startup --exit-after-load`; wait up to 1.5 s, record whether
  it exits, otherwise close and join it. All eight needed closure. The
  fixture was removed; only review-owned children were controlled.
- Rendering: release probes compiled with `opt-level=3`, `lto=fat`,
  `codegen-units=1`, and the project's `x86-64-v3` target. After one warm-up,
  three samples per workload measured layout and drawing without GDI/DWM.
  `target/m2_render_review_probe.exe` and
  `target/m2_render_clip_review.exe` reproduce M2R3/M2R6. These are narrow
  probes, not a frozen-hardware or display-refresh certification.
- Gate robustness: compile unchanged `xtask/src/main.rs` into an isolated
  `target/m2-gate-review/` fixture with a successful fake build command and
  controlled failing child. Run `review-xtask.exe bench-startup --runs 3
  --warmup 0 <sample.c>`: one sample, all three children exit 7, gate exits 0.
  The real application and harness binaries were not replaced.
- All probe executables and generated fixtures are under ignored `target/`
  or temporary storage. No application source or source tests were changed.
- Recording checks: preserve the original task-document byte prefix, verify
  one appended reviewer record and M2R1-M2R7, resolve new file/line links,
  and compare 25 implementation/build file hashes.
- Not run: GitHub-hosted CI/release, cold start after reboot, native
  minimize/restore failure reproduction, multi-monitor DPI moves, snap-layout
  checks, or an actual 4K/144 Hz display. The reference-machine gate remains
  distinct from these local checks.

## Review Response

### 2026_10_23 - Claude Opus 5.5

- Role: Implementer
- Activity: Implementation
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: M2R1-M2R7 fixed with regression tests; all reproduced probes now
  pass. Worst-case combining-mark frame 3.8 ms (was about 50 ms).

### Response M2R1

Fixed. [`loader::spawn`](../crates/rte_app/src/loader.rs#L58) wraps the loader
thread with [`WakeOnDrop`](../crates/rte_app/src/loader.rs#L47), so the UI is
woken after the last event and again when the thread ends.
[`Editor::drain_loads`](../crates/rte_app/src/editor.rs#L114) runs once in
`Editor::new` (after the waker is installed) and on every wake, so events
queued before the window existed are applied. A disconnected loader that sent
no final event becomes `Failed("loading stopped unexpectedly")`.
Tests: `loader_finished_before_the_waker_is_applied_at_startup` (Ready, final
snapshot, `--exit-after-load` closes), `late_events_are_applied_on_wake`,
`loader_that_stops_without_a_final_event_is_reported`,
`spawned_loader_wakes_after_its_last_event`.

### Response M2R2

Fixed. Decoded UTF-16 output is pushed in
[`BUFFER_MAX` parts](../crates/rte_app/src/loader.rs#L126). Worker failure or
disconnection is visible through M2R1's handling. Test:
`utf16_expanding_past_the_buffer_limit_loads` (LE and BE, U+4E2D across the
second input block, 25.5 MiB of UTF-8).

### Response M2R3

Fixed by an explicit truncation policy:
[`MAX_MARKS_PER_CELL`](../crates/rte_view/src/lib.rs#L17) (4) zero-width marks
are drawn per cell; further marks are skipped, not removed from the text. With
the existing 64 KiB row scan bound, glyphs per row are at most
`(cols + 1) × 5` and scanned bytes at most 64 KiB. Test:
`zero_width_marks_per_cell_are_capped`. Benchmark `combining_marks` (1080p,
rows of `a` plus 32,760 U+0301) added to `crates/rte_render/benches/frame.rs`:
median 3.81 ms, within the 4 ms frame budget; the remaining cost is the row
scan itself. Architecture updated.

### Response M2R4

Fixed. [`run_traced`](../xtask/src/main.rs#L293) pipes stdout/stderr through
reader threads, bounds each run (10 s, then kill) and requires a successful
exit status. [`parse_trace`](../xtask/src/main.rs#L333) requires every
non-empty line to be a trace line with a finite, non-negative value, unique
stages and `first_frame`. Every run, warm-ups included, must pass; the first
failure prints its run number and diagnostics and fails the task. Tests cover
success, exit 7, silent, timed-out (300 ms limit on a 30 s sleep), malformed,
NaN/inf/negative, empty-stage and duplicate results.

### Response M2R5

Fixed. Title and status centering use
[`saturating_sub`](../crates/rte_view/src/lib.rs#L223), and the platform
[skips painting](../crates/rte_platform/src/lib.rs#L677) a zero-size client
instead of forcing 1 × 1. Test: `every_glyph_is_clipped_to_its_region` builds
frames at widths 0-400 and heights 0-200 in a debug build (previously a
subtraction overflow). Native check: three minimize/restore cycles of the
release build with `ShowWindow`; the process stayed alive and closed with 0.

### Response M2R6

Fixed. `Glyph` carries a `clip` rectangle (title, status, gutter or text
region) and [`blit`](../crates/rte_render/src/lib.rs#L219) intersects it with
the framebuffer. Test: `glyphs_stay_inside_their_region` renders 40 lines at
width 400 and heights 200-219 and requires every status-strip pixel to keep
the status background. With clipping disabled the test fails at height 200.

### Response M2R7

Fixed. The view strips a trailing CR only when
[an LF follows](../crates/rte_view/src/lib.rs#L276) (`range.end < len`).
Test: `carriage_return_without_line_feed_stays_visible` (`a\r` shows `␍`;
`a\r\r\nb` shows one `␍`); the CRLF case remains in
`builds_rows_with_line_numbers_and_crlf`.

### Review Response Validation Results

| Command or check                                                 | Outcome                                    |
| ---------------------------------------------------------------- | ------------------------------------------ |
| `cargo fmt --all -- --check`                                     | Pass                                       |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Pass                                       |
| `cargo test --workspace --locked`                                | 46 tests pass                              |
| Clip test with clipping disabled (temporary edit, reverted)      | Fails at height 200, as expected           |
| Exactly 1 MiB file, `--exit-after-load`, 10 release runs         | All exit 0 in 29-35 ms                     |
| 18 MB UTF-16 LE and BE + BOM, `--exit-after-load`, 3 runs each   | All exit 0 in 54-56 ms                     |
| Minimize/restore, 3 cycles (`ShowWindow` 6 and 9)                | Process alive; closes with exit code 0     |
| `cargo xtask bench-startup` (50 runs, 5 warm-up)                 | Gate PASS, `first_frame` p95 28.48 ms      |
| `cargo bench -p rte_render --bench frame` (median)               | 1080p 0.35 ms, 4K 1.06 ms, marks 3.81 ms   |

Not run: GitHub-hosted CI and release workflows, `cargo xtask package`,
cold start after reboot, 4K and 144 Hz displays, multi-monitor DPI moves.
