//! The user dictionary file: one word per line.

/// Largest user dictionary file.
pub const USER_MAX_BYTES: usize = 64 * 1024;
/// Most words in the user dictionary.
pub const USER_MAX_WORDS: usize = 10_000;

/// A word that can be added: no whitespace, at most [`MAX_WORD_BYTES`].
///
/// [`MAX_WORD_BYTES`]: crate::MAX_WORD_BYTES
pub fn valid_word(word: &str) -> bool {
    !word.is_empty() && word.len() <= crate::MAX_WORD_BYTES && !word.contains(char::is_whitespace)
}

/// The words of a user dictionary file, sorted and unique; `None` when it
/// is too large, not UTF-8 or holds an invalid line.
pub fn parse_user(bytes: &[u8]) -> Option<Vec<String>> {
    if bytes.len() > USER_MAX_BYTES {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let mut words = Vec::new();
    for line in text.lines() {
        let word = line.trim();
        if word.is_empty() {
            continue;
        }
        if !valid_word(word) {
            return None;
        }
        words.push(word.to_owned());
    }
    words.sort();
    words.dedup();
    (words.len() <= USER_MAX_WORDS).then_some(words)
}

/// The file text of sorted, unique `words`; `None` above the limits.
pub fn encode_user(words: &[String]) -> Option<Vec<u8>> {
    if words.len() > USER_MAX_WORDS {
        return None;
    }
    let mut out = Vec::new();
    for word in words {
        out.extend_from_slice(word.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    (out.len() <= USER_MAX_BYTES).then_some(out)
}

/// `words` with `added` merged in, sorted and unique.
pub fn merge_user(mut words: Vec<String>, added: &[String]) -> Vec<String> {
    words.extend(added.iter().cloned());
    words.sort();
    words.dedup();
    words
}
