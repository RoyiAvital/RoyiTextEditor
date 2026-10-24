//! Immutable document snapshots and their queries.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use crate::change::{Change, ChangeSet, EditError, Transaction};
use crate::tree::{self, Kind, Node};

/// Document revision; increments with every applied transaction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Rev(pub u64);

/// Immutable document text. Cloning is a reference-count increment.
#[derive(Clone)]
pub struct Snapshot {
    root: Arc<Node>,
    rev: Rev,
}

impl Default for Snapshot {
    fn default() -> Snapshot {
        Snapshot::empty()
    }
}

impl fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Snapshot")
            .field("rev", &self.rev)
            .field("len", &self.len())
            .field("newlines", &self.newlines())
            .field("height", &self.root.height())
            .finish()
    }
}

impl Snapshot {
    pub fn empty() -> Snapshot {
        Snapshot::from_root(Node::empty(), Rev(0))
    }

    /// Snapshot holding a copy of `bytes`.
    pub fn from_bytes(bytes: &[u8]) -> Snapshot {
        Snapshot::from_root(tree::text_node(bytes), Rev(0))
    }

    pub(crate) fn from_root(root: Arc<Node>, rev: Rev) -> Snapshot {
        Snapshot { root, rev }
    }

    pub fn rev(&self) -> Rev {
        self.rev
    }

    pub fn len(&self) -> u64 {
        self.root.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Number of `\n` bytes.
    pub fn newlines(&self) -> u64 {
        self.root.summary().newlines
    }

    /// Number of lines; a document always has at least one.
    pub fn line_count(&self) -> u64 {
        self.newlines() + 1
    }

    /// True when both snapshots share the same tree, i.e. identical content
    /// by construction (used for the clean state).
    pub fn same_tree(&self, other: &Snapshot) -> bool {
        Arc::ptr_eq(&self.root, &other.root)
    }

    /// Byte offset where `line` (0-based) starts, or `None` past the last line.
    pub fn line_start(&self, line: u64) -> Option<u64> {
        if line == 0 {
            return Some(0);
        }
        if line > self.newlines() {
            return None;
        }
        let mut node: &Node = &self.root;
        let mut remaining = line;
        let mut offset = 0;
        loop {
            match node.kind() {
                Kind::Internal(children) => {
                    let mut next = None;
                    for child in children {
                        let newlines = child.summary().newlines;
                        if newlines >= remaining {
                            next = Some(child);
                            break;
                        }
                        remaining -= newlines;
                        offset += child.len();
                    }
                    node = next.expect("summary counts are consistent");
                }
                Kind::Leaf(chunk) => {
                    let index = memchr::memchr_iter(b'\n', chunk.bytes())
                        .nth((remaining - 1) as usize)
                        .expect("summary counts are consistent");
                    return Some(offset + index as u64 + 1);
                }
            }
        }
    }

    /// Line (0-based) containing byte `offset`: the number of `\n` before it.
    /// Offsets past the end are clamped.
    pub fn line_of(&self, offset: u64) -> u64 {
        let mut remaining = offset.min(self.len());
        let mut node: &Node = &self.root;
        let mut lines = 0;
        loop {
            match node.kind() {
                Kind::Internal(children) => {
                    let mut next = None;
                    for child in children {
                        if remaining < child.len() {
                            next = Some(child);
                            break;
                        }
                        remaining -= child.len();
                        lines += child.summary().newlines;
                    }
                    match next {
                        Some(child) => node = child,
                        None => return lines,
                    }
                }
                Kind::Leaf(chunk) => {
                    return lines + tree::count_newlines(&chunk.bytes()[..remaining as usize]);
                }
            }
        }
    }

    /// Byte range of `line` without its `\n`, or `None` past the last line.
    pub fn line_range(&self, line: u64) -> Option<Range<u64>> {
        let start = self.line_start(line)?;
        let end = match self.line_start(line + 1) {
            Some(next) => next - 1,
            None => self.len(),
        };
        Some(start..end)
    }

    /// The leaf slice containing byte `offset` and the offset where it
    /// starts; `None` at or past the end. For cursors that read nearby bytes.
    pub fn chunk_at(&self, offset: u64) -> Option<(u64, &[u8])> {
        if offset >= self.len() {
            return None;
        }
        let mut node: &Node = &self.root;
        let mut remaining = offset;
        let mut start = 0;
        loop {
            match node.kind() {
                Kind::Internal(children) => {
                    let mut next = None;
                    for child in children {
                        if remaining < child.len() {
                            next = Some(child);
                            break;
                        }
                        remaining -= child.len();
                        start += child.len();
                    }
                    node = next.expect("offset is inside the tree");
                }
                Kind::Leaf(chunk) => return Some((start, chunk.bytes())),
            }
        }
    }

    /// Contiguous slices covering `range` in order. The range is clamped.
    pub fn chunks(&self, range: Range<u64>) -> Chunks<'_> {
        let end = range.end.min(self.len());
        let start = range.start.min(end);
        let mut stack = Vec::new();
        if start < end {
            stack.push(Frame {
                node: &self.root,
                next: 0,
                offset: 0,
            });
        }
        Chunks { stack, start, end }
    }

