# Performance

## 0.6.0

### Main Performance Oriented Changes

- Spelling costs nothing while off (the default): no thread, no dictionary
  and no work per frame. Turned on, its thread starts after the first frame
  and checks the visible rows: the first marks of a 1 MiB text file
  follow the first paint by 12.4 ms (P06), scrolling shows complete marks
  within 5.3-5.9 ms (p95, P01-P03) and `F8` selects a misspelling 4 MiB
  ahead in 38.7 ms (P04).
- Many carets or selections on one line read the line once, in one
  advancing column walk, instead of once per caret: drawing, `Up` / `Down`,
  `Ctrl+Alt+Up` / `Ctrl+Alt+Down`, `Tab`, `Ctrl+Shift+K` and `Enter`.
  Drawing stops at the right edge of the window.
- Copying or cutting one large selection makes one full copy fewer: C03
  takes 101.2 ms instead of 112.7 ms and peaks at 491.0 MiB instead of
  522.9 MiB (same-session A/B with 0.5.2).
- Clipboard retries wait at most 20 ms (was 50 ms) on the UI thread.
- The Read-only attribute is checked only when saving, on the save thread;
  loading is unchanged.
- Markdown spelling exclusions read only the syntax nodes that intersect
  the checked rows.

### Performance Results

Using the Performance Benchmark v`1` (`cargo xtask bench-perf`).

Reference machine: AMD Ryzen 9 9950X, 94 GiB RAM, Windows 11 Pro for
Workstations, 2560 × 1600 at 59 Hz, release build, warm file cache.

