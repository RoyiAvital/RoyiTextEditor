//! Trims the upstream queries of each grammar into `queries/<grammar>/`
//! (`cargo xtask syntax-queries`; a test keeps the files up to date).
//!
//! Highlights keep only patterns with a themed capture ([`style_of`]) and
//! without predicates tree-sitter cannot evaluate itself. Patterns are
//! ordered so that the first pattern matching a node wins: more specific
//! patterns (more nodes and predicates) first, then upstream precedence.
//! Upstream files mix conventions (most current grammars let the last
//! pattern win, but still list a method call before the generic property
//! pattern that also matches it), so specificity decides first.

use std::collections::HashSet;

use tree_sitter::Query;

use crate::{Grammar, style_of};

struct Upstream {
    /// Crates (and files) the queries come from; all MIT licensed.
    origin: &'static str,
    highlights: &'static [&'static str],
    /// Upstream precedence is "the last matching pattern wins".
    later_wins: bool,
    tags: &'static [&'static str],
}

const JULIA: &str = include_str!("../queries/upstream/julia-highlights.scm");
const MATLAB: &str = include_str!("../queries/upstream/matlab-highlights.scm");
const MATLAB_TAGS: &str = include_str!("../queries/upstream/matlab-tags.scm");

fn upstream(grammar: Grammar) -> Upstream {
    let up = |origin, highlights, later_wins, tags| Upstream {
        origin,
        highlights,
        later_wins,
        tags,
    };
    use tree_sitter_javascript as js;
    use tree_sitter_typescript as ts;
    match grammar {
        Grammar::Rust => up(
            "tree-sitter-rust",
            &[tree_sitter_rust::HIGHLIGHTS_QUERY],
            true,
            &[tree_sitter_rust::TAGS_QUERY],
        ),
        Grammar::C => up(
            "tree-sitter-c",
            &[tree_sitter_c::HIGHLIGHT_QUERY],
            true,
            &[tree_sitter_c::TAGS_QUERY],
        ),
        Grammar::Cpp => up(
            "tree-sitter-c, tree-sitter-cpp",
            &[
                tree_sitter_c::HIGHLIGHT_QUERY,
                tree_sitter_cpp::HIGHLIGHT_QUERY,
            ],
            true,
            &[tree_sitter_cpp::TAGS_QUERY],
        ),
        Grammar::CSharp => up(
            "tree-sitter-c-sharp",
            &[tree_sitter_c_sharp::HIGHLIGHTS_QUERY],
            true,
            &[tree_sitter_c_sharp::TAGS_QUERY],
        ),
        Grammar::Python => up(
            "tree-sitter-python",
            &[tree_sitter_python::HIGHLIGHTS_QUERY],
            true,
            &[tree_sitter_python::TAGS_QUERY],
        ),
        Grammar::JavaScript => up(
            "tree-sitter-javascript",
            &[js::HIGHLIGHT_QUERY, js::JSX_HIGHLIGHT_QUERY],
            true,
            &[js::TAGS_QUERY],
        ),
        Grammar::TypeScript => up(
            "tree-sitter-javascript, tree-sitter-typescript",
            &[js::HIGHLIGHT_QUERY, ts::HIGHLIGHTS_QUERY],
            true,
            &[js::TAGS_QUERY, ts::TAGS_QUERY],
        ),
        Grammar::Tsx => up(
            "tree-sitter-javascript, tree-sitter-typescript",
            &[
                js::HIGHLIGHT_QUERY,
                js::JSX_HIGHLIGHT_QUERY,
                ts::HIGHLIGHTS_QUERY,
            ],
            true,
            &[js::TAGS_QUERY, ts::TAGS_QUERY],
        ),
        Grammar::Json => up(
            "tree-sitter-json",
            &[tree_sitter_json::HIGHLIGHTS_QUERY],
            false,
            &[],
        ),
        Grammar::Toml => up(
            "tree-sitter-toml-ng",
            &[tree_sitter_toml_ng::HIGHLIGHTS_QUERY],
            true,
            &[],
        ),
        Grammar::Yaml => up(
            "tree-sitter-yaml",
            &[tree_sitter_yaml::HIGHLIGHTS_QUERY],
            true,
            &[],
        ),
        Grammar::Markdown => up(
            "tree-sitter-md (block)",
            &[tree_sitter_md::HIGHLIGHT_QUERY_BLOCK],
            true,
            &[],
        ),
        Grammar::MarkdownInline => up(
            "tree-sitter-md (inline)",
            &[tree_sitter_md::HIGHLIGHT_QUERY_INLINE],
            true,
            &[],
        ),
        Grammar::Html => up(
            "tree-sitter-html",
            &[tree_sitter_html::HIGHLIGHTS_QUERY],
            false,
            &[],
        ),
        Grammar::Css => up(
            "tree-sitter-css",
            &[tree_sitter_css::HIGHLIGHTS_QUERY],
            false,
            &[],
        ),
        Grammar::Xml => up(
            "tree-sitter-xml",
            &[tree_sitter_xml::XML_HIGHLIGHT_QUERY],
            false,
            &[],
        ),
        Grammar::Bash => up(
            "tree-sitter-bash",
            &[tree_sitter_bash::HIGHLIGHT_QUERY],
            true,
            &[],
        ),
        Grammar::Batch => up(
            "tree-sitter-batch",
            &[tree_sitter_batch::HIGHLIGHTS_QUERY],
            true,
            &[],
        ),
        Grammar::PowerShell => up(
            "tree-sitter-powershell",
            &[tree_sitter_powershell::HIGHLIGHTS_QUERY],
            false,
            &[],
        ),
        Grammar::Lua => up(
            "tree-sitter-lua",
            &[tree_sitter_lua::HIGHLIGHTS_QUERY],
            true,
            &[tree_sitter_lua::TAGS_QUERY],
        ),
        Grammar::Sql => up(
            "tree-sitter-sequel",
            &[tree_sitter_sequel::HIGHLIGHTS_QUERY],
            true,
            &[],
        ),
        Grammar::Go => up(
            "tree-sitter-go",
            &[tree_sitter_go::HIGHLIGHTS_QUERY],
            false,
            &[tree_sitter_go::TAGS_QUERY],
        ),
        Grammar::Java => up(
            "tree-sitter-java",
            &[tree_sitter_java::HIGHLIGHTS_QUERY],
            true,
            &[tree_sitter_java::TAGS_QUERY],
        ),
        Grammar::Matlab => up(
            "tree-sitter-matlab (helix highlights, neovim tags)",
            &[MATLAB],
            false,
            &[MATLAB_TAGS],
        ),
        Grammar::Julia => up("tree-sitter-julia", &[JULIA], true, &[]),
    }
}

