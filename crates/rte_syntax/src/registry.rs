//! Built-in languages and file detection.

use std::path::Path;

/// A compiled-in tree-sitter grammar with its trimmed queries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Grammar {
    Rust,
    C,
    Cpp,
    CSharp,
    Python,
    JavaScript,
    TypeScript,
    Tsx,
    Json,
    Toml,
    Yaml,
    Markdown,
    /// Inline elements of Markdown (injected into the block grammar).
    MarkdownInline,
    Html,
    Css,
    Xml,
    Bash,
    Batch,
    PowerShell,
    Lua,
    Sql,
    Go,
    Java,
    Matlab,
    Julia,
}

impl Grammar {
    pub const ALL: [Grammar; 25] = [
        Grammar::Rust,
        Grammar::C,
        Grammar::Cpp,
        Grammar::CSharp,
        Grammar::Python,
        Grammar::JavaScript,
        Grammar::TypeScript,
        Grammar::Tsx,
        Grammar::Json,
        Grammar::Toml,
        Grammar::Yaml,
        Grammar::Markdown,
        Grammar::MarkdownInline,
        Grammar::Html,
        Grammar::Css,
        Grammar::Xml,
        Grammar::Bash,
        Grammar::Batch,
        Grammar::PowerShell,
        Grammar::Lua,
        Grammar::Sql,
        Grammar::Go,
        Grammar::Java,
        Grammar::Matlab,
        Grammar::Julia,
    ];

    /// Directory of the trimmed queries under `queries/`.
    pub fn dir(self) -> &'static str {
        match self {
            Grammar::Rust => "rust",
            Grammar::C => "c",
            Grammar::Cpp => "cpp",
            Grammar::CSharp => "csharp",
            Grammar::Python => "python",
            Grammar::JavaScript => "javascript",
            Grammar::TypeScript => "typescript",
            Grammar::Tsx => "tsx",
            Grammar::Json => "json",
            Grammar::Toml => "toml",
            Grammar::Yaml => "yaml",
            Grammar::Markdown => "markdown",
            Grammar::MarkdownInline => "markdown_inline",
            Grammar::Html => "html",
            Grammar::Css => "css",
            Grammar::Xml => "xml",
            Grammar::Bash => "bash",
            Grammar::Batch => "batch",
            Grammar::PowerShell => "powershell",
            Grammar::Lua => "lua",
            Grammar::Sql => "sql",
            Grammar::Go => "go",
            Grammar::Java => "java",
            Grammar::Matlab => "matlab",
            Grammar::Julia => "julia",
        }
    }

    pub fn language(self) -> tree_sitter::Language {
        match self {
            Grammar::Rust => tree_sitter_rust::LANGUAGE.into(),
            Grammar::C => tree_sitter_c::LANGUAGE.into(),
            Grammar::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Grammar::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
            Grammar::Python => tree_sitter_python::LANGUAGE.into(),
            Grammar::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Grammar::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Grammar::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Grammar::Json => tree_sitter_json::LANGUAGE.into(),
            Grammar::Toml => tree_sitter_toml_ng::LANGUAGE.into(),
            Grammar::Yaml => tree_sitter_yaml::LANGUAGE.into(),
            Grammar::Markdown => tree_sitter_md::LANGUAGE.into(),
            Grammar::MarkdownInline => tree_sitter_md::INLINE_LANGUAGE.into(),
            Grammar::Html => tree_sitter_html::LANGUAGE.into(),
            Grammar::Css => tree_sitter_css::LANGUAGE.into(),
            Grammar::Xml => tree_sitter_xml::LANGUAGE_XML.into(),
            Grammar::Bash => tree_sitter_bash::LANGUAGE.into(),
            Grammar::Batch => tree_sitter_batch::LANGUAGE.into(),
            Grammar::PowerShell => tree_sitter_powershell::LANGUAGE.into(),
            Grammar::Lua => tree_sitter_lua::LANGUAGE.into(),
            Grammar::Sql => tree_sitter_sequel::LANGUAGE.into(),
            Grammar::Go => tree_sitter_go::LANGUAGE.into(),
            Grammar::Java => tree_sitter_java::LANGUAGE.into(),
            Grammar::Matlab => tree_sitter_matlab::LANGUAGE.into(),
            Grammar::Julia => tree_sitter_julia::LANGUAGE.into(),
        }
    }
}

