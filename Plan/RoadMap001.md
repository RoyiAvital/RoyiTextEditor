# Road Map 001

Features not built yet: what the [Architecture](../Done/Architecture001.md)
planned but did not build (M0-M7 are done), and the pending plans. Each one
gets a decision; accepted ones get their own task document.

| #  | Title                            | Description                                                                                                          | Decision by Royi |
| -- | -------------------------------- | -------------------------------------------------------------------------------------------------------------------- | ---------------- |
| 1  | Edit journal and crash recovery  | Unsaved edits survive a crash, a kill or a power loss; a recovery hint on open ([Data Safety 001](DataSafety001.md)) |                  |
| 2  | 3-way merge of outside changes   | Merge outside changes with unsaved edits instead of Reload / Save Current ([Data Safety 001](DataSafety001.md))      |                  |
| 3  | Minimap                          | VS Code style outline of the lines at the right of the text, with a slider ([Minimap](Minimap.md))                   |                  |
| 4  | Font choice                      | Select one of the five bundled fonts; all ship in `Fonts/`, only JetBrains Mono is used                              |                  |
| 5  | Light theme                      | A built-in light theme beside the dark one (deferred at M7)                                                          |                  |
| 6  | Custom key bindings              | `keybindings:` in `RoyiTextEditor.yaml`, VS Code semantics (M7 review: VS Code defaults only)                        |                  |
| 7  | Hide title strip and status line | A setting that hides the title strip and the status line                                                             |                  |
| 8  | Mica backdrop                    | Windows 11 Mica material behind the title strip                                                                      |                  |
| 9  | Auto-indent rules per language   | `indents.scm` rules beyond the indent after an opening bracket (TODO lists 13 languages)                             |                  |
| 10 | Highlight other occurrences      | Mark the other occurrences of the word at the caret in the visible rows, as VS Code                                  |                  |
| 11 | Open a file (`Ctrl+O`)           | A file dialog that opens a file into this session                                                                    | Defer            |
| 12 | Palette prefixes `:` and `@`     | Typing `:` or `@` in the Command Palette switches to Go to Line or Go to Symbol, as VS Code                          |                  |
| 13 | Word wrap (`Alt+Z`)              | Long lines wrap at the window edge; the view already walks screen rows                                               |                  |
| 14 | Folding                          | Fold and unfold regions (`Ctrl+Shift+[`, `Ctrl+K Ctrl+0`, ...) from `folds.scm` or indentation                       |                  |
| 15 | Smooth scrolling                 | An animated scroll mode (`editor.smoothScrolling`); today wheel and keys move whole lines                            |                  |
| 16 | Blinking caret                   | A setting for a blinking caret; today it is steady                                                                   |                  |
| 17 | Text shaping                     | `editor.textShaping`: shape non-ASCII clusters with `swash`; today one glyph per character                           |                  |
| 18 | IME input                        | Composition input for CJK languages; dead keys and AltGr already work                                                |                  |
| 19 | CSV / TSV table mode             | A table view over the same text, with a background record index                                                      | Defer            |
| 20 | LCD subpixel text                | Subpixel anti-aliasing as a setting; today grayscale                                                                 |                  |
| 21 | Screen reader support            | UI Automation accessibility                                                                                          | Defer            |
| 22 | Legacy encodings                 | Code pages other than UTF-8 and UTF-16                                                                               |                  |
| 23 | Edit malformed UTF-16            | Lossless editing (WTF-8 style storage); today such files are read-only                                               |                  |
| 24 | System font fallback             | Draw characters missing from the bundled fonts with a system font; today a box                                       |                  |
| 25 | Exact long-line columns          | A background index for exact columns past 64 KiB on a line; today one column per byte                                |                  |
| 26 | GPU presenter                    | A Direct3D 11 presenter, only if the CPU renderer misses the frame budgets                                           | Defer            |
| 27 | Damage tracking                  | Redraw only changed rows, only if a profile shows a need                                                             |                  |
| 28 | Grammars as DLLs                 | Load grammars lazily from DLLs, only if the executable size hurts startup                                            | Defer            |

Items 1-12 were planned for v1; 13-24 after v1; 25-28 only if a
measurement shows a need. Other open items (spelling in code, minify, ...)
are in [TODO.md](../TODO.md).

## Reviewers

### 2026_11_02 - Claude Opus 5.5

- Role: Reviewer
- Activity: Design
- Agent: GitHub Copilot
- Model: Claude Opus 5.5
- Effort: Medium
- Context Window: 872K
- Outcome: Compiled when the Architecture was marked done: its planned but
  unbuilt items, the after-v1 list, the measurement-gated options and the
  pending Data Safety 001 and Minimap plans. Decisions pending.
