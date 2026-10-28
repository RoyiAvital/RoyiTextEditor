//! Editing model over `rte_text` snapshots: selections, movement, pure edit
//! commands and the undo history. No I/O and no platform code.
//!
//! Positions are byte offsets. Movement never stops inside a UTF-8 sequence,
//! between `\r` and `\n` of a CRLF, or before a zero-width mark (marks belong
//! to the previous character, as the view draws them).

mod code;
mod columns;
mod commands;
mod cursor;
mod history;
mod lines;
mod motion;
mod multi;
mod selection;

pub use code::{
    Coded, CommentMode, CommentTokens, auto_close, delete_pair, line_comment, newline_indent,
    toggle_block_comment, toggle_line_comment,
};
pub use columns::{
    COLUMN_SCAN_LIMIT, cell_width, column_after, column_of, column_until, offset_at_column,
    scan_end,
};
pub use commands::{
    Clip, Command, EditKind, EditOptions, apply, backspace, copy, cut, delete_forward,
    delete_word_left, delete_word_right, insert, newline, outdent, paste, tab,
};
pub use history::{History, MAX_HISTORY_BYTES, MAX_HISTORY_STEPS, Step};
pub use lines::{copy_lines, delete_lines, indent_lines, insert_line, line_blocks, move_lines};
pub use motion::{
    MOTION_SCAN_LIMIT, column_at, content_range, line_end, line_home, next_boundary,
    offset_in_line, prev_boundary, snap_to_boundary, vertical, word_at, word_left, word_right,
};
pub use multi::{add_cursors_vertical, box_select, cell_of, cursors_at_line_ends, select_lines};
pub use selection::{Selection, Selections};

#[cfg(test)]
mod tests;
