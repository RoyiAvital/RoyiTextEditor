//! Snapshot reads for the regex engines: a chunk cursor that never splits a
//! UTF-8 character, and small byte/character reads.

use regex_cursor::Cursor;
use rte_text::Snapshot;

/// Byte reads that keep the current leaf and seek only when leaving it.
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

    /// Character at `pos` (`None` for an invalid byte) and its length.
    pub(crate) fn char_at(&mut self, pos: u64) -> Option<(Option<char>, u64)> {
        let lead = self.get(pos)?;
        let len = utf8_len(lead);
        if len == 1 {
            return Some(((lead < 0x80).then(|| char::from(lead)), 1));
        }
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

    /// Character ending at `pos` and its length.
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

/// Length of the UTF-8 sequence a lead byte starts (1 for invalid leads).
fn utf8_len(lead: u8) -> usize {
    match lead {
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => 1,
    }
}

/// The valid multi-byte character that contains the leaf boundary `at`
/// (starts before it and ends after it), as `(start, len)`.
fn straddling(b: &mut Bytes<'_>, len: u64, at: u64) -> Option<(u64, u64)> {
    if at == 0 || at >= len {
        return None;
    }
    for k in 1..=at.min(3) {
        let byte = b.get(at - k)?;
        if byte & 0xC0 == 0x80 {
            continue;
        }
        let n = utf8_len(byte) as u64;
        let start = at - k;
        if n <= k || start + n > len {
            return None;
        }
        let valid = (start + 1..start + n).all(|p| b.get(p).is_some_and(|c| c & 0xC0 == 0x80));
        return valid.then_some((start, n));
    }
    None
}

enum Seg<'a> {
    Slice(&'a [u8]),
    Owned([u8; 4], usize),
}

/// A [`Cursor`] over a snapshot whose chunks are leaf slices, except that a
/// character split across leaves is returned as its own small chunk. Chunk
/// boundaries depend only on the text, so advancing and backtracking agree.
/// `stop` is polled on every advance; when it returns true the cursor
/// reports the end of the data.
pub(crate) struct DocCursor<'a> {
    snapshot: &'a Snapshot,
    bytes: Bytes<'a>,
    len: u64,
    start: u64,
    end: u64,
    seg: Seg<'a>,
    stop: &'a dyn Fn() -> bool,
    stopped: bool,
}

impl<'a> DocCursor<'a> {
    /// Cursor on the chunk containing `pos` (the last chunk at the end).
    pub(crate) fn at(snapshot: &'a Snapshot, pos: u64, stop: &'a dyn Fn() -> bool) -> Self {
        let mut c = DocCursor {
            snapshot,
            bytes: Bytes::new(snapshot),
            len: snapshot.len(),
            start: 0,
            end: 0,
            seg: Seg::Slice(&[]),
            stop,
            stopped: false,
        };
        if c.len == 0 {
            return c;
        }
        if pos >= c.len {
            c.load_before(c.len);
            return c;
        }
        let (leaf_start, _) = snapshot.chunk_at(pos).expect("pos is inside");
        let mut start = leaf_start;
        if let Some((cs, n)) = straddling(&mut c.bytes, c.len, leaf_start) {
            start = if pos < cs + n { cs } else { cs + n };
        }
        c.load_from(start);
        while c.end <= pos && c.end < c.len {
            c.load_from(c.end);
        }
        c
    }

    pub(crate) fn stopped(&self) -> bool {
        self.stopped
    }

    fn set(&mut self, start: u64, end: u64) {
        self.start = start;
        self.end = end;
        let (leaf_start, leaf) = self.snapshot.chunk_at(start).expect("start is inside");
        let leaf_end = leaf_start + leaf.len() as u64;
        self.seg = if end <= leaf_end {
            Seg::Slice(&leaf[(start - leaf_start) as usize..(end - leaf_start) as usize])
        } else {
            let mut buf = [0; 4];
            let n = (end - start) as usize;
            for (i, slot) in buf.iter_mut().enumerate().take(n) {
                *slot = self.bytes.get(start + i as u64).unwrap_or(0);
            }
            Seg::Owned(buf, n)
        };
    }