/// A language of the registry.
#[derive(Debug)]
pub struct Lang {
    /// Registry id (`--syntax <id>`).
    pub id: &'static str,
    pub name: &'static str,
    pub grammar: Grammar,
    /// Lower-case, without the dot.
    pub extensions: &'static [&'static str],
    /// Exact file names (case-insensitive).
    pub file_names: &'static [&'static str],
    /// Interpreters named on a `#!` first line.
    pub interpreters: &'static [&'static str],
    /// Other names: `--syntax` and Markdown code fence info strings.
    pub aliases: &'static [&'static str],
    pub line_comment: Option<&'static str>,
    pub block_comment: Option<(&'static str, &'static str)>,
    pub brackets: &'static [(char, char)],
    /// Pairs closed automatically when the opening one is typed.
    pub auto_close: &'static [(char, char)],
}

const BRACKETS: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}')];
const CLOSE_C: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('\'', '\'')];
const CLOSE_RUST: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"')];
const CLOSE_JS: &[(char, char)] = &[
    ('(', ')'),
    ('[', ']'),
    ('{', '}'),
    ('"', '"'),
    ('\'', '\''),
    ('`', '`'),
];
const CLOSE_TAGS: &[(char, char)] = &[('"', '"'), ('\'', '\'')];
const CLOSE_MATLAB: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}'), ('"', '"')];
const C_BLOCK: Option<(&str, &str)> = Some(("/*", "*/"));

const fn lang(id: &'static str, name: &'static str, grammar: Grammar) -> Lang {
    Lang {
        id,
        name,
        grammar,
        extensions: &[],
        file_names: &[],
        interpreters: &[],
        aliases: &[],
        line_comment: None,
        block_comment: None,
        brackets: BRACKETS,
        auto_close: CLOSE_C,
    }
}

