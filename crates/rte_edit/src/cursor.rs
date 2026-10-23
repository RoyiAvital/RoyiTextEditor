//! Byte and character reads over a snapshot without allocation: the cursor
//! keeps the current leaf slice and seeks the tree only when leaving it.

use rte_text::Snapshot;

pub(crate) struct Bytes<'a> {
    snapshot: &'a Snapshot,
    start: u64,
    chunk: &'a [u8],
}

impl<'a> Bytes<'a> {
    pub(crate) fn new(snapshot: &'a Snapshot) -> Bytes<'a> {
        Bytes {
            snapshot,
            start: 0,
            chunk: &[],
        }
    }

    pub(crate) fn get(&mut self, pos: u64) -> Option<u8> {
        if pos < self.start || pos >= self.start + self.chunk.len() as u64 {
            let (start, chunk) = self.snapshot.chunk_at(pos)?;
            self.start = start;
            self.chunk = chunk;
        }
        Some(self.chunk[(pos - self.start) as usize])
    }

    /// Character starting at `pos`: `Some(char)`, or `None` for an invalid
    /// byte (length 1), with its length.
    pub(crate) fn char_at(&mut self, pos: u64) -> Option<(Option<char>, u64)> {
        let lead = self.get(pos)?;
        let len = match lead {
            0x00..=0x7F => return Some((Some(char::from(lead)), 1)),
            0xC2..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF4 => 4,
            _ => return Some((None, 1)),
        };
        let mut buf = [lead, 0, 0, 0];
        for (i, slot) in buf.iter_mut().enumerate().take(len).skip(1) {
            match self.get(pos + i as u64) {
                Some(b) => *slot = b,
                None => return Some((None, 1)),
            }
        }
        Some(match std::str::from_utf8(&buf[..len]) {
            Ok(text) => (text.chars().next(), len as u64),
            Err(_) => (None, 1),
        })
    }

    /// Character ending at `pos`, with its length.
    pub(crate) fn char_before(&mut self, pos: u64) -> Option<(Option<char>, u64)> {
        if pos == 0 {
            return None;
        }
        for k in 1..=pos.min(4) {
            let b = self.get(pos - k)?;
            if b & 0xC0 == 0x80 {
                continue;
            }
            return Some(match self.char_at(pos - k) {
                Some((Some(ch), len)) if len == k => (Some(ch), len),
                _ => (None, 1),
            });
        }
        Some((None, 1))
    }
}
