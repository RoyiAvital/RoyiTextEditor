//! `RoyiTextEditor.yaml` next to the executable: read once at startup, never
//! written. Problems are reported but never block startup; an invalid value
//! keeps its default.
//!
//! ```yaml
//! palette:
//!   recentCount: 3   # recent commands shown by Ctrl+P, 0 to 20
//! editor:
//!   multiCursorLimit: 10000   # most cursors, 1 to 1000000
//! search:
//!   replaceAllMaxMiB: 512     # memory Replace All may use, 1 to 4096
//! syntax:
//!   maxFileMiB: 32            # larger files are not highlighted, 1 to 1024
//!   injections: true          # embedded languages (Markdown, HTML)
//! ```

use std::path::Path;

use saphyr_parser::{Event, Parser};

pub const FILE_NAME: &str = concat!(env!("RTE_FILE_NAME"), ".yaml");
/// Largest `palette.recentCount`.
pub const RECENT_MAX: usize = 20;
const RECENT_PROBLEM: &str = "palette.recentCount must be a number from 0 to 20";
const CURSORS_PROBLEM: &str = "editor.multiCursorLimit must be a number from 1 to 1000000";
const REPLACE_PROBLEM: &str = "search.replaceAllMaxMiB must be a number from 1 to 4096";
const SYNTAX_MAX_PROBLEM: &str = "syntax.maxFileMiB must be a number from 1 to 1024";
const INJECTIONS_PROBLEM: &str = "syntax.injections must be true or false";
const SECTIONS: &[&str] = &["palette", "editor", "search", "syntax"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub recent_count: usize,
    pub multi_cursor_limit: usize,
    pub replace_all_max_mib: u64,
    pub syntax_max_mib: u64,
    pub injections: bool,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            recent_count: 3,
            multi_cursor_limit: 10_000,
            replace_all_max_mib: 512,
            syntax_max_mib: 32,
            injections: true,
        }
    }
}

/// Plain YAML: maps, lists and scalars only.
#[derive(Debug, PartialEq)]
enum Value {
    Scalar(String),
    List(Vec<Value>),
    Map(Vec<(String, Value)>),
}

/// Reads `path`; a missing file gives the defaults without a problem.
pub fn load(path: &Path) -> (Config, Option<String>) {
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Config::default(), None),
        Err(e) => (Config::default(), Some(format!("{FILE_NAME}: {e}"))),
    }
}

/// Settings in `text` and the first problem found.
pub fn parse(text: &str) -> (Config, Option<String>) {
    let mut config = Config::default();
    let mut problems = Vec::new();
    match read(text) {
        Err(e) => problems.push(e),
        Ok(None) => {}
        Ok(Some(Value::Map(entries))) => {
            for (key, value) in entries {
                let section = key.clone();
                let Value::Map(settings) = value else {
                    problems.push(if SECTIONS.contains(&key.as_str()) {
                        format!("{key} must be a map")
                    } else {
                        format!("unknown setting {key}")
                    });
                    continue;
                };
                for (name, value) in settings {
                    let number = |max: u64, min: u64| match &value {
                        Value::Scalar(s) => {
                            s.parse::<u64>().ok().filter(|n| (min..=max).contains(n))
                        }
                        _ => None,
                    };
                    match (section.as_str(), name.as_str()) {
                        ("palette", "recentCount") => match number(RECENT_MAX as u64, 0) {
                            Some(n) => config.recent_count = n as usize,
                            None => problems.push(RECENT_PROBLEM.to_owned()),
                        },
                        ("editor", "multiCursorLimit") => match number(1_000_000, 1) {
                            Some(n) => config.multi_cursor_limit = n as usize,
                            None => problems.push(CURSORS_PROBLEM.to_owned()),
                        },
                        ("search", "replaceAllMaxMiB") => match number(4096, 1) {
                            Some(n) => config.replace_all_max_mib = n,
                            None => problems.push(REPLACE_PROBLEM.to_owned()),
                        },
                        ("syntax", "maxFileMiB") => match number(1024, 1) {
                            Some(n) => config.syntax_max_mib = n,
                            None => problems.push(SYNTAX_MAX_PROBLEM.to_owned()),
                        },
                        ("syntax", "injections") => match &value {
                            Value::Scalar(s) if s == "true" || s == "false" => {
                                config.injections = s == "true";
                            }
                            _ => problems.push(INJECTIONS_PROBLEM.to_owned()),
                        },
                        (section, name) if SECTIONS.contains(&section) => {
                            problems.push(format!("unknown setting {section}.{name}"));
                        }
                        _ => {
                            problems.push(format!("unknown setting {section}"));
                            break;
                        }
                    }
                }
            }
        }
        Ok(Some(_)) => problems.push("the top level must be a map".to_owned()),
    }
    let problem = problems.into_iter().next();
    (config, problem.map(|p| format!("{FILE_NAME}: {p}")))
}

enum Open {
    List(Vec<Value>),
    /// Entries and the key awaiting its value.
    Map(Vec<(String, Value)>, Option<String>),
}

