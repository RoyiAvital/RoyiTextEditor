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

| Option          | Effect                                                         |
| --------------- | -------------------------------------------------------------- |
| `--read-only`   | Open the file without allowing edits                           |
| `--syntax <id>` | Highlighting language, e.g. `json`; `none` or `off` disables it (applies once highlighting ships) |

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
| <kbd>Ctrl</kbd>+<kbd>W</kbd>, <kbd>Alt</kbd>+<kbd>F4</kbd>     | Close; unsaved changes ask first           |
| <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>P</kbd>, <kbd>F1</kbd>   | Command Palette                            |
| <kbd>Ctrl</kbd>+<kbd>P</kbd>                                   | Recent and favorite commands               |

## Command Palette

Every command is in the palette, grouped in families: `Application:`, `File:`,
`Edit:`, `Selection:`, `Go:` and `View:`. Type any part of a name (for example
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
  recentCount: 3   # recent commands listed by Ctrl+P, 0 to 20
```

> [!NOTE]
> Syntax highlighting, search and multi-cursor editing are planned for later
> versions.