| ID  | Test                                          | Metric                       | 0.6.0  | 0.4.0  | 0.3.0  | 0.2.0  |
| --- | --------------------------------------------- | ---------------------------- | ------ | ------ | ------ | ------ |
| L01 | Empty file                                    | First paint [ms]             | 25.0   | 22.1   | 21.3   | 20.2   |
| L01 | Empty file                                    | Peak private [MiB]           | 5.2    | 5.1    | 5.0    | 5.0    |
| L02 | Text, 64 KiB                                  | First paint [ms]             | 24.2   | 21.6   | 21.4   | 20.6   |
| L02 | Text, 64 KiB                                  | Peak private [MiB]           | 5.7    | 5.5    | 5.3    | 5.3    |
| L03 | Text, 256 MiB                                 | First paint [ms]             | 27.5   | 23.6   | 23.3   | 21.5   |
| L03 | Text, 256 MiB                                 | File read [ms]               | 93.1   | 76.9   | 77.7   | 71.9   |
| L03 | Text, 256 MiB                                 | Peak private [MiB]           | 267.2  | 267.6  | 267.4  | 267.3  |
| L04 | Python, 64 KiB                                | First paint [ms]             | 25.2   | 21.5   | 21.2 ¹ | 20.2 ¹ |
| L04 | Python, 64 KiB                                | Peak private [MiB]           | 11.5   | 11.6   | 5.2    | 5.2    |
| L05 | C, 64 KiB                                     | First paint [ms]             | 26.1   | 22.7   | 21.1 ¹ | 20.3 ¹ |
| L05 | C, 64 KiB                                     | Peak private [MiB]           | 10.2   | 10.1   | 5.2    | 5.2    |
| L06 | JSON, 64 KiB                                  | First paint [ms]             | 26.1   | 22.1   | 21.8 ¹ | 20.4 ¹ |
| L06 | JSON, 64 KiB                                  | Peak private [MiB]           | 10.9   | 10.7   | 5.6    | 5.5    |
| L07 | Markdown, 64 KiB                              | First paint [ms]             | 24.3   | 21.9   | 21.1 ¹ | 20.0 ¹ |
| L07 | Markdown, 64 KiB                              | Peak private [MiB]           | 19.3   | 19.2   | 5.1    | 5.1    |
| L08 | CSV, 64 KiB                                   | First paint [ms]             | 24.0   | 21.5   | 20.9 ¹ | 19.9 ¹ |
| L08 | CSV, 64 KiB                                   | Peak private [MiB]           | 5.4    | 5.3    | 5.2    | 5.2    |
| L09 | Python, 8 MiB                                 | First paint [ms]             | 24.5   | 22.7   | 21.4 ¹ | 20.4 ¹ |
| L09 | Python, 8 MiB                                 | File read [ms]               | 9.0    | 9.2    | 8.9    | 8.5    |
| L09 | Python, 8 MiB                                 | Highlighted [ms]             | 39.0   | 30.8   | n/i    | n/i    |
| L09 | Python, 8 MiB                                 | Fully highlighted [ms]       | 799.0  | 797.8  | n/i    | n/i    |
| L09 | Python, 8 MiB                                 | Peak private [MiB]           | 393.3  | 393.1  | 13.5   | 13.5   |
| L10 | C, 8 MiB                                      | First paint [ms]             | 24.3   | 23.4   | 22.0 ¹ | 20.0 ¹ |
| L10 | C, 8 MiB                                      | File read [ms]               | 9.1    | 8.9    | 8.6    | 8.4    |
| L10 | C, 8 MiB                                      | Peak private [MiB]           | 23.7   | 23.5   | 13.4   | 13.4   |
| L11 | JSON, 8 MiB                                   | First paint [ms]             | 25.2   | 23.5   | 22.8 ¹ | 20.8 ¹ |
| L11 | JSON, 8 MiB                                   | File read [ms]               | 8.9    | 9.0    | 8.8    | 8.0    |
| L11 | JSON, 8 MiB                                   | Peak private [MiB]           | 32.8   | 33.1   | 13.4   | 13.5   |
| L12 | Markdown, 8 MiB                               | First paint [ms]             | 24.4   | 23.6   | 23.1 ¹ | 20.1 ¹ |
| L12 | Markdown, 8 MiB                               | File read [ms]               | 8.9    | 9.0    | 9.2    | 8.3    |
| L12 | Markdown, 8 MiB                               | Peak private [MiB]           | 38.3   | 37.7   | 13.5   | 13.4   |
| L13 | CSV, 8 MiB                                    | First paint [ms]             | 24.1   | 21.4   | 22.8 ¹ | 20.0 ¹ |
| L13 | CSV, 8 MiB                                    | File read [ms]               | 9.0    | 9.0    | 9.2    | 8.2    |
| L13 | CSV, 8 MiB                                    | Peak private [MiB]           | 13.7   | 13.6   | 13.5   | 13.5   |
| L14 | Read-only text, 256 MiB                       | First paint [ms]             | 27.0   | 22.7   | 24.8   | 20.8   |
| L14 | Read-only text, 256 MiB                       | File read [ms]               | 82.2   | 77.9   | 85.2   | 72.7   |
| L14 | Read-only text, 256 MiB                       | Peak private [MiB]           | 267.4  | 267.5  | 267.3  | 267.4  |
| L15 | Read-only Python, 64 KiB                      | First paint [ms]             | 24.9   | 22.1   | 23.0 ¹ | 19.7 ¹ |
| L15 | Read-only Python, 64 KiB                      | Peak private [MiB]           | 11.5   | 11.6   | 5.2    | 5.2    |
| L16 | Single line text, 64 MiB                      | First paint [ms]             | 25.6   | 23.4   | 23.0   | 21.9   |
| L16 | Single line text, 64 MiB                      | File read [ms]               | 26.6   | 26.5   | 25.2   | 24.3   |
| L16 | Single line text, 64 MiB                      | Peak private [MiB]           | 72.2   | 72.2   | 72.0   | 72.1   |
| L17 | Single line JSON, 8 MiB                       | First paint [ms]             | 27.4   | 23.1   | 21.2 ¹ | 19.9 ¹ |
| L17 | Single line JSON, 8 MiB                       | File read [ms]               | 9.4    | 8.8    | 8.1    | 8.0    |
| L17 | Single line JSON, 8 MiB                       | Peak private [MiB]           | 37.0   | 38.1   | 13.4   | 13.5   |
| L18 | Notebook as JSON, 8 MiB                       | First paint [ms]             | 24.1   | 22.9   | 21.4 ¹ | 20.4 ¹ |
| L18 | Notebook as JSON, 8 MiB                       | File read [ms]               | 8.8    | 8.6    | 8.4    | 8.1    |
| L18 | Notebook as JSON, 8 MiB                       | Peak private [MiB]           | 22.2   | 22.4   | 13.4   | 13.4   |
| L19 | Markdown, 128 MiB                             | First paint [ms]             | 25.9   | 24.4   | 23.4   | 21.4   |
| L19 | Markdown, 128 MiB                             | File read [ms]               | 44.4   | 47.0   | 43.1   | 39.8   |
| L19 | Markdown, 128 MiB                             | Peak private [MiB]           | 142.9  | 142.9  | 136.8  | 136.8  |
| L20 | Python, 50,000 lines                          | First paint [ms]             | 25.9   | 23.7   | n/i    | n/i    |
| L20 | Python, 50,000 lines                          | Highlighted [ms]             | 36.4   | 34.6   | n/i    | n/i    |
| L20 | Python, 50,000 lines                          | Fully highlighted [ms]       | 174.1  | 171.4  | n/i    | n/i    |
| L20 | Python, 50,000 lines                          | Peak private [MiB]           | 88.7   | 88.6   | n/i    | n/i    |
| L21 | C, 50,000 lines                               | First paint [ms]             | 23.6   | 22.3   | n/i    | n/i    |
| L21 | C, 50,000 lines                               | Highlighted [ms]             | 36.4   | 39.8   | n/i    | n/i    |
| L21 | C, 50,000 lines                               | Fully highlighted [ms]       | 311.5  | 312.3  | n/i    | n/i    |
| L21 | C, 50,000 lines                               | Peak private [MiB]           | 84.8   | 84.4   | n/i    | n/i    |
| L22 | Single line JSON, 8 MiB, highlighted          | First paint [ms]             | 25.2   | 23.1   | n/i    | n/i    |
| L22 | Single line JSON, 8 MiB, highlighted          | Highlighted [ms]             | 48.2   | 39.8   | n/i    | n/i    |
| L22 | Single line JSON, 8 MiB, highlighted          | Fully highlighted [ms]       | 704.1  | 686.6  | n/i    | n/i    |
| L22 | Single line JSON, 8 MiB, highlighted          | Peak private [MiB]           | 415.4  | 415.3  | n/i    | n/i    |
| L23 | Python, 10,000 lines                          | First paint [ms]             | 24.8   | 22.4   | n/i    | n/i    |
| L23 | Python, 10,000 lines                          | Highlighted [ms]             | 38.2   | 36.1   | n/i    | n/i    |
| L23 | Python, 10,000 lines                          | Fully highlighted [ms]       | 42.3   | 40.7   | n/i    | n/i    |
| L23 | Python, 10,000 lines                          | Peak private [MiB]           | 26.7   | 26.7   | n/i    | n/i    |
| L24 | C, 10,000 lines                               | First paint [ms]             | 24.4   | 22.5   | n/i    | n/i    |
| L24 | C, 10,000 lines                               | Highlighted [ms]             | 35.9   | 41.4   | n/i    | n/i    |
| L24 | C, 10,000 lines                               | Fully highlighted [ms]       | 69.7   | 68.5   | n/i    | n/i    |
| L24 | C, 10,000 lines                               | Peak private [MiB]           | 25.2   | 25.1   | n/i    | n/i    |
| L25 | TypeScript, 10,000 lines                      | First paint [ms]             | 24.4   | 23.0   | n/i    | n/i    |
| L25 | TypeScript, 10,000 lines                      | Highlighted [ms]             | 34.6   | 35.8   | n/i    | n/i    |
| L25 | TypeScript, 10,000 lines                      | Fully highlighted [ms]       | 35.1   | 35.8   | n/i    | n/i    |
| L25 | TypeScript, 10,000 lines                      | Peak private [MiB]           | 22.2   | 22.0   | n/i    | n/i    |
| L26 | Start empty session                           | First paint [ms]             | 24.5   | n/i    | n/i    | n/i    |
| L26 | Start empty session                           | Peak private [MiB]           | 5.2    | n/i    | n/i    | n/i    |
| U01 | Show the Command Palette                      | Key to frame [ms]            | 0.44   | 0.42   | 0.38   | 0.38   |
| U01 | Show the Command Palette                      | Key to frame, p95 [ms]       | 0.78   | 0.66   | 4.97   | 0.61   |
| U02 | Show favorites                                | Key to frame [ms]            | 0.36   | 0.41   | 0.41   | 0.33   |
| U02 | Show favorites                                | Key to frame, p95 [ms]       | 0.68   | 9.16   | 4.96   | 0.58   |
| C01 | Copy 1 KiB                                    | Key to clipboard [ms]        | 0.56   | 0.52   | 0.53   | n/m    |
| C02 | Copy 1 MiB                                    | Key to clipboard [ms]        | 1.40   | 1.63   | 1.61   | n/m    |
| C03 | Copy 64 MiB                                   | Key to clipboard [ms]        | 108.64 | 111.39 | 113.88 | n/m    |
| C04 | Paste 1 KiB into Python, 8 MiB                | Key to frame [ms]            | 0.69   | 0.62   | 0.60   | n/m    |
| C05 | Paste 1 MiB into Python, 8 MiB                | Key to frame [ms]            | 2.47   | 2.88   | 2.02   | n/m    |
| C06 | Paste 64 MiB into text                        | Key to frame [ms]            | 121.23 | 106.59 | 104.72 | n/m    |
| C06 | Paste 64 MiB into text                        | Peak private [MiB]           | 781.9  | 781.9  | 781.4  | n/m    |
| E01 | Typing in Python, 8 MiB                       | Key to frame [ms]            | 0.43   | 0.47   | 0.35   | n/m    |
| E02 | Typing at the start of a 64 MiB line          | Key to frame [ms]            | 0.28   | 0.30   | 0.30   | n/m    |
| E03 | End and Home on a 64 MiB line                 | Key to frame [ms]            | 0.47   | 0.40   | 0.42   | n/m    |
| E04 | Move line down, Python, 8 MiB                 | Key to frame [ms]            | 0.48   | n/i    | n/i    | n/i    |
| E05 | Copy line down, Python, 8 MiB                 | Key to frame [ms]            | 0.47   | n/i    | n/i    | n/i    |
| M01 | Add next occurrence                           | Key to frame [ms]            | 0.56   | 0.57   | 0.42   | n/i    |
| M02 | Select all occurrences                        | Key to frame [ms]            | 0.89   | 0.93   | 0.80   | n/i    |
| M03 | Column selection, 1,000 lines                 | Key to frame [ms]            | 1.25   | 1.23   | 1.21   | n/i    |
| M04 | Insert cursor below                           | Key to frame [ms]            | 0.52   | 0.55   | 0.44   | n/i    |
| M05 | Type at 10,000 carets                         | Key to frame [ms]            | 4.37   | 4.28   | 3.64   | n/i    |
| M05 | Type at 10,000 carets                         | Peak private [MiB]           | 141.2  | 134.1  | 33.8   | n/i    |
| M06 | Backspace at 10,000 carets                    | Key to frame [ms]            | 5.04   | 4.11   | 4.25   | n/i    |
| M07 | Paste 10,000 pieces                           | Key to frame [ms]            | 5.15   | 4.98   | 5.22   | n/i    |
| M08 | Undo and redo, 10,000 carets                  | Key to frame [ms]            | 0.70   | 0.74   | 0.49   | n/i    |
| S01 | Scroll and jump, Markdown, 64 KiB             | Key to frame, p95 [ms]       | 3.46   | 1.19   | 2.13   | n/m    |
| S01 | Scroll and jump, Markdown, 64 KiB             | Key to frame, max [ms]       | 4.99   | 3.90   | 2.52   | n/m    |
| S01 | Scroll and jump, Markdown, 64 KiB             | Missed refreshes [%]         | 0.0    | 0.0    | 0.0    | n/m    |
| S02 | Scroll and jump, Markdown, 8 MiB              | Key to frame, p95 [ms]       | 2.60   | 1.23   | 2.49   | n/m    |
| S02 | Scroll and jump, Markdown, 8 MiB              | Key to frame, max [ms]       | 4.66   | 3.92   | 3.88   | n/m    |
| S02 | Scroll and jump, Markdown, 8 MiB              | Missed refreshes [%]         | 0.0    | 0.0    | 0.0    | n/m    |
| S02 | Scroll and jump, Markdown, 8 MiB              | Jump to highlighted [ms]     | 4.13   | 4.08   | n/i    | n/i    |
| S03 | Scroll and jump, Markdown, 128 MiB            | Key to frame, p95 [ms]       | 2.11   | 1.17   | 2.62   | n/m    |
| S03 | Scroll and jump, Markdown, 128 MiB            | Key to frame, max [ms]       | 4.62   | 1.33   | 2.87   | n/m    |
| S03 | Scroll and jump, Markdown, 128 MiB            | Missed refreshes [%]         | 0.0    | 0.0    | 0.0    | n/m    |
| F01 | Find next, text, 256 MiB                      | Key to frame [ms]            | 0.55   | 0.55   | 0.53   | n/i    |
| F02 | Count matches, text, 256 MiB                  | Key to final count [ms]      | 222.31 | 225.73 | 218.81 | n/i    |
| F03 | Toggle RegEx, Python, 8 MiB                   | Key to final count [ms]      | 3.07   | 3.15   | 3.08   | n/i    |
| K01 | Typing in Python, 50,000 lines                | Key to frame, p95 [ms]       | 4.46   | 3.74   | n/i    | n/i    |
| K01 | Typing in Python, 50,000 lines                | Key to highlighted, p95 [ms] | 5.77   | 4.83   | n/i    | n/i    |
| K02 | Enter and Backspace, Python, 50,000 lines     | Key to frame, p95 [ms]       | 2.02   | 1.30   | n/i    | n/i    |
| K02 | Enter and Backspace, Python, 50,000 lines     | Key to highlighted, p95 [ms] | 69.90  | 70.30  | n/i    | n/i    |
| K03 | Typing in C, 50,000 lines                     | Key to frame, p95 [ms]       | 1.99   | 1.33   | n/i    | n/i    |
| K03 | Typing in C, 50,000 lines                     | Key to highlighted, p95 [ms] | 38.49  | 39.66  | n/i    | n/i    |
| K04 | Scroll and jump, Python, 50,000 lines         | Key to frame, p95 [ms]       | 1.94   | 1.20   | n/i    | n/i    |
| K04 | Scroll and jump, Python, 50,000 lines         | Missed refreshes [%]         | 0.0    | 0.0    | n/i    | n/i    |
| K04 | Scroll and jump, Python, 50,000 lines         | Jump to highlighted [ms]     | 0.91   | 0.81   | n/i    | n/i    |
| K05 | Go to Symbol, Python, 50,000 lines            | Key to frame [ms]            | 64.82  | 65.00  | n/i    | n/i    |
| K06 | Typing in Python, 10,000 lines                | Key to frame, p95 [ms]       | 4.46   | 4.20   | n/i    | n/i    |
| K06 | Typing in Python, 10,000 lines                | Key to highlighted, p95 [ms] | 5.79   | 5.16   | n/i    | n/i    |
| K07 | Typing in C, 10,000 lines                     | Key to frame, p95 [ms]       | 4.37   | 3.91   | n/i    | n/i    |
| K07 | Typing in C, 10,000 lines                     | Key to highlighted, p95 [ms] | 8.36   | 8.17   | n/i    | n/i    |
| K08 | Typing in TypeScript, 10,000 lines            | Key to frame, p95 [ms]       | 4.62   | 4.35   | n/i    | n/i    |
| K08 | Typing in TypeScript, 10,000 lines            | Key to highlighted, p95 [ms] | 6.10   | 5.25   | n/i    | n/i    |
| K09 | Enter and Backspace, TypeScript, 10,000 lines | Key to frame, p95 [ms]       | 4.56   | 3.86   | n/i    | n/i    |
| K09 | Enter and Backspace, TypeScript, 10,000 lines | Key to highlighted, p95 [ms] | 5.90   | 5.00   | n/i    | n/i    |
| K10 | Go to Symbol, Python, 10,000 lines            | Key to frame [ms]            | 13.54  | 14.40  | n/i    | n/i    |
| P01 | Scroll and jump, spelling, text, 1 MiB        | Key to marks, p95 [ms]       | 5.28   | n/i    | n/i    | n/i    |
| P01 | Scroll and jump, spelling, text, 1 MiB        | Key to marks, max [ms]       | 5.74   | n/i    | n/i    | n/i    |
| P02 | Scroll and jump, spelling, Markdown, 1 MiB    | Key to marks, p95 [ms]       | 5.71   | n/i    | n/i    | n/i    |
| P02 | Scroll and jump, spelling, Markdown, 1 MiB    | Key to marks, max [ms]       | 7.47   | n/i    | n/i    | n/i    |
| P03 | Scroll sideways, spelling, dense lines        | Key to marks, p95 [ms]       | 5.87   | n/i    | n/i    | n/i    |
| P03 | Scroll sideways, spelling, dense lines        | Key to marks, max [ms]       | 6.33   | n/i    | n/i    | n/i    |
| P03 | Scroll sideways, spelling, dense lines        | Missed refreshes [%]         | 0.0    | n/i    | n/i    | n/i    |
| P04 | `F8` to a misspelling 4 MiB ahead             | Key to selection [ms]        | 38.68  | n/i    | n/i    | n/i    |
| P05 | `Ctrl+.` on a misspelling                     | Key to suggestions [ms]      | 5.48   | n/i    | n/i    | n/i    |
| P06 | First marks, text, 1 MiB                      | First paint to marks [ms]    | 12.4   | n/i    | n/i    | n/i    |
| P06 | First marks, text, 1 MiB                      | Peak private [MiB]           | 7.6    | n/i    | n/i    | n/i    |
| —   | Whole suite (implemented tests)               | Run time [s]                 | 56.9   | 54.9   | 21.8   | 9.6    |

