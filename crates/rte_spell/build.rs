//! Ensures `assets/dictionaries` holds the pinned en_US Hunspell dictionary
//! (`tools/Get-Dictionaries.ps1`), then expands it into the built-in word
//! list: every stem with its affixes, sorted, one word per line.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let script = root.join("tools").join("Get-Dictionaries.ps1");
    let dir = root.join("assets").join("dictionaries");
    println!("cargo:rerun-if-changed={}", script.display());
    println!("cargo:rerun-if-changed={}", dir.display());
    // As in rte_assets: pwsh 7 hides Windows PowerShell's own modules otherwise.
    let output = Command::new("powershell.exe")
        .env_remove("PSModulePath")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script)
        .output();
    match output {
        Ok(out) if out.status.success() => {}
        Ok(out) => panic!(
            "{} failed:\n{}\n{}",
            script.display(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
        Err(e) => panic!("cannot run powershell.exe for {}: {e}", script.display()),
    }
    let read = |name: &str| {
        std::fs::read_to_string(dir.join(name))
            .unwrap_or_else(|e| panic!("{}: {e}", dir.join(name).display()))
    };
    let (words, nosuggest) = expand(&read("en_US.aff"), &read("en_US.dic"));
    let out = Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR")).to_path_buf();
    let join = |set: &BTreeSet<String>| {
        let mut text = String::new();
        for word in set {
            text.push_str(word);
            text.push('\n');
        }
        text
    };
    std::fs::write(out.join("en_US.words"), join(&words)).expect("write en_US.words");
    std::fs::write(out.join("en_US.nosuggest"), join(&nosuggest)).expect("write nosuggest");
}

struct Rule {
    strip: String,
    add: String,
    condition: Vec<Class>,
}

/// One position of an affix condition.
enum Class {
    Any,
    Set(Vec<char>, bool),
}

impl Class {
    fn matches(&self, c: char) -> bool {
        match self {
            Class::Any => true,
            Class::Set(chars, negated) => chars.contains(&c) != *negated,
        }
    }
}

struct Affix {
    cross: bool,
    rules: Vec<Rule>,
}

fn condition(text: &str) -> Vec<Class> {
    let mut out = Vec::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '.' => out.push(Class::Any),
            '[' => {
                let mut set = Vec::new();
                let mut negated = false;
                for c in chars.by_ref() {
                    match c {
                        ']' => break,
                        '^' if set.is_empty() && !negated => negated = true,
                        c => set.push(c),
                    }
                }
                out.push(Class::Set(set, negated));
            }
            c => out.push(Class::Set(vec![c], false)),
        }
    }
    out
}

/// Words of the dictionary (stems and affixed forms) and those never to
/// suggest. Compound-only stems and words with digits are left out.
fn expand(aff: &str, dic: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut prefixes = std::collections::HashMap::new();
    let mut suffixes = std::collections::HashMap::new();
    let mut only_compound = None;
    let mut no_suggest = None;
    for line in aff.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        match fields.as_slice() {
            ["ONLYINCOMPOUND", flag, ..] => only_compound = flag.chars().next(),
            ["NOSUGGEST", flag, ..] => no_suggest = flag.chars().next(),
            [kind @ ("PFX" | "SFX"), flag, cross, count] if count.parse::<usize>().is_ok() => {
                let map = if *kind == "PFX" {
                    &mut prefixes
                } else {
                    &mut suffixes
                };
                let flag = flag.chars().next().expect("flag");
                map.insert(
                    flag,
                    Affix {
                        cross: *cross == "Y",
                        rules: Vec::new(),
                    },
                );
            }
            [kind @ ("PFX" | "SFX"), flag, strip, add, cond, ..] => {
                let map = if *kind == "PFX" {
                    &mut prefixes
                } else {
                    &mut suffixes
                };
                let flag = flag.chars().next().expect("flag");
                let field = |s: &str| {
                    if s == "0" {
                        String::new()
                    } else {
                        s.to_owned()
                    }
                };
                if let Some(affix) = map.get_mut(&flag) {
                    affix.rules.push(Rule {
                        strip: field(strip),
                        add: field(add),
                        condition: condition(cond),
                    });
                }
            }
            _ => {}
        }
    }
    let mut words = BTreeSet::new();
    let mut nosuggest = BTreeSet::new();
    for line in dic.lines().skip(1) {
        let (stem, flags) = line.split_once('/').unwrap_or((line, ""));
        let stem = stem.trim();
        if stem.is_empty() || stem.chars().any(|c| c.is_ascii_digit()) {
            continue;
        }
        let flags: Vec<char> = flags.trim().chars().collect();
        let mut forms = Vec::new();
        if !only_compound.is_some_and(|f| flags.contains(&f)) {
            forms.push(stem.to_owned());
        }
        let cross_prefixes: Vec<&Affix> = flags
            .iter()
            .filter_map(|f| prefixes.get(f))
            .filter(|a| a.cross)
            .collect();
        for affix in flags.iter().filter_map(|f| suffixes.get(f)) {
            for rule in &affix.rules {
                let Some(form) = apply_suffix(stem, rule) else {
                    continue;
                };
                if affix.cross {
                    for prefix in &cross_prefixes {
                        for p in &prefix.rules {
                            if let Some(both) = apply_prefix(&form, p) {
                                forms.push(both);
                            }
                        }
                    }
                }
                forms.push(form);
            }
        }
        for affix in flags.iter().filter_map(|f| prefixes.get(f)) {
            for rule in &affix.rules {
                if let Some(form) = apply_prefix(stem, rule) {
                    forms.push(form);
                }
            }
        }
        let hidden = no_suggest.is_some_and(|f| flags.contains(&f));
        for form in forms {
            if hidden {
                nosuggest.insert(form.clone());
            }
            words.insert(form);
        }
    }
    (words, nosuggest)
}

fn apply_suffix(stem: &str, rule: &Rule) -> Option<String> {
    let chars: Vec<char> = stem.chars().collect();
    let n = rule.condition.len();
    if chars.len() < n || !stem.ends_with(&rule.strip) {
        return None;
    }
    let tail = &chars[chars.len() - n..];
    if !rule
        .condition
        .iter()
        .zip(tail)
        .all(|(c, ch)| c.matches(*ch))
    {
        return None;
    }
    Some(format!(
        "{}{}",
        &stem[..stem.len() - rule.strip.len()],
        rule.add
    ))
}

fn apply_prefix(stem: &str, rule: &Rule) -> Option<String> {
    let chars: Vec<char> = stem.chars().collect();
    let n = rule.condition.len();
    if chars.len() < n || !stem.starts_with(&rule.strip) {
        return None;
    }
    if !rule
        .condition
        .iter()
        .zip(&chars)
        .all(|(c, ch)| c.matches(*ch))
    {
        return None;
    }
    Some(format!("{}{}", rule.add, &stem[rule.strip.len()..]))
}
