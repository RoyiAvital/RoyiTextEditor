//! Incremental construction from loaded blocks.

use std::sync::Arc;

use crate::snapshot::{Rev, Snapshot};
use crate::tree::{self, BUFFER_MAX, Node};

/// Builds a document from blocks appended in file order. Each block is shared,
/// not copied. `snapshot` can publish the prefix loaded so far.
#[derive(Default)]
pub struct TreeBuilder {
    leaves: Vec<Arc<Node>>,
    bytes: u64,
}

impl TreeBuilder {
    pub fn new() -> TreeBuilder {
        TreeBuilder::default()
    }

    /// Appends the next block. Blocks may be at most [`BUFFER_MAX`] bytes.
    pub fn push_block(&mut self, block: Arc<[u8]>) {
        assert!(block.len() <= BUFFER_MAX, "block exceeds BUFFER_MAX");
        self.bytes += block.len() as u64;
        tree::leaves_of(&block, &mut self.leaves);
    }

    /// Bytes appended so far.
    pub fn len(&self) -> u64 {
        self.bytes
    }

    pub fn is_empty(&self) -> bool {
        self.bytes == 0
    }

    /// Snapshot of everything appended so far (revision 0).
    pub fn snapshot(&self) -> Snapshot {
        Snapshot::from_root(tree::build(self.leaves.clone()), Rev(0))
    }

    pub fn finish(self) -> Snapshot {
        Snapshot::from_root(tree::build(self.leaves), Rev(0))
    }
}
