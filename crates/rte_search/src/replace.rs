//! Replacement strings (VS Code syntax) and Preserve Case.

/// Most a case conversion grows UTF-8 text (for example `ΐ` to `Ϊ́`).
pub(crate) const CASE_GROWTH: u64 = 3;

/// A capture group reference in a replacement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Group {
    Index(usize),
    Name(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Case {
    UpperFirst,
    LowerFirst,
    Upper,
    Lower,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Piece {
    Text(String),
    Group(Group),
    Case(Case),
}

/// A parsed replacement. Regex mode understands `$1`..`$99`, `$0` / `$&`,
/// `$<name>`, `$$`, `\n`, `\t`, `\\` and the case operators `\u` `\l` `\U`
/// `\L`, which apply to the next group or text piece. Other modes insert the
/// text as typed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Replacement {
    pieces: Vec<Piece>,
}

impl Replacement {
    pub fn literal(text: &str) -> Replacement {
        Replacement {
            pieces: vec![Piece::Text(text.to_owned())],
        }
    }

    pub fn parse(text: &str) -> Replacement {
        let chars: Vec<char> = text.chars().collect();
        let mut pieces = Vec::new();
        let mut buf = String::new();
        let flush = |buf: &mut String, pieces: &mut Vec<Piece>| {
            if !buf.is_empty() {
                pieces.push(Piece::Text(std::mem::take(buf)));
            }
        };
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            let next = chars.get(i + 1).copied();
            match (c, next) {
                ('$', Some('$')) => {
                    buf.push('$');
                    i += 2;
                }
                ('$', Some('&')) => {
                    flush(&mut buf, &mut pieces);
                    pieces.push(Piece::Group(Group::Index(0)));
                    i += 2;
                }
                ('$', Some(d)) if d.is_ascii_digit() => {
                    flush(&mut buf, &mut pieces);
                    let mut n = d.to_digit(10).unwrap_or(0) as usize;
                    i += 2;
                    if let Some(d2) = chars.get(i).and_then(|c| c.to_digit(10)) {
                        n = n * 10 + d2 as usize;
                        i += 1;
                    }
                    pieces.push(Piece::Group(Group::Index(n)));
                }
                ('$', Some('<')) => match chars[i + 2..].iter().position(|&c| c == '>') {
                    Some(len) if len > 0 => {
                        flush(&mut buf, &mut pieces);
                        let name: String = chars[i + 2..i + 2 + len].iter().collect();
                        pieces.push(Piece::Group(Group::Name(name)));
                        i += 3 + len;
                    }
                    _ => {
                        buf.push('$');
                        i += 1;
                    }
                },
                ('\\', Some(e)) => {
                    let case = match e {
                        'u' => Some(Case::UpperFirst),
                        'l' => Some(Case::LowerFirst),
                        'U' => Some(Case::Upper),
                        'L' => Some(Case::Lower),
                        _ => None,
                    };
                    match (case, e) {
                        (Some(case), _) => {
                            flush(&mut buf, &mut pieces);
                            pieces.push(Piece::Case(case));
                        }
                        (None, 'n') => buf.push('\n'),
                        (None, 't') => buf.push('\t'),
                        (None, '\\') => buf.push('\\'),
                        (None, other) => {
                            buf.push('\\');
                            buf.push(other);
                        }
                    }
                    i += 2;
                }
                _ => {
                    buf.push(c);
                    i += 1;
                }
            }
        }
        flush(&mut buf, &mut pieces);
        Replacement { pieces }
    }

    /// Whether expansion reads capture groups.
    pub fn uses_groups(&self) -> bool {
        self.pieces.iter().any(|p| matches!(p, Piece::Group(_)))
    }

    /// Most bytes [`Replacement::expand`] can produce, with `group_len`
    /// giving a group's length (missing groups are empty).
    pub fn bound(&self, group_len: &mut dyn FnMut(&Group) -> Option<u64>) -> u64 {
        let mut total = 0u64;
        let mut cased = false;
        for piece in &self.pieces {
            let len = match piece {
                Piece::Case(_) => {
                    cased = true;
                    continue;
                }
                Piece::Text(text) => text.len() as u64,
                Piece::Group(g) => group_len(g).unwrap_or(0),
            };
            let growth = if cased { CASE_GROWTH } else { 1 };
            total = total.saturating_add(len.saturating_mul(growth));
            cased = false;
        }
        total
    }

    /// Expands with `group` returning a group's text (missing groups are empty).
    pub fn expand(&self, group: &mut dyn FnMut(&Group) -> Option<Vec<u8>>) -> Vec<u8> {
        let mut out = Vec::new();
        let mut cases: Vec<Case> = Vec::new();
        for piece in &self.pieces {
            let bytes = match piece {
                Piece::Case(case) => {
                    cases.push(*case);
                    continue;
                }
                Piece::Text(text) => text.as_bytes().to_vec(),
                Piece::Group(g) => group(g).unwrap_or_default(),
            };
            if cases.is_empty() {
                out.extend_from_slice(&bytes);
                continue;
            }
            match String::from_utf8(bytes) {
                Ok(mut text) => {
                    for case in cases.drain(..) {
                        text = apply_case(&text, case);
                    }
                    out.extend_from_slice(text.as_bytes());
                }
                Err(err) => {
                    cases.clear();
                    out.extend_from_slice(err.as_bytes());
                }
            }
        }
        out
    }
}

