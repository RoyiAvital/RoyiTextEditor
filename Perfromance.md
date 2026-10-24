# Performance

## 0.2.0

### Main Performance Oriented Changes

- Command Palette loaded lazily: nothing palette-specific runs at startup. The
  favorites file is read by a worker thread started on first open; a second
  popup font exists only while the editor is zoomed.
- Palette state I/O never blocks the UI: changes apply at once, the worker
  persists them (bounded file: 64 KiB, 256 favorites).
- Palette matching reuses its buffers; repaints only when the visible popup
  state changes.

### Performance Results

Using the Performance Benchmark v`1` (`cargo xtask bench-perf`).

Reference machine: AMD Ryzen 9 9950X, 94 GiB RAM, Windows 11 Pro for
Workstations, 2560 × 1600 at 59 Hz, release build, warm file cache.

| ID  | Test                                 | Metric                   | 0.2.0  | 0.1.0   | Prev. 2 | Prev. 3 |
| --- | ------------------------------------ | ------------------------ | ------ | ------- | ------- | ------- |
| L01 | Empty file                           | First paint [ms]         | 20.2   | 13.2    | —       | —       |
| L01 | Empty file                           | Peak private [MiB]       | 5.0    | 4.6     | —       | —       |
| L02 | Text, 64 KiB                         | First paint [ms]         | 20.6   | 13.6    | —       | —       |
| L02 | Text, 64 KiB                         | Peak private [MiB]       | 5.3    | 4.9     | —       | —       |
| L03 | Text, 256 MiB                        | First paint [ms]         | 21.5   | 13.6    | —       | —       |
| L03 | Text, 256 MiB                        | File read [ms]           | 71.9   | 70.1    | —       | —       |
| L03 | Text, 256 MiB                        | Peak private [MiB]       | 267.3  | 267.0   | —       | —       |
| L04 | Python, 64 KiB                       | First paint [ms]         | 20.2 ¹ | 13.5 ¹  | —       | —       |
| L04 | Python, 64 KiB                       | Peak private [MiB]       | 5.2    | 4.8     | —       | —       |
| L05 | C, 64 KiB                            | First paint [ms]         | 20.3 ¹ | 13.6 ¹  | —       | —       |
| L05 | C, 64 KiB                            | Peak private [MiB]       | 5.2    | 4.8     | —       | —       |
| L06 | JSON, 64 KiB                         | First paint [ms]         | 20.4 ¹ | 13.7 ¹  | —       | —       |
| L06 | JSON, 64 KiB                         | Peak private [MiB]       | 5.5    | 5.2     | —       | —       |
| L07 | Markdown, 64 KiB                     | First paint [ms]         | 20.0 ¹ | 13.5 ¹  | —       | —       |
| L07 | Markdown, 64 KiB                     | Peak private [MiB]       | 5.1    | 4.8     | —       | —       |
| L08 | CSV, 64 KiB                          | First paint [ms]         | 19.9 ¹ | 13.8 ¹  | —       | —       |
| L08 | CSV, 64 KiB                          | Peak private [MiB]       | 5.2    | 4.8     | —       | —       |
| L09 | Python, 8 MiB                        | First paint [ms]         | 20.4 ¹ | 14.2 ¹  | —       | —       |
| L09 | Python, 8 MiB                        | File read [ms]           | 8.5    | 7.9     | —       | —       |
| L09 | Python, 8 MiB                        | Fully highlighted [ms]   | n/i    | n/i     | —       | —       |
| L09 | Python, 8 MiB                        | Peak private [MiB]       | 13.5   | 13.1    | —       | —       |
| L10 | C, 8 MiB                             | First paint [ms]         | 20.0 ¹ | 14.2 ¹  | —       | —       |
| L10 | C, 8 MiB                             | File read [ms]           | 8.4    | 7.9     | —       | —       |
| L10 | C, 8 MiB                             | Peak private [MiB]       | 13.4   | 13.2    | —       | —       |
| L11 | JSON, 8 MiB                          | First paint [ms]         | 20.8 ¹ | 14.9 ¹  | —       | —       |
| L11 | JSON, 8 MiB                          | File read [ms]           | 8.0    | 7.8     | —       | —       |
| L11 | JSON, 8 MiB                          | Peak private [MiB]       | 13.5   | 13.2    | —       | —       |
| L12 | Markdown, 8 MiB                      | First paint [ms]         | 20.1 ¹ | 14.2 ¹  | —       | —       |
| L12 | Markdown, 8 MiB                      | File read [ms]           | 8.3    | 8.0     | —       | —       |
| L12 | Markdown, 8 MiB                      | Peak private [MiB]       | 13.4   | 13.2    | —       | —       |
| L13 | CSV, 8 MiB                           | First paint [ms]         | 20.0 ¹ | 14.1 ¹  | —       | —       |
| L13 | CSV, 8 MiB                           | File read [ms]           | 8.2    | 7.9     | —       | —       |
| L13 | CSV, 8 MiB                           | Peak private [MiB]       | 13.5   | 13.2    | —       | —       |
| L14 | Read-only text, 256 MiB              | First paint [ms]         | 20.8   | 13.4    | —       | —       |
| L14 | Read-only text, 256 MiB              | File read [ms]           | 72.7   | 71.5    | —       | —       |
| L14 | Read-only text, 256 MiB              | Peak private [MiB]       | 267.4  | 267.0   | —       | —       |
| L15 | Read-only Python, 64 KiB             | First paint [ms]         | 19.7 ¹ | 13.4 ¹  | —       | —       |
| L15 | Read-only Python, 64 KiB             | Peak private [MiB]       | 5.2    | 4.8     | —       | —       |
| L16 | Single line text, 64 MiB             | First paint [ms]         | 21.9   | 13.9    | —       | —       |
| L16 | Single line text, 64 MiB             | File read [ms]           | 24.3   | 23.8    | —       | —       |
| L16 | Single line text, 64 MiB             | Peak private [MiB]       | 72.1   | 71.7    | —       | —       |
| L17 | Single line JSON, 8 MiB              | First paint [ms]         | 19.9 ¹ | 14.8 ¹  | —       | —       |
| L17 | Single line JSON, 8 MiB              | File read [ms]           | 8.0    | 8.5     | —       | —       |
| L17 | Single line JSON, 8 MiB              | Peak private [MiB]       | 13.5   | 13.2    | —       | —       |
| L18 | Notebook as JSON, 8 MiB              | First paint [ms]         | 20.4 ¹ | 14.3 ¹  | —       | —       |
| L18 | Notebook as JSON, 8 MiB              | File read [ms]           | 8.1    | 8.2     | —       | —       |
| L18 | Notebook as JSON, 8 MiB              | Peak private [MiB]       | 13.4   | 13.1    | —       | —       |
| L19 | Markdown, 128 MiB                    | First paint [ms]         | 21.4   | 13.7    | —       | —       |
| L19 | Markdown, 128 MiB                    | File read [ms]           | 39.8   | 39.6    | —       | —       |
| L19 | Markdown, 128 MiB                    | Peak private [MiB]       | 136.8  | 136.5   | —       | —       |
| U01 | Show the Command Palette             | Key to frame [ms]        | 0.38   | n/i     | —       | —       |
| U01 | Show the Command Palette             | Key to frame, p95 [ms]   | 0.61   | n/i     | —       | —       |
| U02 | Show favorites                       | Key to frame [ms]        | 0.33   | n/i     | —       | —       |
| U02 | Show favorites                       | Key to frame, p95 [ms]   | 0.58   | n/i     | —       | —       |
| C01 | Copy 1 KiB                           | Key to clipboard [ms]    | n/m    | n/m     | —       | —       |
| C02 | Copy 1 MiB                           | Key to clipboard [ms]    | n/m    | n/m     | —       | —       |
| C03 | Copy 64 MiB                          | Key to clipboard [ms]    | n/m    | n/m     | —       | —       |
| C04 | Paste 1 KiB into Python, 8 MiB       | Key to frame [ms]        | n/m    | n/m     | —       | —       |
| C05 | Paste 1 MiB into Python, 8 MiB       | Key to frame [ms]        | n/m    | n/m     | —       | —       |
| C06 | Paste 64 MiB into text               | Key to frame [ms]        | n/m    | n/m     | —       | —       |
| C06 | Paste 64 MiB into text               | Peak private [MiB]       | n/m    | n/m     | —       | —       |
| E01 | Typing in Python, 8 MiB              | Key to frame [ms]        | n/m    | n/m     | —       | —       |
| E02 | Typing at the start of a 64 MiB line | Key to frame [ms]        | n/m    | n/m     | —       | —       |
| E03 | End and Home on a 64 MiB line        | Key to frame [ms]        | n/m    | n/m     | —       | —       |
| M01 | Add next occurrence                  | Key to frame [ms]        | n/i    | n/i     | —       | —       |
| M02 | Select all occurrences               | Key to frame [ms]        | n/i    | n/i     | —       | —       |
| M03 | Column selection, 1,000 lines        | Key to frame [ms]        | n/i    | n/i     | —       | —       |
| M04 | Insert cursor below                  | Key to frame [ms]        | n/i    | n/i     | —       | —       |
| M05 | Type at 10,000 carets                | Key to frame [ms]        | n/i    | n/i     | —       | —       |
| M05 | Type at 10,000 carets                | Peak private [MiB]       | n/i    | n/i     | —       | —       |
| M06 | Backspace at 10,000 carets           | Key to frame [ms]        | n/i    | n/i     | —       | —       |
| M07 | Paste 10,000 pieces                  | Key to frame [ms]        | n/i    | n/i     | —       | —       |
| M08 | Undo and redo, 10,000 carets         | Key to frame [ms]        | n/i    | n/i     | —       | —       |
| S01 | Scroll and jump, Markdown, 64 KiB    | Key to frame, p95 [ms]   | n/m    | n/m     | —       | —       |
| S01 | Scroll and jump, Markdown, 64 KiB    | Key to frame, max [ms]   | n/m    | n/m     | —       | —       |
| S01 | Scroll and jump, Markdown, 64 KiB    | Missed refreshes [%]     | n/m    | n/m     | —       | —       |
| S02 | Scroll and jump, Markdown, 8 MiB     | Key to frame, p95 [ms]   | n/m    | n/m     | —       | —       |
| S02 | Scroll and jump, Markdown, 8 MiB     | Key to frame, max [ms]   | n/m    | n/m     | —       | —       |
| S02 | Scroll and jump, Markdown, 8 MiB     | Missed refreshes [%]     | n/m    | n/m     | —       | —       |
| S02 | Scroll and jump, Markdown, 8 MiB     | Jump to highlighted [ms] | n/i    | n/i     | —       | —       |
| S03 | Scroll and jump, Markdown, 128 MiB   | Key to frame, p95 [ms]   | n/m    | n/m     | —       | —       |
| S03 | Scroll and jump, Markdown, 128 MiB   | Key to frame, max [ms]   | n/m    | n/m     | —       | —       |
| S03 | Scroll and jump, Markdown, 128 MiB   | Missed refreshes [%]     | n/m    | n/m     | —       | —       |
| —   | Whole suite (implemented tests)      | Run time [s]             | 9.6    | 6.7     | —       | —       |

