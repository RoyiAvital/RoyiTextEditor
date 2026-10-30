# CLI Commands

## Command-Line Options

```text
RoyiTextEditor.exe [--read-only] [--syntax <id>] [--spelling <engine>] [--trace-startup] [--exit-after-first-frame | --exit-after-load | --exit-after-highlight] [--bench-setup <sequence>] [--bench-keys <sequence>] [--bench-runs <count>] [--bench-select <start>..<end>] [--bench-handled] [--bench-paced] [file]
```

| Option                          | Purpose                                                                                                                                                                                                                  |
| ------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `[file]`                        | Open the specified file. A missing path opens an empty document that can be saved to that path; a folder is refused. Omit it to start in the empty state (drop a file on the window to open it).                     |
| `--read-only`                   | Open every file of the session without allowing edits.                                                                                                                                                                   |
| `--syntax <id>`                 | Highlight the command-line file as language `<id>` (table below; ids and aliases, any case) instead of the language of its name; dropped files follow their names. `none` or `off` disables highlighting. An unknown id keeps the detected language and shows a notice. |
| `--spelling <engine>`           | Spelling for the session: `off`, or on with the `builtin`, `hunspell` or `windows` engine (overrides `spelling.enabled` and `spelling.engine`). An unknown value is reported and ignored.                       |
| `--trace-startup`               | Print startup timing and memory measurements to standard output.                                                                                                                                                         |
| `--exit-after-first-frame`      | Exit after drawing the first frame. Intended for startup measurements.                                                                                                                                                   |
| `--exit-after-load`             | Exit after the file finishes loading. Intended for performance measurements.                                                                                                                                             |
| `--exit-after-highlight`        | Like `--exit-after-load`; a highlighted file also waits until it is highlighted from a parse of the whole file (trace stage `fully_highlighted`), and with spelling on until its first complete marks (`spelling_marks`). Intended for performance measurements. |
| `--bench-setup <sequence>`      | Run an untimed key sequence once before the scripted repetitions, for example `"Ctrl+F"`. Used with `--bench-keys`.                                                                                                      |
| `--bench-select <start>..<end>` | Select these bytes once the file is loaded, before the setup. Used with `--bench-keys`.                                                                                                                                  |
| `--bench-handled`               | Time steps that draw nothing (copies) until they are handled. Used with `--bench-keys`.                                                                                                                                  |
| `--bench-paced`                 | Send one step per display refresh, like a held key; with highlighting, each step first waits (at most 1 s) for its highlighted frame. Used with `--bench-keys`.                                                          |
| `--bench-keys <sequence>`       | Run a scripted sequence after loading, for example `"Ctrl+Shift+P,Escape"`. Quoted text such as `'abc'` is typed one character at a time; `Down*40` repeats a key. Developer benchmark option.                           |
| `--bench-runs <count>`          | Set the number of scripted repetitions. Used with `--bench-keys`.                                                                                                                                                        |

Examples:

```powershell
RoyiTextEditor.exe --read-only .\logs\today.log
RoyiTextEditor.exe --syntax json .\notebook.ipynb
RoyiTextEditor.exe --trace-startup --exit-after-first-frame .\src\main.rs
```

The first example opens a log read-only, the second highlights a notebook as
JSON (`.ipynb` is not detected as JSON), the third measures startup.

## Syntax IDs

| ID           | Language       | Aliases                             |
| ------------ | -------------- | ----------------------------------- |
| `bash`       | Bash           | `sh`, `shell`, `shellscript`, `zsh` |
| `batch`      | Batch          | `bat`, `cmd`, `dosbatch`            |
| `c`          | C              |                                     |
| `csharp`     | C#             | `c#`                                |
| `cpp`        | C++            | `c++`, `cxx`, `cc`                  |
| `css`        | CSS            |                                     |
| `go`         | Go             | `golang`                            |
| `html`       | HTML           | `htm`, `xhtml`                      |
| `ini`        | INI            | `dosini`                            |
| `java`       | Java           |                                     |
| `javascript` | JavaScript     | `js`, `jsx`, `node`                 |
| `json`       | JSON           | `jsonc`, `json5`                    |
| `julia`      | Julia          | `jl`                                |
| `lua`        | Lua            |                                     |
| `markdown`   | Markdown       | `md`                                |
| `matlab`     | MATLAB         |                                     |
| `octave`     | Octave         |                                     |
| `powershell` | PowerShell     | `ps1`, `pwsh`, `ps`                 |
| `python`     | Python         | `py`, `python3`                     |
| `rust`       | Rust           | `rs`                                |
| `sql`        | SQL            |                                     |
| `toml`       | TOML           |                                     |
| `typescript` | TypeScript     | `ts`                                |
| `tsx`        | TypeScript JSX | `typescriptreact`                   |
| `xml`        | XML            |                                     |
| `yaml`       | YAML           | `yml`                               |

The same names (and file extensions) select the language of Markdown code
fences.

Windows Batch files are detected by their `.bat` or `.cmd` extension; INI
files by `.ini`, `.cfg` or `.inf`, and the names `.editorconfig`, `.gitconfig`
and `.gitmodules`.