[![Visitors](https://api.visitorbadge.io/api/combined?path=https%3A%2F%2Fgithub.com%2FRoyiAvital%2FStackExchangeCodes&labelColor=%23f47373&countColor=%23555555&style=plastic)](https://github.com/RoyiAvital/RoyiFileManager) <!-- https://www.visitorbadge.io -->
[![Documentation](https://img.shields.io/badge/documentation-online-brightgreen?style=plastic)](https://royiavital.github.io/RoyiFileManager)

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

![](https://i.imgur.com/2t8CmJF.png)

## Installation

1. Open the [latest release](https://github.com/RoyiAvital/RoyiFileManager/releases/latest).
2. Download `RoyiTextEditor-<version>-windows-x86_64.zip`.
3. Extract the ZIP file to a folder of your choice.
4. Run `RoyiTextEditor.exe` from the extracted folder.

> [!TIP]
> _RoyiTextEditor_ is portable. Keep the extracted files together and move the folder whenever needed.

Requirements: Windows 11, x64 CPU with AVX2.

## Usage

```text
RoyiTextEditor.exe [--read-only] [file]
```

| Keys                                         | Action                |
| -------------------------------------------- | --------------------- |
| Wheel, `Up` / `Down`                         | Scroll lines          |
| `PgUp` / `PgDn`                              | Scroll pages          |
| `Ctrl+Home` / `Ctrl+End`                     | Start / end of file   |
| `Shift`+wheel, `Left` / `Right`, `Home`      | Horizontal scrolling  |
| `Ctrl+=` / `Ctrl+-` / `Ctrl+0`, `Ctrl`+wheel | Zoom in / out / reset |
| `Ctrl+W`, `Alt+F4`                           | Close                 |

The current version shows files read-only; editing comes next.

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
(`.cargo/config.toml`).

```powershell
cargo test --workspace                                # Tests
cargo clippy --workspace --all-targets -- -D warnings # Lints
cargo fmt --all                                       # Formatting
cargo bench -p rte_text                               # Text engine benchmarks (Ropey baseline)
cargo bench -p rte_render                             # Full-frame render cost (1080p, 4K)
cargo run --release -p rte_app -- <file>              # Run the editor
cargo xtask bench-startup                             # Startup benchmark (gate: p95 < 100 ms)
cargo xtask package                                   # Release ZIP under target/package
```

Fonts in `assets/fonts` are not stored in Git (`assets/` is ignored). The build
downloads missing files automatically: the `rte_assets` build script runs
`.\tools\Get-Fonts.ps1`, which fetches the five families from pinned releases
and verifies their SHA-256 hashes. Archives are cached in `target/font-cache`.
Run the script directly to refresh (`-Force`) or to fetch without building.

### Documentation
