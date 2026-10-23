# Workspace and Text Engine

## Task

First iteration of [Architecture](../Plan/Architecture.md): milestones M0 (Cargo
workspace, toolchain, lints, CI) and M1 (`rte_text`, the persistent piece tree
text engine). Every later milestone builds on this engine, so it must be
correct, fast and fully tested before any UI exists.

## Scope

### Included

- Cargo workspace (edition 2024), `rust-toolchain.toml` (stable,
  `x86_64-pc-windows-gnu`, `rustfmt`, `clippy`), workspace lints, release profile.
- CI workflow `.github/workflows/ci.yml` on `windows-latest`: fmt, clippy
  (`-D warnings`), tests, with the GNU host toolchain.
- `crates/rte_text`:
  - Persistent B-tree over immutable buffer slices (`Chunk`), `Summary` of
    bytes and newlines.
  - `TreeBuilder`: blocks appended in order, prefix snapshots for progressive load.
  - Queries: length, line count, line start, line of offset, chunk iteration
    over a range, range copy.
  - `Transaction` (sorted, non-overlapping edits), `apply` → new `Snapshot` +
    `ChangeSet`; `ChangeSet::map` with bias.
  - Encodings: BOM detection, streaming UTF-16 decoder (malformed detection)
    and encoder; dominant line-ending detection.
- Unit tests, property tests against a `Vec<u8>` model, benches against Ropey.

### Excluded

- Any binary, window, rendering or file I/O (M2 onward).
- Replacing `release.yml` and `pages.yml` (moved to M2, when a binary exists).
  Both inherited workflows remain Python/conda based and will fail if run.
- `xtask` startup harness (M2).

### Compatibility Constraints

- Core crate: no OS APIs, no `unsafe`.
- Bytes are never normalized; invalid UTF-8 is stored as is.

## Design

- **Tree.** `Node { height, summary, kind }`, `kind` is a leaf `Chunk` or up
  to 32 children. Leaves hold ≤ 8 KiB (short scans for line lookups and
  splits). Three edit paths, all persistent:
  - **Local:** an edit inside one leaf whose parent has room copies only the
    root-to-leaf path.
  - **Batch:** multi-edit transactions whose edits stay inside leaves rebuild
    each touched node once; overflowing nodes are regrouped.
  - **General:** height-aware B-tree split and concatenate (O(log² n)).
- **Leaf hygiene.** A leaf is a healthy child when ≥ 1 KiB. When two adjacent
  leaves meet during concatenation: total ≤ 4 KiB → one new buffer; otherwise
  a small leaf borrows bytes from its neighbor (copy ≤ 1 KiB). Typing never
  copies more than 4 KiB.
- **Chunk.** `{ buf: Arc<[u8]>, start: u32, len: u32 }` for loaded blocks and
  inserted text alike.
- **Builder.** Splits each block into ≤ 8 KiB leaves; `snapshot()` builds a
  balanced tree bottom-up from the leaves seen so far (O(leaves)).
- **Transactions** are validated (bounds, order, no duplicate insertion point)
  and applied by the batch path when possible, otherwise last to first. `rev`
  increments per apply.
- **ChangeSet** stores old ranges and new lengths with prefix deltas; `map`
  uses binary search. Positions inside a replaced range go to its start
  (`Left`) or end (`Right`); insertions at a position follow the bias.
- **Encodings.** UTF-16 decoder keeps a split byte and a pending high
  surrogate across blocks; unpaired units become U+FFFD and set `malformed`.
  The encoder keeps an incomplete UTF-8 tail across chunks and rejects invalid
  UTF-8.
- **Threading.** Snapshots are `Send + Sync`; workers read them freely.
- **Failure behavior.** Invalid transactions return `EditError`; nothing panics
  on user input.

## Alternatives

- **Copy-on-write in place (`Arc::make_mut`).** Rejected for now: every undo
  entry shares the tree, so in-place mutation rarely applies; split/concat is
  simpler and uniformly persistent.
- **Ropey as the engine.** Kept as the benchmark baseline only (copies and
  validates all text on load).
- **Separate `Original` / `Owned` chunk variants.** Rejected: one slice shape
  covers both with less code.

## Runtime Effects

- No threads, I/O or timers in this crate.
- Memory: loaded blocks are shared, not copied; ~100 bytes of node overhead
  per 8 KiB leaf (~13 MiB per 1 GiB).
