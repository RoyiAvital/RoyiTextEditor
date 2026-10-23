# Changelog

Notable implemented changes to the application are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

 - Added documentation based on <>. 

