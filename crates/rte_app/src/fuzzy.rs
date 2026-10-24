//! Fuzzy matching of palette labels: the query characters must appear in
//! order (case-insensitive, query whitespace ignored). Matches at word starts
//! and runs of consecutive characters score higher.

const MATCH: i32 = 1;
const WORD_START: i32 = 8;
const CONSECUTIVE: i32 = 8;
const FIRST_CHAR: i32 = 4;

fn fold(ch: char) -> char {
    ch.to_lowercase().next().unwrap_or(ch)
}

fn is_word_start(text: &[char], i: usize) -> bool {
    i == 0 || {
        let prev = text[i - 1];
        !prev.is_alphanumeric() || (prev.is_lowercase() && text[i].is_uppercase())
    }
}

/// Scores labels against one query, reusing its buffers between calls.
#[derive(Default)]
pub struct Matcher {
    query: Vec<char>,
    text: Vec<char>,
    folded: Vec<char>,
    /// `best[i * m + j]`: best score with query[i] matched at text[j].
    best: Vec<i32>,
    /// Text index of query[i - 1] for that best score.
    from: Vec<usize>,
}

impl Matcher {
    pub fn set_query(&mut self, query: &str) {
        self.query.clear();
        self.query
            .extend(query.chars().filter(|c| !c.is_whitespace()).map(fold));
    }

    /// Best score of the query in `label`, appending the matched character
    /// indices to `matched`; `None` when it does not match. An empty query
    /// matches with score 0.
    pub fn score(&mut self, label: &str, matched: &mut Vec<u32>) -> Option<i32> {
        let n = self.query.len();
        if n == 0 {
            return Some(0);
        }
        self.text.clear();
        self.text.extend(label.chars());
        self.folded.clear();
        self.folded.extend(self.text.iter().copied().map(fold));
        let m = self.text.len();
        if n > m {
            return None;
        }
        let (query, text, folded) = (&self.query, &self.text, &self.folded);
        self.best.clear();
        self.best.resize(n * m, i32::MIN);
        self.from.clear();
        self.from.resize(n * m, usize::MAX);
        let (best, from) = (&mut self.best, &mut self.from);
        for i in 0..n {
            // Best predecessor ending before j: (score, index).
            let mut prev = (i32::MIN, usize::MAX);
            for j in i..m {
                if i > 0 && j > 0 {
                    let candidate = best[(i - 1) * m + j - 1];
                    if candidate > prev.0 {
                        prev = (candidate, j - 1);
                    }
                }
                if folded[j] != query[i] {
                    continue;
                }
                let mut bonus = MATCH;
                if is_word_start(text, j) {
                    bonus += WORD_START;
                }
                if i == 0 {
                    if j == 0 {
                        bonus += FIRST_CHAR;
                    }
                    best[j] = bonus;
                    continue;
                }
                let mut choice = prev;
                let adjacent = best[(i - 1) * m + j - 1];
                if j > 0 && adjacent != i32::MIN && adjacent + CONSECUTIVE >= choice.0 {
                    choice = (adjacent + CONSECUTIVE, j - 1);
                }
                if choice.0 != i32::MIN {
                    best[i * m + j] = choice.0 + bonus;
                    from[i * m + j] = choice.1;
                }
            }
        }
        let row = &best[(n - 1) * m..];
        let (end, &total) = row
            .iter()
            .enumerate()
            .filter(|&(_, &s)| s != i32::MIN)
            .max_by_key(|&(j, &s)| (s, std::cmp::Reverse(j)))?;
        let start = matched.len();
        matched.resize(start + n, 0);
        let mut j = end;
        for i in (0..n).rev() {
            matched[start + i] = j as u32;
            j = from[i * m + j];
        }
        Some(total)
    }
}

/// [`Matcher::score`] for one query and label.
#[cfg(test)]
pub fn score(query: &str, label: &str) -> Option<(i32, Vec<u32>)> {
    let mut matcher = Matcher::default();
    matcher.set_query(query);
    let mut matched = Vec::new();
    matcher.score(label, &mut matched).map(|s| (s, matched))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn positions(query: &str, label: &str) -> Option<Vec<u32>> {
        score(query, label).map(|(_, p)| p)
    }

    #[test]
    fn characters_must_appear_in_order() {
        assert!(score("save", "File: Save").is_some());
        assert!(score("evas", "File: Save").is_none());
        assert!(score("toolong", "Go").is_none());
        assert_eq!(score("", "File: Save"), Some((0, Vec::new())));
        assert_eq!(score("  ", "File: Save"), Some((0, Vec::new())));
    }

    #[test]
    fn matching_ignores_case_and_query_spaces() {
        assert_eq!(
            positions("FILE SAVE", "File: Save"),
            Some(vec![0, 1, 2, 3, 6, 7, 8, 9])
        );
        assert_eq!(positions("fs", "File: Save"), Some(vec![0, 6]));
    }

    #[test]
    fn word_starts_and_runs_win() {
        // "s" of "Save" (a word start) beats the earlier "s" in "Close".
        assert_eq!(positions("s", "File: Close Save"), Some(vec![12]));
        // Consecutive "zo" in "Zoom" rather than scattered letters.
        assert_eq!(positions("zo", "View: Zoom Out"), Some(vec![6, 7]));
        let initials = score("sac", "View: Show All Commands").unwrap().0;
        let scattered = score("sac", "Edit: Paste Clipboard").map_or(i32::MIN, |s| s.0);
        assert!(initials > scattered);
    }

    #[test]
    fn camel_case_humps_are_word_starts() {
        assert_eq!(positions("rO", "toggleReadOnly"), Some(vec![6, 10]));
    }
}
