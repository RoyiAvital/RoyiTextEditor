# Royi Text Editor

A portable, keyboard-focused, lightweight and fast text code editor for
Windows.

- **Minimalistic** and focused UI: the whole window is dedicated to text.
- **Performance**: a file opens in about 14 ms; huge files stay responsive.
- **Keyboard** oriented workflow, compatible with VS Code keys.
- **Safety**: atomic saving with a verified backup; external changes are detected.

<figure class="product-shot">
  <img src="assets/editor-screenshot.png" alt="RoyiTextEditor showing the requested two lines in its editor window">
  <figcaption>Royi Text Editor</figcaption>
</figure>

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

| Option          | Effect                                                                                     |
| --------------- | ------------------------------------------------------------------------------------------ |
| `--read-only`   | Open files without allowing edits                                                          |
| `--syntax <id>` | Highlighting language, e.g. `json` ([ids](cli.md#syntax-ids)); `none` or `off` disables it |

New files are created only from the command line: a path that does not exist
opens an empty document and the first save creates it.

## One File per Session

Each window is a session that edits one file. It never starts, closes or
talks to another session. This keeps the editor simple and fast, and fits
its main use: opening a file from a file manager.

- **Empty state.** Started without a file, or after <kbd>Ctrl</kbd>+<kbd>W</kbd>,
  the window shows `Drop a file here to open it`. Nothing can be edited; the
  Command Palette, About, Keyboard Shortcuts and zoom still work.
- **Drag and drop.** Drop a file on the window to open it. It replaces the
  current file only when that file has no unsaved changes; otherwise the
  status line asks you to save or close it first. Only the first of several
  dropped files opens; folders are refused.
- **Close and quit.** <kbd>Ctrl</kbd>+<kbd>W</kbd> closes the file and keeps
  the window; <kbd>Alt</kbd>+<kbd>F4</kbd> quits. Both ask first about
  unsaved changes.

> [!NOTE]
> Windows does not deliver drops from Explorer to an editor running as
> administrator.

## Keys

| Keys                                                                                       | Action                                              |
| ------------------------------------------------------------------------------------------ | --------------------------------------------------- |
| <kbd>Ctrl</kbd>+<kbd>Left</kbd> / <kbd>Ctrl</kbd>+<kbd>Right</kbd>                         | Move by word                                        |
| <kbd>PgUp</kbd> / <kbd>PgDn</kbd>                                                          | Move by page                                        |
| <kbd>Home</kbd> / <kbd>End</kbd>                                                           | First non-blank character / line end                |
| <kbd>Ctrl</kbd>+<kbd>Home</kbd> / <kbd>Ctrl</kbd>+<kbd>End</kbd>                           | Start / end of file                                 |
| <kbd>Shift</kbd> with any move                                                             | Extend the selection                                |
| <kbd>Ctrl</kbd>+<kbd>Z</kbd>, <kbd>Ctrl</kbd>+<kbd>Y</kbd>                                 | Undo, redo                                          |
| <kbd>Ctrl</kbd>+<kbd>C</kbd> / <kbd>X</kbd> / <kbd>V</kbd>                                 | Copy / cut / paste (whole line without a selection) |
| <kbd>Ctrl</kbd>+<kbd>S</kbd>                                                               | Save                                                |
| <kbd>Ctrl</kbd>+<kbd>=</kbd> / <kbd>Ctrl</kbd>+<kbd>-</kbd> / <kbd>Ctrl</kbd>+<kbd>0</kbd> | Zoom in / out / reset                               |
| <kbd>Ctrl</kbd>+<kbd>W</kbd>                                                               | Close the file (back to the empty state)            |
| <kbd>Alt</kbd>+<kbd>F4</kbd>, <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>W</kbd>                | Quit; unsaved changes ask first                     |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>P</kbd>, <kbd>F1</kbd>                               | Command Palette                                     |
| <kbd>Ctrl</kbd>+<kbd>P</kbd>                                                               | Recent and favorite commands                        |
| <kbd>Ctrl</kbd>+<kbd>F</kbd> / <kbd>Ctrl</kbd>+<kbd>H</kbd>                                | Find / replace                                      |
| <kbd>Ctrl</kbd>+<kbd>D</kbd>, <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>L</kbd>                | Add next occurrence, select all occurrences         |
| <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>Up</kbd> / <kbd>Down</kbd>                             | Add a cursor above / below                          |
| <kbd>Alt</kbd>+click, <kbd>Shift</kbd>+<kbd>Alt</kbd>+drag                                 | Add a cursor, column selection                      |
| <kbd>Ctrl</kbd>+<kbd>K</kbd> <kbd>M</kbd>                                                  | Change Language Mode                                |
| <kbd>Ctrl</kbd>+<kbd>/</kbd>, <kbd>Shift</kbd>+<kbd>Alt</kbd>+<kbd>A</kbd>                 | Toggle line / block comment                         |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>O</kbd>                                              | Go to a symbol                                      |

All keys, including search options and replace, are listed in
[Keyboard Shortcuts](keyboard-shortcuts.md).

## Search

<kbd>Ctrl</kbd>+<kbd>F</kbd> opens the search box, seeded with the selection
or the word at the caret. Three modes: **Text** (default), **Glob**
(<kbd>Alt</kbd>+<kbd>G</kbd>) and **RegEx** (<kbd>Alt</kbd>+<kbd>R</kbd>), with
Match Case, Whole Word and Find in Selection. Large files are searched on a
background thread: the count streams in and the editor keeps responding.
<kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>Enter</kbd> replaces all matches in one
undo step.

## Syntax Highlighting

Code is highlighted with built-in tree-sitter grammars for Bash, Batch, C, C#, C++,
CSS, Go, HTML, Java, JavaScript, JSON, Julia, Lua, Markdown, MATLAB, Octave,
PowerShell, Python, Rust, SQL, TOML, TypeScript (and TSX), XML and YAML. The
language follows the file name or a `#!` line and is shown on the status line;
<kbd>Ctrl</kbd>+<kbd>K</kbd> <kbd>M</kbd> changes it. The window opens with
plain text and the highlights follow within milliseconds; large files are
highlighted around the view first. Markdown also highlights code fences, front
matter and HTML blocks, and HTML its scripts and styles.

Windows Batch files (`.bat` and `.cmd`) use Batch highlighting automatically.
Use `--syntax batch` or select Batch in Change Language Mode to override it.

Code-aware editing: comments (<kbd>Ctrl</kbd>+<kbd>/</kbd>,
<kbd>Shift</kbd>+<kbd>Alt</kbd>+<kbd>A</kbd>), auto-closing brackets and quotes,
indentation after an opening bracket, the matching bracket
(<kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>\\</kbd>), Expand and Shrink Selection
(<kbd>Shift</kbd>+<kbd>Alt</kbd>+<kbd>Right</kbd> / <kbd>Left</kbd>) and Go to
Symbol (<kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>O</kbd>).

## Command Palette

Every command is in the palette, grouped in families: `Application:`, `File:`,
`Edit:`, `Selection:`, `Search:`, `Go:` and `View:`. Type any part of a name (for example
`fsave`), choose with <kbd>Up</kbd> / <kbd>Down</kbd>, run with
<kbd>Enter</kbd> or a click, close with <kbd>Esc</kbd>.

- Click the star of a command, or press <kbd>Ctrl</kbd>+<kbd>Space</kbd>, to
  add or remove it from the favorites.
- <kbd>Ctrl</kbd>+<kbd>P</kbd> lists the last commands run from the palette,
  then the favorites.
- `Application: Keyboard Shortcuts` lists all keys; `Application: About` shows
  the version and links.
- `View: Toggle Syntax Highlighting` turns highlighting off or on.

Favorites and recent commands are kept in `UserSettings/Palette.json` beside
the executable.

## Configuration

An optional `RoyiTextEditor.yaml` beside the executable is read at startup.
The editor never writes it. Problems are shown on the status line and invalid
values keep their defaults.

```yaml
palette:
  recentCount: 3          # recent commands listed by Ctrl+P, 0 to 20
editor:
  multiCursorLimit: 10000 # most selections, 1 to 1,000,000
search:
  replaceAllMaxMiB: 512   # memory for one Replace All [MiB], 1 to 4096
syntax:
  maxFileMiB: 32          # larger files are not highlighted, 1 to 1024
  injections: true        # embedded languages (Markdown, HTML)
```