# Search and Selections

Find text as you type, narrow the search, then replace matches or edit them
together.

![Find panel showing the query "Text", two matches, and search options](assets/search-panel.png)

## Find

Press `Ctrl+F`. Selected text becomes the query; with no selection, the word
at the caret is used. Press `Enter` / `Shift+Enter` to move between matches;
search wraps at the document ends. Press `Esc` to close the panel.

| Panel control  | Use it for                                                   | Shortcut |
| -------------- | ------------------------------------------------------------ | -------- |
| `Aa`           | Match letter case                                            | `Alt+C`  |
| `ab`           | Match whole words                                            | `Alt+W`  |
| `*?`           | Glob patterns: `*` within a line, `?` for one character      | `Alt+G`  |
| `.*`           | Regular expressions                                          | `Alt+R`  |
| Selection icon | Search only inside the current selection. Select text first. | `Alt+L`  |

Rest the pointer on a control of the panel to see its name and shortcut.

- **Whole words** keep accents (combining marks) with their word, as in VS Code.
- **Regular expressions** use Rust syntax. With look-around or
  backreferences, write line breaks as `\n`: `.` with `(?s)` or `\s` does not
  reach the next line. Such a pattern that backtracks heavily on a very long
  line keeps the search busy until it finishes; `Esc` does not interrupt it.

## Replace and Select

Press `Ctrl+H`, or click the arrow at the left of the panel, to show Replace. `Enter` in the Replace field replaces the
current match and moves to the next. `Ctrl+Alt+Enter` replaces all matches in
one undo step; `Alt+P` toggles case-preserving replacement.

| Shortcut                        | Result                                       |
| ------------------------------- | -------------------------------------------- |
| `F3` / `Shift+F3`               | Next / previous match                        |
| `Alt+Enter`                     | Select every match as a separate selection   |
| `Ctrl+D`                        | Add the next occurrence of the selected text |
| `Ctrl+Shift+L`                  | Select all occurrences of the selected text  |
| `Ctrl+Alt+Up` / `Ctrl+Alt+Down` | Add a caret on the line above / below        |