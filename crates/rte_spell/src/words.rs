//! Words of prose: what is checked and what is skipped.

/// Longer words are not checked.
pub const MAX_WORD_BYTES: usize = 64;

/// A word to check: its byte range in the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Word {
    pub start: u64,
    pub end: u64,
}

fn is_apostrophe(c: char) -> bool {
    c == '\'' || c == '\u{2019}'
}

/// Appends the words of `bytes` (text starting at offset `base`) to `out`.
///
/// Tokens are separated by whitespace; a token with `://`, `@`, `/` or `\`
/// (URLs, e-mail addresses, paths) is skipped whole. Other tokens split at
/// characters that are not letters, digits, `_` or apostrophes; parts with
/// a digit or `_` are skipped. A word loses its outer apostrophes and a
/// trailing `'s`; all-capital words (acronyms) and words longer than
/// [`MAX_WORD_BYTES`] are skipped. Invalid UTF-8 separates tokens.
pub fn words(bytes: &[u8], base: u64, out: &mut Vec<Word>) {
    let mut token: Option<usize> = None;
    let mut offset = 0;
    for chunk in bytes.utf8_chunks() {
        for (i, c) in chunk.valid().char_indices() {
            let at = offset + i;
            if c.is_whitespace() {
                if let Some(start) = token.take() {
                    token_words(bytes, start, at, base, out);
                }
            } else if token.is_none() {
                token = Some(at);
            }
        }
        offset += chunk.valid().len();
        if !chunk.invalid().is_empty() {
            if let Some(start) = token.take() {
                token_words(bytes, start, offset, base, out);
            }
            offset += chunk.invalid().len();
        }
    }
    if let Some(start) = token {
        token_words(bytes, start, offset, base, out);
    }
}

fn token_words(bytes: &[u8], start: usize, end: usize, base: u64, out: &mut Vec<Word>) {
    let Ok(token) = std::str::from_utf8(&bytes[start..end]) else {
        return;
    };
    if token.contains("://") || token.contains(['@', '/', '\\']) {
        return;
    }
    let mut part: Option<usize> = None;
    for (i, c) in token.char_indices() {
        let inside = c.is_alphabetic() || c.is_ascii_digit() || c == '_' || is_apostrophe(c);
        if inside {
            part.get_or_insert(i);
        } else if let Some(s) = part.take() {
            part_word(token, s, i, start, base, out);
        }
    }
    if let Some(s) = part {
        part_word(token, s, token.len(), start, base, out);
    }
}

fn part_word(token: &str, start: usize, end: usize, at: usize, base: u64, out: &mut Vec<Word>) {
    let part = &token[start..end];
    if part.contains(|c: char| c.is_ascii_digit() || c == '_') {
        return;
    }
    let trimmed = part.trim_start_matches(is_apostrophe);
    let start = start + (part.len() - trimmed.len());
    let mut word = trimmed.trim_end_matches(is_apostrophe);
    for suffix in ["'s", "\u{2019}s"] {
        if let Some(stem) = word.strip_suffix(suffix) {
            word = stem.trim_end_matches(is_apostrophe);
        }
    }
    if word.is_empty() || word.len() > MAX_WORD_BYTES {
        return;
    }
    let letters = word.chars().filter(|c| c.is_alphabetic()).count();
    if letters >= 2 && !word.chars().any(char::is_lowercase) {
        return;
    }
    let s = (at + start) as u64;
    out.push(Word {
        start: base + s,
        end: base + s + word.len() as u64,
    });
}

/// `word` with typographic apostrophes as `'` (dictionaries use ASCII).
pub fn normalized(word: &str) -> std::borrow::Cow<'_, str> {
    if word.contains('\u{2019}') {
        word.replace('\u{2019}', "'").into()
    } else {
        word.into()
    }
}
