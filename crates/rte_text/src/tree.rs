//! Persistent B-tree of immutable buffer slices.
//!
//! Nodes are never mutated after construction. Edits build new nodes along
//! the touched path and share everything else with older trees.

use std::cmp::Ordering;
use std::ops::Range;
use std::sync::Arc;

use crate::change::Edit;

// Small limits under test build deep trees from small inputs.
#[cfg(not(test))]
mod limits {
    pub const MAX_CHILDREN: usize = 32;
    pub const LEAF_MAX: usize = 8 * 1024;
    pub const COALESCE_MAX: usize = 4 * 1024;
    pub const MIN_LEAF: usize = 1024;
}
#[cfg(test)]
mod limits {
    pub const MAX_CHILDREN: usize = 4;
    pub const LEAF_MAX: usize = 64;
    pub const COALESCE_MAX: usize = 16;
    pub const MIN_LEAF: usize = 4;
}
pub(crate) use limits::{COALESCE_MAX, LEAF_MAX, MAX_CHILDREN, MIN_LEAF};

pub(crate) const MIN_CHILDREN: usize = MAX_CHILDREN / 2;

/// Largest shared buffer; keeps chunk offsets within `u32`.
pub const BUFFER_MAX: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Summary {
    pub bytes: u64,
    pub newlines: u64,
}

impl Summary {
    fn add(self, other: Summary) -> Summary {
        Summary {
            bytes: self.bytes + other.bytes,
            newlines: self.newlines + other.newlines,
        }
    }
}

pub(crate) fn count_newlines(bytes: &[u8]) -> u64 {
    memchr::memchr_iter(b'\n', bytes).count() as u64
}

/// A slice of a shared immutable buffer: a loaded block or inserted text.
#[derive(Clone)]
pub(crate) struct Chunk {
    buf: Arc<[u8]>,
    start: u32,
    len: u32,
}

impl Chunk {
    pub fn new(buf: Arc<[u8]>, range: Range<usize>) -> Chunk {
        assert!(range.start <= range.end && range.end <= buf.len());
        let len = u32::try_from(range.len()).expect("chunk longer than u32::MAX");
        let start = u32::try_from(range.start).expect("chunk offset beyond u32::MAX");
        Chunk { buf, start, len }
    }

    fn empty() -> Chunk {
        Chunk::new(Arc::from(&[][..]), 0..0)
    }

    fn copied(bytes: &[u8]) -> Chunk {
        let len = bytes.len();
        Chunk::new(Arc::from(bytes), 0..len)
    }

    pub fn bytes(&self) -> &[u8] {
        &self.buf[self.start as usize..(self.start + self.len) as usize]
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    fn slice(&self, range: Range<usize>) -> Chunk {
        assert!(range.start <= range.end && range.end <= self.len());
        Chunk {
            buf: self.buf.clone(),
            start: self.start + range.start as u32,
            len: range.len() as u32,
        }
    }
}

pub(crate) struct Node {
    height: u8,
    summary: Summary,
    kind: Kind,
}

pub(crate) enum Kind {
    Leaf(Chunk),
    Internal(Vec<Arc<Node>>),
}

impl Node {
    pub fn empty() -> Arc<Node> {
        Node::leaf(Chunk::empty())
    }

    pub fn leaf(chunk: Chunk) -> Arc<Node> {
        let newlines = count_newlines(chunk.bytes());
        Node::leaf_with(chunk, newlines)
    }

    fn leaf_with(chunk: Chunk, newlines: u64) -> Arc<Node> {
        let summary = Summary {
            bytes: chunk.len() as u64,
            newlines,
        };
        Arc::new(Node {
            height: 0,
            summary,
            kind: Kind::Leaf(chunk),
        })
    }

    fn from_children(children: Vec<Arc<Node>>) -> Arc<Node> {
        debug_assert!((2..=MAX_CHILDREN).contains(&children.len()));
        let height = children[0].height + 1;
        debug_assert!(children.iter().all(|c| c.height + 1 == height));
        let summary = children
            .iter()
            .fold(Summary::default(), |s, c| s.add(c.summary));
        Arc::new(Node {
            height,
            summary,
            kind: Kind::Internal(children),
        })
    }

    pub fn summary(&self) -> Summary {
        self.summary
    }

    pub fn len(&self) -> u64 {
        self.summary.bytes
    }

    pub fn height(&self) -> u8 {
        self.height
    }

    pub fn kind(&self) -> &Kind {
        &self.kind
    }