Values are medians. — no earlier version; n/i: feature not implemented
yet (multi-cursor: M5; highlighting: M6); n/m: not measured, the
interaction harness does not cover the test yet.
¹ Plain text: syntax highlighting is not implemented in 0.2.0.

U01 and U02 use the editor's scripted input mode (`--bench-keys`,
`--bench-runs`): after the fixture is loaded, the editor sends the key itself
and times it from handling the key to the presented frame (after `BitBlt`),
30 repetitions after one warm-up.

This session ran at the slower environment level described under 0.1.0:
about 20 ms first paint instead of 13 ms. 0.1.0, rebuilt from its tag and run
alongside in the same session, measured the same as 0.2.0 (first paint −1.0 to
+0.8 ms, file read −2.0 to +1.1 ms, peak memory ±0.1 MiB; empty file 21.2 ms
and 5.0 MiB). The differences to the 0.1.0 column come from the environment,
not from the code.

## 0.1.0

### Main Performance Oriented Changes

- Persistent piece tree: files load into shared blocks without copying; edits
  copy only one root-to-leaf path.
- Progressive loading: large files show their first block immediately and
  finish in the background.
- CPU renderer on a fixed cell grid: full frame 0.38 ms at 1080p, 1.28 ms at 4K.
- Code generation for AVX2-class CPUs (`x86-64-v3`).
- Load buffers are allocated as shared blocks and sized by the remaining file
  length: small-file peak memory 17.4 → 4.9 MiB, 512 MiB file 541 → 527 MiB.
