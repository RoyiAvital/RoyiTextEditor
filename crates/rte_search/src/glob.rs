//! Glob patterns translated to the automata regex syntax.

/// Translates a glob: `?` one character, `*` any text within a line
/// (shortest; a trailing `*` runs to the line end), `[abc]` / `[!abc]` sets,
/// `\` escapes the next character. Everything else is literal.
pub(crate) fn to_regex(glob: &str) -> String {
    let chars: Vec<char> = glob.chars().collect();
    let mut out = String::with_capacity(glob.len() * 2);
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' => {
                while i + 1 < chars.len() && chars[i + 1] == '*' {
                    i += 1;
                }
                out.push_str(if i + 1 == chars.len() {
                    r"[^\r\n]*"
                } else {
                    r"[^\r\n]*?"
                });
            }
            '?' => out.push_str(r"[^\r\n]"),
            '\\' if i + 1 < chars.len() => {
                i += 1;
                push_literal(&mut out, chars[i]);
            }
            '[' => match class_end(&chars, i) {
                Some(end) => {
                    push_class(&mut out, &chars[i + 1..end]);
                    i = end;
                }
                None => push_literal(&mut out, '['),
            },
            c => push_literal(&mut out, c),
        }
        i += 1;
    }
    out
}

/// Index of the `]` closing the set opened at `open`; a `]` right after
/// `[` or `[!` is a member.
fn class_end(chars: &[char], open: usize) -> Option<usize> {
    let mut j = open + 1;
    if chars.get(j) == Some(&'!') || chars.get(j) == Some(&'^') {
        j += 1;
    }
    if chars.get(j) == Some(&']') {
        j += 1;
    }
    (j..chars.len()).find(|&k| chars[k] == ']')
}

fn push_class(out: &mut String, body: &[char]) {
    let (negated, body) = match body.first() {
        Some('!' | '^') => (true, &body[1..]),
        _ => (false, body),
    };
    out.push('[');
    if negated {
        out.push('^');
    }
    for &c in body {
        if c == '-' {
            out.push('-');
        } else {
            push_escaped(out, c);
        }
    }
    if negated {
        out.push_str(r"\r\n");
    }
    out.push(']');
}

fn push_literal(out: &mut String, c: char) {
    push_escaped(out, c);
}

fn push_escaped(out: &mut String, c: char) {
    if c.is_ascii_punctuation() {
        out.push('\\');
    }
    out.push(c);
}

/// Escapes `text` so the automata engine matches it literally.
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for c in text.chars() {
        match c {
            '\n' => out.push_str(r"\n"),
            '\r' => out.push_str(r"\r"),
            '\t' => out.push_str(r"\t"),
            c => push_escaped(&mut out, c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation_table() {
        let cases = [
            ("abc", "abc"),
            ("a?c", r"a[^\r\n]c"),
            ("foo*bar", r"foo[^\r\n]*?bar"),
            ("TODO*", r"TODO[^\r\n]*"),
            ("a**", r"a[^\r\n]*"),
            ("[abc]", "[abc]"),
            ("[!a-z]x", r"[^a-z\r\n]x"),
            ("[]a]", r"[\]a]"),
            ("[x", r"\[x"),
            (r"\*.", r"\*\."),
            ("a.b(c)", r"a\.b\(c\)"),
        ];
        for (glob, regex) in cases {
            assert_eq!(to_regex(glob), regex, "glob {glob}");
        }
    }
}