    fn is_ok_child(&self) -> bool {
        match &self.kind {
            Kind::Leaf(chunk) => chunk.len() >= MIN_LEAF,
            Kind::Internal(children) => children.len() >= MIN_CHILDREN,
        }
    }

    fn children(&self) -> &[Arc<Node>] {
        match &self.kind {
            Kind::Internal(children) => children,
            Kind::Leaf(_) => unreachable!("leaf has no children"),
        }
    }

    fn chunk(&self) -> &Chunk {
        match &self.kind {
            Kind::Leaf(chunk) => chunk,
            Kind::Internal(_) => unreachable!("internal node has no chunk"),
        }
    }
}

/// Balanced tree over `leaves` (all height 0), built bottom-up.
pub(crate) fn build(mut level: Vec<Arc<Node>>) -> Arc<Node> {
    if level.is_empty() {
        return Node::empty();
    }
    while level.len() > 1 {
        let groups = level.len().div_ceil(MAX_CHILDREN);
        let base = level.len() / groups;
        let extra = level.len() % groups;
        let mut next = Vec::with_capacity(groups);
        let mut rest = level.into_iter();
        for g in 0..groups {
            let size = base + usize::from(g < extra);
            next.push(Node::from_children(rest.by_ref().take(size).collect()));
        }
        level = next;
    }
    level.pop().unwrap()
}

/// Leaves of at most `LEAF_MAX` bytes over `buf`.
pub(crate) fn leaves_of(buf: &Arc<[u8]>, out: &mut Vec<Arc<Node>>) {
    let mut start = 0;
    while start < buf.len() {
        let end = (start + LEAF_MAX).min(buf.len());
        out.push(Node::leaf(Chunk::new(buf.clone(), start..end)));
        start = end;
    }
}

/// Tree holding a copy of `text`, in buffers of at most `BUFFER_MAX` bytes.
pub(crate) fn text_node(text: &[u8]) -> Arc<Node> {
    let mut leaves = Vec::new();
    for part in text.chunks(BUFFER_MAX) {
        leaves_of(&Arc::from(part), &mut leaves);
    }
    build(leaves)
}

fn group(children: &[Arc<Node>]) -> Arc<Node> {
    match children.len() {
        0 => Node::empty(),
        1 => children[0].clone(),
        _ => Node::from_children(children.to_vec()),
    }
}

fn merge_nodes(left: &[Arc<Node>], right: &[Arc<Node>]) -> Arc<Node> {
    let total = left.len() + right.len();
    let all: Vec<Arc<Node>> = left.iter().chain(right).cloned().collect();
    if total <= MAX_CHILDREN {
        return Node::from_children(all);
    }
    let mid = total / 2;
    Node::from_children(vec![group(&all[..mid]), group(&all[mid..])])
}

fn merge_leaves(a: Arc<Node>, b: Arc<Node>) -> Arc<Node> {
    let (ca, cb) = (a.chunk(), b.chunk());
    let total = ca.len() + cb.len();
    if total <= COALESCE_MAX {
        let mut bytes = Vec::with_capacity(total);
        bytes.extend_from_slice(ca.bytes());
        bytes.extend_from_slice(cb.bytes());
        return Node::leaf(Chunk::copied(&bytes));
    }
    if a.is_ok_child() && b.is_ok_child() {
        return Node::from_children(vec![a, b]);
    }
    // One side is small: move bytes from the large side, copying at most MIN_LEAF.
    let (left, right) = if cb.len() < MIN_LEAF {
        let keep = ca.len() - (MIN_LEAF - cb.len());
        let mut tail = ca.bytes()[keep..].to_vec();
        tail.extend_from_slice(cb.bytes());
        (ca.slice(0..keep), Chunk::copied(&tail))
    } else {
        let take = MIN_LEAF - ca.len();
        let mut head = ca.bytes().to_vec();
        head.extend_from_slice(&cb.bytes()[..take]);
        (Chunk::copied(&head), cb.slice(take..cb.len()))
    };
    Node::from_children(vec![Node::leaf(left), Node::leaf(right)])
}

/// Concatenation keeping all leaves at equal depth.
pub(crate) fn concat(a: Arc<Node>, b: Arc<Node>) -> Arc<Node> {
    if a.len() == 0 {
        return b;
    }
    if b.len() == 0 {
        return a;
    }
    let (ha, hb) = (a.height, b.height);
    match ha.cmp(&hb) {
        Ordering::Less => {
            let bc = b.children();
            if ha + 1 == hb && a.is_ok_child() {
                return merge_nodes(&[a], bc);
            }
            let left = concat(a, bc[0].clone());
            if left.height + 1 == hb {
                merge_nodes(&[left], &bc[1..])
            } else {
                merge_nodes(left.children(), &bc[1..])
            }
        }
        Ordering::Equal => {
            if ha == 0 {
                return merge_leaves(a, b);
            }
            if a.is_ok_child() && b.is_ok_child() {
                return Node::from_children(vec![a, b]);
            }
            merge_nodes(a.children(), b.children())
        }
        Ordering::Greater => {
            let ac = a.children();
            let last = ac.len() - 1;
            if hb + 1 == ha && b.is_ok_child() {
                return merge_nodes(ac, &[b]);
            }
            let right = concat(ac[last].clone(), b);
            if right.height + 1 == ha {
                merge_nodes(&ac[..last], &[right])
            } else {
                merge_nodes(&ac[..last], right.children())
            }
        }
    }
}

/// Splits `node` into `[0, at)` and `[at, len)`.
pub(crate) fn split(node: &Arc<Node>, at: u64) -> (Arc<Node>, Arc<Node>) {
    debug_assert!(at <= node.len());
    if at == 0 {
        return (Node::empty(), node.clone());
    }
    if at == node.len() {
        return (node.clone(), Node::empty());
    }
    match &node.kind {
        Kind::Leaf(chunk) => {
            let at = at as usize;
            let total = node.summary.newlines;
            // Scan only the shorter side; the other follows from the total.
            let (left_nl, right_nl) = if at <= chunk.len() / 2 {
                let left = count_newlines(&chunk.bytes()[..at]);
                (left, total - left)
            } else {
                let right = count_newlines(&chunk.bytes()[at..]);
                (total - right, right)
            };
            (
                Node::leaf_with(chunk.slice(0..at), left_nl),
                Node::leaf_with(chunk.slice(at..chunk.len()), right_nl),
            )
        }
        Kind::Internal(children) => {
            let mut offset = at;
            let mut index = 0;
            while offset >= children[index].len() {
                offset -= children[index].len();
                index += 1;
            }
            let (child_left, child_right) = split(&children[index], offset);
            let left = concat(group(&children[..index]), child_left);
            let right = concat(child_right, group(&children[index + 1..]));
            (left, right)
        }
    }
}

/// Applies sorted, non-overlapping edits in one traversal: every touched node
/// is rebuilt once. Returns `None` when an edit crosses a leaf boundary or
/// inserts more than `LEAF_MAX` bytes; the caller then applies edits one by one.
pub(crate) fn apply_batch(root: &Arc<Node>, edits: &[Edit]) -> Option<Arc<Node>> {
    let mut level = rebuild(root, 0, edits)?;
    if level.is_empty() {
        return Some(Node::empty());
    }
    while level.len() > 1 {
        level = regroup(level);
    }
    level.pop()
}

/// Nodes of `node`'s height that replace it after `edits` (absolute offsets).
fn rebuild(node: &Arc<Node>, start: u64, edits: &[Edit]) -> Option<Vec<Arc<Node>>> {
    if edits.is_empty() {
        return Some(vec![node.clone()]);
    }
    match &node.kind {
        Kind::Leaf(chunk) => rebuild_leaf(node, chunk, start, edits),
        Kind::Internal(children) => {
            let mut out = Vec::with_capacity(children.len() + edits.len());
            let mut child_start = start;
            let mut rest = edits;
            for (i, child) in children.iter().enumerate() {
                let child_end = child_start + child.len();
                let last = i + 1 == children.len();
                // Edits starting in this child; boundary insertions go left.
                let count = rest
                    .iter()
                    .take_while(|e| {
                        e.range.start < child_end
                            || (e.range.start == child_end && e.range.is_empty())
                    })
                    .count();
                let (mine, tail) = rest.split_at(count);
                if mine.iter().any(|e| e.range.end > child_end) || (last && !tail.is_empty()) {
                    return None;
                }
                out.extend(rebuild(child, child_start, mine)?);
                rest = tail;
                child_start = child_end;
            }
            match out.len() {
                0 => Some(out),
                1 => None,
                n if n <= MAX_CHILDREN => Some(vec![Node::from_children(out)]),
                _ => Some(regroup(out)),
            }
        }
    }
}

/// Groups same-height nodes into parents of at most `MAX_CHILDREN` (at least
/// `MIN_CHILDREN` when more than one parent is needed).
fn regroup(nodes: Vec<Arc<Node>>) -> Vec<Arc<Node>> {
    if nodes.len() == 1 {
        return nodes;
    }
    let groups = nodes.len().div_ceil(MAX_CHILDREN);
    let base = nodes.len() / groups;
    let extra = nodes.len() % groups;
    let mut rest = nodes.into_iter();
    (0..groups)
        .map(|g| {
            let size = base + usize::from(g < extra);
            let group: Vec<_> = rest.by_ref().take(size).collect();
            if group.len() == 1 {
                group.into_iter().next().unwrap()
            } else {
                Node::from_children(group)
            }
        })
        .collect()
}

fn rebuild_leaf(node: &Node, chunk: &Chunk, start: u64, edits: &[Edit]) -> Option<Vec<Arc<Node>>> {
    let bytes = chunk.bytes();
    // Kept original slices (by range) interleaved with inserted texts.
    enum Piece<'a> {
        Kept(Range<usize>),
        Text(&'a [u8]),
    }
    let mut pieces = Vec::with_capacity(2 * edits.len() + 1);
    let mut pos = 0;
    let mut removed_newlines = 0;
    for edit in edits {
        let text = edit.text.bytes();
        if text.len() > LEAF_MAX {
            return None;
        }
        let (s, e) = (
            (edit.range.start - start) as usize,
            (edit.range.end - start) as usize,
        );
        if pos < s {
            pieces.push(Piece::Kept(pos..s));
        }
        removed_newlines += count_newlines(&bytes[s..e]);
        if !text.is_empty() {
            pieces.push(Piece::Text(text));
        }
        pos = e;
    }
    if pos < bytes.len() {
        pieces.push(Piece::Kept(pos..bytes.len()));
    }

    // The longest kept slice gets its newline count by subtraction.
    let longest = pieces
        .iter()
        .enumerate()
        .filter_map(|(i, p)| match p {
            Piece::Kept(r) => Some((r.len(), i)),
            Piece::Text(_) => None,
        })
        .max()
        .map(|(_, i)| i);
    let mut counts = vec![0u64; pieces.len()];
    let mut known = 0;
    for (i, piece) in pieces.iter().enumerate() {
        if let Piece::Kept(r) = piece
            && Some(i) != longest
        {
            counts[i] = count_newlines(&bytes[r.clone()]);
            known += counts[i];
        }
    }
    if let Some(i) = longest {
        counts[i] = node.summary.newlines - removed_newlines - known;
    }

    // Pack: large kept slices stay shared; small pieces are copied together.
    let mut out = Vec::new();
    let mut pending: Vec<u8> = Vec::new();
    for (piece, newlines) in pieces.iter().zip(counts) {
        match piece {
            Piece::Kept(r) if r.len() >= MIN_LEAF => {
                flush(&mut pending, &mut out);
                out.push(Node::leaf_with(chunk.slice(r.clone()), newlines));
            }
            Piece::Kept(r) => push_small(&bytes[r.clone()], &mut pending, &mut out),
            Piece::Text(text) => push_small(text, &mut pending, &mut out),
        }
    }
    flush(&mut pending, &mut out);
    Some(out)
}

fn push_small(bytes: &[u8], pending: &mut Vec<u8>, out: &mut Vec<Arc<Node>>) {
    if pending.len() + bytes.len() > COALESCE_MAX {
        flush(pending, out);
    }
    if bytes.len() > COALESCE_MAX {
        out.push(Node::leaf(Chunk::copied(bytes)));
    } else {
        pending.extend_from_slice(bytes);
    }
}

fn flush(pending: &mut Vec<u8>, out: &mut Vec<Arc<Node>>) {
    if !pending.is_empty() {
        out.push(Node::leaf(Chunk::copied(pending)));
        pending.clear();
    }
}

/// Replaces `range` of `root` with `text`.
pub(crate) fn replace(root: &Arc<Node>, range: Range<u64>, text: &[u8]) -> Arc<Node> {
    if let Some(node) = replace_local(root, range.clone(), text) {
        return node;
    }
    replace_node(root, range, text_node(text))
}

/// Replaces `range` of `root` with the tree `node`: O(log n), sharing leaves.
pub(crate) fn replace_node(root: &Arc<Node>, range: Range<u64>, node: Arc<Node>) -> Arc<Node> {
    let (left, rest) = split(root, range.start);
    let (_, right) = split(&rest, range.end - range.start);
    concat(concat(left, node), right)
}

/// The bytes of `range` as a tree sharing `root`'s leaves: O(log n).
pub(crate) fn slice(root: &Arc<Node>, range: Range<u64>) -> Arc<Node> {
    let (_, rest) = split(root, range.start);
    split(&rest, range.end - range.start).0
}

/// Fast path for an edit inside one leaf whose parent has room for the
/// resulting pieces: copies only the path from the root to that leaf.
fn replace_local(node: &Arc<Node>, range: Range<u64>, text: &[u8]) -> Option<Arc<Node>> {
    let Kind::Internal(children) = &node.kind else {
        return None;
    };
    // An insertion at a child boundary goes to the left child, so typing extends
    // the leaf just typed into.
    let mut offset = 0;
    let mut index = 0;
    while range.start > offset + children[index].len() {
        offset += children[index].len();
        index += 1;
    }
    let child = &children[index];
    if range.end > offset + child.len() {
        return None;
    }
    let local = range.start - offset..range.end - offset;
    let mut new_children = Vec::with_capacity(children.len() + 2);
    new_children.extend_from_slice(&children[..index]);
    match &child.kind {
        Kind::Leaf(chunk) => {
            leaf_pieces(child, chunk, local, text, &mut new_children)?;
        }
        Kind::Internal(_) => new_children.push(replace_local(child, local, text)?),
    }
    new_children.extend_from_slice(&children[index + 1..]);
    if !(2..=MAX_CHILDREN).contains(&new_children.len()) {
        return None;
    }
    Some(Node::from_children(new_children))
}

/// Leaves for `chunk` with `range` replaced by `text`, appended to `out`.
fn leaf_pieces(
    leaf: &Node,
    chunk: &Chunk,
    range: Range<u64>,
    text: &[u8],
    out: &mut Vec<Arc<Node>>,
) -> Option<()> {
    if text.len() > LEAF_MAX {
        return None;
    }
    let (s, e) = (range.start as usize, range.end as usize);
    let bytes = chunk.bytes();
    let (left, right) = (&bytes[..s], &bytes[e..]);
    if left.len() + text.len() + right.len() <= COALESCE_MAX {
        let mut joined = Vec::with_capacity(left.len() + text.len() + right.len());
        joined.extend_from_slice(left);
        joined.extend_from_slice(text);
        joined.extend_from_slice(right);
        if !joined.is_empty() {
            out.push(Node::leaf(Chunk::copied(&joined)));
        }
        return Some(());
    }
    // Count the removed part and the shorter side; derive the longer side.
    let kept = leaf.summary.newlines - count_newlines(&bytes[s..e]);
    let (left_nl, right_nl) = if left.len() <= right.len() {
        let l = count_newlines(left);
        (l, kept - l)
    } else {
        let r = count_newlines(right);
        (kept - r, r)
    };
    let left_leaf = (!left.is_empty()).then(|| Node::leaf_with(chunk.slice(0..s), left_nl));
    let right_leaf =
        (!right.is_empty()).then(|| Node::leaf_with(chunk.slice(e..chunk.len()), right_nl));
    let mut middle = (!text.is_empty()).then(|| Node::leaf(Chunk::copied(text)));
    // Fold the new text into a small neighbor instead of adding a tiny leaf.
    let (mut left_leaf, mut right_leaf) = (left_leaf, right_leaf);
    if let Some(m) = &middle {
        if let Some(l) = left_leaf
            .as_ref()
            .filter(|l| l.len() + m.len() <= COALESCE_MAX as u64)
        {
            let joined = [l.chunk().bytes(), text].concat();
            left_leaf = Some(Node::leaf(Chunk::copied(&joined)));
            middle = None;
        } else if let Some(r) = right_leaf
            .as_ref()
            .filter(|r| r.len() + m.len() <= COALESCE_MAX as u64)
        {
            let joined = [text, r.chunk().bytes()].concat();
            right_leaf = Some(Node::leaf(Chunk::copied(&joined)));
            middle = None;
        }
    }
    out.extend(left_leaf);
    out.extend(middle);
    out.extend(right_leaf);
    Some(())
}

/// Verifies structural invariants; returns the number of leaves.
#[cfg(test)]
pub(crate) fn check_invariants(node: &Node) -> usize {
    match &node.kind {
        Kind::Leaf(chunk) => {
            assert_eq!(node.height, 0);
            assert!(chunk.len() <= LEAF_MAX);
            assert_eq!(node.summary.bytes, chunk.len() as u64);
            assert_eq!(node.summary.newlines, count_newlines(chunk.bytes()));
            1
        }
        Kind::Internal(children) => {
            assert!((2..=MAX_CHILDREN).contains(&children.len()));
            let mut summary = Summary::default();
            let mut leaves = 0;
            for child in children {
                assert_eq!(child.height + 1, node.height);
                summary = summary.add(child.summary);
                leaves += check_invariants(child);
            }
            assert_eq!(summary, node.summary);
            leaves
        }
    }
}
