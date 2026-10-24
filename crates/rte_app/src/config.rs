//! `RoyiTextEditor.yaml` next to the executable: read once at startup, never
//! written. Problems are reported but never block startup; an invalid value
//! keeps its default.
//!
//! ```yaml
//! palette:
//!   recentCount: 3   # recent commands shown by Ctrl+P, 0 to 20
//! ```

use std::path::Path;

use saphyr_parser::{Event, Parser};

pub const FILE_NAME: &str = concat!(env!("RTE_FILE_NAME"), ".yaml");
/// Largest `palette.recentCount`.
pub const RECENT_MAX: usize = 20;
const RECENT_PROBLEM: &str = "palette.recentCount must be a number from 0 to 20";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub recent_count: usize,
}

impl Default for Config {
    fn default() -> Config {
        Config { recent_count: 3 }
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
                match (key.as_str(), value) {
                    ("palette", Value::Map(palette)) => {
                        for (key, value) in palette {
                            match (key.as_str(), value) {
                                ("recentCount", Value::Scalar(s)) => match s.parse() {
                                    Ok(n) if n <= RECENT_MAX => config.recent_count = n,
                                    _ => problems.push(RECENT_PROBLEM.to_owned()),
                                },
                                ("recentCount", _) => problems.push(RECENT_PROBLEM.to_owned()),
                                (key, _) => problems.push(format!("unknown setting palette.{key}")),
                            }
                        }
                    }
                    ("palette", _) => problems.push("palette must be a map".to_owned()),
                    (key, _) => problems.push(format!("unknown setting {key}")),
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
