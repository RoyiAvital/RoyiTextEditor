# Changelog

Notable implemented changes to the application are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] - 2026-10-24

This release focuses on _Selections_ and _Search and Replace_.  
It matches the features of VS Code in those areas with the additional Glob search option.  
The focus, as always, was performance and correctness.

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
full results in `Perfromance.md`.

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
full results in `Perfromance.md`. The machine ran at a slower level in this
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
full results in `Perfromance.md`.

### Documentation

- Added online documentation.