Values are medians of one full run; P01-P06 (spelling, built-in engine) are
opt-in and ran separately (`--only P01,P02,P03,P04,P05,P06`, 7.6 s), so the
whole suite excludes them. n/i: feature not implemented in that version;
n/m: not measured. ¹ Plain text: syntax highlighting was not implemented
before 0.4.0.

- **First paint** is 23.6-27.5 ms (0.4.0: 21.4-24.4 ms). A same-session A/B
  with 0.5.2, three alternating runs of L01, L03, L09 and L16, gives equal
  medians (L01 24.7 ms in 0.6.0, 24.8 ms in 0.5.2). A later same-session run
  of 0.4.0, 0.5.2 and 0.6.0 gives L01 medians of 25.1, 23.6 and 23.5 ms: the
  machine is slower than when 0.4.0 was recorded, not the code. The first
  highlighted frames of L09 and L22 vary between 31 and 48 ms in both
  versions.
- **Paced tests** (S01-S03, K01-K04): p95 is 0.7-2.3 ms higher than the
  0.4.0 record, equal to 0.5.2 in a same-session A/B, with no missed
  refreshes.
- **C06** (121.23 ms) is a slow run: three alternating runs give medians of
  108.2 ms (0.6.0) and 108.5 ms (0.5.2). M05, M06, M08, E01, E03 and U02 are
  equal to 0.5.2 the same way; the M05 peak varies between 134 and 157 MiB
  across runs of both versions.
- **Many carets on one row** (release probes of the operation alone,
  without paint, from [Done/CodeReview001.md](Done/CodeReview001.md)):
  10,000 carets draw in 0.02 ms (was 134-190 ms) and 10,000 selections in
  0.02 ms (was 214 ms); `Up` / `Down` takes 15 ms (was 242 ms), `Tab`
  1.7 ms (was 112 ms) and `Enter` on a code line 4.2 ms (was 17.6 ms).
  `Ctrl+Shift+K` on 2,000 separate lines takes 67 ms (was 102 ms) and now
  grows linearly with the lines.
- The whole suite runs three more tests than 0.4.0 (L26, E04, E05).

## 0.4.0

### Main Performance Oriented Changes

- Syntax highlighting never delays the first frame: a code file opens as
  plain text and its first highlighted frame follows within about one
  refresh (L23: first paint 22.4 ms, highlighted 36.1 ms).
- Highlighting starts while the window is created: the loader thread starts
  the syntax thread once the text is read, and the highlight query compiles
  on its own thread meanwhile. Files of 10,000 lines of TypeScript, Python
  and C are fully highlighted 35.8, 40.7 and 68.5 ms after process start.
- Above 128 KiB a 32 KiB window around the view is parsed first; the whole
  file parses at the same time on its own thread and is adopted as soon as
  it is ready.
- The UI thread highlights the visible rows within 1 ms per frame; the
  syntax thread highlights the rest, collects Go to Symbol and frees
  replaced trees. Typing in a 10,000-line file is highlighted within
  5.0-8.2 ms (p95).
- Tree-sitter allocates through mimalloc (6-16% faster parses, Go to Symbol
  about 25% faster); the rest of the application keeps the Windows heap.
- Plain text, disabled highlighting and files above `syntax.maxFileMiB`
  run no syntax work.

### Performance Results

Using the Performance Benchmark v`1` (`cargo xtask bench-perf`).

Reference machine: AMD Ryzen 9 9950X, 94 GiB RAM, Windows 11 Pro for
Workstations, 2560 × 1600 at 59 Hz, release build, warm file cache.

