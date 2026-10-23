//! Encoding detection, streaming UTF-16 conversion and line-ending detection.

use std::fmt;

/// File encoding, identified by its byte-order mark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// No BOM. Bytes are stored as they are, valid UTF-8 or not.
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
}

impl Encoding {
    /// Detects the encoding from the first bytes of a file.
    pub fn detect(prefix: &[u8]) -> Encoding {
        if prefix.starts_with(&[0xEF, 0xBB, 0xBF]) {
            Encoding::Utf8Bom
        } else if prefix.starts_with(&[0xFF, 0xFE]) {
            Encoding::Utf16Le
        } else if prefix.starts_with(&[0xFE, 0xFF]) {
            Encoding::Utf16Be
        } else {
            Encoding::Utf8
        }
    }

    pub fn bom(self) -> &'static [u8] {
        match self {
            Encoding::Utf8 => &[],
            Encoding::Utf8Bom => &[0xEF, 0xBB, 0xBF],
            Encoding::Utf16Le => &[0xFF, 0xFE],
            Encoding::Utf16Be => &[0xFE, 0xFF],
        }
    }
}

const REPLACEMENT: [u8; 3] = [0xEF, 0xBF, 0xBD];

/// Streaming UTF-16 → UTF-8 decoder (input excludes the BOM).
///
/// Unpaired surrogates and an odd trailing byte become U+FFFD and mark the
/// input as malformed; such documents must not be saved back.
#[derive(Debug)]
pub struct Utf16Decoder {
    little_endian: bool,
    odd_byte: Option<u8>,
    high_surrogate: Option<u16>,
    malformed: bool,
}

impl Utf16Decoder {
    pub fn new(little_endian: bool) -> Utf16Decoder {
        Utf16Decoder {
            little_endian,
            odd_byte: None,
            high_surrogate: None,
            malformed: false,
        }
    }

    pub fn is_malformed(&self) -> bool {
        self.malformed
    }

    /// Decodes the next part of the input, appending UTF-8 to `out`.
    pub fn decode(&mut self, mut input: &[u8], out: &mut Vec<u8>) {
        out.reserve(input.len() * 3 / 2);
        if let Some(first) = self.odd_byte.take() {
            match input.split_first() {
                Some((&second, rest)) => {
                    self.unit(self.combine(first, second), out);
                    input = rest;
                }
                None => {
                    self.odd_byte = Some(first);
                    return;
                }
            }
        }
        let (pairs, rest) = input.as_chunks::<2>();
        for &[a, b] in pairs {
            self.unit(self.combine(a, b), out);
        }
        if let [last] = rest {
            self.odd_byte = Some(*last);
        }
    }

    /// Ends the input; returns `true` when the whole input was well-formed.
    pub fn finish(mut self, out: &mut Vec<u8>) -> bool {
        // A pending high surrogate precedes a pending odd byte; each is its own error.
        if self.high_surrogate.take().is_some() {
            self.malformed_unit(out);
        }
        if self.odd_byte.take().is_some() {
            self.malformed_unit(out);
        }
        !self.malformed
    }

    fn combine(&self, a: u8, b: u8) -> u16 {
        if self.little_endian {
            u16::from_le_bytes([a, b])
        } else {
            u16::from_be_bytes([a, b])
        }
    }

    fn unit(&mut self, unit: u16, out: &mut Vec<u8>) {
        match unit {
            0xD800..=0xDBFF => {
                if self.high_surrogate.replace(unit).is_some() {
                    self.malformed_unit(out);
                }
            }
            0xDC00..=0xDFFF => match self.high_surrogate.take() {
                Some(high) => {
                    let c =
                        0x10000 + ((u32::from(high) - 0xD800) << 10) + (u32::from(unit) - 0xDC00);
                    push_char(char::from_u32(c).expect("valid surrogate pair"), out);
                }
                None => self.malformed_unit(out),
            },
            _ => {
                if self.high_surrogate.take().is_some() {
                    self.malformed_unit(out);
                }
                push_char(
                    char::from_u32(u32::from(unit)).expect("non-surrogate unit"),
                    out,
                );
            }
        }
    }

    fn malformed_unit(&mut self, out: &mut Vec<u8>) {
        out.extend_from_slice(&REPLACEMENT);
        self.malformed = true;
    }
}

fn push_char(c: char, out: &mut Vec<u8>) {
    let mut buf = [0; 4];
    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
}

/// Text that cannot be encoded as UTF-16 because it is not valid UTF-8.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidUtf8;

impl fmt::Display for InvalidUtf8 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("text is not valid UTF-8")
    }
}

impl std::error::Error for InvalidUtf8 {}

/// Streaming UTF-8 → UTF-16 encoder (output excludes the BOM). Chunks may
/// split a UTF-8 sequence.
#[derive(Debug)]
pub struct Utf16Encoder {
    little_endian: bool,
    pending: Vec<u8>,
}

impl Utf16Encoder {
    pub fn new(little_endian: bool) -> Utf16Encoder {
        Utf16Encoder {
            little_endian,
            pending: Vec::new(),
        }
    }

    pub fn encode(&mut self, chunk: &[u8], out: &mut Vec<u8>) -> Result<(), InvalidUtf8> {
        let joined;
        let input = if self.pending.is_empty() {
            chunk
        } else {
            let mut bytes = std::mem::take(&mut self.pending);
            bytes.extend_from_slice(chunk);
            joined = bytes;
            &joined[..]
        };
        let (text, tail) = match std::str::from_utf8(input) {
            Ok(text) => (text, &[][..]),
            Err(e) if e.error_len().is_none() => {
                let (valid, tail) = input.split_at(e.valid_up_to());
                (std::str::from_utf8(valid).expect("validated prefix"), tail)
            }
            Err(_) => return Err(InvalidUtf8),
        };
        out.reserve(text.len() * 2);
        for unit in text.encode_utf16() {
            let bytes = if self.little_endian {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            };
            out.extend_from_slice(&bytes);
        }
        self.pending.extend_from_slice(tail);
        Ok(())
    }

    pub fn finish(self) -> Result<(), InvalidUtf8> {
        if self.pending.is_empty() {
            Ok(())
        } else {
            Err(InvalidUtf8)
        }
    }
}

/// Line-ending style used for newly inserted line breaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    CrLf,
}

impl LineEnding {
    /// Dominant style in `sample`; CRLF (the Windows default) when there is
    /// no line break or a tie.
    pub fn detect(sample: &[u8]) -> LineEnding {
        let mut crlf = 0u64;
        let mut lf = 0u64;
        for i in memchr::memchr_iter(b'\n', sample) {
            if i > 0 && sample[i - 1] == b'\r' {
                crlf += 1;
            } else {
                lf += 1;
            }
        }
        if lf > crlf {
            LineEnding::Lf
        } else {
            LineEnding::CrLf
        }
    }

    pub fn as_bytes(self) -> &'static [u8] {
        match self {
            LineEnding::Lf => b"\n",
            LineEnding::CrLf => b"\r\n",
        }
    }
}