- Caret movement scans at most 64 KiB per keystroke through a borrowed chunk
  cursor (worst case 1.1 ms; was up to 918 ms).
- Glyph bitmaps shared by glyph; repaints only when the visible state changes.
- Segment heap measured and not enabled (no gain).

### Performance Results

Using the Performance Benchmark v`1` (`cargo xtask bench-perf`).

Reference machine: AMD Ryzen 9 9950X, 94 GiB RAM, Windows 11 Pro for
Workstations, 2560 × 1600 at 59 Hz, release build, warm file cache.

| ID  | Test                                 | Metric                   | 0.1.0  | Prev. 1 | Prev. 2 | Prev. 3 |
| --- | ------------------------------------ | ------------------------ | ------ | ------- | ------- | ------- |
| L01 | Empty file                           | First paint [ms]         | 13.2   | —       | —       | —       |
| L01 | Empty file                           | Peak private [MiB]       | 4.6    | —       | —       | —       |
| L02 | Text, 64 KiB                         | First paint [ms]         | 13.6   | —       | —       | —       |
| L02 | Text, 64 KiB                         | Peak private [MiB]       | 4.9    | —       | —       | —       |
| L03 | Text, 256 MiB                        | First paint [ms]         | 13.6   | —       | —       | —       |
| L03 | Text, 256 MiB                        | File read [ms]           | 70.1   | —       | —       | —       |
| L03 | Text, 256 MiB                        | Peak private [MiB]       | 267.0  | —       | —       | —       |
| L04 | Python, 64 KiB                       | First paint [ms]         | 13.5 ¹ | —       | —       | —       |
| L04 | Python, 64 KiB                       | Peak private [MiB]       | 4.8    | —       | —       | —       |
| L05 | C, 64 KiB                            | First paint [ms]         | 13.6 ¹ | —       | —       | —       |
| L05 | C, 64 KiB                            | Peak private [MiB]       | 4.8    | —       | —       | —       |
| L06 | JSON, 64 KiB                         | First paint [ms]         | 13.7 ¹ | —       | —       | —       |
| L06 | JSON, 64 KiB                         | Peak private [MiB]       | 5.2    | —       | —       | —       |
| L07 | Markdown, 64 KiB                     | First paint [ms]         | 13.5 ¹ | —       | —       | —       |
| L07 | Markdown, 64 KiB                     | Peak private [MiB]       | 4.8    | —       | —       | —       |
| L08 | CSV, 64 KiB                          | First paint [ms]         | 13.8 ¹ | —       | —       | —       |
| L08 | CSV, 64 KiB                          | Peak private [MiB]       | 4.8    | —       | —       | —       |
| L09 | Python, 8 MiB                        | First paint [ms]         | 14.2 ¹ | —       | —       | —       |
| L09 | Python, 8 MiB                        | File read [ms]           | 7.9    | —       | —       | —       |
| L09 | Python, 8 MiB                        | Fully highlighted [ms]   | n/i    | —       | —       | —       |
| L09 | Python, 8 MiB                        | Peak private [MiB]       | 13.1   | —       | —       | —       |
| L10 | C, 8 MiB                             | First paint [ms]         | 14.2 ¹ | —       | —       | —       |
| L10 | C, 8 MiB                             | File read [ms]           | 7.9    | —       | —       | —       |
| L10 | C, 8 MiB                             | Peak private [MiB]       | 13.2   | —       | —       | —       |
| L11 | JSON, 8 MiB                          | First paint [ms]         | 14.9 ¹ | —       | —       | —       |
| L11 | JSON, 8 MiB                          | File read [ms]           | 7.8    | —       | —       | —       |
| L11 | JSON, 8 MiB                          | Peak private [MiB]       | 13.2   | —       | —       | —       |
| L12 | Markdown, 8 MiB                      | First paint [ms]         | 14.2 ¹ | —       | —       | —       |
| L12 | Markdown, 8 MiB                      | File read [ms]           | 8.0    | —       | —       | —       |
| L12 | Markdown, 8 MiB                      | Peak private [MiB]       | 13.2   | —       | —       | —       |
| L13 | CSV, 8 MiB                           | First paint [ms]         | 14.1 ¹ | —       | —       | —       |
| L13 | CSV, 8 MiB                           | File read [ms]           | 7.9    | —       | —       | —       |
| L13 | CSV, 8 MiB                           | Peak private [MiB]       | 13.2   | —       | —       | —       |
| L14 | Read-only text, 256 MiB              | First paint [ms]         | 13.4   | —       | —       | —       |
| L14 | Read-only text, 256 MiB              | File read [ms]           | 71.5   | —       | —       | —       |
| L14 | Read-only text, 256 MiB              | Peak private [MiB]       | 267.0  | —       | —       | —       |
| L15 | Read-only Python, 64 KiB             | First paint [ms]         | 13.4 ¹ | —       | —       | —       |
| L15 | Read-only Python, 64 KiB             | Peak private [MiB]       | 4.8    | —       | —       | —       |
| L16 | Single line text, 64 MiB             | First paint [ms]         | 13.9   | —       | —       | —       |
| L16 | Single line text, 64 MiB             | File read [ms]           | 23.8   | —       | —       | —       |
| L16 | Single line text, 64 MiB             | Peak private [MiB]       | 71.7   | —       | —       | —       |
| L17 | Single line JSON, 8 MiB              | First paint [ms]         | 14.8 ¹ | —       | —       | —       |
| L17 | Single line JSON, 8 MiB              | File read [ms]           | 8.5    | —       | —       | —       |
| L17 | Single line JSON, 8 MiB              | Peak private [MiB]       | 13.2   | —       | —       | —       |
| L18 | Notebook as JSON, 8 MiB              | First paint [ms]         | 14.3 ¹ | —       | —       | —       |
| L18 | Notebook as JSON, 8 MiB              | File read [ms]           | 8.2    | —       | —       | —       |
| L18 | Notebook as JSON, 8 MiB              | Peak private [MiB]       | 13.1   | —       | —       | —       |
| L19 | Markdown, 128 MiB                    | First paint [ms]         | 13.7   | —       | —       | —       |
| L19 | Markdown, 128 MiB                    | File read [ms]           | 39.6   | —       | —       | —       |
| L19 | Markdown, 128 MiB                    | Peak private [MiB]       | 136.5  | —       | —       | —       |
| U01 | Show the Command Palette             | Key to frame [ms]        | n/i    | —       | —       | —       |
| U02 | Show favorites                       | Key to frame [ms]        | n/i    | —       | —       | —       |
| C01 | Copy 1 KiB                           | Key to clipboard [ms]    | n/m    | —       | —       | —       |
| C02 | Copy 1 MiB                           | Key to clipboard [ms]    | n/m    | —       | —       | —       |
| C03 | Copy 64 MiB                          | Key to clipboard [ms]    | n/m    | —       | —       | —       |
| C04 | Paste 1 KiB into Python, 8 MiB       | Key to frame [ms]        | n/m    | —       | —       | —       |
| C05 | Paste 1 MiB into Python, 8 MiB       | Key to frame [ms]        | n/m    | —       | —       | —       |
| C06 | Paste 64 MiB into text               | Key to frame [ms]        | n/m    | —       | —       | —       |
| C06 | Paste 64 MiB into text               | Peak private [MiB]       | n/m    | —       | —       | —       |
| E01 | Typing in Python, 8 MiB              | Key to frame [ms]        | n/m    | —       | —       | —       |
| E02 | Typing at the start of a 64 MiB line | Key to frame [ms]        | n/m    | —       | —       | —       |
| E03 | End and Home on a 64 MiB line        | Key to frame [ms]        | n/m    | —       | —       | —       |
| M01 | Add next occurrence                  | Key to frame [ms]        | n/i    | —       | —       | —       |
| M02 | Select all occurrences               | Key to frame [ms]        | n/i    | —       | —       | —       |
| M03 | Column selection, 1,000 lines        | Key to frame [ms]        | n/i    | —       | —       | —       |
| M04 | Insert cursor below                  | Key to frame [ms]        | n/i    | —       | —       | —       |
| M05 | Type at 10,000 carets                | Key to frame [ms]        | n/i    | —       | —       | —       |
| M05 | Type at 10,000 carets                | Peak private [MiB]       | n/i    | —       | —       | —       |
| M06 | Backspace at 10,000 carets           | Key to frame [ms]        | n/i    | —       | —       | —       |
| M07 | Paste 10,000 pieces                  | Key to frame [ms]        | n/i    | —       | —       | —       |
| M08 | Undo and redo, 10,000 carets         | Key to frame [ms]        | n/i    | —       | —       | —       |
| S01 | Scroll and jump, Markdown, 64 KiB    | Key to frame, p95 [ms]   | n/m    | —       | —       | —       |
| S01 | Scroll and jump, Markdown, 64 KiB    | Key to frame, max [ms]   | n/m    | —       | —       | —       |
| S01 | Scroll and jump, Markdown, 64 KiB    | Missed refreshes [%]     | n/m    | —       | —       | —       |
| S02 | Scroll and jump, Markdown, 8 MiB     | Key to frame, p95 [ms]   | n/m    | —       | —       | —       |
| S02 | Scroll and jump, Markdown, 8 MiB     | Key to frame, max [ms]   | n/m    | —       | —       | —       |
| S02 | Scroll and jump, Markdown, 8 MiB     | Missed refreshes [%]     | n/m    | —       | —       | —       |
| S02 | Scroll and jump, Markdown, 8 MiB     | Jump to highlighted [ms] | n/i    | —       | —       | —       |
| S03 | Scroll and jump, Markdown, 128 MiB   | Key to frame, p95 [ms]   | n/m    | —       | —       | —       |
| S03 | Scroll and jump, Markdown, 128 MiB   | Key to frame, max [ms]   | n/m    | —       | —       | —       |
| S03 | Scroll and jump, Markdown, 128 MiB   | Missed refreshes [%]     | n/m    | —       | —       | —       |
| —   | Whole suite (load tests)             | Run time [s]             | 6.7    | —       | —       | —       |