    /// Copy of `range` (clamped).
    pub fn to_vec(&self, range: Range<u64>) -> Vec<u8> {
        let mut out = Vec::new();
        for chunk in self.chunks(range) {
            out.extend_from_slice(chunk);
        }
        out
    }

    /// The single change turning `self` into `new`: the bytes between their
    /// common prefix and suffix. Leaves shared by both trees are skipped
    /// without comparing bytes, so related snapshots (undo, redo) diff fast.
    pub fn diff(&self, new: &Snapshot) -> ChangeSet {
        if self.same_tree(new) {
            return ChangeSet::default();
        }
        let (old_len, new_len) = (self.len(), new.len());
        let limit = old_len.min(new_len);
        // Common prefix.
        let mut prefix = 0;
        while prefix < limit {
            let (a_start, a) = self.chunk_at(prefix).expect("inside");
            let (b_start, b) = new.chunk_at(prefix).expect("inside");
            let a = &a[(prefix - a_start) as usize..];
            let b = &b[(prefix - b_start) as usize..];
            let n = a.len().min(b.len()).min((limit - prefix) as usize);
            if std::ptr::eq(a.as_ptr(), b.as_ptr()) {
                prefix += n as u64;
                continue;
            }
            match a[..n].iter().zip(&b[..n]).position(|(x, y)| x != y) {
                Some(i) => {
                    prefix += i as u64;
                    break;
                }
                None => prefix += n as u64,
            }
        }
        // Common suffix, not overlapping the prefix.
        let mut suffix = 0;
        while suffix < limit - prefix {
            let (a_end, b_end) = (old_len - suffix, new_len - suffix);
            let (a_start, a) = self.chunk_at(a_end - 1).expect("inside");
            let (b_start, b) = new.chunk_at(b_end - 1).expect("inside");
            let a = &a[..(a_end - a_start) as usize];
            let b = &b[..(b_end - b_start) as usize];
            let n = a.len().min(b.len()).min((limit - prefix - suffix) as usize);
            let (a, b) = (&a[a.len() - n..], &b[b.len() - n..]);
            if std::ptr::eq(a.as_ptr(), b.as_ptr()) {
                suffix += n as u64;
                continue;
            }
            match a.iter().rev().zip(b.iter().rev()).position(|(x, y)| x != y) {
                Some(i) => {
                    suffix += i as u64;
                    break;
                }
                None => suffix += n as u64,
            }
        }
        let old = prefix..old_len - suffix;
        let inserted = new_len - suffix - prefix;
        if old.is_empty() && inserted == 0 {
            return ChangeSet::default();
        }
        ChangeSet::from_changes(vec![Change {
            old,
            new_len: inserted,
        }])
    }

    /// Applies `tx` and returns the new snapshot with the position mapping.
    pub fn apply(&self, tx: &Transaction) -> Result<(Snapshot, ChangeSet), EditError> {
        tx.validate(self.len())?;
        let edits = tx.edits();
        let batched = if edits.len() > 1 {
            tree::apply_batch(&self.root, edits)
        } else {
            None
        };
        let root = batched.unwrap_or_else(|| {
            let mut root = self.root.clone();
            for edit in edits.iter().rev() {
                root = tree::replace(&root, edit.range.clone(), &edit.text);
            }
            root
        });
        let next = Snapshot::from_root(root, Rev(self.rev.0 + 1));
        Ok((next, ChangeSet::from_transaction(tx)))
    }

    #[cfg(test)]
    pub(crate) fn root(&self) -> &Node {
        &self.root
    }

    #[cfg(test)]
    pub(crate) fn root_arc(&self) -> &Arc<Node> {
        &self.root
    }
}

struct Frame<'a> {
    node: &'a Node,
    /// Next child to visit (internal nodes).
    next: usize,
    /// Document offset of that child (internal) or of the leaf.
    offset: u64,
}

/// Iterator over the slices of a snapshot range.
pub struct Chunks<'a> {
    stack: Vec<Frame<'a>>,
    start: u64,
    end: u64,
}

impl<'a> Iterator for Chunks<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        while let Some(top) = self.stack.last_mut() {
            let node: &'a Node = top.node;
            match node.kind() {
                Kind::Leaf(chunk) => {
                    let leaf_start = top.offset;
                    self.stack.pop();
                    let s = self.start.max(leaf_start);
                    let e = self.end.min(leaf_start + chunk.len() as u64);
                    if s < e {
                        return Some(
                            &chunk.bytes()[(s - leaf_start) as usize..(e - leaf_start) as usize],
                        );
                    }
                }
                Kind::Internal(children) => {
                    if top.next >= children.len() || top.offset >= self.end {
                        self.stack.pop();
                        continue;
                    }
                    let child: &'a Node = &children[top.next];
                    let child_start = top.offset;
                    top.next += 1;
                    top.offset += child.len();
                    if top.offset > self.start {
                        self.stack.push(Frame {
                            node: child,
                            next: 0,
                            offset: child_start,
                        });
                    }
                }
            }
        }
        None
    }
}