| ID  | Test                                          | Metric                       | 0.4.0  | 0.3.0  | 0.2.0  | 0.1.0  |
| --- | --------------------------------------------- | ---------------------------- | -------| ------ | ------ | ------ |
| L01 | Empty file                                    | First paint [ms]             | 22.1   | 21.3   | 20.2   | 13.2   |
| L01 | Empty file                                    | Peak private [MiB]           | 5.1    | 5.0    | 5.0    | 4.6    |
| L02 | Text, 64 KiB                                  | First paint [ms]             | 21.6   | 21.4   | 20.6   | 13.6   |
| L02 | Text, 64 KiB                                  | Peak private [MiB]           | 5.5    | 5.3    | 5.3    | 4.9    |
| L03 | Text, 256 MiB                                 | First paint [ms]             | 23.6   | 23.3   | 21.5   | 13.6   |
| L03 | Text, 256 MiB                                 | File read [ms]               | 76.9   | 77.7   | 71.9   | 70.1   |
| L03 | Text, 256 MiB                                 | Peak private [MiB]           | 267.6  | 267.4  | 267.3  | 267.0  |
| L04 | Python, 64 KiB                                | First paint [ms]             | 21.5   | 21.2 ¹ | 20.2 ¹ | 13.5 ¹ |
| L04 | Python, 64 KiB                                | Peak private [MiB]           | 11.6   | 5.2    | 5.2    | 4.8    |
| L05 | C, 64 KiB                                     | First paint [ms]             | 22.7   | 21.1 ¹ | 20.3 ¹ | 13.6 ¹ |
| L05 | C, 64 KiB                                     | Peak private [MiB]           | 10.1   | 5.2    | 5.2    | 4.8    |
| L06 | JSON, 64 KiB                                  | First paint [ms]             | 22.1   | 21.8 ¹ | 20.4 ¹ | 13.7 ¹ |
| L06 | JSON, 64 KiB                                  | Peak private [MiB]           | 10.7   | 5.6    | 5.5    | 5.2    |
| L07 | Markdown, 64 KiB                              | First paint [ms]             | 21.9   | 21.1 ¹ | 20.0 ¹ | 13.5 ¹ |
| L07 | Markdown, 64 KiB                              | Peak private [MiB]           | 19.2   | 5.1    | 5.1    | 4.8    |
| L08 | CSV, 64 KiB                                   | First paint [ms]             | 21.5   | 20.9 ¹ | 19.9 ¹ | 13.8 ¹ |
| L08 | CSV, 64 KiB                                   | Peak private [MiB]           | 5.3    | 5.2    | 5.2    | 4.8    |
| L09 | Python, 8 MiB                                 | First paint [ms]             | 22.7   | 21.4 ¹ | 20.4 ¹ | 14.2 ¹ |
| L09 | Python, 8 MiB                                 | File read [ms]               | 9.2    | 8.9    | 8.5    | 7.9    |
| L09 | Python, 8 MiB                                 | Highlighted [ms]             | 30.8   | n/i    | n/i    | n/i    |
| L09 | Python, 8 MiB                                 | Fully highlighted [ms]       | 797.8  | n/i    | n/i    | n/i    |
| L09 | Python, 8 MiB                                 | Peak private [MiB]           | 393.1  | 13.5   | 13.5   | 13.1   |
| L10 | C, 8 MiB                                      | First paint [ms]             | 23.4   | 22.0 ¹ | 20.0 ¹ | 14.2 ¹ |
| L10 | C, 8 MiB                                      | File read [ms]               | 8.9    | 8.6    | 8.4    | 7.9    |
| L10 | C, 8 MiB                                      | Peak private [MiB]           | 23.5   | 13.4   | 13.4   | 13.2   |
| L11 | JSON, 8 MiB                                   | First paint [ms]             | 23.5   | 22.8 ¹ | 20.8 ¹ | 14.9 ¹ |
| L11 | JSON, 8 MiB                                   | File read [ms]               | 9.0    | 8.8    | 8.0    | 7.8    |
| L11 | JSON, 8 MiB                                   | Peak private [MiB]           | 33.1   | 13.4   | 13.5   | 13.2   |
| L12 | Markdown, 8 MiB                               | First paint [ms]             | 23.6   | 23.1 ¹ | 20.1 ¹ | 14.2 ¹ |
| L12 | Markdown, 8 MiB                               | File read [ms]               | 9.0    | 9.2    | 8.3    | 8.0    |
| L12 | Markdown, 8 MiB                               | Peak private [MiB]           | 37.7   | 13.5   | 13.4   | 13.2   |
| L13 | CSV, 8 MiB                                    | First paint [ms]             | 21.4   | 22.8 ¹ | 20.0 ¹ | 14.1 ¹ |
| L13 | CSV, 8 MiB                                    | File read [ms]               | 9.0    | 9.2    | 8.2    | 7.9    |
| L13 | CSV, 8 MiB                                    | Peak private [MiB]           | 13.6   | 13.5   | 13.5   | 13.2   |
| L14 | Read-only text, 256 MiB                       | First paint [ms]             | 22.7   | 24.8   | 20.8   | 13.4   |
| L14 | Read-only text, 256 MiB                       | File read [ms]               | 77.9   | 85.2   | 72.7   | 71.5   |
| L14 | Read-only text, 256 MiB                       | Peak private [MiB]           | 267.5  | 267.3  | 267.4  | 267.0  |
| L15 | Read-only Python, 64 KiB                      | First paint [ms]             | 22.1   | 23.0 ¹ | 19.7 ¹ | 13.4 ¹ |
| L15 | Read-only Python, 64 KiB                      | Peak private [MiB]           | 11.6   | 5.2    | 5.2    | 4.8    |
| L16 | Single line text, 64 MiB                      | First paint [ms]             | 23.4   | 23.0   | 21.9   | 13.9   |
| L16 | Single line text, 64 MiB                      | File read [ms]               | 26.5   | 25.2   | 24.3   | 23.8   |
| L16 | Single line text, 64 MiB                      | Peak private [MiB]           | 72.2   | 72.0   | 72.1   | 71.7   |
| L17 | Single line JSON, 8 MiB                       | First paint [ms]             | 23.1   | 21.2 ¹ | 19.9 ¹ | 14.8 ¹ |
| L17 | Single line JSON, 8 MiB                       | File read [ms]               | 8.8    | 8.1    | 8.0    | 8.5    |
| L17 | Single line JSON, 8 MiB                       | Peak private [MiB]           | 38.1   | 13.4   | 13.5   | 13.2   |
| L18 | Notebook as JSON, 8 MiB                       | First paint [ms]             | 22.9   | 21.4 ¹ | 20.4 ¹ | 14.3 ¹ |
| L18 | Notebook as JSON, 8 MiB                       | File read [ms]               | 8.6    | 8.4    | 8.1    | 8.2    |
| L18 | Notebook as JSON, 8 MiB                       | Peak private [MiB]           | 22.4   | 13.4   | 13.4   | 13.1   |
| L19 | Markdown, 128 MiB                             | First paint [ms]             | 24.4   | 23.4   | 21.4   | 13.7   |
| L19 | Markdown, 128 MiB                             | File read [ms]               | 47.0   | 43.1   | 39.8   | 39.6   |
| L19 | Markdown, 128 MiB                             | Peak private [MiB]           | 142.9  | 136.8  | 136.8  | 136.5  |
| L20 | Python, 50,000 lines                          | First paint [ms]             | 23.7   | n/i    | n/i    | n/i    |
| L20 | Python, 50,000 lines                          | Highlighted [ms]             | 34.6   | n/i    | n/i    | n/i    |
| L20 | Python, 50,000 lines                          | Fully highlighted [ms]       | 171.4  | n/i    | n/i    | n/i    |
| L20 | Python, 50,000 lines                          | Peak private [MiB]           | 88.6   | n/i    | n/i    | n/i    |
| L21 | C, 50,000 lines                               | First paint [ms]             | 22.3   | n/i    | n/i    | n/i    |
| L21 | C, 50,000 lines                               | Highlighted [ms]             | 39.8   | n/i    | n/i    | n/i    |
| L21 | C, 50,000 lines                               | Fully highlighted [ms]       | 312.3  | n/i    | n/i    | n/i    |
| L21 | C, 50,000 lines                               | Peak private [MiB]           | 84.4   | n/i    | n/i    | n/i    |
| L22 | Single line JSON, 8 MiB, highlighted          | First paint [ms]             | 23.1   | n/i    | n/i    | n/i    |
| L22 | Single line JSON, 8 MiB, highlighted          | Highlighted [ms]             | 39.8   | n/i    | n/i    | n/i    |
| L22 | Single line JSON, 8 MiB, highlighted          | Fully highlighted [ms]       | 686.6  | n/i    | n/i    | n/i    |
| L22 | Single line JSON, 8 MiB, highlighted          | Peak private [MiB]           | 415.3  | n/i    | n/i    | n/i    |
| L23 | Python, 10,000 lines                          | First paint [ms]             | 22.4   | n/i    | n/i    | n/i    |
| L23 | Python, 10,000 lines                          | Highlighted [ms]             | 36.1   | n/i    | n/i    | n/i    |
| L23 | Python, 10,000 lines                          | Fully highlighted [ms]       | 40.7   | n/i    | n/i    | n/i    |
| L23 | Python, 10,000 lines                          | Peak private [MiB]           | 26.7   | n/i    | n/i    | n/i    |
| L24 | C, 10,000 lines                               | First paint [ms]             | 22.5   | n/i    | n/i    | n/i    |
| L24 | C, 10,000 lines                               | Highlighted [ms]             | 41.4   | n/i    | n/i    | n/i    |
| L24 | C, 10,000 lines                               | Fully highlighted [ms]       | 68.5   | n/i    | n/i    | n/i    |
| L24 | C, 10,000 lines                               | Peak private [MiB]           | 25.1   | n/i    | n/i    | n/i    |
| L25 | TypeScript, 10,000 lines                      | First paint [ms]             | 23.0   | n/i    | n/i    | n/i    |
| L25 | TypeScript, 10,000 lines                      | Highlighted [ms]             | 35.8   | n/i    | n/i    | n/i    |
| L25 | TypeScript, 10,000 lines                      | Fully highlighted [ms]       | 35.8   | n/i    | n/i    | n/i    |
| L25 | TypeScript, 10,000 lines                      | Peak private [MiB]           | 22.0   | n/i    | n/i    | n/i    |
| U01 | Show the Command Palette                      | Key to frame [ms]            | 0.42   | 0.38   | 0.38   | n/i    |
| U01 | Show the Command Palette                      | Key to frame, p95 [ms]       | 0.66   | 4.97   | 0.61   | n/i    |
| U02 | Show favorites                                | Key to frame [ms]            | 0.41   | 0.41   | 0.33   | n/i    |
| U02 | Show favorites                                | Key to frame, p95 [ms]       | 9.16   | 4.96   | 0.58   | n/i    |
| C01 | Copy 1 KiB                                    | Key to clipboard [ms]        | 0.52   | 0.53   | n/m    | n/m    |
| C02 | Copy 1 MiB                                    | Key to clipboard [ms]        | 1.63   | 1.61   | n/m    | n/m    |
| C03 | Copy 64 MiB                                   | Key to clipboard [ms]        | 111.39 | 113.88 | n/m    | n/m    |
| C04 | Paste 1 KiB into Python, 8 MiB                | Key to frame [ms]            | 0.62   | 0.60   | n/m    | n/m    |
| C05 | Paste 1 MiB into Python, 8 MiB                | Key to frame [ms]            | 2.88   | 2.02   | n/m    | n/m    |
| C06 | Paste 64 MiB into text                        | Key to frame [ms]            | 106.59 | 104.72 | n/m    | n/m    |
| C06 | Paste 64 MiB into text                        | Peak private [MiB]           | 781.9  | 781.4  | n/m    | n/m    |
| E01 | Typing in Python, 8 MiB                       | Key to frame [ms]            | 0.47   | 0.35   | n/m    | n/m    |
| E02 | Typing at the start of a 64 MiB line          | Key to frame [ms]            | 0.30   | 0.30   | n/m    | n/m    |
| E03 | End and Home on a 64 MiB line                 | Key to frame [ms]            | 0.40   | 0.42   | n/m    | n/m    |
| M01 | Add next occurrence                           | Key to frame [ms]            | 0.57   | 0.42   | n/i    | n/i    |
| M02 | Select all occurrences                        | Key to frame [ms]            | 0.93   | 0.80   | n/i    | n/i    |
| M03 | Column selection, 1,000 lines                 | Key to frame [ms]            | 1.23   | 1.21   | n/i    | n/i    |
| M04 | Insert cursor below                           | Key to frame [ms]            | 0.55   | 0.44   | n/i    | n/i    |
| M05 | Type at 10,000 carets                         | Key to frame [ms]            | 4.28   | 3.64   | n/i    | n/i    |
| M05 | Type at 10,000 carets                         | Peak private [MiB]           | 134.1  | 33.8   | n/i    | n/i    |
| M06 | Backspace at 10,000 carets                    | Key to frame [ms]            | 4.11   | 4.25   | n/i    | n/i    |
| M07 | Paste 10,000 pieces                           | Key to frame [ms]            | 4.98   | 5.22   | n/i    | n/i    |
| M08 | Undo and redo, 10,000 carets                  | Key to frame [ms]            | 0.74   | 0.49   | n/i    | n/i    |
| S01 | Scroll and jump, Markdown, 64 KiB             | Key to frame, p95 [ms]       | 1.19   | 2.13   | n/m    | n/m    |
| S01 | Scroll and jump, Markdown, 64 KiB             | Key to frame, max [ms]       | 3.90   | 2.52   | n/m    | n/m    |
| S01 | Scroll and jump, Markdown, 64 KiB             | Missed refreshes [%]         | 0.0    | 0.0    | n/m    | n/m    |
| S02 | Scroll and jump, Markdown, 8 MiB              | Key to frame, p95 [ms]       | 1.23   | 2.49   | n/m    | n/m    |
| S02 | Scroll and jump, Markdown, 8 MiB              | Key to frame, max [ms]       | 3.92   | 3.88   | n/m    | n/m    |
| S02 | Scroll and jump, Markdown, 8 MiB              | Missed refreshes [%]         | 0.0    | 0.0    | n/m    | n/m    |
| S02 | Scroll and jump, Markdown, 8 MiB              | Jump to highlighted [ms]     | 4.08   | n/i    | n/i    | n/i    |
| S03 | Scroll and jump, Markdown, 128 MiB            | Key to frame, p95 [ms]       | 1.17   | 2.62   | n/m    | n/m    |
| S03 | Scroll and jump, Markdown, 128 MiB            | Key to frame, max [ms]       | 1.33   | 2.87   | n/m    | n/m    |
| S03 | Scroll and jump, Markdown, 128 MiB            | Missed refreshes [%]         | 0.0    | 0.0    | n/m    | n/m    |
| F01 | Find next, text, 256 MiB                      | Key to frame [ms]            | 0.55   | 0.53   | n/i    | n/i    |
| F02 | Count matches, text, 256 MiB                  | Key to final count [ms]      | 225.73 | 218.81 | n/i    | n/i    |
| F03 | Toggle RegEx, Python, 8 MiB                   | Key to final count [ms]      | 3.15   | 3.08   | n/i    | n/i    |
| K01 | Typing in Python, 50,000 lines                | Key to frame, p95 [ms]       | 3.74   | n/i    | n/i    | n/i    |
| K01 | Typing in Python, 50,000 lines                | Key to highlighted, p95 [ms] | 4.83   | n/i    | n/i    | n/i    |
| K02 | Enter and Backspace, Python, 50,000 lines     | Key to frame, p95 [ms]       | 1.30   | n/i    | n/i    | n/i    |
| K02 | Enter and Backspace, Python, 50,000 lines     | Key to highlighted, p95 [ms] | 70.30  | n/i    | n/i    | n/i    |
| K03 | Typing in C, 50,000 lines                     | Key to frame, p95 [ms]       | 1.33   | n/i    | n/i    | n/i    |
| K03 | Typing in C, 50,000 lines                     | Key to highlighted, p95 [ms] | 39.66  | n/i    | n/i    | n/i    |
| K04 | Scroll and jump, Python, 50,000 lines         | Key to frame, p95 [ms]       | 1.20   | n/i    | n/i    | n/i    |
| K04 | Scroll and jump, Python, 50,000 lines         | Missed refreshes [%]         | 0.0    | n/i    | n/i    | n/i    |
| K04 | Scroll and jump, Python, 50,000 lines         | Jump to highlighted [ms]     | 0.81   | n/i    | n/i    | n/i    |
| K05 | Go to Symbol, Python, 50,000 lines            | Key to frame [ms]            | 65.00  | n/i    | n/i    | n/i    |
| K06 | Typing in Python, 10,000 lines                | Key to frame, p95 [ms]       | 4.20   | n/i    | n/i    | n/i    |
| K06 | Typing in Python, 10,000 lines                | Key to highlighted, p95 [ms] | 5.16   | n/i    | n/i    | n/i    |
| K07 | Typing in C, 10,000 lines                     | Key to frame, p95 [ms]       | 3.91   | n/i    | n/i    | n/i    |
| K07 | Typing in C, 10,000 lines                     | Key to highlighted, p95 [ms] | 8.17   | n/i    | n/i    | n/i    |
| K08 | Typing in TypeScript, 10,000 lines            | Key to frame, p95 [ms]       | 4.35   | n/i    | n/i    | n/i    |
| K08 | Typing in TypeScript, 10,000 lines            | Key to highlighted, p95 [ms] | 5.25   | n/i    | n/i    | n/i    |
| K09 | Enter and Backspace, TypeScript, 10,000 lines | Key to frame, p95 [ms]       | 3.86   | n/i    | n/i    | n/i    |
| K09 | Enter and Backspace, TypeScript, 10,000 lines | Key to highlighted, p95 [ms] | 5.00   | n/i    | n/i    | n/i    |
| K10 | Go to Symbol, Python, 10,000 lines            | Key to frame [ms]            | 14.40  | n/i    | n/i    | n/i    |
| —   | Whole suite (implemented tests)               | Run time [s]                 | 54.9   | 21.8   | 9.6    | 6.7    |