Values are medians. — no earlier version; n/i: feature not implemented
yet (Command Palette and favorites: M4; multi-cursor: M5; highlighting: M6);
n/m: not measured, the interaction harness does not exist yet.
¹ Plain text: syntax highlighting is not implemented in 0.1.0.

Two consecutive runs agreed within 1.0 ms in first paint, 1.2 ms in file read
and 0.1 MiB in peak memory. The same build also runs at a second, equally
stable level: 18.5–20.5 ms first paint, 2–5 ms more file read at 256 MiB
and 0.1–0.3 MiB more peak memory. It appeared when the harness ran from a
nested PowerShell with redirected output, in an earlier session, and once
between two tests of one run. The cause is not identified yet (window
activation and keyboard focus are suspected). Until the harness controls it,
compare versions under the same conditions; the table shows the lower level,
run directly from the VS Code terminal.

## Performance Benchmarks

### v1

Status: load tests (L01-L19) and the Command Palette tests (U01-U02) are
implemented in `cargo xtask bench-perf` ([xtask/src/perf.rs](xtask/src/perf.rs));
they run in about 7-10 s. U01-U02 use the editor's scripted input mode
(`--bench-keys <keys> --bench-runs <n>`); the other interaction tests are
specified and need more scripted actions. The whole
suite should not exceed 60 seconds on the reference machine, including
fixture generation on a first run; faster is better.

