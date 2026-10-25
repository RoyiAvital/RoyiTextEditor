# Changelog

Notable implemented changes to the application are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.2] - 2026-10-25

This version shapes the application around its main use: one file per
window, opened from a file manager.  
Started without a file, the window opens in an empty state.  
A file can be dragged onto the window (_Drag and Drop_). It opens when 
the current file has no unsaved changes or int an empty state.

### Added

- Empty state: started without a file, the window shows `Drop a file here to
  open it`; nothing can be edited and closing never asks. The Command
  Palette, About, Keyboard Shortcuts, zoom and Quit work.
- Drag and drop: a file dropped on the window opens in the empty state or
  in place of a file without unsaved changes. With unsaved changes or a save
  running nothing opens and the status line says why; of several files only
  the first opens; folders are refused. Startup is unchanged (`shell32`
  loads on the first drop).
- `Application: Quit` also on `Ctrl+Shift+W`.
- Performance Benchmark v1: L26, Start empty session.

### Changed

- `File: Close` (`Ctrl+W`, `Ctrl+F4`) closes the file and keeps the window
  in the empty state; `Alt+F4` and `Ctrl+Shift+W` quit. Both ask first about
  unsaved changes.
- `--read-only` applies to every file opened in the session; `--syntax`
  applies to the file named on the command line.
- A folder named on the command line starts the empty state with a notice.
- Closing or replacing a file while it loads stops its loader.
- The unsaved-changes prompt labels `D` as `Discard`; the application name in
  the title strip uses bold type.

### Removed

- The untitled scratch document, its amber marker and the "Discard the
  untitled document?" prompt.

### Documentation

- README and the online guide describe the single-session design, the empty
  state, drag and drop and the close and quit keys; the Developer Guide
  shows the file lifecycle.

## [0.4.1] - 2026-10-24

### Added

- Windows Batch syntax highlighting for `.bat` and `.cmd` files, with Batch
  in Change Language Mode, `--syntax batch` (aliases `bat`, `cmd`, `dosbatch`),
  and Markdown code fences using the same names.

### Changed

- Release ZIPs use the `file name` from application metadata for their
  top-level folder (for example `RoyiTextEditor/`), while retaining the
  version and platform in the ZIP filename.

## [0.4.0] - 2026-10-24

This release focuses on adding _Syntax Highlighting_.  
We have optimized the code path to be parallel with optimized memory allocator.
The rendering performance are instantaneous for files with up to 50,000 _lines of code_.
The design choice was to prefer speed and responsiveness even with larger memory 
footprint.

### Added

- Syntax highlighting with built-in tree-sitter grammars for Bash, C, C#,
  C++, CSS, Go, HTML, Java, JavaScript, JSON, Julia, Lua, Markdown, MATLAB,
  Octave, PowerShell, Python, Rust, SQL, TOML, TypeScript (and TSX), XML and
  YAML, in a dark theme. The language follows the file name or a `#!` line
  (`.m`: MATLAB, or Octave by its content; `.ipynb` stays plain text) and is
  shown on the status line.
- Highlighting never delays the first frame: a code file opens as plain text
  and is highlighted within milliseconds. Parsing runs on a syntax thread,
  incrementally after each edit; the drawn part of the visible rows is
  highlighted on the UI thread within 1 ms per frame, the rest by the syntax
  thread. Files above 128 KiB are highlighted around the view first (within
  about a frame), then completely in the background. `syntax.maxFileMiB`
  also applies after edits: growing past it turns highlighting off,
  shrinking below turns it back on.
- Embedded languages: Markdown inline elements, fenced code blocks by their
  info string, YAML and TOML front matter and HTML blocks; HTML `<script>`
  (JavaScript, JSON, TypeScript) and `<style>` (CSS).
- Change Language Mode (`Ctrl+K M`, including Plain Text); `--syntax <id>`
  accepts the language ids and aliases (an unknown id shows a notice).