/// Pattern rewrites after trimming: (grammar, pattern, replacement), for
/// upstream captures that differ from the other grammars.
const RENAMES: &[(Grammar, &str, &str)] = &[
    (
        Grammar::Rust,
        "(integer_literal) @constant.builtin",
        "(integer_literal) @number",
    ),
    (
        Grammar::Rust,
        "(float_literal) @constant.builtin",
        "(float_literal) @number",
    ),
];

/// Generated files: (path under `queries/`, content).
pub fn generate() -> Result<Vec<(String, String)>, String> {
    let mut files = Vec::new();
    for grammar in Grammar::ALL {
        let up = upstream(grammar);
        let dir = grammar.dir();
        let highlights = trim(grammar, up.highlights, up.later_wins, Keep::Highlights)?;
        files.push((
            format!("{dir}/highlights.scm"),
            render(up.origin, "highlights", &highlights),
        ));
        if !up.tags.is_empty() {
            let tags = trim(grammar, up.tags, false, Keep::Tags)?;
            files.push((format!("{dir}/tags.scm"), render(up.origin, "tags", &tags)));
        }
    }
    Ok(files)
}

fn render(origin: &str, kind: &str, patterns: &[String]) -> String {
    let mut out = format!(
        "; Generated by `cargo xtask syntax-queries` from the {kind} queries of\n\
         ; {origin} (MIT License). Do not edit.\n"
    );
    for pattern in patterns {
        out.push('\n');
        out.push_str(pattern);
        out.push('\n');
    }
    out
}