Values are medians of one full run. n/i: feature not implemented in that
version; n/m: not measured. ¹ Plain text: syntax highlighting was not
implemented before this version.

- **Memory:** code files now hold their syntax trees. 64 KiB files peak
  5-14 MiB higher (Markdown has the most layers). The 8 MiB load tests
  exit after load while the view is highlighted and the whole file still
  parses (22-38 MiB); L09 and L22 wait for the whole tree (393 and 415 MiB,
  about 50 bytes per text byte). M05 (134.1 MiB) includes the trees of its
  8 MiB C file. CSV and plain text are unchanged. `--syntax none`
  (highlighting off) restores the memory footprint of 0.3.0: L06 peaks at
  5.7 MiB instead of 10.7 MiB (0.3.0: 5.6 MiB; medians of 8 launches).
- **Highlighted files:** E01, C05, M01, M02, M04, M05 and M08 are 0.1-0.9 ms
  slower than in 0.3.0: each frame also queries the visible rows (at most
  1 ms) and edits update the trees.
- **K02** (Enter and Backspace at 50,000 Python lines, 70.3 ms to the
  highlighted frame) is bound by tree-sitter's incremental reparse of
  tree-reshaping edits; frames stay at 1.3 ms. K03 (C) reparses about 1 µs
  per top-level item on every edit.
- **U02 p95** (9.16 ms) is the second slowest of 30 repetitions and varies
  between runs, as in 0.3.0.
- **First paint** is 21.4-24.4 ms, as in 0.3.0 (20.9-24.8 ms); it was not
  A/B compared with 0.3.0 in this session. The executable grows from 4.0 MiB
  to 31 MiB (grammar tables).
- The whole suite takes longer because it runs 16 more tests (L20-L25,
  K01-K10), which wait for their files to be fully highlighted.
- Design, alternatives and the measurements behind these choices are in
  [Done/SyntaxPerformance.md](Done/SyntaxPerformance.md).

## 0.3.0

### Main Performance Oriented Changes

- Search and multi-cursor cost nothing at startup: the search box, the search
  thread and the count thread are created on first use.
- Search never blocks typing: only bounded literal scans run on the UI
  thread. Counts run as one exact enumeration on their own thread, so
  navigation and highlights are answered while a count runs; a newer query
  cancels older work.
- Highlights are bounded by what is visible: at most 64 KiB per row, 1 MiB
  and 65,536 ranges per request; the view places them in one pass per row.
- Literal search scans the text pieces in place; look-around patterns read
  and validate each line once.
- Limits are checked before work: the cursor limit before selections are
  built, the Replace All memory cap before each allocation and expansion.

### Performance Results

Using the Performance Benchmark v`1` (`cargo xtask bench-perf`).

Reference machine: AMD Ryzen 9 9950X, 94 GiB RAM, Windows 11 Pro for
Workstations, 2560 × 1600 at 59 Hz, release build, warm file cache.