fn apply_case(text: &str, case: Case) -> String {
    let mut chars = text.chars();
    match case {
        Case::Upper => text.to_uppercase(),
        Case::Lower => text.to_lowercase(),
        Case::UpperFirst => match chars.next() {
            Some(c) => c.to_uppercase().chain(chars).collect(),
            None => String::new(),
        },
        Case::LowerFirst => match chars.next() {
            Some(c) => c.to_lowercase().chain(chars).collect(),
            None => String::new(),
        },
    }
}

/// Adapts `replacement` to the case of `matched` (VS Code Preserve Case):
/// all upper, all lower, or the first letter; hyphen- and underscore-joined
/// words are adapted part by part when both have the same number of parts.
pub fn preserve_case(matched: &str, replacement: &str) -> String {
    if matched.is_empty() || replacement.is_empty() {
        return replacement.to_owned();
    }
    for sep in ['-', '_'] {
        if matched.contains(sep) && replacement.contains(sep) {
            let m: Vec<&str> = matched.split(sep).collect();
            let r: Vec<&str> = replacement.split(sep).collect();
            if m.len() == r.len() {
                let parts: Vec<String> =
                    m.iter().zip(&r).map(|(m, r)| preserve_case(m, r)).collect();
                return parts.join(&sep.to_string());
            }
        }
    }
    let first = matched.chars().next().unwrap_or_default();
    if matched.to_uppercase() == matched {
        replacement.to_uppercase()
    } else if matched.to_lowercase() == matched {
        replacement.to_lowercase()
    } else if first.is_uppercase() {
        apply_case(replacement, Case::UpperFirst)
    } else if first.is_lowercase() {
        apply_case(replacement, Case::LowerFirst)
    } else {
        replacement.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expand(template: &str, groups: &[&str]) -> String {
        let names = ["first", "second"];
        let rep = Replacement::parse(template);
        let out = rep.expand(&mut |g| {
            let i = match g {
                Group::Index(i) => *i,
                Group::Name(name) => names.iter().position(|n| n == name)? + 1,
            };
            groups.get(i).map(|s| s.as_bytes().to_vec())
        });
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn groups_and_escapes() {
        let g = ["foo_cfg", "foo", "cfg"];
        assert_eq!(expand("$1_config", &g), "foo_config");
        assert_eq!(expand("[$&]", &g), "[foo_cfg]");
        assert_eq!(expand("$0", &g), "foo_cfg");
        assert_eq!(expand("$<second>-$<first>", &g), "cfg-foo");
        assert_eq!(expand("$$1", &g), "$1");
        assert_eq!(expand(r"a\nb\tc\\d\q", &g), "a\nb\tc\\d\\q");
        assert_eq!(expand("$9", &g), "");
        assert_eq!(expand("$<oops", &g), "$<oops");
    }

    #[test]
    fn case_operators_apply_to_the_next_piece() {
        let g = ["hello world", "hello", "world"];
        assert_eq!(expand(r"\u$1 $2", &g), "Hello world");
        assert_eq!(expand(r"\U$1-$2", &g), "HELLO-world");
        assert_eq!(expand(r"\L\u$0", &["ABC def"]), "Abc def");
        assert_eq!(expand(r"\lXyz", &g), "xyz");
    }

    #[test]
    fn literal_is_inserted_as_typed() {
        let rep = Replacement::literal(r"$1\n");
        assert!(!rep.uses_groups());
        assert_eq!(rep.expand(&mut |_| None), br"$1\n");
    }

    #[test]
    fn preserve_case_follows_the_match() {
        assert_eq!(preserve_case("FOO", "bar"), "BAR");
        assert_eq!(preserve_case("foo", "BAR"), "bar");
        assert_eq!(preserve_case("Foo", "bar"), "Bar");
        assert_eq!(preserve_case("fOO", "Bar"), "bar");
        assert_eq!(preserve_case("Foo-Bar", "abc-def"), "Abc-Def");
        assert_eq!(preserve_case("foo_BAR", "abc_def"), "abc_DEF");
        assert_eq!(preserve_case("123", "abc"), "ABC");
    }
}
