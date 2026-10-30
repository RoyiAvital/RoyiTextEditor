//! The Hunspell engine: a `.aff` / `.dic` pair read with `spellbook`.

use std::path::Path;
use std::time::Instant;

use crate::Speller;

/// Largest `.aff` and `.dic` together.
pub const MAX_HUNSPELL_BYTES: u64 = 16 * 1024 * 1024;

pub struct Hunspell {
    dict: spellbook::Dictionary,
    out: Vec<String>,
}

impl Hunspell {
    /// Reads `<base>.aff` and `<base>.dic` and builds the dictionary (tens
    /// of milliseconds: call it off the UI thread).
    pub fn load(base: &Path) -> Result<Hunspell, String> {
        let path = |ext: &str| {
            let mut p = base.as_os_str().to_owned();
            p.push(ext);
            std::path::PathBuf::from(p)
        };
        let (aff, dic) = (path(".aff"), path(".dic"));
        let size = |p: &Path| {
            std::fs::metadata(p)
                .map(|m| m.len())
                .map_err(|e| format!("{}: {e}", p.display()))
        };
        if size(&aff)? + size(&dic)? > MAX_HUNSPELL_BYTES {
            return Err(format!("{} is larger than 16 MiB", base.display()));
        }
        let read =
            |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
        Hunspell::new(&read(&aff)?, &read(&dic)?)
    }

    pub fn new(aff: &str, dic: &str) -> Result<Hunspell, String> {
        let dict =
            spellbook::Dictionary::new(aff, dic).map_err(|e| format!("invalid dictionary: {e}"))?;
        Ok(Hunspell {
            dict,
            out: Vec::new(),
        })
    }
}

impl Speller for Hunspell {
    fn name(&self) -> &'static str {
        "hunspell"
    }

    fn check(&mut self, word: &str) -> Result<bool, String> {
        Ok(self.dict.check(word))
    }

    fn suggest(
        &mut self,
        word: &str,
        max: usize,
        _deadline: Instant,
    ) -> Result<Vec<String>, String> {
        // One call: spellbook takes no deadline.
        self.dict.suggest(word, &mut self.out);
        Ok(self.out.drain(..).take(max).collect())
    }
}
