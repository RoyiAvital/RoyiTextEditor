# Syntax Highlighting

Code is detected from its file name or `#!` line. It opens immediately as
plain text; syntax colors follow as parsing completes.

![Rust source with syntax highlighting and the detected language in the status bar](assets/syntax-highlighted-code.png)

Press `Ctrl+K M` to choose a language or **Plain Text**. The picker filters as
you type. To toggle highlighting for the current document, open the Command
Palette with `Ctrl+Shift+P` and run **View: Toggle Syntax Highlighting**.

![Language picker with supported modes](assets/syntax-language-picker.png)

Supported modes: Bash, Batch, C, C#, C++, CSS, Go, HTML, INI, Java,
JavaScript, JSON, Julia, Lua, Markdown, MATLAB, Octave, PowerShell, Python,
Rust, SQL, TOML, TSX, TypeScript, XML, and YAML.

Files larger than 32 MiB are plain text by default. Set `syntax.maxFileMiB`
from 1 to 1024 in `RoyiTextEditor.yaml` to change the limit. Markdown and HTML
also highlight embedded code by default; set `syntax.injections: false` to
disable it. Launch with `--syntax none` to start with highlighting off.