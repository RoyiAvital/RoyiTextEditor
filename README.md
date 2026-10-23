[![Visitors](https://api.visitorbadge.io/api/combined?path=https%3A%2F%2Fgithub.com%2FRoyiAvital%2FStackExchangeCodes&labelColor=%23f47373&countColor=%23555555&style=plastic)](https://github.com/RoyiAvital/RoyiTextEditor) <!-- https://www.visitorbadge.io -->
[![Documentation](https://img.shields.io/badge/documentation-online-brightgreen?style=plastic)](https://RoyiAvital.github.io/RoyiTextEditor)

# Royi Text Editor

`RoyiTextEditor` is a portable, keyboard workflow focused, lightweight and performance oriented Text and Code Editor. 

The _File Manager_ focuses on:
 - **Minimalistic** and focused UI.
 - High performance to generate **reactive** experience.
 - **Commands** through _Command Center_ / _Command Palette_.
 - **Keyboard** oriented workflow.
 - Developed and verified on **Windows OS**.

Development prioritizes _correctness_ and _performance_.  
Each release is validated by hundreds of correctness tests and accompanied by a detailed performance report.

<!-- ![](https://i.postimg.cc/xdZDJThH/image.png) -->
![](https://i.imgur.com/8n35oed.png)

## Installation

1. Open the [latest release](https://github.com/RoyiAvital/RoyiTextEditor/releases/latest).
2. Download `RoyiTextEditor-<version>-windows-x86_64.zip`.
3. Extract the ZIP file to a folder of your choice.
4. Run `RoyiTextEditor.exe` from the extracted folder.

> [!TIP]
> _RoyiTextEditor_ is portable. Keep the extracted files together and move the folder whenever needed.

Requirements: Windows 11, x64 CPU with AVX2.

## Usage

```text
RoyiTextEditor.exe [--read-only] [--syntax <id>] [file]
```

`--syntax <id>` chooses the highlighting language (for example `--syntax json`
for a `.ipynb` notebook). It is accepted now and applies once syntax
highlighting ships.

| Keys                                                | Action                                                 |
| --------------------------------------------------- | ------------------------------------------------------ |
| `Left` / `Right`, `Ctrl+Left` / `Ctrl+Right`        | Move by character / word                               |
| `Up` / `Down`, `PgUp` / `PgDn`                      | Move by line / page                                    |
| `Home` / `End`                                      | First non-blank character or line start / line end     |
| `Ctrl+Home` / `Ctrl+End`                            | Start / end of file                                    |
| `Shift` with any move                               | Extend the selection                                   |
| `Ctrl+A`, `Esc`                                     | Select all, collapse the selection                     |
| `Backspace` / `Delete`, with `Ctrl`                 | Delete a character, a word                             |
| `Enter`                                             | New line keeping the indentation                       |
| `Tab` / `Shift+Tab`                                 | Insert a tab or indent lines / outdent lines           |
| `Insert`                                            | Toggle overtype                                        |
| `Ctrl+Z`, `Ctrl+Y` / `Ctrl+Shift+Z`                 | Undo, redo                                             |
| `Ctrl+C` / `Ctrl+X` / `Ctrl+V`                      | Copy / cut / paste (whole line without a selection)    |
| `Ctrl+S`                                            | Save                                                   |
| Click, double-click, triple-click, `Shift`+click    | Place the caret, select a word, a line, extend         |
| Wheel, `Ctrl+Up` / `Ctrl+Down`                      | Scroll lines                                           |
| `Shift`+wheel                                       | Horizontal scrolling                                   |
| `Ctrl+=` / `Ctrl+-` / `Ctrl+0`, `Ctrl`+wheel        | Zoom in / out / reset                                  |
| `Ctrl+W`, `Ctrl+F4`, `Alt+F4`                       | Close; unsaved changes ask Save / Don't Save / Cancel  |

Unsaved changes show `●` in the title strip. Saving replaces the file
atomically; when another program changed the file, the editor asks before
overwriting it.

New files are created only from the command line: a path that does not exist
opens an empty document and the first save creates it. Without a path the
editor opens an untitled scratch document that cannot be saved (amber marker).

## Features

- **Performance**: The start up time should be below 150 [Mili Sec] on modern PC. Each operation should feel instant.

> [!TIP]
> Open an issue for new feature requests.  
> Feedback is highly appreciated.

## Performance

> [!IMPORTANT]
> We take performance seriously.
> We aim to minimize _Time to load_ (TTL), _Time to load a file_ (TTLF) and _Time to render a keystroke_ (TTRK).
    
Results of the performance test suite per version. Run time in [ms], lower is better.




## Development

See [DEVELOPMENT.md](DEVELOPMENT.md) and [Plan/Architecture.md](Plan/Architecture.md).

### Environment

Rust with the `x86_64-pc-windows-gnu` toolchain (`rust-toolchain.toml` installs
`rustfmt` and `clippy`). Linking uses the MinGW runtime bundled with Rust
(`.cargo/config.toml`). The application manifest is compiled by MinGW-w64
`windres`, which must be on `PATH` (or set `WINDRES` to its path).

The application name and version are set in `Meta/AppMeta.json`; the build fails
when its version differs from the workspace version in `Cargo.toml`.

The executable icon is built from `Meta/Icon.svg` (64 px and up) and `Meta/IconSmall.svg`
(48 px and below, the names drawn as bars). The build script renders both with
`resvg` into a 16–256 px `.ico` and embeds it.

`tools/` also holds auxiliary tools that are not part of the build: the
tree-sitter measurement probe (`SyntaxProbe`), the icon text-to-outline and
preview tool (`IconOutlines`) and `Export-IconPng.ps1`. Usage is at the top of
each file.

```powershell
cargo test --workspace                                # Tests
cargo clippy --workspace --all-targets -- -D warnings # Lints
cargo fmt --all                                       # Formatting
cargo bench -p rte_text                               # Text engine benchmarks (Ropey baseline)
cargo bench -p rte_render                             # Full-frame render cost (1080p, 4K)
cargo bench -p rte_edit                               # Worst-case caret movement latency
cargo run --release -p rte_app -- <file>              # Run the editor
cargo xtask bench-startup                             # Startup benchmark (gate: p95 < 100 ms)
cargo xtask bench-perf                                # Performance Benchmark (Perfromance.md)
cargo xtask bench-perf --short                        # Short Benchmark: release-notes table
cargo xtask package                                   # Release ZIP under target/package
```

Fonts in `assets/fonts` are not stored in Git (`assets/` is ignored). The build
downloads missing files automatically: the `rte_assets` build script runs
`.\tools\Get-Fonts.ps1`, which fetches the five families from pinned releases
and verifies their SHA-256 hashes. Archives are cached in `target/font-cache`.
Run the script directly to refresh (`-Force`) or to fetch without building.

### Release

1. Set the same version in `Meta/AppMeta.json` and in `[workspace.package]` of
   `Cargo.toml`.
2. In `CHANGELOG.md`, move the `## [Unreleased]` entries under a new
   `## [X.Y.Z] - YYYY-MM-DD` section and keep an empty `## [Unreleased]` above it.
   For performance results, put the Short Benchmark table under
   `### Performance` (see `Perfromance.md`: copied from the version's full
   results, or from `cargo xtask bench-perf --short` when there are none).
3. Check locally: `.\.github\scripts\Resolve-Version.ps1`.
4. Commit, then push an annotated tag: `git tag -a vX.Y.Z -m "X.Y.Z"` and
   `git push origin vX.Y.Z`.

The `Release` workflow checks that the versions and the tag match, takes the
release notes from the `CHANGELOG.md` section, runs the tests, builds and
packages the ZIP with its SHA-256 file, and publishes the GitHub release.
A pre-release suffix (`v1.0.0-rc.1`) publishes a pre-release. Running the
workflow manually is a dry run: the ZIP and notes (from `## [Unreleased]`) are
uploaded as a workflow artifact and nothing is published.

### Documentation
