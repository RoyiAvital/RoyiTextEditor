# Performance

The editor is designed to show a first frame quickly while large files finish
loading in the background. The figures below are medians from Performance
Benchmark v1 for release 0.6.0.

## Recorded Results

| Scenario                 | First paint | File read | Peak private memory | Key to frame |
| ------------------------ | ----------- | --------- | ------------------- | ------------ |
| Empty file               | 25.0 ms     | —         | 5.2 MiB             | —            |
| Python, 8 MiB            | 24.5 ms     | 9.0 ms    | 393.3 MiB           | —            |
| Log file, 256 MiB        | 27.5 ms     | 93.1 ms   | 267.2 MiB           | —            |
| Single-line text, 64 MiB | 25.6 ms     | 26.6 ms   | 72.2 MiB            | —            |
| Show Command Palette     | —           | —         | —                   | 0.44 ms      |
| Show favorites           | —           | —         | —                   | 0.36 ms      |

The machine was an AMD Ryzen 9 9950X with 94 GiB RAM, Windows 11 Pro for
Workstations, and a 2560 × 1600 display at 59 Hz. The release build used a
warm file cache. These values are specific to that setup and benchmark
version.

**First paint** is the time from process start until the first frame is drawn
to the back buffer. It is not the later DWM-composed display time. **File
read** measures completion of background loading. **Key to frame** measures
the time from a handled key to the frame being drawn. The palette result uses
30 repetitions after one warm-up; its p95 was 0.78 ms.

## Reproduce

Run the short report from the repository root:

```powershell
cargo xtask bench-perf --short
```

The complete procedure, fixtures, and recorded results are in
[Performance.md](https://github.com/RoyiAvital/RoyiTextEditor/blob/main/Performance.md).
The harness also provides `cargo xtask bench-startup` for the startup gate.
Crate-specific benchmarks are available with `cargo bench -p rte_text`,
`cargo bench -p rte_edit`, and `cargo bench -p rte_render`.

Code fixtures are highlighted. L09 also reports the time until the whole 8 MiB
Python file is highlighted (`--exit-after-highlight`, 799 ms); its peak memory
includes the syntax trees of the whole file. The scroll tests report the time
from a jump to its first highlighted frame.