#### Procedure

- Builds the release binary, then runs every test in fresh processes.
- **Load tests** launch `RoyiTextEditor.exe --trace-startup --exit-after-load
  [--read-only] [--syntax <id>] <file>`. Times are measured from process
  creation (as in `cargo xtask bench-startup`):
  - **First paint:** the first frame is drawn into the back buffer. This is the
    compared startup metric.
  - **First frame:** the first frame has been composed by DWM. It waits for
    vertical sync, so it moves in steps of the display refresh (17 ms at
    59 Hz). The harness prints it; it is not compared.
  - **File read:** the loader thread has read the whole file (large files only;
    small files are read before the first paint).
  - **Peak private:** `PeakPagefileUsage` reported by the editor at exit.
- **Interaction tests** run one process per scenario with a scripted input
  sequence, started after the file is fully loaded. Each repetition measures
  from the input message to the frame drawn (after `BitBlt`, without the
  composition wait), and to clipboard ownership for copies.
- **Scroll and jump tests** (S01-S03) send one key per display refresh, like
  a held key at the display rate: `Down` × 40, `Up` × 40, `PgDn` × 20,
  `PgUp` × 20, `End` and `Home` alternating × 10 on long lines, `Ctrl+End`
  and `Ctrl+Home` alternating × 10 (140 keys, about 2.4 s at 59 Hz). They
  report the p95 and the maximum key-to-frame time and the share of keys whose
  frame missed the next refresh (16.9 ms at 59 Hz). Smooth means no missed
  refresh. S02 also reports the time from a `Ctrl+End` or `Ctrl+Home` jump to
  the first highlighted frame (viewport-first highlighting, see
  [Plan/Architecture.md](Plan/Architecture.md)). S03 is above
  `syntax.maxFileMiB` and scrolls plain text.