| ID  | Test                                 | Metric                   | 0.3.0  | 0.2.0  | 0.1.0  | Prev. 3 |
| --- | ------------------------------------ | ------------------------ | ------ | ------ | ------ | ------- |
| L01 | Empty file                           | First paint [ms]         | 21.3   | 20.2   | 13.2   | —       |
| L01 | Empty file                           | Peak private [MiB]       | 5.0    | 5.0    | 4.6    | —       |
| L02 | Text, 64 KiB                         | First paint [ms]         | 21.4   | 20.6   | 13.6   | —       |
| L02 | Text, 64 KiB                         | Peak private [MiB]       | 5.3    | 5.3    | 4.9    | —       |
| L03 | Text, 256 MiB                        | First paint [ms]         | 23.3   | 21.5   | 13.6   | —       |
| L03 | Text, 256 MiB                        | File read [ms]           | 77.7   | 71.9   | 70.1   | —       |
| L03 | Text, 256 MiB                        | Peak private [MiB]       | 267.4  | 267.3  | 267.0  | —       |
| L04 | Python, 64 KiB                       | First paint [ms]         | 21.2 ¹ | 20.2 ¹ | 13.5 ¹ | —       |
| L04 | Python, 64 KiB                       | Peak private [MiB]       | 5.2    | 5.2    | 4.8    | —       |
| L05 | C, 64 KiB                            | First paint [ms]         | 21.1 ¹ | 20.3 ¹ | 13.6 ¹ | —       |
| L05 | C, 64 KiB                            | Peak private [MiB]       | 5.2    | 5.2    | 4.8    | —       |
| L06 | JSON, 64 KiB                         | First paint [ms]         | 21.8 ¹ | 20.4 ¹ | 13.7 ¹ | —       |
| L06 | JSON, 64 KiB                         | Peak private [MiB]       | 5.6    | 5.5    | 5.2    | —       |
| L07 | Markdown, 64 KiB                     | First paint [ms]         | 21.1 ¹ | 20.0 ¹ | 13.5 ¹ | —       |
| L07 | Markdown, 64 KiB                     | Peak private [MiB]       | 5.1    | 5.1    | 4.8    | —       |
| L08 | CSV, 64 KiB                          | First paint [ms]         | 20.9 ¹ | 19.9 ¹ | 13.8 ¹ | —       |
| L08 | CSV, 64 KiB                          | Peak private [MiB]       | 5.2    | 5.2    | 4.8    | —       |
| L09 | Python, 8 MiB                        | First paint [ms]         | 21.4 ¹ | 20.4 ¹ | 14.2 ¹ | —       |
| L09 | Python, 8 MiB                        | File read [ms]           | 8.9    | 8.5    | 7.9    | —       |
| L09 | Python, 8 MiB                        | Fully highlighted [ms]   | n/i    | n/i    | n/i    | —       |
| L09 | Python, 8 MiB                        | Peak private [MiB]       | 13.5   | 13.5   | 13.1   | —       |
| L10 | C, 8 MiB                             | First paint [ms]         | 22.0 ¹ | 20.0 ¹ | 14.2 ¹ | —       |
| L10 | C, 8 MiB                             | File read [ms]           | 8.6    | 8.4    | 7.9    | —       |
| L10 | C, 8 MiB                             | Peak private [MiB]       | 13.4   | 13.4   | 13.2   | —       |
| L11 | JSON, 8 MiB                          | First paint [ms]         | 22.8 ¹ | 20.8 ¹ | 14.9 ¹ | —       |
| L11 | JSON, 8 MiB                          | File read [ms]           | 8.8    | 8.0    | 7.8    | —       |
| L11 | JSON, 8 MiB                          | Peak private [MiB]       | 13.4   | 13.5   | 13.2   | —       |
| L12 | Markdown, 8 MiB                      | First paint [ms]         | 23.1 ¹ | 20.1 ¹ | 14.2 ¹ | —       |
| L12 | Markdown, 8 MiB                      | File read [ms]           | 9.2    | 8.3    | 8.0    | —       |
| L12 | Markdown, 8 MiB                      | Peak private [MiB]       | 13.5   | 13.4   | 13.2   | —       |
| L13 | CSV, 8 MiB                           | First paint [ms]         | 22.8 ¹ | 20.0 ¹ | 14.1 ¹ | —       |
| L13 | CSV, 8 MiB                           | File read [ms]           | 9.2    | 8.2    | 7.9    | —       |
| L13 | CSV, 8 MiB                           | Peak private [MiB]       | 13.5   | 13.5   | 13.2   | —       |
| L14 | Read-only text, 256 MiB              | First paint [ms]         | 24.8   | 20.8   | 13.4   | —       |
| L14 | Read-only text, 256 MiB              | File read [ms]           | 85.2   | 72.7   | 71.5   | —       |
| L14 | Read-only text, 256 MiB              | Peak private [MiB]       | 267.3  | 267.4  | 267.0  | —       |
| L15 | Read-only Python, 64 KiB             | First paint [ms]         | 23.0 ¹ | 19.7 ¹ | 13.4 ¹ | —       |
| L15 | Read-only Python, 64 KiB             | Peak private [MiB]       | 5.2    | 5.2    | 4.8    | —       |
| L16 | Single line text, 64 MiB             | First paint [ms]         | 23.0   | 21.9   | 13.9   | —       |
| L16 | Single line text, 64 MiB             | File read [ms]           | 25.2   | 24.3   | 23.8   | —       |
| L16 | Single line text, 64 MiB             | Peak private [MiB]       | 72.0   | 72.1   | 71.7   | —       |
| L17 | Single line JSON, 8 MiB              | First paint [ms]         | 21.2 ¹ | 19.9 ¹ | 14.8 ¹ | —       |
| L17 | Single line JSON, 8 MiB              | File read [ms]           | 8.1    | 8.0    | 8.5    | —       |
| L17 | Single line JSON, 8 MiB              | Peak private [MiB]       | 13.4   | 13.5   | 13.2   | —       |
| L18 | Notebook as JSON, 8 MiB              | First paint [ms]         | 21.4 ¹ | 20.4 ¹ | 14.3 ¹ | —       |
| L18 | Notebook as JSON, 8 MiB              | File read [ms]           | 8.4    | 8.1    | 8.2    | —       |
| L18 | Notebook as JSON, 8 MiB              | Peak private [MiB]       | 13.4   | 13.4   | 13.1   | —       |
| L19 | Markdown, 128 MiB                    | First paint [ms]         | 23.4   | 21.4   | 13.7   | —       |
| L19 | Markdown, 128 MiB                    | File read [ms]           | 43.1   | 39.8   | 39.6   | —       |
| L19 | Markdown, 128 MiB                    | Peak private [MiB]       | 136.8  | 136.8  | 136.5  | —       |
| U01 | Show the Command Palette             | Key to frame [ms]        | 0.38   | 0.38   | n/i    | —       |
| U01 | Show the Command Palette             | Key to frame, p95 [ms]   | 4.97   | 0.61   | n/i    | —       |
| U02 | Show favorites                       | Key to frame [ms]        | 0.41   | 0.33   | n/i    | —       |
| U02 | Show favorites                       | Key to frame, p95 [ms]   | 4.96   | 0.58   | n/i    | —       |
| C01 | Copy 1 KiB                           | Key to clipboard [ms]    | 0.53   | n/m    | n/m    | —       |
| C02 | Copy 1 MiB                           | Key to clipboard [ms]    | 1.61   | n/m    | n/m    | —       |
| C03 | Copy 64 MiB                          | Key to clipboard [ms]    | 113.88 | n/m    | n/m    | —       |
| C04 | Paste 1 KiB into Python, 8 MiB       | Key to frame [ms]        | 0.60   | n/m    | n/m    | —       |
| C05 | Paste 1 MiB into Python, 8 MiB       | Key to frame [ms]        | 2.02   | n/m    | n/m    | —       |
| C06 | Paste 64 MiB into text               | Key to frame [ms]        | 104.72 | n/m    | n/m    | —       |
| C06 | Paste 64 MiB into text               | Peak private [MiB]       | 781.4  | n/m    | n/m    | —       |
| E01 | Typing in Python, 8 MiB              | Key to frame [ms]        | 0.35   | n/m    | n/m    | —       |
| E02 | Typing at the start of a 64 MiB line | Key to frame [ms]        | 0.30   | n/m    | n/m    | —       |
| E03 | End and Home on a 64 MiB line        | Key to frame [ms]        | 0.42   | n/m    | n/m    | —       |
| M01 | Add next occurrence                  | Key to frame [ms]        | 0.42   | n/i    | n/i    | —       |
| M02 | Select all occurrences               | Key to frame [ms]        | 0.80   | n/i    | n/i    | —       |
| M03 | Column selection, 1,000 lines        | Key to frame [ms]        | 1.21   | n/i    | n/i    | —       |
| M04 | Insert cursor below                  | Key to frame [ms]        | 0.44   | n/i    | n/i    | —       |
| M05 | Type at 10,000 carets                | Key to frame [ms]        | 3.64   | n/i    | n/i    | —       |
| M05 | Type at 10,000 carets                | Peak private [MiB]       | 33.8   | n/i    | n/i    | —       |
| M06 | Backspace at 10,000 carets           | Key to frame [ms]        | 4.25   | n/i    | n/i    | —       |
| M07 | Paste 10,000 pieces                  | Key to frame [ms]        | 5.22   | n/i    | n/i    | —       |
| M08 | Undo and redo, 10,000 carets         | Key to frame [ms]        | 0.49   | n/i    | n/i    | —       |
| S01 | Scroll and jump, Markdown, 64 KiB    | Key to frame, p95 [ms]   | 2.13   | n/m    | n/m    | —       |
| S01 | Scroll and jump, Markdown, 64 KiB    | Key to frame, max [ms]   | 2.52   | n/m    | n/m    | —       |
| S01 | Scroll and jump, Markdown, 64 KiB    | Missed refreshes [%]     | 0.0    | n/m    | n/m    | —       |
| S02 | Scroll and jump, Markdown, 8 MiB     | Key to frame, p95 [ms]   | 2.49   | n/m    | n/m    | —       |
| S02 | Scroll and jump, Markdown, 8 MiB     | Key to frame, max [ms]   | 3.88   | n/m    | n/m    | —       |
| S02 | Scroll and jump, Markdown, 8 MiB     | Missed refreshes [%]     | 0.0    | n/m    | n/m    | —       |
| S02 | Scroll and jump, Markdown, 8 MiB     | Jump to highlighted [ms] | n/i    | n/i    | n/i    | —       |
| S03 | Scroll and jump, Markdown, 128 MiB   | Key to frame, p95 [ms]   | 2.62   | n/m    | n/m    | —       |
| S03 | Scroll and jump, Markdown, 128 MiB   | Key to frame, max [ms]   | 2.87   | n/m    | n/m    | —       |
| S03 | Scroll and jump, Markdown, 128 MiB   | Missed refreshes [%]     | 0.0    | n/m    | n/m    | —       |
| F01 | Find next, text, 256 MiB             | Key to frame [ms]        | 0.53   | n/i    | n/i    | —       |
| F02 | Count matches, text, 256 MiB         | Key to final count [ms]  | 218.81 | n/i    | n/i    | —       |
| F03 | Toggle RegEx, Python, 8 MiB          | Key to final count [ms]  | 3.08   | n/i    | n/i    | —       |
| —   | Whole suite (implemented tests)      | Run time [s]             | 21.8   | 9.6    | 6.7    | —       |