- Code-aware editing: Toggle Line Comment (`Ctrl+/`) and Toggle Block Comment
  (`Shift+Alt+A`); auto-closing brackets and quotes that wrap a selection,
  are stepped over and are deleted in pairs; `Enter` after an opening bracket
  (or a Python and YAML `:`) indents; the bracket pair at the caret is marked
  and Go to Bracket (`Ctrl+Shift+\`) jumps between them; Expand and Shrink
  Selection (`Shift+Alt+Right` / `Left`) by syntax nodes; Go to Symbol in
  Editor (`Ctrl+Shift+O`, collected on the syntax thread). Comments, pairs,
  indentation and brackets follow the embedded language at each selection
  (for example JavaScript inside `<script>`).
- Configuration keys `syntax.maxFileMiB` (default 32) and `syntax.injections`
  (default true).
- Benchmark options `--exit-after-highlight` and `--bench-full` (start a
  script once the whole file is highlighted), trace stages `highlighted` and
  `fully_highlighted`; interaction benchmarks report the time from each step
  to its first highlighted frame (`bench.hl<step>_ms`).

### Changed

- `View: Toggle Syntax Highlighting` and `--syntax none` now switch real
  highlighting; plain text and files without a language run no syntax work.
- The executable grows from about 4 MiB to 31 MiB (grammar tables, linked
  statically and paged in only when used).
- Tree-sitter parses with the mimalloc allocator (MIT); the rest of the
  application keeps the system heap.

### Performance

| Question                                  | Scenario                                  | Measure             | Result    |
| ----------------------------------------- | ----------------------------------------- | ------------------- | --------- |
| How fast does it start?                   | Open an empty file                        | Time to first frame | 22.1 ms   |
| How light is it?                          | Open an empty file                        | Peak memory         | 5.1 MiB   |
| How fast does a normal code file open?    | Open an 8 MiB Python file                 | Time to first frame | 22.7 ms   |
| Does a huge file still open instantly?    | Open a 256 MiB log file                   | Time to first frame | 23.6 ms   |
| How long until all of it is loaded?       | Open a 256 MiB log file                   | Time to load fully  | 76.9 ms   |
| What memory does a huge file cost?        | Open a 256 MiB log file                   | Peak memory         | 267.6 MiB |
| Does a pathological file break it?        | Open a 64 MiB file with one line          | Time to first frame | 23.4 ms   |
| Are commands instant?                     | Show the Command Palette                  | Key to frame        | 0.42 ms   |
| Is typing instant in a big file?          | Type in an 8 MiB Python file              | Key to frame        | 0.47 ms   |
| Is pasting a lot of text fast?            | Paste 1 MiB into an 8 MiB Python file     | Key to frame        | 2.88 ms   |
| Is scrolling smooth?                      | Scroll and jump in an 8 MiB Markdown file | Missed refreshes    | 0.0 %     |
| Is multi-cursor editing instant at scale? | Type at 10,000 carets                     | Key to frame        | 4.28 ms   |

Medians of the Performance Benchmark v1 on an AMD Ryzen 9 9950X (Windows 11);
full results in `Performance.md`.

- Files of 10,000 lines of TypeScript, Python and C are fully highlighted
  35.8, 40.7 and 68.5 ms after process start; 50,000 lines of Python and C
  in 171 and 312 ms. An 8 MiB Python file shows its first highlighted frame
  at 30.8 ms and is fully highlighted at 798 ms.
- Typing in a 10,000-line file is highlighted within 5.0-8.2 ms (p95); Go
  to Symbol opens in 14 ms at 10,000 lines and 65 ms at 50,000.
- Code files hold their syntax trees: a 64 KiB Python file peaks at
  11.6 MiB (5.2 MiB in 0.3.0), a 10,000-line Python file at 26.7 MiB.
  Edits in highlighted files take 0.1-0.9 ms longer than in 0.3.0. First
  paint stays at 21.4-24.4 ms.

### Documentation

- The Developer Guide shows the parallel syntax threads in a diagram and
  explains the memory allocation, including the choice of a larger memory
  footprint for faster highlighting.

## [0.3.0] - 2026-10-24

This release focuses on _Selections_ and _Search and Replace_.  
It matches the features of VS Code in those areas with the additional Glob search option.  
The focus, as always, was performance and correctness.  
Next milestone is adding _Syntax Highlighting_.

### Added

- Search box (`Ctrl+F`) at the top right, seeded with the selection or the
  word at the caret. Modes Text (default), Glob (`Alt+G`; `?` one character,
  `*` the shortest text within a line, a trailing `*` the rest of the line)
  and RegEx (`Alt+R`), with Match Case (`Alt+C`), Whole Word (`Alt+W`) and
  Find in Selection (`Alt+L`). Next and previous with `Enter` / `Shift+Enter`
  and `F3` / `Shift+F3`; `Alt+Enter` selects all matches.
- Large files are searched on a background thread: the match count streams in
  (`2 of 1,204+`), visible matches are highlighted, and typing cancels stale
  work. Invalid patterns and lines skipped by the bounded look-around and
  backreference engine are reported below the field.
- Replace (`Ctrl+H`): `Enter` or `Ctrl+Shift+1` replaces one match,
  `Ctrl+Alt+Enter` replaces all in one undo step, `Alt+P` toggles Preserve
  Case. RegEx replacements use the VS Code syntax (`$1`, `$<name>`, `$0`,
  `$$`, `\n`, `\t`, `\u`, `\l`, `\U`, `\L`).
- Multiple cursors with VS Code keys: `Ctrl+D`, `Ctrl+K Ctrl+D`,
  `Ctrl+Shift+L`, `Ctrl+F2`, `Ctrl+Alt+Up` / `Down`, `Shift+Alt+I`, `Ctrl+L`,
  `Ctrl+U` (cursor undo) and `Alt`+click. Pasting text with one line per
  cursor puts one line at each.
- Column selection with `Shift+Alt`+drag, middle-drag and
  `Ctrl+Shift+Alt` + arrows / `PgUp` / `PgDn`.
- `Search:` command family in the Command Palette; two-key chords such as
  `Ctrl+K Ctrl+D`.
- Configuration keys `editor.multiCursorLimit` (default 10,000) and
  `search.replaceAllMaxMiB` (default 512).
- Benchmark option `--bench-setup <keys>` runs untimed keys before the
  measured ones; scripts accept quoted text (`'abc'`), typed one character
  per step, and repeated keys (`Down*40`). `--bench-select <start>..<end>`,
  `--bench-handled` (copies) and `--bench-paced` (one key per display
  refresh) cover the clipboard, typing and scrolling tests; the results
  report peak memory.

### Performance

| Question                                  | Scenario                                  | Measure             | Result                      |
| ----------------------------------------- | ----------------------------------------- | ------------------- | --------------------------- |
| How fast does it start?                   | Open an empty file                        | Time to first frame | 21.3 ms                     |
| How light is it?                          | Open an empty file                        | Peak memory         | 5.0 MiB                     |
| How fast does a normal code file open?    | Open an 8 MiB Python file                 | Time to first frame | 21.4 ms                     |
| Does a huge file still open instantly?    | Open a 256 MiB log file                   | Time to first frame | 23.3 ms                     |
| How long until all of it is loaded?       | Open a 256 MiB log file                   | Time to load fully  | 77.7 ms                     |
| What memory does a huge file cost?        | Open a 256 MiB log file                   | Peak memory         | 267.4 MiB                   |
| Does a pathological file break it?        | Open a 64 MiB file with one line          | Time to first frame | 23.0 ms                     |
| Are commands instant?                     | Show the Command Palette                  | Key to frame        | 0.38 ms                     |
| Is typing instant in a big file?          | Type in an 8 MiB Python file              | Key to frame        | 0.35 ms                     |
| Is pasting a lot of text fast?            | Paste 1 MiB into an 8 MiB Python file     | Key to frame        | 2.02 ms                     |
| Is scrolling smooth?                      | Scroll and jump in an 8 MiB Markdown file | Missed refreshes    | 0.0 %                       |
| Is multi-cursor editing instant at scale? | Type at 10,000 carets                     | Key to frame        | 3.64 ms                     |

Medians of the Performance Benchmark v1 on an AMD Ryzen 9 9950X (Windows 11);
full results in `Performance.md`.

- The search box and the search thread are created on first use; nothing of
  search or multi-cursor runs at startup. Startup is unchanged against 0.2.0 (empty
  file, 30 interleaved launches: first paint 21.0 ms vs 21.3 ms, peak memory
  5.0 MiB for both). The executable grows from 2.0 MiB to 4.0 MiB (regular
  expression engines).

## [0.2.0] - 2026-10-24

This release added the _Command Palette_ / _Command Center_.
It misses the Search (M5) and Syntax Highlighting (M6) features.  

### Added

- Command Palette (`Ctrl+Shift+P`, `F1`): every command in VS Code style
  families (`Application:`, `File:`, `Edit:`, `Selection:`, `Go:`, `View:`) with fuzzy
  search, matched characters highlighted and key hints. Keys and the palette
  run the same commands; commands that edit are dimmed while the document is
  read-only. The search field has the usual editing keys, `Ctrl+A` and
  `Ctrl+V`. Popups keep their size when the editor is zoomed and fit small
  windows.
- Favorites: a star beside each palette command (click or `Ctrl+Space`).
  `Ctrl+P` lists the recent palette commands, then the favorites. Both are
  stored in `UserSettings/Palette.json` beside the executable, written in the
  background; a file that cannot be read or is not accepted is left unchanged.
- Configuration file `RoyiTextEditor.yaml` beside the executable (read only):
  `palette.recentCount` (default 3). Problems are reported on the status line.
- `Application: About` (icon, version, repository and documentation links, and
  "The Zen of Royi Text Editor" from `Meta/Manifest.md`),
  `Application: Keyboard Shortcuts` (table of all keys),
  `Application: Documentation` and `Application: Quit` (`Alt+F4`).
- `File: Toggle Read-Only` (palette only).
- `--syntax none` / `--syntax off` (any case) and
  `View: Toggle Syntax Highlighting` turn syntax highlighting off; the status
  line shows `Syntax off`. Takes effect once highlighting ships.
- Benchmark input mode: `--bench-keys <keys> --bench-runs <n>` sends the keys
  after loading and prints key-to-frame times; `cargo xtask bench-perf` uses it
  for the Command Palette tests (U01-U02).

### Changed

- The window title, title strip, About panel and release title show the display
  name "Royi Text Editor"; the executable, configuration file and release
  archive keep the file name `RoyiTextEditor`.
- Shortcuts match their modifiers exactly; for example `Ctrl+Shift+S` no
  longer saves and `Ctrl+Shift+C` no longer copies.

### Performance

| Question                                  | Scenario                                  | Measure             | Result                      |
| ----------------------------------------- | ----------------------------------------- | ------------------- | --------------------------- |
| How fast does it start?                   | Open an empty file                        | Time to first frame | 20.2 ms                     |
| How light is it?                          | Open an empty file                        | Peak memory         | 5.0 MiB                     |
| How fast does a normal code file open?    | Open an 8 MiB Python file                 | Time to first frame | 20.4 ms                     |
| Does a huge file still open instantly?    | Open a 256 MiB log file                   | Time to first frame | 21.5 ms                     |
| How long until all of it is loaded?       | Open a 256 MiB log file                   | Time to load fully  | 71.9 ms                     |
| What memory does a huge file cost?        | Open a 256 MiB log file                   | Peak memory         | 267.3 MiB                   |
| Does a pathological file break it?        | Open a 64 MiB file with one line          | Time to first frame | 21.9 ms                     |
| Are commands instant?                     | Show the Command Palette                  | Key to frame        | 0.38 ms                     |
| Is typing instant in a big file?          | Type in an 8 MiB Python file              | Key to frame        | Not measured yet            |
| Is pasting a lot of text fast?            | Paste 1 MiB into an 8 MiB Python file     | Key to frame        | Not measured yet            |
| Is scrolling smooth?                      | Scroll and jump in an 8 MiB Markdown file | Missed refreshes    | Not measured yet            |
| Is multi-cursor editing instant at scale? | Type at 10,000 carets                     | Key to frame        | Feature not implemented yet |

Medians of the Performance Benchmark v1 on an AMD Ryzen 9 9950X (Windows 11);
full results in `Performance.md`. The machine ran at a slower level in this
session: 0.1.0, re-measured alongside, gave the same results (21.2 ms, 5.0 MiB
for an empty file), so the Command Palette added no startup cost. Showing the
Command Palette is timed from the key to the presented frame.

### Documentation

- README and online documentation describe the palette, favorites and the
  configuration file.
- Documentation is grouped into Users and Developers, with separate keyboard
  shortcuts and command-line reference pages.
- The documentation theme more closely matches Material for MkDocs, including
  sidebar navigation, header search and bottom Previous/Next links.

## [0.1.0] - 2026-10-23

This is the first release (M3) with functioning View and Edit features.  
It misses the _Command Center_ (M4), Search (M5) and Syntax Highlighting (M6).  

### Added

- Rust workspace on the `x86_64-pc-windows-gnu` toolchain, linking against the
  MinGW runtime bundled with Rust. The release workflow runs format, Clippy and
  tests on Windows.
- `rte_text` text engine: persistent piece tree with cheap snapshots, line
  queries, multi-edit transactions with position mapping, progressive block
  loading, and streaming UTF-16 conversion with malformed-input detection.
- `RoyiTextEditor.exe`: frameless window with line numbers, title strip (file
  path on the left; `│` divider and application name on the right) and
  status line (caret position, encoding, line endings, `RO`). Scrolling with
  wheel and keys, horizontal scrolling, zoom (`Ctrl+=`, `Ctrl+-`, `Ctrl+0`,
  `Ctrl`+wheel), close with `Ctrl+W`.
- Editing: caret and selections by keyboard and mouse (click, double-click
  word, triple-click line, drag, `Shift`+click), typing and overtype, word
  movement and deletion, smart `Home`, `Enter` keeping indentation, `Tab` /
  `Shift+Tab` indent and outdent, undo and redo, clipboard with whole-line
  copy, cut and paste.
- Saving with `Ctrl+S`: atomic replacement with a verified backup, a prompt
  when the file changed on disk (Overwrite / Cancel), a prompt for unsaved
  changes on close, and an unsaved marker in the title. New files are created
  only from the command line; untitled documents are never saved.
- Read-only states (`--read-only`, loading, malformed UTF-16, read errors)
  refuse edits with a status-line notice.
- Application manifest (per-monitor DPI awareness, long paths, UTF-8 code
  page), embedded at build time with MinGW `windres`.
- Application icon, rendered at build time from `Meta/Icon.svg` (64 px and up) and
  `Meta/IconSmall.svg` (48 px and below) into a 10-size `.ico` embedded in the
  executable. The window uses it too (taskbar, Alt+Tab).
- Large files load in the background; the first part shows immediately.
- Command line: `RoyiTextEditor.exe [--read-only] [--syntax <id>] [--trace-startup] [file]`.
  `--syntax` is accepted now and applies once syntax highlighting ships.
- Bundled fonts (SIL Open Font License): JetBrains Mono, Cascadia Mono, Fira
  Mono, Iosevka Fixed and Lilex. They are not stored in Git; the build
  downloads missing fonts from pinned releases with SHA-256 verification
  (`tools/Get-Fonts.ps1`).
- Release packaging (`cargo xtask package`) and a Rust release workflow. The
  application name and version come from `Meta/AppMeta.json`; the build fails when
  it disagrees with `Cargo.toml`. The release workflow verifies both and the
  tag before building, and publishes the notes from this file.

### Changed

- Builds target AVX2-class CPUs (`x86-64-v3`); older CPUs are not supported.

### Fixed

### Performance

| Question                                  | Scenario                                  | Measure             | Result                      |
| ----------------------------------------- | ----------------------------------------- | ------------------- | --------------------------- |
| How fast does it start?                   | Open an empty file                        | Time to first frame | 13.2 ms                     |
| How light is it?                          | Open an empty file                        | Peak memory         | 4.6 MiB                     |
| How fast does a normal code file open?    | Open an 8 MiB Python file                 | Time to first frame | 14.2 ms                     |
| Does a huge file still open instantly?    | Open a 256 MiB log file                   | Time to first frame | 13.6 ms                     |
| How long until all of it is loaded?       | Open a 256 MiB log file                   | Time to load fully  | 70.1 ms                     |
| What memory does a huge file cost?        | Open a 256 MiB log file                   | Peak memory         | 267.0 MiB                   |
| Does a pathological file break it?        | Open a 64 MiB file with one line          | Time to first frame | 13.9 ms                     |
| Are commands instant?                     | Show the Command Palette                  | Key to frame        | Feature not implemented yet |
| Is typing instant in a big file?          | Type in an 8 MiB Python file              | Key to frame        | Not measured yet            |
| Is pasting a lot of text fast?            | Paste 1 MiB into an 8 MiB Python file     | Key to frame        | Not measured yet            |
| Is scrolling smooth?                      | Scroll and jump in an 8 MiB Markdown file | Missed refreshes    | Not measured yet            |
| Is multi-cursor editing instant at scale? | Type at 10,000 carets                     | Key to frame        | Feature not implemented yet |

Medians of the Performance Benchmark v1 on an AMD Ryzen 9 9950X (Windows 11);
full results in `Performance.md`.

### Documentation

- Added online documentation.