- Statistics: median of all repetitions; p95 for tests with ≥ 20 repetitions;
  one warm-up per test, not counted.
- Each test has a time cap; a test over its cap fails the run, so the suite
  cannot exceed its total budget.
- Results are comparable only within one benchmark version. Once a version
  has published results, changing a fixture, a scenario or the procedure
  creates a new version; v1 is still open.

#### Reproducibility

- **Command:** `cargo xtask bench-perf`. The harness, the fixture generators
  and the recorded fixture hashes are in the repository.
- **Fixtures:** generated by fixed code without randomness or time stamps, and
  verified by XXH3-64 (table below). Existing files are reused when their hash
  matches and regenerated otherwise. A generator that produces a different
  hash fails the run; after a version is published, a fixture change needs a
  new benchmark version.
  `cargo xtask bench-perf --print-hashes` regenerates all fixtures and prints
  their sizes and hashes.
- **Build:** the toolchain is pinned by `rust-toolchain.toml`; the release
  profile and target CPU are set in the repository.
- **Recorded environment:** the output starts with the application name and
  version from [Meta/AppMeta.json](Meta/AppMeta.json), the benchmark version, the
  `rustc` version and the CPU.
- **Machine state:** AC power, Windows power mode *Best performance*, no other
  heavy load, the display at its normal refresh rate. The warm-up run of each
  test loads the fixture into the file cache.

