//! Suggestions by edit distance over any dictionary lookup.

use std::time::Instant;

const LETTERS: &str = "abcdefghijklmnopqrstuvwxyz'";

/// Strings one edit from `word`: transpositions, replacements, insertions,
/// then deletions. Returns where the replacements, insertions and deletions
/// start in `out`.
fn edits(word: &[char], out: &mut Vec<String>) -> [usize; 3] {
    let n = word.len();
    let s = |v: &[char]| v.iter().collect::<String>();
    for i in 1..n {
        let mut w = word.to_vec();
        w.swap(i - 1, i);
        out.push(s(&w));
    }
    let replacements = out.len();
    for i in 0..n {
        for c in LETTERS.chars() {
            if word[i] != c {
                let mut w = word.to_vec();
                w[i] = c;
                out.push(s(&w));
            }
        }
    }
    let insertions = out.len();
    for i in 0..=n {
        for c in LETTERS.chars() {
            let mut w = word.to_vec();
            w.insert(i, c);
            out.push(s(&w));
        }
    }
    let deletions = out.len();
    for i in 0..n {
        let mut w = word.to_vec();
        w.remove(i);
        out.push(s(&w));
    }
    [replacements, insertions, deletions]
}

/// Up to `max` words `known` accepts at edit distance 1, then 2 until
/// `deadline` (checked between lookups), in the case of `word`.
pub fn candidates(
    word: &str,
    max: usize,
    deadline: Instant,
    known: &mut dyn FnMut(&str) -> bool,
) -> Vec<String> {
    let capital = word.chars().next().is_some_and(char::is_uppercase);
    let lower: Vec<char> = word.to_lowercase().chars().collect();
    let mut found: Vec<String> = Vec::new();
    let mut take = |w: &String, found: &mut Vec<String>| {
        if found.len() < max && !found.contains(w) && known(w) {
            found.push(w.clone());
        }
    };
    let mut first = Vec::new();
    let [replacements, insertions, deletions] = edits(&lower, &mut first);
    // Most likely typos first: swapped, missing and extra letters, then wrong ones.
    for range in [
        0..replacements,
        insertions..deletions,
        deletions..first.len(),
        replacements..insertions,
    ] {
        for w in &first[range] {
            take(w, &mut found);
        }
    }
    let mut second = Vec::new();
    'outer: for w in &first {
        if found.len() >= max || Instant::now() >= deadline {
            break;
        }
        second.clear();
        edits(&w.chars().collect::<Vec<_>>(), &mut second);
        for (i, w2) in second.iter().enumerate() {
            if i % 256 == 0 && Instant::now() >= deadline {
                break 'outer;
            }
            take(w2, &mut found);
        }
    }
    if capital {
        for w in &mut found {
            let mut chars = w.chars();
            if let Some(c) = chars.next() {
                *w = c.to_uppercase().chain(chars).collect();
            }
        }
    }
    found
}