#[derive(Clone, Copy)]
enum Keep {
    Highlights,
    Tags,
}

fn trim(
    grammar: Grammar,
    sources: &[&str],
    later_wins: bool,
    keep: Keep,
) -> Result<Vec<String>, String> {
    let text = sources.join("\n");
    let query = Query::new(&grammar.language(), &text)
        .map_err(|e| format!("{}: upstream query: {e}", grammar.dir()))?;
    let mut starts = HashSet::new();
    let mut patterns = Vec::new();
    for index in 0..query.pattern_count() {
        let start = query.start_byte_for_pattern(index);
        if !starts.insert(start) {
            continue;
        }
        let source = strip_comments(&text[start..query.end_byte_for_pattern(index)]);
        let captures = capture_names(&source);
        let wanted = match keep {
            Keep::Highlights => {
                query.general_predicates(index).is_empty()
                    && captures
                        .iter()
                        .any(|name| style_of(name).is_some() || *name == "none")
            }
            Keep::Tags => captures.iter().any(|name| name.starts_with("definition.")),
        };
        if wanted {
            let renamed = RENAMES
                .iter()
                .find(|(g, from, _)| *g == grammar && *from == source)
                .map(|(_, _, to)| to.to_string());
            patterns.push(renamed.unwrap_or(source));
        }
    }
    if later_wins {
        patterns.reverse();
    }
    // Stable: equal specificity keeps the upstream precedence.
    patterns.sort_by_key(|p| std::cmp::Reverse(specificity(p)));
    let joined = patterns.join("\n");
    Query::new(&grammar.language(), &joined)
        .map_err(|e| format!("{}: trimmed query: {e}", grammar.dir()))?;
    Ok(patterns)
}

/// Named nodes, anonymous nodes (strings outside predicates) and predicates
/// of a pattern.
fn specificity(pattern: &str) -> usize {
    let bytes = pattern.as_bytes();
    let mut count = 0;
    let mut in_string = false;
    let mut escaped = false;
    // Parenthesis depth of the predicate being read, if any.
    let mut depth = 0usize;
    let mut predicate_depth = None;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            _ if escaped => escaped = false,
            b'\\' if in_string => escaped = true,
            b'"' => {
                if !in_string && predicate_depth.is_none() {
                    count += 1;
                }
                in_string = !in_string;
            }
            _ if in_string => {}
            b'(' => {
                depth += 1;
                match bytes.get(i + 1) {
                    Some(b'#') => {
                        count += 1;
                        predicate_depth = Some(depth);
                    }
                    Some(c) if c.is_ascii_alphabetic() || *c == b'_' => count += 1,
                    _ => {}
                }
            }
            b')' => {
                if predicate_depth == Some(depth) {
                    predicate_depth = None;
                }
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    count
}

/// Pattern text without `;` comments, blank lines and trailing spaces.
fn strip_comments(pattern: &str) -> String {
    let mut out = String::new();
    for line in pattern.lines() {
        let mut in_string = false;
        let mut escaped = false;
        let mut end = line.len();
        for (i, c) in line.char_indices() {
            match c {
                _ if escaped => escaped = false,
                '\\' if in_string => escaped = true,
                '"' => in_string = !in_string,
                ';' if !in_string => {
                    end = i;
                    break;
                }
                _ => {}
            }
        }
        let kept = line[..end].trim_end();
        if !kept.trim().is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(kept);
        }
    }
    out
}

/// `@name` captures of a pattern (outside strings).
fn capture_names(pattern: &str) -> Vec<&str> {
    let mut names = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    let bytes = pattern.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        i += 1;
        match b {
            _ if escaped => escaped = false,
            b'\\' if in_string => escaped = true,
            b'"' => in_string = !in_string,
            b'@' if !in_string => {
                let start = i;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'.' | b'-'))
                {
                    i += 1;
                }
                names.push(&pattern[start..i]);
            }
            _ => {}
        }
    }
    names
}
