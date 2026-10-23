# Changelog

Notable implemented changes to the application are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Rust workspace on the `x86_64-pc-windows-gnu` toolchain, linking against the
  MinGW runtime bundled with Rust. CI runs format, Clippy and tests on Windows.
- `rte_text` text engine: persistent piece tree with cheap snapshots, line
  queries, multi-edit transactions with position mapping, progressive block
  loading, and streaming UTF-16 conversion with malformed-input detection.
- `RoyiTextEditor.exe`: frameless window showing a file read-only with line
  numbers, title strip and status line (encoding, line endings, `RO`).
  Scrolling with wheel and keys, horizontal scrolling, zoom (`Ctrl+=`,
  `Ctrl+-`, `Ctrl+0`, `Ctrl`+wheel), close with `Ctrl+W`.
- Large files load in the background; the first part shows immediately.
- Command line: `RoyiTextEditor.exe [--read-only] [--trace-startup] [file]`.
- Bundled fonts (SIL Open Font License): JetBrains Mono, Cascadia Mono, Fira
  Mono, Iosevka Fixed and Lilex. They are not stored in Git; the build
  downloads missing fonts from pinned releases with SHA-256 verification
  (`tools/Get-Fonts.ps1`).
- Release packaging (`cargo xtask package`) and a Rust release workflow.

### Changed

- Builds target AVX2-class CPUs (`x86-64-v3`); older CPUs are not supported.

### Fixed

### Performance

### Documentation