    /// Loads the chunk starting at boundary `start`.
    fn load_from(&mut self, start: u64) {
        let (leaf_start, leaf) = self.snapshot.chunk_at(start).expect("start is inside");
        let leaf_end = leaf_start + leaf.len() as u64;
        let end = match straddling(&mut self.bytes, self.len, leaf_end) {
            Some((cs, n)) if cs <= start => cs + n,
            Some((cs, _)) => cs,
            None => leaf_end,
        };
        self.set(start, end);
    }

    /// Loads the chunk ending at boundary `end`.
    fn load_before(&mut self, end: u64) {
        let (leaf_start, _) = self.snapshot.chunk_at(end - 1).expect("end is inside");
        let mut start = leaf_start;
        if let Some((_, ch_len)) = self.bytes.char_before(end)
            && ch_len > 1
            && end - ch_len < leaf_start
        {
            start = end - ch_len;
        } else if let Some((cs, n)) = straddling(&mut self.bytes, self.len, leaf_start)
            && cs + n < end
        {
            start = cs + n;
        }
        self.set(start, end);
    }
}

impl Cursor for DocCursor<'_> {
    fn chunk(&self) -> &[u8] {
        match &self.seg {
            Seg::Slice(slice) => slice,
            Seg::Owned(buf, n) => &buf[..*n],
        }
    }

    fn advance(&mut self) -> bool {
        if self.end >= self.len || self.stopped {
            return false;
        }
        if (self.stop)() {
            self.stopped = true;
            return false;
        }
        self.load_from(self.end);
        true
    }

    fn backtrack(&mut self) -> bool {
        if self.start == 0 {
            return false;
        }
        self.load_before(self.start);
        true
    }

    fn total_bytes(&self) -> Option<usize> {
        Some(self.len as usize)
    }

    fn offset(&self) -> usize {
        self.start as usize
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rte_text::TreeBuilder;

    fn never() -> bool {
        false
    }

    /// A snapshot whose leaves split `text` at every byte offset in `cuts`.
    pub(crate) fn split(text: &[u8], cuts: &[usize]) -> Snapshot {
        let mut b = TreeBuilder::new();
        let mut prev = 0;
        let mut bounds = cuts.to_vec();
        bounds.push(text.len());
        for cut in bounds {
            b.push_block(text[prev..cut].to_vec().into());
            prev = cut;
        }
        b.finish()
    }

    fn chunks(c: &mut DocCursor<'_>) -> Vec<(usize, Vec<u8>)> {
        let mut out = vec![(c.offset(), c.chunk().to_vec())];
        while c.advance() {
            out.push((c.offset(), c.chunk().to_vec()));
        }
        out
    }

    #[test]
    fn chunks_cover_the_text_without_splitting_characters() {
        let text = "aé€😀b".repeat(200);
        let s = Snapshot::from_bytes(text.as_bytes());
        let mut c = DocCursor::at(&s, 0, &never);
        let all = chunks(&mut c);
        let joined: Vec<u8> = all.iter().flat_map(|(_, b)| b.clone()).collect();
        assert_eq!(joined, text.as_bytes());
        for (_, chunk) in &all {
            assert!(!chunk.is_empty());
            assert!(
                std::str::from_utf8(chunk).is_ok(),
                "chunk splits a character"
            );
        }
        let mut back = Vec::new();
        loop {
            back.push((c.offset(), c.chunk().to_vec()));
            if !c.backtrack() {
                break;
            }
        }
        back.reverse();
        assert_eq!(back, all);
    }

    #[test]
    fn split_characters_are_stitched_across_tiny_leaves() {
        let text = "x€y";
        let s = split(text.as_bytes(), &[2, 3]);
        let mut c = DocCursor::at(&s, 0, &never);
        let all = chunks(&mut c);
        assert_eq!(
            all,
            vec![
                (0, b"x".to_vec()),
                (1, "€".as_bytes().to_vec()),
                (4, b"y".to_vec())
            ]
        );
        let mid = DocCursor::at(&s, 2, &never);
        assert_eq!(mid.offset(), 1);
        let end = DocCursor::at(&s, 5, &never);
        assert_eq!(end.offset(), 4);
    }

    #[test]
    fn stop_ends_the_data_early() {
        let s = Snapshot::from_bytes(&vec![b'a'; 100_000]);
        let stop = || true;
        let mut c = DocCursor::at(&s, 0, &stop);
        assert!(!c.advance());
        assert!(c.stopped());
    }
}