Values are medians. — no earlier version; n/i: feature not implemented
yet (highlighting: M6; search and multi-cursor before 0.3.0); n/m: not
measured, the interaction harness did not cover the test yet. ¹ Plain text:
syntax highlighting is not implemented in 0.3.0.

All tests of v1 run from 0.3.0 on. Interaction tests use the scripted input
mode with untimed setup steps (see [Tests](#tests)). A key that starts
search-thread work is timed to the frame that shows its result: F02 and F03
include the full count of the file. Copies (C01-C03) are timed until the
clipboard is written; their peak memory includes the 256 MiB fixture. The
scroll tests send one key per display refresh (16.67 ms here) and count a
missed refresh when a key took longer than one refresh. The whole suite takes
longer than in 0.2.0 because it runs 23 more tests. The U01 and U02 p95
values are the second slowest of 30 repetitions and vary between runs (0.77
and 0.57 ms in another full run of this session).

This session ran at the slower environment level described under 0.1.0, as
for 0.2.0. First paint and file read differ from 0.2.0 by up to 4.0 ms and
12.5 ms without a startup-related change; only L01 was compared directly: an
A/B against 0.2.0 rebuilt from its tag, 30 interleaved launches each,
measured first paint 21.0 ms against 21.3 ms and 5.0 MiB peak memory for
both (before the review fixes of 0.3.0, which do not touch startup). The
executable grows from 2.0 MiB to 4.0 MiB (regular expression engines).

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

Status: all tests are implemented in `cargo xtask bench-perf`
([xtask/src/perf.rs](xtask/src/perf.rs)); the suite runs in about 22 s,
plus about 35 s for the code-file tests (L20-L25, K01-K10).
Interaction tests use the editor's scripted input mode
(`--bench-setup <steps> --bench-keys <steps> --bench-runs <n>`, where a
step is a key, a repeated key such as `Down*40`, or quoted text such as
`'abc'`, typed one character per step; `--bench-select <start>..<end>`,
`--bench-handled`, `--bench-paced` and `--bench-full`, see below).
`--only L01,F01` runs a subset (its total is not checked). The whole
suite should not exceed 60 seconds on the reference machine, including
fixture generation on a first run; faster is better.
`--spelling builtin|hunspell|windows` turns spelling on with that engine
in every selected test (P01-P06 use `builtin` without it); input tests
made without spelling get 3 s more on their cap, since a script then starts
after the first marks. The spelling tests (P01-P06, about 8 s) run only
when named in `--only` or with `--spelling`; the total cap applies to the
default suite.

#### Procedure

- Builds the release binary, then runs every test in fresh processes.
- **Load tests** launch `RoyiTextEditor.exe --trace-startup --exit-after-load
  [--read-only] [--syntax <id>] <file>` (L26 without a file: an empty
  state). Times are measured from process
  creation (as in `cargo xtask bench-startup`):
  - **First paint:** the first frame is drawn into the back buffer. This is the
    compared startup metric.
  - **First frame:** the first frame has been composed by DWM. It waits for
    vertical sync, so it moves in steps of the display refresh (17 ms at
    59 Hz). The harness prints it; it is not compared.
  - **File read:** the loader thread has read the whole file (large files only;
    small files are read before the first paint).
  - **Peak private:** `PeakPagefileUsage` reported by the editor at exit.
  - **Highlighted** and **Fully highlighted** (L09, L20-L25;
    `--exit-after-highlight` instead of `--exit-after-load`): the first frame
    highlighted from current trees (trace stage `highlighted`; the view of a
    large file comes first), and the first one from a parse of the whole file
    (`fully_highlighted`).
- **Interaction tests** run one process per scenario with a scripted input
  sequence, started after the file is fully loaded and, when highlighted,
  its visible rows are highlighted. An initial selection
  (`--bench-select`) and untimed setup keys come first. Each repetition
  measures from the input message to the frame drawn (after `BitBlt`,
  without the composition wait); copies draw nothing and are measured until
  the clipboard is written (`--bench-handled`). A key that starts
  search-thread work (navigation, select all, the count and highlights of the
  search box) is measured to the frame that shows its result. Peak private
  memory is reported at exit.
- **Scroll and jump tests** (S01-S03, `--bench-paced`) send each key once
  the previous frame has been composed, one key per display refresh like a
  held key: `Down` × 40, `Up` × 40, `PgDn` × 20, `PgUp` × 20, `End` and
  `Home` alternating × 10, `Ctrl+End` and `Ctrl+Home` alternating × 10
  (140 keys, about 2.4 s at 60 Hz). They report the p95 and the maximum
  key-to-frame time and the share of keys whose frame took longer than one
  refresh period (from DWM, 16.7 ms at 60 Hz). Smooth means no missed
  refresh. S02 also reports the time from a `Ctrl+End` or `Ctrl+Home` jump to
  the first highlighted frame (viewport-first highlighting, see
  [Plan/Architecture.md](Plan/Architecture.md)); with highlighting, each key
  waits for its highlighted frame (at most 1 s) before the next refresh
  starts the next key. S03 is above
  `syntax.maxFileMiB` and scrolls plain text.
- **Code files** (K01-K10) open a 50,000-line (K01-K05) or 10,000-line
  (K06-K10) file and start once it is fully highlighted (`--bench-full`),
  with the caret in its middle (K04 at the top). The paced ones (K01-K04,
  K06-K09) also report the
  p95 time from a key to its first highlighted frame (current trees) and the
  number of keys not highlighted within 1 s (a paced run ends after the last
  key's highlighted frame). K05 and K10 are timed until the symbol
  list is shown; typing `x` after each run makes every run collect anew.
- **Spelling** (P01-P06, `--spelling`): with spelling on, a script starts
  once the marks of the view are complete, and a key is timed until the
  frame that shows the complete marks of the new view (also the palette
  with its suggestions, P05, and the selected misspelling, P04). P01-P03
  are paced like S01-S03 (P03: `End`, `Home`, `Down` × 20 on 16 KiB lines).
  P06 launches fresh processes (`--exit-after-highlight`) and reports the
  time from the first paint to the first complete marks (trace stage
  `spelling_marks`).
- Statistics: median of all repetitions; p95 for tests with ≥ 20 repetitions;
  one warm-up per test, not counted (none for the scroll tests).
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

| File              | Bytes       | XXH3-64              | Content                                           |
| ----------------- | ----------- | -------------------- | ------------------------------------------------- |
| `empty.txt`       | 0           | `0x2d06800538d394c2` | Nothing                                           |
| `small.txt`       | 65,528      | `0xb9279797cb449924` | Prose lines, LF                                   |
| `large.log`       | 268,435,456 | `0x2f843a96190ac6c8` | Log lines, CRLF                                   |
| `small.py`        | 65,106      | `0x20f0895dc8ca07af` | Python block, repeated                            |
| `medium.py`       | 8,388,270   | `0x6e6658331bdaf703` | Python block, repeated                            |
| `small.c`         | 65,082      | `0x9bc8db30d412b721` | C block, repeated                                 |
| `medium.c`        | 8,388,345   | `0x667a78bba9ac72ae` | C block, repeated                                 |
| `small.json`      | 65,447      | `0x5cccfe7086c520c0` | Array, one object per line                        |
| `medium.json`     | 8,388,523   | `0x2f7b9d6dfd037f74` | Array, one object per line                        |
| `medium.ipynb`    | 8,386,352   | `0xdbaab2506662a999` | Jupyter Notebook, 618 Markdown and 618 code cells |
| `small.md`        | 65,467      | `0xad1cf323b2be13aa` | Front matter, Markdown block, repeated            |
| `medium.md`       | 8,388,443   | `0xea9a6a2abe0b1430` | Front matter, Markdown block, repeated            |
| `large.md`        | 134,217,541 | `0xb5187e60707ad0a9` | Front matter, Markdown block, repeated            |
| `small.csv`       | 65,504      | `0xb10add99c5c4f459` | Header, CSV row, repeated                         |
| `medium.csv`      | 8,388,604   | `0x8641de1a0b731974` | Header, CSV row, repeated                         |
| `line.txt`        | 67,108,864  | `0x178f1d1aaeb2328c` | Words without any line break                      |
| `line.json`       | 8,388,551   | `0xcda2d7050b0bfede` | Minified JSON array on one line                   |
| `code.py`         | 1,693,627   | `0x5ebabdd1ff1a5b60` | Python block, repeated to 50,000 lines            |
| `code.c`          | 2,353,242   | `0xc89d830565836307` | C block, repeated to 50,000 lines                 |
| `10k.py`          | 337,182     | `0x8e415b15e8bd4e98` | Python block, repeated to 10,000 lines            |
| `10k.c`           | 464,767     | `0x057e008afce340c3` | C block, repeated to 10,000 lines                 |
| `10k.ts`          | 283,728     | `0xa69187b2ee0c321f` | TypeScript block, repeated to 10,000 lines        |
| `spell.txt`       | 1,048,572   | `0x19a974ea626c00df` | Prose, 80-byte lines, 2 % misspelled              |
| `spell.md`        | 1,048,213   | `0xf951c45e3c5457dd` | Prose sections with code, links, HTML and fences  |
| `spell-dense.txt` | 2,097,147   | `0x2f5f18ad7f447f32` | Prose, 16 KiB lines, 2 % misspelled               |
| `spell-far.txt`   | 4,259,853   | `0x0f5a3175454a96bc` | 4 MiB correct prose, one misspelling, more prose  |

Each code fixture repeats a block containing every highlighted element:

- **Python:** classes, decorators, type hints, `async`/`await`, docstrings,
  f-strings, raw and byte strings, comprehensions, numbers in several bases,
  `True`/`False`/`None`, comments.
- **C:** `#include` and `#define`, structs, typedefs, functions, `switch`,
  line and block comments, string and character literals with escapes,
  numeric suffixes.
- **TypeScript:** imports, interfaces with generics and optional members,
  enums, classes with parameter properties and getters, doc and line
  comments, `async` functions, rest parameters, regular expressions,
  template literals, arrow functions with type parameters.
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
- **Spelling:** sentences of 12 common words; every 50th word is
  misspelled, alternately a common misspelling (repeated) and a new word.
  `spell.md` adds per section a heading, a code span, a link, an HTML tag
  and a fenced block, all holding misspellings that must not be marked.

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
| L20-L21 | Load and highlight 50,000 lines (Python, C) | `code.py`, `code.c`             | 5 processes each  | 5 each  |
| L22     | Load and highlight single-line JSON         | `line.json`                     | 5 processes       | 10      |
| L23-L25 | Load and highlight 10,000 lines (Py, C, TS) | `10k.py`, `10k.c`, `10k.ts`     | 10 processes each | 2 each  |
| L26     | Start empty session                         | None                            | 20 processes      | 2       |
| U01     | Show and close the Command Palette          | `small.py`                      | 30                | 1       |
| U02     | Show and close favorites (`Ctrl+P`)         | `small.py`                      | 30                | 1       |
| C01-C03 | Copy 1 KiB, 1 MiB, 64 MiB                   | `large.log`                     | 20, 5, 3          | 2       |
| C04-C05 | Paste 1 KiB, 1 MiB (re-highlight)           | `medium.py`                     | 20, 5             | 2       |
| C06     | Paste 64 MiB                                | `large.log`                     | 3                 | 2       |
| E01     | Type 200 characters                         | `medium.py`                     | 200               | 2       |
| E02     | Type 50 characters at the line start        | `line.txt`                      | 50                | 1       |
| E03     | `End`, then `Home`                          | `line.txt`                      | 20                | 1       |
| E04     | Move Line Down (`Alt+Down`)                 | `medium.py`                     | 50                | 2       |
| E05     | Copy Line Down (`Shift+Alt+Down`)           | `medium.py`                     | 50                | 2       |
| M01     | Add next occurrence (`Ctrl+D`)              | `medium.c`                      | 100               | 1       |
| M02     | Select all occurrences of `#` (10,000 cap)  | `medium.c`                      | 5                 | 1       |
| M03     | Column selection over 1,000 lines           | `medium.c`                      | 5                 | 1       |
| M04     | Insert cursor below (`Ctrl+Alt+Down`)       | `medium.c`                      | 100               | 1       |
| M05     | Type 20 characters at 10,000 carets         | `medium.c`                      | 20                | 1       |
| M06     | Backspace 20 times at 10,000 carets         | `medium.c`                      | 20                | 1       |
| M07     | Paste 10,000 pieces, one per caret          | `medium.c`                      | 3                 | 1       |
| M08     | Undo and redo a 10,000-caret edit           | `medium.c`                      | 10                | 1       |
| F01     | Find next (`F3`), seeded `2026`             | `large.log`                     | 30                | 1       |
| F02     | Open the search box: count all matches      | `large.log`                     | 3                 | 2       |
| F03     | Toggle RegEx: count and highlights          | `medium.py`                     | 30                | 1       |
| S01     | Scroll and jump, small file                 | `small.md`                      | 140 keys          | 3       |
| S02     | Scroll and jump, medium file                | `medium.md`                     | 140 keys          | 3       |
| S03     | Scroll and jump, large file                 | `large.md`                      | 140 keys          | 3       |
| K01     | Type, 50,000 lines of Python                | `code.py`                       | 100               | 8       |
| K02     | `Enter`, `Backspace`, 50,000 lines          | `code.py`                       | 20                | 8       |
| K03     | Type, 50,000 lines of C                     | `code.c`                        | 50                | 8       |
| K04     | Scroll and jump, 50,000 lines               | `code.py`                       | 140 keys          | 8       |
| K05     | Go to Symbol, 50,000 lines                  | `code.py`                       | 10                | 5       |
| K06-K08 | Type, 10,000 lines (Python, C, TypeScript)  | `10k.py`, `10k.c`, `10k.ts`     | 100 each          | 8 each  |
| K09     | `Enter`, `Backspace`, 10,000 lines          | `10k.ts`                        | 50                | 8       |
| K10     | Go to Symbol, 10,000 lines                  | `10k.py`                        | 10                | 2       |
| P01     | Scroll and jump, spelling                   | `spell.txt`                     | 140 keys          | 3       |
| P02     | Scroll and jump, spelling Markdown          | `spell.md`                      | 140 keys          | 3       |
| P03     | Scroll dense long lines sideways            | `spell-dense.txt`               | 60 keys           | 6       |
| P04     | Next Misspelling (`F8`) 4 MiB ahead         | `spell-far.txt`                 | 5                 | 3       |
| P05     | Spelling suggestions (`Ctrl+.`)             | `spell.txt`                     | 20                | 2       |
| P06     | First marks after start                     | `spell.txt`                     | 20 processes      | 4       |

Load caps add up to 47 s, interaction caps to 104 s and fixture generation is
capped at 3 s; the total cap (60 s) is checked separately. Load caps are
about twice the measured time. On the reference machine the load tests take
about 19 s, plus about 1 s for fixtures on a first run. CSV highlighting
requires adding CSV to the language registry; it is not in the initial
language list.

Setup and keys of the scripted interaction tests:

| ID      | Setup (untimed)                                   | Timed keys                      |
| ------- | ------------------------------------------------- | ------------------------------- |
| C01-C03 | Select bytes 0 to 1 KiB, 1 MiB, 64 MiB            | `Ctrl+C` (until handled)        |
| C04-C05 | Select bytes 0 to 1 KiB, 1 MiB; `Ctrl+C`, `Right` | `Ctrl+V`                        |
| C06     | Select bytes 0 to 64 MiB; `Ctrl+C`, `Right`       | `Ctrl+V`                        |
| E01-E02 | None                                              | `'x'`                           |
| E03     | None                                              | `End`, `Home` (both timed)      |
| E04     | None                                              | `Alt+Down`                      |
| E05     | None                                              | `Shift+Alt+Down`                |
| M01     | None                                              | `Ctrl+D`                        |
| M02     | `Ctrl+D` (selects `#`)                            | `Ctrl+Shift+L` (refused)        |
| M03     | `Ctrl+Shift+Alt+PgDn` × 30                        | `Ctrl+Shift+Alt+Right`, `Left`  |
| M04     | None                                              | `Ctrl+Alt+Down`                 |
| M05     | 10K carets                                        | `'x'`                           |
| M06     | 10K carets                                        | `Backspace`                     |
| M07     | 10K selections, `Ctrl+C`                          | `Ctrl+V`                        |
| M08     | 10K carets, `'x'`                                 | `Ctrl+Z`, `Ctrl+Y` (both timed) |
| F01     | `Ctrl+F` (seeds `2026`)                           | `F3`                            |
| F02     | None                                              | `Ctrl+F`, `Escape`              |
| F03     | `Ctrl+Right`, `Ctrl+F`                            | `Alt+R`                         |
| S01-S03 | None                                              | The 140 scroll keys (all timed) |
| K01     | Caret at byte 1,000,000                           | `'x'`                           |
| K02     | Caret at byte 1,000,000                           | `Enter`, `Backspace` (timed)    |
| K03     | Caret at byte 1,000,000                           | `'x'`                           |
| K04     | None                                              | The 140 scroll keys (all timed) |
| K05     | None                                              | `Ctrl+Shift+O`, `Escape`, `'x'` |
| K06-K08 | Caret at byte 168,000, 232,000, 142,000           | `'x'`                           |
| K09     | Caret at byte 142,000                             | `Enter`, `Backspace` (timed)    |
| K10     | None                                              | `Ctrl+Shift+O`, `Escape`, `'x'` |
| P01-P02 | None                                              | The 140 scroll keys (all timed) |
| P03     | None                                              | `End`, `Home`, `Down` × 20      |
| P04     | None                                              | `F8`, `Ctrl+Home`               |
| P05     | Caret at byte 297 (in `recieve`)                  | `Ctrl+.`, `Escape`              |

**10K selections:** `Ctrl+F`, `Alt+R`, `'sum_\d{1,4}\('`, `Alt+Enter`,
`Escape` selects `sum_0(` to `sum_9999(`, exactly 10,000 matches (the
default `editor.multiCursorLimit`). **10K carets** adds `Right`, a caret after
each. C01-C06 and M07 replace the system clipboard.

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
