[![Visitors](https://api.visitorbadge.io/api/combined?path=https%3A%2F%2Fgithub.com%2FRoyiAvital%2FStackExchangeCodes&labelColor=%23f47373&countColor=%23555555&style=plastic)](https://github.com/RoyiAvital/RoyiTextEditor) <!-- https://www.visitorbadge.io -->
[![Documentation](https://img.shields.io/badge/documentation-online-brightgreen?style=plastic)](https://RoyiAvital.github.io/RoyiTextEditor)

# Royi Text Editor

`RoyiTextEditor` is a portable, keyboard workflow focused, lightweight and performance oriented Text and Code Editor. 

The editor focuses on:
 - **Minimalistic** and focused UI.
 - High performance to generate **reactive** experience.
 - **Commands** through _Command Center_ / _Command Palette_.
 - **Keyboard** oriented workflow.
 - Developed and verified on **Windows OS**.

Development prioritizes _correctness_ and _performance_.  
Each release is validated by hundreds of correctness tests and accompanied by a detailed performance report.

![](https://i.imgur.com/SvrIr7Y.png)
<!-- ![](https://i.postimg.cc/1tjmS1pD/image.png) -->

## Installation

1. Open the [latest release](https://github.com/RoyiAvital/RoyiTextEditor/releases/latest).
2. Download `RoyiTextEditor-<version>-windows-x86_64.zip`.
3. Extract the ZIP file to a folder of your choice; its top-level folder is
  `RoyiTextEditor` (the `file name` in the application metadata).
4. Run `RoyiTextEditor.exe` from the extracted folder.

> [!TIP]
> _RoyiTextEditor_ is portable. Keep the extracted files together and move the folder whenever needed.

Requirements: Windows 11, x64 CPU with AVX2.

## Usage

```text
RoyiTextEditor.exe [--read-only] [--syntax <id>] [file]
```

`--syntax <id>` chooses the highlighting language (for example `--syntax json`
for a `.ipynb` notebook); `--syntax none` or `--syntax off` (any case) turns
highlighting off, as does `View: Toggle Syntax Highlighting` in the palette.
An unknown id keeps the language of the file name and shows a notice.

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
| `Ctrl+W`, `Ctrl+F4`                                 | Close the file (back to the empty state)               |
| `Alt+F4`, `Ctrl+Shift+W`                            | Quit; unsaved changes ask first                        |
| `Ctrl+Shift+P`, `F1`                                | Command Palette: all commands, fuzzy search            |
| `Ctrl+P`                                            | Recent and favorite commands                           |

Unsaved changes show `●` in the title strip. Saving replaces the file
atomically; when another program changed the file, the editor asks before
overwriting it.

### Files and Sessions

Each session (window) edits one file and never starts, closes or talks to
another session. A file comes from the command line or is dragged onto the
window:

- **No file:** started without a path, or after `Ctrl+W`, the window shows
  the empty state, `Drop a file here to open it`. Nothing can be edited; the
  palette, About, Keyboard Shortcuts, zoom and `Alt+F4` work.
- **Drag and drop:** a file dropped on the empty state, or on a file without
  unsaved changes, opens in its place. With unsaved changes (or a save
  running) nothing opens and the status line says why. Of several files only
  the first opens; folders are refused.
- **Close:** `Ctrl+W` / `Ctrl+F4` close the file (asking first about unsaved
  changes) and the window stays in the empty state; `Alt+F4` /
  `Ctrl+Shift+W` quit.
- **New files** are created only from the command line: a path that does not
  exist opens an empty document and the first save creates it.
- `--read-only` applies to every file opened in the session; `--syntax`
  applies to the file named on the command line.
- Windows does not deliver drops from Explorer to an editor running as
  administrator.

### Search and Replace

`Ctrl+F` opens the search box at the top right, seeded with the selection or
the word at the caret. The count of matches streams in while a large file is
searched (`2 of 1,204+`); invalid patterns and skipped lines are reported
below the field.

| Keys                                       | Action                              |
| ------------------------------------------ | ----------------------------------- |
| `Ctrl+F`, `Ctrl+H`                         | Find; find and replace              |
| `Enter` / `Shift+Enter`, `F3` / `Shift+F3` | Next / previous match               |
| `Alt+Enter`                                | Select all matches (multi-cursor)   |
| `Alt+C` / `Alt+W`                          | Match Case / Whole Word             |
| `Alt+G` / `Alt+R`                          | Glob / RegEx mode; both off is Text |
| `Alt+L`                                    | Find in the selection               |
| `Enter` or `Ctrl+Shift+1` (replace field)  | Replace one and go to the next      |
| `Ctrl+Alt+Enter`                           | Replace all, one undo step          |
| `Alt+P`                                    | Preserve Case                       |
| `Esc`                                      | Close the search box                |

- **Text** finds the typed text anywhere. **Glob**: `?` is one character, `*`
  the shortest text within a line, a trailing `*` the rest of the line.
  **RegEx** uses Rust regular expressions; look-around and backreferences use
  a bounded engine that skips lines it cannot finish.
- Replacements use the VS Code syntax: `$1`, `$<name>`, `$0`, `$$`, `\n`,
  `\t` and the case operators `\u`, `\l`, `\U`, `\L`.

### Multiple Cursors

The keys follow VS Code. Typing, deleting, pasting and moving apply to every
selection; `Esc` returns to one. Pasted text with one line per cursor puts one
line at each.

| Keys                                         | Action                                           |
| -------------------------------------------- | ------------------------------------------------ |
| `Ctrl+D`; `Ctrl+K Ctrl+D`                    | Add the next occurrence; move the last one to it |
| `Ctrl+Shift+L`, `Ctrl+F2`                    | Select all occurrences                           |
| `Ctrl+Alt+Up` / `Ctrl+Alt+Down`              | Add a cursor above / below                       |
| `Shift+Alt+I`                                | Add cursors to the ends of the selected lines    |
| `Ctrl+L`                                     | Select the line, then extend by lines            |
| `Ctrl+U`                                     | Undo the last cursor operation                   |
| `Alt`+click                                  | Add or remove a cursor                           |
| `Shift+Alt`+drag, middle-drag                | Column selection                                 |
| `Ctrl+Shift+Alt+Arrows` / `PgUp` / `PgDn`    | Column selection by keys                         |

### Syntax Highlighting and Code

Code is highlighted with built-in [tree-sitter](https://tree-sitter.github.io)
grammars: Bash, Batch, C, C#, C++, CSS, Go, HTML, Java, JavaScript, JSON, Julia, Lua,
Markdown, MATLAB, Octave, PowerShell, Python, Rust, SQL, TOML, TypeScript
(and TSX), XML and YAML. The language follows the file name (or a `#!` line);
`.m` files are MATLAB unless they look like Octave. The status line shows it.
Windows Batch files (`.bat` and `.cmd`) are detected automatically; use
`--syntax batch` (aliases `bat`, `cmd`, `dosbatch`) to select Batch explicitly.

- The window opens at once with plain text; the highlights follow within
  milliseconds. Large files are highlighted around the view first, then
  completely in the background. Files above `syntax.maxFileMiB` (32 MiB)
  stay plain text, also when an edit grows them past it.
- Markdown highlights its inline elements, fenced code blocks (by the info
  string, for example ` ```python `), YAML front matter and HTML blocks; HTML
  highlights `<script>` and `<style>`. Comments, auto-closing pairs and
  brackets follow the embedded language.

| Keys                                  | Action                                          |
| ------------------------------------- | ----------------------------------------------- |
| `Ctrl+K M`                            | Change Language Mode (or Plain Text)            |
| `Ctrl+/`                              | Toggle Line Comment                             |
| `Shift+Alt+A`                         | Toggle Block Comment                            |
| `Ctrl+Shift+\`                        | Go to the matching bracket                      |
| `Shift+Alt+Right` / `Shift+Alt+Left`  | Expand / shrink the selection by syntax nodes   |
| `Ctrl+Shift+O`                        | Go to a symbol (functions, classes, headings)   |

Brackets, quotes and backticks close automatically (and are stepped over),
`Backspace` in an empty pair deletes both, a selection is wrapped when an
opening bracket or quote is typed, and `Enter` after an opening bracket (or a
Python or YAML `:`) indents the new line. The bracket pair at the caret is
marked.

### Command Palette

Commands are grouped in families: `Application:`, `File:`, `Edit:`,
`Selection:`, `Search:`, `Go:` and `View:`. Type any part of a name (for
example `fsave`), move with `Up` / `Down` / `PgUp` / `PgDn`, run with `Enter`
or a click, close with `Esc`. The search field supports the usual editing
keys, `Ctrl+A` and `Ctrl+V`.

- Click the star of a command, or press `Ctrl+Space`, to add or remove it from
  the favorites.
- `Ctrl+P` lists the last commands run from the palette, then the favorites.
- `Application: Keyboard Shortcuts` shows all keys; `Application: About` shows
  the version and links; `Application: Documentation` opens the online
  documentation; `Application: Quit` (`Alt+F4`, `Ctrl+Shift+W`) closes the editor.

Favorites and recent commands are kept in `UserSettings/Palette.json` beside
the executable. A file the editor cannot read or does not accept is left
unchanged, and favorites then last for the session.

### Configuration

Optional `RoyiTextEditor.yaml` beside the executable, read at startup and never
written by the editor. Problems are shown on the status line; invalid values
keep their defaults.

```yaml
palette:
  recentCount: 3          # recent commands listed by Ctrl+P, 0 to 20
editor:
  multiCursorLimit: 10000 # most selections, 1 to 1,000,000
search:
  replaceAllMaxMiB: 512   # memory for one Replace All [MiB], 1 to 4096
syntax:
  maxFileMiB: 32          # larger files are not highlighted, 1 to 1024
  injections: true        # embedded languages (Markdown, HTML)
```

## Features

- **Performance**: The start up time should be below 100 [Mili Sec] on modern PC. Each operation should feel instant.

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

The application name, file name and version are set in `Meta/AppMeta.json`; the
build fails when its version differs from the workspace version in `Cargo.toml`
or its file name differs from the executable name.

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
cargo xtask bench-perf                                # Performance Benchmark (Performance.md)
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
   `### Performance` (see `Performance.md`: copied from the version's full
   results, or from `cargo xtask bench-perf --short` when there are none).
3. Check locally: `.\.github\scripts\Resolve-Version.ps1`.
4. Commit, then push an annotated tag: `git tag -a vX.Y.Z -m "X.Y.Z"` and
   `git push origin vX.Y.Z`.

The `Release` workflow checks that the versions and the tag match, takes the
release notes from the `CHANGELOG.md` section, runs the format check, Clippy
and the tests, builds and
packages the ZIP with its SHA-256 file, and publishes the GitHub release.
A pre-release suffix (`v1.0.0-rc.1`) publishes a pre-release. Running the
workflow manually is a dry run: the ZIP and notes (from `## [Unreleased]`) are
uploaded as a workflow artifact and nothing is published.

### Documentation

[docs/src/SUMMARY.md](docs/src/SUMMARY.md) defines the sections and page hierarchy.
Page content is separate, starting with
[docs/src/introduction.md](docs/src/introduction.md).

Build or preview through the script, which reads `SUMMARY.md` directly and
regenerates screenshots for the book:

```powershell
.\tools\Build-Docs.ps1
.\tools\Build-Docs.ps1 -Serve
```
