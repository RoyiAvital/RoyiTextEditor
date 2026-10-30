# Spelling

Spelling marks misspelled US English words in plain text files and in the
prose of Markdown files. It is off by default.

## Turning It On

- In `RoyiTextEditor.yaml`: `spelling.enabled: true` (see
  [Configuration](configuration.md#spelling)).
- For one session: start with `--spelling builtin` (or `hunspell`,
  `windows`); `--spelling off` turns it off.
- While editing: run **View: Toggle Spelling** in the Command Palette.

Spelling starts after the window is shown and runs on its own thread, so it
never delays opening a file. With spelling off, nothing spelling-related
runs.

## What Is Checked

- **Files:** plain text and Markdown. Code files are not checked.
- **Rows:** the visible rows; marks follow as you scroll.
- **Markdown:** prose only. Code spans, fenced and indented code blocks,
  link destinations, autolinks, HTML and front matter are skipped. The
  structure comes from the syntax thread, so Markdown is checked while
  syntax highlighting is on; with `--syntax none` a Markdown file is checked
  as plain text.
- **Words:** letters with inner apostrophes (`don't`); a trailing `'s` is
  dropped. Skipped: words with digits or `_`, all-capital words (acronyms),
  words longer than 64 bytes, URLs, e-mail addresses and paths.
- **Case:** a word is correct when it, its lowercase form or (for a
  capitalized dictionary word) its capitalized form is in the dictionary.

A misspelled word gets a blue wavy underline. With the caret on one, the
status line shows `Spelling: recieve (Ctrl+.)`, and `Ctrl+.` opens the
Spelling palette:

![A misspelled word with a blue wavy underline and the Spelling palette: suggestions, a separator, then Add to Dictionary, Ignore, Next and Previous Misspelling](assets/spelling.png)

In the picture the caret is on `Recieve`: it has the wavy underline, the
status line names it, and the palette lists its suggestions, a separator
and the spelling commands.

## Fixing Words

| Keys                       | Action                                                         |
| -------------------------- | -------------------------------------------------------------- |
| `Ctrl+.`                   | Suggestions for the word at the caret, then the commands below |
| `Enter` (Spelling palette) | Replace the word with the selected suggestion (one undo step)  |
| `F8`; `Shift+F8`           | Select the next; previous misspelled word (wraps at the ends)  |

The Spelling palette lists up to 8 suggestions, closest first, then a
separator and the commands.

It works like the Command Palette: `Up` / `Down` move (and wrap around),
`Enter` or a click runs the selected row, typing filters the rows and
`Esc` closes it. The first suggestion is selected, so `Ctrl+.` then
`Enter` fixes a word; `Ctrl+Z` undoes it. While the suggestions are being
collected the first row says so and `Enter` waits.

- **Add to Dictionary:** the word is correct from now on, in every session
  and for every engine.
- **Ignore:** the word is correct in this window until it closes (or
  spelling is toggled off).
- **Next Misspelling** and **Previous Misspelling** (`F8`, `Shift+F8`).

Away from a misspelled word, `Ctrl+.` lists only Next and Previous. `F8`
in a long file shows its progress on the status line; it reports
`No other misspellings` when the word at the caret is the only one, and
`No misspellings found` when there is none.

## Commands

All spelling actions are also in the Command Palette (`Ctrl+Shift+P`), in
the `Spelling:` family; type `spell` to list them.

| Command                          | Key        | Action                                            |
| -------------------------------- | ---------- | ------------------------------------------------- |
| `Spelling: Show Suggestions`     | `Ctrl+.`   | The Spelling palette for the word at the caret    |
| `Spelling: Next Misspelling`     | `F8`       | Select the next misspelled word                   |
| `Spelling: Previous Misspelling` | `Shift+F8` | Select the previous misspelled word               |
| `Spelling: Add to Dictionary`    | None       | Add the word at the caret to the user dictionary  |
| `Spelling: Ignore Word`          | None       | Ignore the word at the caret in this window       |
| `View: Toggle Spelling`          | None       | Turn spelling off or on for this window           |

The keys are also listed by `Application: Keyboard Shortcuts`
(`Ctrl+K Ctrl+S`) and on the [Keyboard Shortcuts](keyboard-shortcuts.md#spelling)
page.

## Engines

Set by `spelling.engine`. All engines see the same words and the same added
and ignored words; their verdicts can differ.

| Engine     | Dictionary                                                         |
| ---------- | ------------------------------------------------------------------ |
| `builtin`  | US English word list in the executable (default)                   |
| `hunspell` | `Dictionaries/en_US.aff` and `.dic` beside the executable          |
| `windows`  | The Windows spell checker, when US English is installed in Windows |

- The shipped Hunspell dictionary is the SCOWL-based `en_US` release; its
  license is `Dictionaries/README_en_US.txt`. `spelling.hunspell` can name
  another `.aff` / `.dic` pair.
- The built-in list is made from the same dictionary when the editor is
  built.
- An engine that cannot start (missing files, US English not installed) is
  reported once on the status line and `builtin` is used.
- The Windows engine never writes to the Windows dictionary; added and
  ignored words stay in the editor.

## The User Dictionary

Added words are kept in `UserSettings/Spelling/en_US.txt` beside the
executable: one word per line, at most 64 KiB and 10,000 words. You can
edit it with any editor; it is read when spelling starts. Adding a word
reads the file, merges the new word and writes it, so words added by
another session are kept. A file that cannot be read or is not a valid word
list is never overwritten; added words then last for the session, with a
notice.
