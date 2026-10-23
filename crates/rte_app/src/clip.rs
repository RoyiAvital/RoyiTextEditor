//! Private clipboard format: how the plain text splits into the copied
//! pieces, and whether they are whole lines.
//!
//! Layout: `RTE1`, line mode (u8), separator length (u8), piece count (u32
//! LE), then each piece's UTF-8 length (u32 LE). Trailing bytes are ignored
//! (clipboard blocks may be rounded up).

use std::borrow::Cow;

use rte_edit::Clip;
use rte_text::LineEnding;

pub const FORMAT: &str = "RoyiTextEditor.Selections";
const MAGIC: &[u8; 4] = b"RTE1";

/// Plain text and private data for `clip`. Invalid UTF-8 becomes U+FFFD;
/// valid pieces are borrowed and copied once, into the result.
pub fn encode(clip: &Clip, line_ending: LineEnding) -> (String, Vec<u8>) {
    let pieces: Vec<Cow<'_, str>> = clip
        .pieces
        .iter()
        .map(|p| String::from_utf8_lossy(p))
        .collect();
    let separator = if clip.line_mode {
        ""
    } else {
        std::str::from_utf8(line_ending.as_bytes()).expect("ASCII")
    };
    let len = pieces.iter().map(|p| p.len()).sum::<usize>()
        + separator.len() * pieces.len().saturating_sub(1);
    let mut text = String::with_capacity(len);
    let mut data = MAGIC.to_vec();
    data.push(u8::from(clip.line_mode));
    data.push(separator.len() as u8);
    data.extend_from_slice(&(pieces.len() as u32).to_le_bytes());
    for (i, piece) in pieces.iter().enumerate() {
        if i > 0 {
            text.push_str(separator);
        }
        text.push_str(piece);
        data.extend_from_slice(&(piece.len() as u32).to_le_bytes());
    }
    (text, data)
}

/// Pieces of `text` described by `data`; plain text when `data` is missing
/// or does not describe `text`. A single piece takes the text's buffer.
pub fn decode(text: String, data: Option<&[u8]>) -> Clip {
    match data.and_then(|data| split(&text, data)) {
        Some(Split::Whole { line_mode }) => Clip {
            pieces: vec![text.into_bytes()],
            line_mode,
        },
        Some(Split::Pieces(clip)) => clip,
        None => Clip {
            pieces: vec![text.into_bytes()],
            line_mode: false,
        },
    }
}

enum Split {
    /// One piece covering the whole text.
    Whole {
        line_mode: bool,
    },
    Pieces(Clip),
}

fn split(text: &str, data: &[u8]) -> Option<Split> {
    let u32_at = |at: usize| {
        data.get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    };
    if data.get(..4)? != MAGIC {
        return None;
    }
    let line_mode = *data.get(4)? != 0;
    let separator = *data.get(5)? as usize;
    let count = u32_at(6)?;
    if count == 1 && u32_at(10)? == text.len() {
        return Some(Split::Whole { line_mode });
    }
    let mut pieces = Vec::with_capacity(count.min(1 << 16));
    let mut at = 0usize;
    for i in 0..count {
        if i > 0 {
            at = at.checked_add(separator)?;
        }
        let len = u32_at(10 + 4 * i)?;
        let end = at.checked_add(len)?;
        pieces.push(text.get(at..end)?.as_bytes().to_vec());
        at = end;
    }
    (at == text.len() && count > 0).then_some(Split::Pieces(Clip { pieces, line_mode }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_rejection() {
        let clip = Clip {
            pieces: vec![b"ab".to_vec(), "\u{4E2D}".as_bytes().to_vec(), Vec::new()],
            line_mode: false,
        };
        let (text, mut data) = encode(&clip, LineEnding::CrLf);
        assert_eq!(text, "ab\r\n\u{4E2D}\r\n");
        data.extend_from_slice(&[0; 7]);
        assert_eq!(decode(text, Some(&data)), clip);
        let lines = Clip {
            pieces: vec![b"x\n".to_vec(), b"y\n".to_vec()],
            line_mode: true,
        };
        let (text, data) = encode(&lines, LineEnding::Lf);
        assert_eq!(decode(text, Some(&data)), lines);
        let line = Clip {
            pieces: vec![b"one line\n".to_vec()],
            line_mode: true,
        };
        let (text, one) = encode(&line, LineEnding::Lf);
        assert_eq!(decode(text, Some(&one)), line);
        // Text replaced by another program, or no private data: plain text.
        let plain = decode("other".to_owned(), Some(&data));
        assert_eq!(plain.pieces, vec![b"other".to_vec()]);
        assert!(!plain.line_mode);
        assert!(!decode("other".to_owned(), Some(&one)).line_mode);
        assert_eq!(decode("t".to_owned(), None).pieces, vec![b"t".to_vec()]);
        let truncated = decode("t".to_owned(), Some(b"RTE1\x01"));
        assert_eq!(truncated.pieces, vec![b"t".to_vec()]);
    }
}