/// The single document of `text`, or `None` when it is empty.
fn read(text: &str) -> Result<Option<Value>, String> {
    let mut open: Vec<Open> = Vec::new();
    let mut root = None;
    let mut documents = 0;
    for event in Parser::new_from_str(text) {
        let (event, span) = event.map_err(|e| e.to_string())?;
        let line = span.start.line();
        let plain = |anchor: usize, tagged: bool| {
            if anchor != 0 || tagged {
                Err(format!("line {line}: anchors and tags are not supported"))
            } else {
                Ok(())
            }
        };
        let value = match event {
            Event::DocumentStart(_) => {
                documents += 1;
                if documents > 1 {
                    return Err(format!("line {line}: only one document is allowed"));
                }
                continue;
            }
            Event::Alias(_) => return Err(format!("line {line}: aliases are not supported")),
            Event::Scalar(text, _, anchor, tag) => {
                plain(anchor, tag.is_some())?;
                Value::Scalar(text.into_owned())
            }
            Event::SequenceStart(anchor, tag) => {
                plain(anchor, tag.is_some())?;
                open.push(Open::List(Vec::new()));
                continue;
            }
            Event::MappingStart(anchor, tag) => {
                plain(anchor, tag.is_some())?;
                open.push(Open::Map(Vec::new(), None));
                continue;
            }
            Event::SequenceEnd | Event::MappingEnd => match open.pop() {
                Some(Open::List(items)) => Value::List(items),
                Some(Open::Map(entries, _)) => Value::Map(entries),
                None => continue,
            },
            _ => continue,
        };
        match open.last_mut() {
            None => root = Some(value),
            Some(Open::List(items)) => items.push(value),
            Some(Open::Map(entries, pending)) => match (pending.take(), value) {
                (Some(key), value) => entries.push((key, value)),
                (None, Value::Scalar(key)) => *pending = Some(key),
                (None, _) => return Err(format!("line {line}: keys must be plain text")),
            },
        }
    }
    // An empty document is one empty scalar.
    Ok(root.filter(|v| *v != Value::Scalar(String::new())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_empty_files_give_the_defaults() {
        let dir = std::env::temp_dir().join(format!("rte_config_{}", std::process::id()));
        assert_eq!(load(&dir.join(FILE_NAME)), (Config::default(), None));
        assert_eq!(parse(""), (Config::default(), None));
        assert_eq!(parse("# only a comment\n"), (Config::default(), None));
    }

    #[test]
    fn reads_the_recent_count() {
        let (config, problem) = parse("palette:\n  recentCount: 7\n");
        assert_eq!(config.recent_count, 7);
        assert_eq!(problem, None);
        assert_eq!(parse("palette: {recentCount: 0}").0.recent_count, 0);
    }

    #[test]
    fn reads_the_search_and_cursor_limits() {
        let (config, problem) =
            parse("editor:\n  multiCursorLimit: 500\nsearch:\n  replaceAllMaxMiB: 64\n");
        assert_eq!(problem, None);
        assert_eq!(config.multi_cursor_limit, 500);
        assert_eq!(config.replace_all_max_mib, 64);
        let (config, problem) = parse("editor:\n  multiCursorLimit: 0\n");
        assert_eq!(config.multi_cursor_limit, 10_000);
        assert!(problem.unwrap().contains("editor.multiCursorLimit"));
        let (_, problem) = parse("search:\n  replaceAllMaxMiB: 5000\n");
        assert!(problem.unwrap().contains("search.replaceAllMaxMiB"));
        let (_, problem) = parse("search:\n  other: 1\n");
        assert!(problem.unwrap().contains("unknown setting search.other"));
    }

    #[test]
    fn reads_the_syntax_settings() {
        let (config, problem) = parse("syntax:\n  maxFileMiB: 8\n  injections: false\n");
        assert_eq!(problem, None);
        assert_eq!((config.syntax_max_mib, config.injections), (8, false));
        let (config, problem) = parse("syntax:\n  maxFileMiB: 0\n");
        assert_eq!(config.syntax_max_mib, 32);
        assert!(problem.unwrap().contains("syntax.maxFileMiB"));
        let (config, problem) = parse("syntax:\n  injections: yes\n");
        assert!(config.injections);
        assert!(problem.unwrap().contains("syntax.injections"));
    }

    #[test]
    fn invalid_values_keep_the_default_and_are_reported() {
        for text in [
            "palette:\n  recentCount: 21\n",
            "palette:\n  recentCount: -1\n",
            "palette:\n  recentCount: no\n",
            "palette:\n  recentCount: [1]\n",
        ] {
            let (config, problem) = parse(text);
            assert_eq!(config, Config::default(), "{text}");
            assert!(problem.unwrap().contains("palette.recentCount"), "{text}");
        }
    }

    #[test]
    fn unknown_settings_and_bad_structure_are_reported() {
        let (config, problem) = parse("palette:\n  recentCount: 5\n  extra: 1\ntheme: x\n");
        assert_eq!(config.recent_count, 5, "valid settings still apply");
        assert_eq!(
            problem.as_deref(),
            Some(format!("{FILE_NAME}: unknown setting palette.extra").as_str())
        );
        assert!(parse("- 1\n- 2\n").1.unwrap().contains("top level"));
        assert!(
            parse("palette: 3\n")
                .1
                .unwrap()
                .contains("palette must be a map")
        );
    }

    #[test]
    fn anchors_tags_aliases_and_syntax_errors_are_rejected() {
        for text in [
            "palette: &p\n  recentCount: 1\n",
            "palette:\n  recentCount: !!int 4\n",
            "a: &x 1\nb: *x\n",
            "palette: [unclosed\n",
            "a: 1\n---\nb: 2\n",
        ] {
            let (config, problem) = parse(text);
            assert_eq!(config, Config::default(), "{text}");
            assert!(problem.is_some(), "{text}");
        }
    }
}