#### Fixtures

Generated deterministically by the harness into `target/perf/v1/`, kept between
runs. Sizes: small 64 KiB, medium 8 MiB, large 128 MiB (Markdown) and 256 MiB
(text). Small and medium code fixtures are below `syntax.maxFileMiB` (32 MiB,
see [Plan/Architecture.md](Plan/Architecture.md)) and are highlighted;
`large.md` is above it and shows plain text. The
single-line fixtures test long lines: rendering, caret movement and editing
must not depend on the line length.

| File           | Bytes       | XXH3-64              | Content                                           |
| -------------- | ----------- | -------------------- | ------------------------------------------------- |
| `empty.txt`    | 0           | `0x2d06800538d394c2` | Nothing                                           |
| `small.txt`    | 65,528      | `0xb9279797cb449924` | Prose lines, LF                                   |
| `large.log`    | 268,435,456 | `0x2f843a96190ac6c8` | Log lines, CRLF                                   |
| `small.py`     | 65,106      | `0x20f0895dc8ca07af` | Python block, repeated                            |
| `medium.py`    | 8,388,270   | `0x6e6658331bdaf703` | Python block, repeated                            |
| `small.c`      | 65,082      | `0x9bc8db30d412b721` | C block, repeated                                 |
| `medium.c`     | 8,388,345   | `0x667a78bba9ac72ae` | C block, repeated                                 |
| `small.json`   | 65,447      | `0x5cccfe7086c520c0` | Array, one object per line                        |
| `medium.json`  | 8,388,523   | `0x2f7b9d6dfd037f74` | Array, one object per line                        |
| `medium.ipynb` | 8,386,352   | `0xdbaab2506662a999` | Jupyter Notebook, 618 Markdown and 618 code cells |
| `small.md`     | 65,467      | `0xad1cf323b2be13aa` | Front matter, Markdown block, repeated            |
| `medium.md`    | 8,388,443   | `0xea9a6a2abe0b1430` | Front matter, Markdown block, repeated            |
| `large.md`     | 134,217,541 | `0xb5187e60707ad0a9` | Front matter, Markdown block, repeated            |
| `small.csv`    | 65,504      | `0xb10add99c5c4f459` | Header, CSV row, repeated                         |
| `medium.csv`   | 8,388,604   | `0x8641de1a0b731974` | Header, CSV row, repeated                         |
| `line.txt`     | 67,108,864  | `0x178f1d1aaeb2328c` | Words without any line break                      |
| `line.json`    | 8,388,551   | `0xcda2d7050b0bfede` | Minified JSON array on one line                   |

Each code fixture repeats a block containing every highlighted element:

- **Python:** classes, decorators, type hints, `async`/`await`, docstrings,
  f-strings, raw and byte strings, comprehensions, numbers in several bases,
  `True`/`False`/`None`, comments.
- **C:** `#include` and `#define`, structs, typedefs, functions, `switch`,
  line and block comments, string and character literals with escapes,
  numeric suffixes.
- **JSON:** one array of nested objects and arrays; strings with escapes,
  numbers with exponents, `true`, `false`, `null`.
- **Jupyter Notebook:** nbformat 4 with alternating Markdown and Python code
  cells; each code cell has a stream output, an image output (a valid 64 × 48
  PNG, base64, different per cell) and a text result. `.ipynb` is not
  registered as JSON; the tests open it with `--syntax json`.
- **Markdown:** YAML front matter, headings, emphasis, strong, strikethrough,
  inline code, inline links `[text](url)`, full `[text][label]` and collapsed
  `[label][]` reference links with their definitions, images, block quotes,
  bullet, ordered and task lists, tables, footnotes, HTML blocks, hard breaks,
  rules, and fenced code blocks in Python, C and JSON (injections).
- **CSV:** a header row; quoted fields with commas, escaped quotes and line
  breaks; numbers, dates, booleans, empty fields.

#### Tests