- Edits: O(log n) nodes created on the local path; ≤ 4 KiB copied per piece.
- Disabled path: not applicable (library only).

## Tests

- `cargo test -p rte_text`: unit tests (builder, queries, edits, change-set
  mapping, encodings, line endings) and property tests with reduced node and
  leaf limits (deep trees on small inputs): model equality, line queries,
  persistence of old snapshots, tree invariants, change-set mapping.
- `cargo bench -p rte_text`: build, random edits, line lookups and 10,000-caret
  typing on a 64 MiB document, Ropey for comparison.
- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`.

## Implementation Steps

1. Workspace files, toolchain file, CI workflow.
2. `rte_text` tree, builder and queries with unit tests.
3. Transactions, `ChangeSet` and property tests.
4. Encodings and line endings with unit tests.
5. Benches.

## Acceptance Criteria

- `cargo fmt`, `clippy -D warnings` and `cargo test --workspace` pass locally.
- Property tests pass for model equality, persistence and invariants.
- Benches run and report Ropey baselines.

## Reviewers

### 2026_10_23 - Claude Opus 5.5

- Role: Reviewer
- Activity: Design
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Task defined from the approved architecture (M0 + M1). Release and
  pages workflow replacement deferred to M2.

## Implementer

### 2026_10_23 - Claude Opus 5.5

- Role: Implementer
- Activity: Implementation
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Workspace, toolchain, CI and `rte_text` implemented. Benchmarks led
  to 8 KiB leaves (was 64 KiB), a local root-to-leaf edit path and a batch
  apply path (both now in the design). The local MinGW lacks `libgcc_eh.a`, so
  `.cargo/config.toml` links with Rust's bundled MinGW runtime
  (`-C link-self-contained=yes`). fmt, Clippy and all tests pass.

## Validation Results

| Command                                                 | Outcome                              |
| ------------------------------------------------------- | ------------------------------------ |
| `cargo fmt --all --check`                               | Pass                                 |
| `cargo clippy --workspace --all-targets -- -D warnings` | Pass                                 |
| `cargo test --workspace`                                | 18 unit/property + 2 integration     |
| `PROPTEST_CASES=5000 cargo test -p rte_text --release`  | Pass                                 |
| `cargo bench -p rte_text`                               | See table below                      |

Benchmarks on a 64 MiB ASCII document, local development machine (median):

| Operation                         | `rte_text` | Ropey    |
| --------------------------------- | ---------- | -------- |
| Build from 16 MiB blocks          | 8.4 ms     | 26.8 ms  |
| 10,000 carets, one transaction    | 3.1 ms     | 7.7 ms   |
| Typing at one place, per key      | 0.61 µs    | n/a      |
| Scattered single insert, per edit | 1.78 µs    | 0.58 µs  |
| Line start lookup                 | 0.40 µs    | 0.15 µs  |
| Line of offset                    | 0.22 µs    | 0.14 µs  |
| 100-line viewport chunks          | 0.93 µs    | n/a      |

Ropey edits mutate in place without snapshots; `rte_text` keeps every
previous snapshot intact. Scattered single edits and line lookups are slower
but far below a frame budget.

Not run: the CI workflow on GitHub (runs on the next push). No binary exists
yet, so no startup or memory measurements.

### 2026_10_23 - Claude Opus 5.5 (review fixes)

- Role: Implementer
- Activity: Implementation
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: High
- Context Window: 872K
- Outcome: Fixed review findings I1-I3 (see the GPT-6.1 Sol implementation
  review in [Architecture](../Plan/Architecture.md)). The `PROPTEST_CASES=5000`
  row above ran only 256 cases; corrected evidence below.

Corrected validation (code generation now `x86-64-v3`):

| Command                                                         | Outcome                       |
| --------------------------------------------------------------- | ----------------------------- |
| `cargo test -p rte_text`                                        | 18 + 2 pass, incl. I1 test    |
| `PROPTEST_CASES=1 cargo test -p rte_text --release --lib`       | Pass, 0.00 s                  |
| `PROPTEST_CASES=5000 cargo test -p rte_text --release --lib`    | Pass, 1.06 s (5,000 cases)    |
| `PROPTEST_CASES=20000 cargo test -p rte_text --release --lib`   | Pass, 4.06 s                  |
| `rustup show active-toolchain`                                  | GNU host from toolchain file  |
