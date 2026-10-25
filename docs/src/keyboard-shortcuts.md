# Keyboard Shortcuts

Shortcuts use the Windows key names shown below. Modifier keys must match the
binding exactly. For example, `Ctrl+Shift+S` is not Save.

| Keys                                                                            | Action                                                                   |
| ------------------------------------------------------------------------------- | ------------------------------------------------------------------------ |
| `Left` / `Right`; `Ctrl+Left` / `Ctrl+Right`                                    | Move by character; move by word                                          |
| `Up` / `Down`; `PgUp` / `PgDn`                                                  | Move by line; move by page                                               |
| `Home`; `End`                                                                   | Move to first non-blank, then line start; move to line end               |
| `Ctrl+Home`; `Ctrl+End`                                                         | Move to start; move to end of file                                       |
| `Shift` + any movement key; `Shift`+click                                       | Extend the selection                                                     |
| `Ctrl+A`; `Esc`                                                                 | Select all; collapse selections                                          |
| Double-click; triple-click                                                      | Select a word; select a line                                             |
| `Backspace` / `Delete`; `Ctrl+Backspace` / `Ctrl+Delete`                        | Delete a character; delete a word                                        |
| `Enter`                                                                         | Insert a line break and keep indentation                                 |
| `Tab`; `Shift+Tab`                                                              | Insert indentation; indent or outdent selected lines                     |
| `Insert`                                                                        | Toggle overtype mode                                                     |
| `Ctrl+Z`; `Ctrl+Y` / `Ctrl+Shift+Z`                                             | Undo; redo                                                               |
| `Ctrl+C` / `Ctrl+X` / `Ctrl+V`                                                  | Copy; cut; paste. Copy or cut the current line when nothing is selected. |
| `Ctrl+S`                                                                        | Save                                                                     |
| `Ctrl+W`; `Ctrl+F4`                                                             | Close the file and return to the empty state (unsaved changes ask first) |
| `Alt+F4`; `Ctrl+Shift+W`                                                        | Quit the application (unsaved changes ask first)                         |
| `Ctrl+Shift+P`; `F1`                                                            | Open the Command Palette                                                 |
| `Ctrl+P`                                                                        | Show recent and favorite commands                                        |
| `Ctrl+Space` (palette)                                                          | Add or remove the selected command from favorites                        |
| `Up` / `Down` / `PgUp` / `PgDn` (palette)                                       | Move through palette entries                                             |
| `Enter` or click (palette); `Esc`                                               | Run the selected command; close the palette                              |
| Wheel; `Ctrl+Up` / `Ctrl+Down`                                                  | Scroll vertically                                                        |
| `Shift`+wheel                                                                   | Scroll horizontally                                                      |
| `Ctrl+=` / `Ctrl+Shift+=` / `Ctrl+Numpad+`; `Ctrl+-` / `Ctrl+Numpad-`; `Ctrl+0` | Zoom in; zoom out; reset zoom                                            |
| `Ctrl`+wheel                                                                    | Zoom in or out                                                           |

Mouse clicks place the caret. `Shift`+click extends the current selection.
Commands without a shortcut are available by name in the Command Palette.

## Search and Replace

| Keys                                       | Action                                              |
| ------------------------------------------ | --------------------------------------------------- |
| `Ctrl+F`; `Ctrl+H`                         | Open the search box; open it with the replace field |
| `Enter` / `Shift+Enter`; `F3` / `Shift+F3` | Next / previous match                               |
| `Alt+Enter`                                | Select all matches                                  |
| `Alt+C`; `Alt+W`                           | Toggle Match Case; toggle Whole Word                |
| `Alt+G`; `Alt+R`                           | Toggle Glob; toggle RegEx. Both off is Text.        |
| `Alt+L`                                    | Toggle Find in Selection                            |
| `Enter` or `Ctrl+Shift+1` (replace field)  | Replace the current match and go to the next        |
| `Ctrl+Alt+Enter`                           | Replace all matches in one undo step                |
| `Alt+P`                                    | Toggle Preserve Case                                |
| `Esc`                                      | Close the search box                                |

Glob: `?` matches one character; `*` matches the shortest text within a line,
and a trailing `*` matches the rest of the line. Replacements use the VS Code
syntax: `$1`, `$<name>`, `$0`, `$$`, `\n`, `\t`, `\u`, `\l`, `\U` and `\L`.

## Multiple Cursors

| Keys                                        | Action                                                 |
| ------------------------------------------- | ------------------------------------------------------ |
| `Ctrl+D`; `Ctrl+K Ctrl+D`                   | Add the next occurrence; move the last selection to it |
| `Ctrl+Shift+L`; `Ctrl+F2`                   | Select all occurrences                                 |
| `Ctrl+Alt+Up` / `Ctrl+Alt+Down`             | Add a cursor above / below                             |
| `Shift+Alt+I`                               | Add cursors to the ends of the selected lines          |
| `Ctrl+L`                                    | Select the line, then extend by lines                  |
| `Ctrl+U`                                    | Undo the last cursor operation                         |
| `Alt`+click                                 | Add or remove a cursor                                 |
| `Shift+Alt`+drag; middle-drag               | Column selection                                       |
| `Ctrl+Shift+Alt` + arrows / `PgUp` / `PgDn` | Column selection by keys                               |

At most `editor.multiCursorLimit` selections (10,000 by default) are created;
larger requests are refused with a message on the status line.

## Code

| Keys                                                     | Action                                                                    |
| -------------------------------------------------------- | ------------------------------------------------------------------------- |
| `Ctrl+K M`                                               | Change Language Mode (or Plain Text)                                      |
| `Ctrl+/`                                                 | Toggle Line Comment (block comments per line in CSS, HTML, XML, Markdown) |
| `Shift+Alt+A`                                            | Toggle Block Comment                                                      |
| `Ctrl+Shift+\`                                           | Go to the matching bracket                                                |
| `Shift+Alt+Right`; `Shift+Alt+Left`                      | Expand the selection to the enclosing syntax node; shrink it back         |
| `Ctrl+Shift+O`                                           | Go to a symbol of the file                                                |
| `(` `[` `{` `"` `'` (language-dependent)                 | Insert the closing character; with a selection, wrap it                   |
| `Backspace` between an empty pair                        | Delete both characters                                                    |
| `Enter` after `(`, `[`, `{` (and `:` in Python and YAML) | New line indented one level                                               |
