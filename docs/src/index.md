# RoyiTextEditor

A portable, keyboard-focused, lightweight and fast text and code editor for
Windows.

- **Minimalistic** and focused UI: the whole window is dedicated to text.
- **Performance**: a file opens in about 14 ms; huge files stay responsive.
- **Keyboard** oriented workflow, compatible with VS Code keys.
- **Safety**: atomic saving with a verified backup; external changes are detected.

## Installation

1. Open the [latest release](https://github.com/RoyiAvital/RoyiTextEditor/releases/latest).
2. Download `RoyiTextEditor-<version>-windows-x86_64.zip`.
3. Extract the ZIP file to a folder of your choice.
4. Run `RoyiTextEditor.exe` from the extracted folder.

> [!TIP]
> RoyiTextEditor is portable. Keep the extracted files together and move the
> folder whenever needed.

Requirements: Windows 11, x64 CPU with AVX2.

## Usage

```text
RoyiTextEditor.exe [--read-only] [--syntax <id>] [file]
```

| Option          | Effect                                                         |
| --------------- | -------------------------------------------------------------- |
| `--read-only`   | Open the file without allowing edits                           |
| `--syntax <id>` | Highlighting language, e.g. `json` (applies once highlighting ships) |

New files are created only from the command line: a path that does not exist
opens an empty document and the first save creates it.

## Keys

| Keys                                                           | Action                                     |
| -------------------------------------------------------------- | ------------------------------------------ |
| <kbd>Ctrl</kbd>+<kbd>Left</kbd> / <kbd>Ctrl</kbd>+<kbd>Right</kbd> | Move by word                           |
| <kbd>PgUp</kbd> / <kbd>PgDn</kbd>                              | Move by page                               |
| <kbd>Home</kbd> / <kbd>End</kbd>                               | First non-blank character / line end       |
| <kbd>Ctrl</kbd>+<kbd>Home</kbd> / <kbd>Ctrl</kbd>+<kbd>End</kbd> | Start / end of file                      |
| <kbd>Shift</kbd> with any move                                 | Extend the selection                       |
| <kbd>Ctrl</kbd>+<kbd>Z</kbd>, <kbd>Ctrl</kbd>+<kbd>Y</kbd>     | Undo, redo                                 |
| <kbd>Ctrl</kbd>+<kbd>C</kbd> / <kbd>X</kbd> / <kbd>V</kbd>     | Copy / cut / paste (whole line without a selection) |
| <kbd>Ctrl</kbd>+<kbd>S</kbd>                                   | Save                                       |
| <kbd>Ctrl</kbd>+<kbd>=</kbd> / <kbd>Ctrl</kbd>+<kbd>-</kbd> / <kbd>Ctrl</kbd>+<kbd>0</kbd> | Zoom in / out / reset |
| <kbd>Ctrl</kbd>+<kbd>W</kbd>                                   | Close; unsaved changes ask first           |

> [!NOTE]
> Syntax highlighting, the Command Palette, search and multi-cursor editing are
> planned for later versions.
