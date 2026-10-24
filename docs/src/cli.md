# CLI Commands

## Command-Line Options

```text
RoyiTextEditor.exe [--read-only] [--syntax <id>] [--trace-startup] [--exit-after-first-frame | --exit-after-load] [--bench-setup <sequence>] [--bench-keys <sequence>] [--bench-runs <count>] [--bench-select <start>..<end>] [--bench-handled] [--bench-paced] [file]
```

| Option | Purpose |
| --- | --- |
| `[file]` | Open the specified file. A missing path opens an empty document that can be saved to that path. Omit it to open an untitled scratch document. |
| `--read-only` | Open the document without allowing edits. |
| `--syntax <id>` | Request a syntax language override. `none` or `off` disables highlighting, case-insensitively. Other IDs are accepted but have no effect until syntax highlighting is implemented. |
| `--trace-startup` | Print startup timing and memory measurements to standard output. |
| `--exit-after-first-frame` | Exit after drawing the first frame. Intended for startup measurements. |
| `--exit-after-load` | Exit after the file finishes loading. Intended for performance measurements. |
| `--bench-setup <sequence>` | Run an untimed key sequence once before the scripted repetitions, for example `"Ctrl+F"`. Used with `--bench-keys`. |
| `--bench-select <start>..<end>` | Select these bytes once the file is loaded, before the setup. Used with `--bench-keys`. |
| `--bench-handled` | Time steps that draw nothing (copies) until they are handled. Used with `--bench-keys`. |
| `--bench-paced` | Send one step per display refresh, like a held key. Used with `--bench-keys`. |
| `--bench-keys <sequence>` | Run a scripted sequence after loading, for example `"Ctrl+Shift+P,Escape"`. Quoted text such as `'abc'` is typed one character at a time; `Down*40` repeats a key. Developer benchmark option. |
| `--bench-runs <count>` | Set the number of scripted repetitions. Used with `--bench-keys`. |

Examples:

```powershell
RoyiTextEditor.exe --read-only .\logs\today.log
RoyiTextEditor.exe --syntax json .\notebook.ipynb
RoyiTextEditor.exe --trace-startup --exit-after-first-frame .\src\main.rs
```

The first and third examples use implemented options. The second shows the
planned syntax override interface; the editor currently has no syntax registry
or highlighting engine.

## Planned Syntax IDs

The architecture proposes the following lowercase IDs. They are not currently
registered or highlighted. `none` and `off` are the only recognized values
besides the illustrative `json` override in the CLI documentation; they both
turn highlighting off.

| ID | Language |
| --- | --- |
| `rust` | Rust |
| `c` | C |
| `cpp` | C++ |
| `csharp` | C# |
| `python` | Python |
| `javascript` | JavaScript |
| `typescript` | TypeScript |
| `json` | JSON |
| `toml` | TOML |
| `yaml` | YAML |
| `markdown` | Markdown |
| `html` | HTML |
| `css` | CSS |
| `xml` | XML |
| `bash` | Bash |
| `powershell` | PowerShell |
| `lua` | Lua |
| `sql` | SQL |
| `go` | Go |
| `java` | Java |
| `matlab` | MATLAB |
| `octave` | Octave |
| `julia` | Julia |