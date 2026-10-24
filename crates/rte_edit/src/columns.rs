//! Display columns of a line, with the view's cell rules.

use unicode_width::UnicodeWidthChar;

/// Bytes of a line scanned exactly for columns; further bytes count one
/// column each (approximation until long-line indexing exists).
pub const COLUMN_SCAN_LIMIT: usize = 64 * 1024;

/// Cells of a character other than tab: controls, DEL and invalid bytes take
/// one cell (drawn as symbols); zero-width marks take none.
pub fn cell_width(ch: char) -> u64 {
    match ch {
        '\0'..='\u{1F}' | '\u{7F}' => 1,
        _ => ch.width().unwrap_or(1) as u64,
    }
}

/// Calls `f(offset, byte_len, cells_before, cells)` for the characters of
/// `line` from the character boundary `from` (offset, column) until it
/// returns `false` or `limit` bytes are passed. Returns the offset and column
/// reached.
fn walk(
    line: &[u8],
    from: (usize, u64),
    limit: usize,
    tab_width: u32,
    mut f: impl FnMut(usize, usize, u64, u64) -> bool,
) -> (usize, u64) {
    let tab = u64::from(tab_width.max(1));
    let (mut offset, mut col) = from;
    let end = line.len().min(limit);
    if offset >= end {
        return (offset, col);
    }
    for chunk in line[offset..end].utf8_chunks() {
        for ch in chunk.valid().chars() {
            let cells = if ch == '\t' {
                (col / tab + 1) * tab - col
            } else {
                cell_width(ch)
            };
            if !f(offset, ch.len_utf8(), col, cells) {
                return (offset, col);
            }
            offset += ch.len_utf8();
            col += cells;
        }
        for _ in chunk.invalid() {
            if !f(offset, 1, col, 1) {
                return (offset, col);
            }
            offset += 1;
            col += 1;
        }
    }
    (offset, col)
}

/// Display column of byte `offset` in `line` (a line's content or a prefix
/// of it; offsets past the scanned bytes count one column per byte).
pub fn column_of(line: &[u8], offset: usize, tab_width: u32) -> u64 {
    column_after(line, (0, 0), offset, tab_width)
}

/// [`column_of`] continued from a known character boundary `from`
/// (offset, column) at or before `offset`: sorted ranges of one row cost
/// one pass over it.
pub fn column_after(line: &[u8], from: (usize, u64), offset: usize, tab_width: u32) -> u64 {
    let (reached, col) = walk(
        line,
        from,
        offset.min(COLUMN_SCAN_LIMIT),
        tab_width,
        |o, _, _, _| o < offset,
    );
    col + offset.saturating_sub(reached) as u64
}

/// Character boundary nearest to display column `col`. Boundaries before
/// zero-width marks are skipped. Past the scanned bytes, one byte per
/// column; callers clamp the result to the line length.
pub fn offset_at_column(line: &[u8], col: u64, tab_width: u32) -> usize {
    let mut target = col;
    let mut found = None;
    let (reached, end_col) = walk(
        line,
        (0, 0),
        COLUMN_SCAN_LIMIT,
        tab_width,
        |o, _, c, cells| {
            if cells == 0 {
                return true;
            }
            if target <= c {
                found = Some(o);
                return false;
            }
            if target < c + cells {
                if (target - c) * 2 < cells {
                    found = Some(o);
                    return false;
                }
                target = c + cells;
            }
            true
        },
    );
    found.unwrap_or_else(|| reached + target.saturating_sub(end_col) as usize)
}