/// All languages, sorted by name.
pub static LANGUAGES: [Lang; 25] = [
    Lang {
        extensions: &["sh", "bash", "zsh", "ksh"],
        file_names: &[
            ".bashrc",
            ".bash_profile",
            ".bash_aliases",
            ".bash_logout",
            ".profile",
            ".zshrc",
            ".zprofile",
            "pkgbuild",
        ],
        interpreters: &["sh", "bash", "zsh", "ksh", "dash"],
        aliases: &["sh", "shell", "shellscript", "zsh"],
        line_comment: Some("#"),
        ..lang("bash", "Bash", Grammar::Bash)
    },
    Lang {
        extensions: &["bat", "cmd"],
        aliases: &["bat", "cmd", "dosbatch"],
        line_comment: Some("REM"),
        brackets: &[('(', ')')],
        auto_close: &[('(', ')'), ('"', '"')],
        ..lang("batch", "Batch", Grammar::Batch)
    },
    Lang {
        extensions: &["c", "h"],
        line_comment: Some("//"),
        block_comment: C_BLOCK,
        ..lang("c", "C", Grammar::C)
    },
    Lang {
        extensions: &["cs", "csx"],
        aliases: &["csharp", "c#"],
        line_comment: Some("//"),
        block_comment: C_BLOCK,
        ..lang("csharp", "C#", Grammar::CSharp)
    },
    Lang {
        extensions: &[
            "cpp", "cc", "cxx", "c++", "hpp", "hh", "hxx", "h++", "inl", "ipp", "tpp", "ino", "cu",
            "cuh",
        ],
        aliases: &["c++", "cxx", "cc"],
        line_comment: Some("//"),
        block_comment: C_BLOCK,
        ..lang("cpp", "C++", Grammar::Cpp)
    },
    Lang {
        extensions: &["css"],
        block_comment: C_BLOCK,
        ..lang("css", "CSS", Grammar::Css)
    },
    Lang {
        extensions: &["go"],
        aliases: &["golang"],
        line_comment: Some("//"),
        block_comment: C_BLOCK,
        auto_close: CLOSE_JS,
        ..lang("go", "Go", Grammar::Go)
    },
    Lang {
        extensions: &["html", "htm", "xhtml", "shtml"],
        aliases: &["htm", "xhtml"],
        block_comment: Some(("<!--", "-->")),
        brackets: &[('<', '>'), ('(', ')'), ('[', ']'), ('{', '}')],
        auto_close: CLOSE_TAGS,
        ..lang("html", "HTML", Grammar::Html)
    },
    Lang {
        extensions: &["java"],
        line_comment: Some("//"),
        block_comment: C_BLOCK,
        ..lang("java", "Java", Grammar::Java)
    },
    Lang {
        extensions: &["js", "mjs", "cjs", "jsx"],
        interpreters: &["node", "nodejs", "deno", "bun"],
        aliases: &["js", "jsx", "node"],
        line_comment: Some("//"),
        block_comment: C_BLOCK,
        auto_close: CLOSE_JS,
        ..lang("javascript", "JavaScript", Grammar::JavaScript)
    },
    Lang {
        extensions: &[
            "json",
            "jsonc",
            "json5",
            "geojson",
            "webmanifest",
            "code-workspace",
            "har",
        ],
        file_names: &[".babelrc", ".eslintrc", ".prettierrc", "composer.lock"],
        aliases: &["jsonc", "json5"],
        line_comment: Some("//"),
        block_comment: C_BLOCK,
        auto_close: &[('[', ']'), ('{', '}'), ('"', '"')],
        ..lang("json", "JSON", Grammar::Json)
    },
    Lang {
        extensions: &["jl"],
        interpreters: &["julia"],
        aliases: &["jl"],
        line_comment: Some("#"),
        block_comment: Some(("#=", "=#")),
        auto_close: CLOSE_RUST,
        ..lang("julia", "Julia", Grammar::Julia)
    },
    Lang {
        extensions: &["lua"],
        interpreters: &["lua", "luajit"],
        line_comment: Some("--"),
        block_comment: Some(("--[[", "]]")),
        ..lang("lua", "Lua", Grammar::Lua)
    },
    Lang {
        extensions: &["md", "markdown", "mdown", "mkd", "mkdn", "mdx"],
        aliases: &["md"],
        block_comment: Some(("<!--", "-->")),
        auto_close: &[('(', ')'), ('[', ']'), ('{', '}'), ('`', '`')],
        ..lang("markdown", "Markdown", Grammar::Markdown)
    },
    Lang {
        extensions: &["m"],
        line_comment: Some("%"),
        block_comment: Some(("%{", "%}")),
        auto_close: CLOSE_MATLAB,
        ..lang("matlab", "MATLAB", Grammar::Matlab)
    },
    Lang {
        interpreters: &["octave", "octave-cli"],
        line_comment: Some("%"),
        block_comment: Some(("%{", "%}")),
        auto_close: CLOSE_MATLAB,
        ..lang("octave", "Octave", Grammar::Matlab)
    },
    Lang {
        extensions: &["ps1", "psm1", "psd1"],
        interpreters: &["pwsh", "powershell"],
        aliases: &["ps1", "pwsh", "ps"],
        line_comment: Some("#"),
        block_comment: Some(("<#", "#>")),
        ..lang("powershell", "PowerShell", Grammar::PowerShell)
    },
    Lang {
        extensions: &["py", "pyw", "pyi", "pyx", "gyp", "gypi"],
        file_names: &["sconstruct", "sconscript"],
        interpreters: &["python", "python2", "python3", "pypy", "pypy3"],
        aliases: &["py", "python3"],
        line_comment: Some("#"),
        ..lang("python", "Python", Grammar::Python)
    },
    Lang {
        extensions: &["rs"],
        aliases: &["rs"],
        line_comment: Some("//"),
        block_comment: C_BLOCK,
        auto_close: CLOSE_RUST,
        ..lang("rust", "Rust", Grammar::Rust)
    },
    Lang {
        extensions: &["sql", "ddl", "dml"],
        line_comment: Some("--"),
        block_comment: C_BLOCK,
        ..lang("sql", "SQL", Grammar::Sql)
    },
    Lang {
        extensions: &["toml"],
        file_names: &[
            "cargo.lock",
            "pipfile",
            "poetry.lock",
            "uv.lock",
            "pdm.lock",
        ],
        line_comment: Some("#"),
        ..lang("toml", "TOML", Grammar::Toml)
    },
    Lang {
        extensions: &["ts", "mts", "cts"],
        interpreters: &["ts-node", "tsx"],
        aliases: &["ts"],
        line_comment: Some("//"),
        block_comment: C_BLOCK,
        auto_close: CLOSE_JS,
        ..lang("typescript", "TypeScript", Grammar::TypeScript)
    },
    Lang {
        extensions: &["tsx"],
        aliases: &["typescriptreact"],
        line_comment: Some("//"),
        block_comment: C_BLOCK,
        auto_close: CLOSE_JS,
        ..lang("tsx", "TypeScript JSX", Grammar::Tsx)
    },
    Lang {
        extensions: &[
            "xml",
            "xsd",
            "xsl",
            "xslt",
            "svg",
            "csproj",
            "vcxproj",
            "fsproj",
            "vbproj",
            "props",
            "targets",
            "xaml",
            "plist",
            "resx",
            "nuspec",
            "wsdl",
            "rss",
            "atom",
            "config",
            "manifest",
            "storyboard",
            "xib",
            "kml",
            "gpx",
            "dae",
        ],
        block_comment: Some(("<!--", "-->")),
        brackets: &[('<', '>'), ('(', ')'), ('[', ']'), ('{', '}')],
        auto_close: CLOSE_TAGS,
        ..lang("xml", "XML", Grammar::Xml)
    },
    Lang {
        extensions: &["yaml", "yml"],
        file_names: &[".clang-format", ".clang-tidy", ".clangd"],
        aliases: &["yml"],
        line_comment: Some("#"),
        ..lang("yaml", "YAML", Grammar::Yaml)
    },
];