| ID      | Test                                        | Fixture                         | Repetitions       | Cap [s] |
| ------- | ------------------------------------------- | ------------------------------- | ----------------- | ------- |
| L01     | Load empty file                             | `empty.txt`                     | 20 processes      | 2       |
| L02     | Load small text                             | `small.txt`                     | 20 processes      | 2       |
| L03     | Load large text                             | `large.log`                     | 5 processes       | 1       |
| L04-L08 | Load small code (Python, C, JSON, MD, CSV)  | `small.*`                       | 10 processes each | 1 each  |
| L09-L13 | Load medium code (Python, C, JSON, MD, CSV) | `medium.*`                      | 5 processes each  | 1 each  |
| L14     | Load large text read-only                   | `large.log`, `--read-only`      | 5 processes       | 1       |
| L15     | Load small Python read-only                 | `small.py`, `--read-only`       | 10 processes      | 1       |
| L16     | Load a single 64 MiB line                   | `line.txt`                      | 5 processes       | 1       |
| L17     | Load single-line JSON                       | `line.json`                     | 5 processes       | 1       |
| L18     | Load a notebook as JSON                     | `medium.ipynb`, `--syntax json` | 5 processes       | 1       |
| L19     | Load large Markdown                         | `large.md`                      | 5 processes       | 1       |
| U01     | Show and close the Command Palette          | `small.py`                      | 30                | 1       |
| U02     | Show and close favorites (`Ctrl+P`)         | `small.py`                      | 30                | 1       |
| C01-C03 | Copy 1 KiB, 1 MiB, 64 MiB                   | `large.log`                     | 20, 5, 3          | 2       |
| C04-C05 | Paste 1 KiB, 1 MiB (re-highlight)           | `medium.py`                     | 20, 5             | 2       |
| C06     | Paste 64 MiB                                | `large.log`                     | 3                 | 2       |
| E01     | Type 200 characters                         | `medium.py`                     | 200               | 2       |
| E02     | Type 50 characters at the line start        | `line.txt`                      | 50                | 1       |
| E03     | `End`, then `Home`                          | `line.txt`                      | 20                | 1       |
| M01     | Add next occurrence (`Ctrl+D`)              | `medium.c`                      | 100               | 2       |
| M02     | Select all occurrences (10,000 cap)         | `medium.c`                      | 5                 | 2       |
| M03     | Column selection over 1,000 lines           | `medium.c`                      | 5                 | 2       |
| M04     | Insert cursor below (`Ctrl+Alt+Down`)       | `medium.c`                      | 100               | 2       |
| M05     | Type 20 characters at 10,000 carets         | `medium.c`                      | 20                | 2       |
| M06     | Backspace 20 times at 10,000 carets         | `medium.c`                      | 20                | 2       |
| M07     | Paste 10,000 pieces, one per caret          | `medium.c`                      | 3                 | 1       |
| M08     | Undo and redo a 10,000-caret edit           | `medium.c`                      | 10                | 1       |
| S01     | Scroll and jump, small file                 | `small.md`                      | 140 keys          | 3       |
| S02     | Scroll and jump, medium file                | `medium.md`                     | 140 keys          | 3       |
| S03     | Scroll and jump, large file                 | `large.md`                      | 140 keys          | 3       |

Load caps add up to 21 s, interaction caps to 35 s and fixture generation is
capped at 3 s, so a run cannot exceed 59 s. Load caps are about twice the
measured time. The harness fails when the load tests leave less than 35 s for
the interaction tests. On the reference machine the load tests take about 7 s,
plus about 1 s for fixtures on a first run. CSV highlighting requires adding
CSV to the language registry; it is not in the initial language list.

#### Short Benchmark

A subset that tells the story for users. Its table is the `### Performance`
section of the release notes in `CHANGELOG.md` when results are requested.

| Question                                  | Test | Measure             |
| ----------------------------------------- | ---- | ------------------- |
| How fast does it start?                   | L01  | Time to first frame |
| How light is it?                          | L01  | Peak memory         |
| How fast does a normal code file open?    | L09  | Time to first frame |
| Does a huge file still open instantly?    | L03  | Time to first frame |
| How long until all of it is loaded?       | L03  | Time to load fully  |
| What memory does a huge file cost?        | L03  | Peak memory         |
| Does a pathological file break it?        | L16  | Time to first frame |
| Are commands instant?                     | U01  | Key to frame        |
| Is typing instant in a big file?          | E01  | Key to frame        |
| Is pasting a lot of text fast?            | C05  | Key to frame        |
| Is scrolling smooth?                      | S02  | Missed refreshes    |
| Is multi-cursor editing instant at scale? | M05  | Key to frame        |

Time to first frame is the First paint metric; Time to load fully is File read.

- When the version's full results are recorded above, copy the matching rows
  from them; do not run the benchmark again.
- Otherwise, `cargo xtask bench-perf --short` runs only these tests (about
  2 s) and prints the release-notes table.
