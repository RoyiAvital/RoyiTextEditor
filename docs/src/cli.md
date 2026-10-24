# CLI Commands

## Command-Line Options

```text
RoyiTextEditor.exe [--read-only] [--syntax <id>] [--trace-startup] [--exit-after-first-frame | --exit-after-load] [--bench-keys <sequence>] [--bench-runs <count>] [file]
```

| Option | Purpose |
| --- | --- |
| `[file]` | Open the specified file. A missing path opens an empty document that can be saved to that path. Omit it to open an untitled scratch document. |
| `--read-only` | Open the document without allowing edits. |
| `--syntax <id>` | Request a syntax language override. `none` or `off` disables highlighting, case-insensitively. Other IDs are accepted but have no effect until syntax highlighting is implemented. |
| `--trace-startup` | Print startup timing and memory measurements to standard output. |
| `--exit-after-first-frame` | Exit after drawing the first frame. Intended for startup measurements. |
| `--exit-after-load` | Exit after the file finishes loading. Intended for performance measurements. |
| `--bench-keys <sequence>` | Run a scripted key sequence after loading, for example `"Ctrl+Shift+P,Escape"`. Developer benchmark option. |
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