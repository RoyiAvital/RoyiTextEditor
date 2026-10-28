//! Text engine of RoyiTextEditor: a persistent piece tree over immutable
//! buffers, with cheap snapshots, line queries, transactions and position
//! mapping. See `Plan/Architecture.md`.

mod builder;
mod change;
mod encoding;
mod snapshot;
mod tree;

pub use builder::TreeBuilder;
pub use change::{Bias, Change, ChangeSet, Edit, EditError, Inserted, MAX_LEN, Piece, Transaction};
pub use encoding::{Encoding, InvalidUtf8, LineEnding, Utf16Decoder, Utf16Encoder};
pub use snapshot::{Chunks, Rev, Snapshot};
pub use tree::BUFFER_MAX;

#[cfg(test)]
mod tests;