/// Markdown's inline grammar (not selectable).
pub static MARKDOWN_INLINE: Lang = lang(
    "markdown_inline",
    "Markdown Inline",
    Grammar::MarkdownInline,
);

/// Language for a registry id or alias (case-insensitive).
pub fn by_id(id: &str) -> Option<&'static Lang> {
    LANGUAGES.iter().find(|l| {
        l.id.eq_ignore_ascii_case(id) || l.aliases.iter().any(|a| a.eq_ignore_ascii_case(id))
    })
}

/// Language of a Markdown code fence info string (`rust`, `py`, `c++`, ...).
pub fn by_fence(info: &str) -> Option<&'static Lang> {
    let word = info
        .trim_start_matches('{')
        .trim_start_matches('.')
        .split(|c: char| c.is_whitespace() || c == ',' || c == '{' || c == '}')
        .next()
        .unwrap_or("");
    if word.is_empty() {
        return None;
    }
    by_id(word).or_else(|| {
        LANGUAGES
            .iter()
            .find(|l| l.extensions.iter().any(|e| e.eq_ignore_ascii_case(word)))
    })
}

/// Language of a file from its name and first bytes (shebang, Octave markers).
pub fn detect(path: &Path, head: &[u8]) -> Option<&'static Lang> {
    let name = path.file_name()?.to_str()?;
    let lower = name.to_ascii_lowercase();
    if let Some(lang) = LANGUAGES
        .iter()
        .find(|l| l.file_names.contains(&lower.as_str()))
    {
        return Some(lang);
    }
    if let Some((_, ext)) = lower.rsplit_once('.')
        && !ext.is_empty()
    {
        if ext == "m" {
            return by_id(if is_octave(head) { "octave" } else { "matlab" });
        }
        if let Some(lang) = LANGUAGES.iter().find(|l| l.extensions.contains(&ext)) {
            return Some(lang);
        }
    }
    shebang(head)
}

/// Language named by a `#!` first line (`#!/usr/bin/env python3`).
fn shebang(head: &[u8]) -> Option<&'static Lang> {
    let rest = head.strip_prefix(b"#!")?;
    let line = &rest[..memchr_newline(rest)];
    let line = std::str::from_utf8(line).ok()?;
    let mut words = line.split_ascii_whitespace();
    let mut program = words.next()?.rsplit('/').next()?;
    if program == "env" {
        program = words.find(|w| !w.starts_with('-'))?;
    }
    // python3.12 → python3
    let program = program.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
    LANGUAGES.iter().find(|l| {
        l.interpreters
            .iter()
            .any(|i| i.trim_end_matches(|c: char| c.is_ascii_digit()) == program)
    })
}

fn memchr_newline(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .position(|&b| b == b'\n')
        .unwrap_or(bytes.len())
}

/// Octave markers in a `.m` file: `#` comments, `end*` keywords of Octave,
/// `!=`, `printf`-style output and `1;` script files.
fn is_octave(head: &[u8]) -> bool {
    let text = String::from_utf8_lossy(&head[..head.len().min(64 * 1024)]);
    for line in text.lines() {
        let line = line.trim_start();
        if line.starts_with('#') {
            return true;
        }
        let code = line.split('%').next().unwrap_or("");
        for word in code.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            if matches!(
                word,
                "endfunction"
                    | "endif"
                    | "endwhile"
                    | "endfor"
                    | "endswitch"
                    | "end_try_catch"
                    | "end_unwind_protect"
                    | "unwind_protect"
                    | "endparfor"
            ) {
                return true;
            }
        }
        if code.contains("!=") {
            return true;
        }
    }
    false
}
