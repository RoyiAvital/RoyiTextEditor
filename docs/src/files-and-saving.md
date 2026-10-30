# Files and Saving

How the editor opens and saves files, and what it does in the unusual
cases.

## Saving

`Ctrl+S` writes the text to a temporary file next to the original, then
replaces the original in one step and checks the result. A failed save
never leaves the file half written.

- **Unsaved changes** show `●` in the title strip.
- **Changed by another program:** before replacing the file, the editor
  checks that it is still the version it loaded. If not, the status line
  shows `<name> changed on disk.   O: Overwrite   Esc: Cancel`.
- **Changed during the save:** the other program's version is kept next to
  the file, and the status line names it.
- **Deleted meanwhile:** the save creates the file again.
- **New file:** a path from the command line that does not exist opens an
  empty document (`New file` on the status line); the first save creates
  it. If a file appears there meanwhile, the editor asks before
  overwriting it.
- **Access denied or in use:** nothing is changed, no temporary file is
  left, and the status line names the cause.

## Read-Only Files

A file with the Windows **Read-only** attribute opens for editing, but it is
not saved. `Ctrl+S` writes nothing; the unsaved dot turns orange and the
status line asks to clear the attribute:

![A read-only file after Ctrl+S: an orange unsaved dot in the title strip and the status line "ReadOnly.txt is read-only. Clear its Read-only attribute and save again"](assets/read-only-save.png)

Clear the attribute (Explorer: Properties, uncheck **Read-only**), then
press `Ctrl+S` again. The editor never changes file attributes itself. The
attribute is checked only when saving, so opening a file stays fast.
Quitting with changes asks `<name> is read-only. Save changes?`.

To open a file without allowing edits, start the editor with
`--read-only`; the status line then shows `RO: read-only`.

## Text Encoding and Line Breaks

| File                                     | What happens                                                    |
| ---------------------------------------- | --------------------------------------------------------------- |
| UTF-8, with or without BOM               | Edited and saved as it was                                      |
| UTF-16 with a BOM                        | Edited as text, saved as UTF-16                                 |
| UTF-16 without a BOM                     | Read as UTF-8: the zero bytes show as `␀`                       |
| Malformed UTF-16                         | Read-only, never saved (`Malformed UTF-16, read-only`)          |
| Invalid UTF-8 bytes                      | Shown as `�`, saved unchanged                                   |
| LF or CRLF line breaks                   | New lines use the style most lines have (CRLF on a tie or none) |
| Line breaks with a lone CR (classic Mac) | Shown as one line, each CR as `␍`; the status line says so      |

## Large Files and Long Lines

- **Loading:** a large file shows its beginning at once and the status line
  shows `Loading N%`. It can be read, but not edited, until it is loaded.
- **Long lines:** lines longer than 64 KiB are drawn to their end. Past
  64 KiB every byte takes one column (exact for ASCII), and the status line
  shows `Col ~N`.
- **Highlighting:** files above `syntax.maxFileMiB` (32 MiB) are plain
  text. See [Configuration](configuration.md).
