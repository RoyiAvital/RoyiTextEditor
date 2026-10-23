//! Unit and property tests. Run with reduced tree limits (see `tree::limits`),
//! so small inputs build deep trees.

use std::sync::Arc;

use proptest::prelude::*;

use crate::tree::{self, LEAF_MAX};
use crate::*;

fn text(s: &Snapshot) -> Vec<u8> {
    s.to_vec(0..s.len())
}

fn check(s: &Snapshot, model: &[u8]) {
    let leaves = tree::check_invariants(s.root());
    // Logarithmic depth: at least two children per internal node.
    let bound = usize::BITS - leaves.leading_zeros();
    assert!(
        u32::from(s.root().height()) <= bound,
        "height {} for {leaves} leaves",
        s.root().height()
    );
    assert_eq!(s.len(), model.len() as u64);
    assert_eq!(text(s), model);
    let newlines: Vec<usize> = model
        .iter()
        .enumerate()
        .filter(|(_, b)| **b == b'\n')
        .map(|(i, _)| i)
        .collect();
    assert_eq!(s.newlines(), newlines.len() as u64);
    assert_eq!(s.line_start(0), Some(0));
    for (k, &i) in newlines.iter().enumerate() {
        assert_eq!(s.line_start(k as u64 + 1), Some(i as u64 + 1));
    }
    assert_eq!(s.line_start(newlines.len() as u64 + 1), None);
}

fn model_line_of(model: &[u8], offset: usize) -> u64 {
    model[..offset.min(model.len())]
        .iter()
        .filter(|b| **b == b'\n')
        .count() as u64
}

#[test]
fn empty_document() {
    let s = Snapshot::empty();
    assert!(s.is_empty());
    assert_eq!(s.line_count(), 1);
    assert_eq!(s.line_range(0), Some(0..0));
    assert_eq!(s.line_range(1), None);
    assert_eq!(s.chunks(0..10).count(), 0);
}

#[test]
fn builder_blocks_and_prefix_snapshots() {
    let mut builder = TreeBuilder::new();
    let mut model = Vec::new();
    for i in 0..20u8 {
        let block: Vec<u8> = (0..(37 * i as usize + 5))
            .map(|j| if j % 7 == 0 { b'\n' } else { b'a' + i })
            .collect();
        model.extend_from_slice(&block);
        builder.push_block(Arc::from(block));
        let prefix = builder.snapshot();
        check(&prefix, &model);
    }
    assert_eq!(builder.len(), model.len() as u64);
    check(&builder.finish(), &model);
}

#[test]
fn line_queries() {
    let s = Snapshot::from_bytes(b"ab\r\ncd\n\nlast");
    assert_eq!(s.line_count(), 4);
    assert_eq!(s.line_range(0), Some(0..3));
    assert_eq!(s.line_range(1), Some(4..6));
    assert_eq!(s.line_range(2), Some(7..7));
    assert_eq!(s.line_range(3), Some(8..12));
    assert_eq!(s.line_of(0), 0);
    assert_eq!(s.line_of(3), 0);
    assert_eq!(s.line_of(4), 1);
    assert_eq!(s.line_of(8), 3);
    assert_eq!(s.line_of(999), 3);
}

#[test]
fn chunks_cover_ranges() {
    let model: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
    let s = Snapshot::from_bytes(&model);
    for (a, b) in [
        (0, 1000),
        (1, 999),
        (63, 65),
        (64, 128),
        (500, 500),
        (900, 2000),
    ] {
        let got: Vec<u8> = s.chunks(a..b).flatten().copied().collect();
        let b = b.min(1000);
        assert_eq!(got, &model[a as usize..b as usize]);
    }
    assert!(
        s.chunks(0..1000)
            .all(|c| !c.is_empty() && c.len() <= LEAF_MAX)
    );
}

#[test]
fn chunk_at_finds_the_leaf_of_every_offset() {
    let model: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
    let s = Snapshot::from_bytes(&model);
    for offset in 0..1000u64 {
        let (start, chunk) = s.chunk_at(offset).unwrap();
        assert!(start <= offset && offset < start + chunk.len() as u64);
        assert_eq!(chunk, &model[start as usize..start as usize + chunk.len()]);
    }
    assert!(s.chunk_at(1000).is_none());
    assert!(Snapshot::empty().chunk_at(0).is_none());
}

#[test]
fn apply_returns_new_revision_and_keeps_old() {
    let s0 = Snapshot::from_bytes(b"hello world");
    let tx = Transaction::new(vec![
        Edit::new(0..5, &b"HELLO"[..]),
        Edit::insert(11, &b"!"[..]),
    ]);
    let (s1, changes) = s0.apply(&tx).unwrap();
    assert_eq!(text(&s0), b"hello world");
    assert_eq!(text(&s1), b"HELLO world!");
    assert_eq!(s1.rev(), Rev(1));
    assert_eq!(changes.changes().len(), 2);
    assert!(!s1.same_tree(&s0));
    assert!(s0.same_tree(&s0.clone()));
}

#[test]
fn invalid_transactions_are_rejected() {
    let s = Snapshot::from_bytes(b"abcdef");
    let out_of_bounds = Transaction::single(Edit::delete(4..7));
    assert_eq!(
        s.apply(&out_of_bounds).unwrap_err(),
        EditError::OutOfBounds { index: 0, len: 6 }
    );
    let overlap = Transaction::new(vec![Edit::delete(0..3), Edit::delete(2..4)]);
    assert_eq!(
        s.apply(&overlap).unwrap_err(),
        EditError::Unordered { index: 1 }
    );
    let same_point = Transaction::new(vec![Edit::insert(2, &b"x"[..]), Edit::insert(2, &b"y"[..])]);
    assert_eq!(
        s.apply(&same_point).unwrap_err(),
        EditError::Unordered { index: 1 }
    );
    let adjacent = Transaction::new(vec![Edit::delete(0..2), Edit::insert(2, &b"x"[..])]);
    assert!(s.apply(&adjacent).is_ok());
}

#[test]
fn change_set_mapping() {
    let s = Snapshot::from_bytes(b"0123456789");
    // Replace 2..4 with "abc", insert "X" at 6, delete 8..9.
    let tx = Transaction::new(vec![
        Edit::new(2..4, &b"abc"[..]),
        Edit::insert(6, &b"X"[..]),
        Edit::delete(8..9),
    ]);
    let (after, map) = s.apply(&tx).unwrap();
    assert_eq!(text(&after), b"01abc45X679");
    assert_eq!(map.map(1, Bias::Left), 1);
    assert_eq!(map.map(2, Bias::Right), 2);
    assert_eq!(map.map(3, Bias::Left), 2);
    assert_eq!(map.map(3, Bias::Right), 5);
    assert_eq!(map.map(4, Bias::Left), 5);
    assert_eq!(map.map(6, Bias::Left), 7);
    assert_eq!(map.map(6, Bias::Right), 8);
    assert_eq!(map.map(8, Bias::Left), 10);
    assert_eq!(map.map(9, Bias::Left), 10);
    assert_eq!(map.map(10, Bias::Left), 11);
}

#[test]
fn line_endings() {
    assert_eq!(LineEnding::detect(b""), LineEnding::CrLf);
    assert_eq!(LineEnding::detect(b"a\nb\nc\r\n"), LineEnding::Lf);
    assert_eq!(LineEnding::detect(b"a\r\nb\r\nc\n"), LineEnding::CrLf);
    assert_eq!(LineEnding::detect(b"\n"), LineEnding::Lf);
}

#[test]
fn encoding_detection() {
    assert_eq!(Encoding::detect(b"\xEF\xBB\xBFx"), Encoding::Utf8Bom);
    assert_eq!(Encoding::detect(b"\xFF\xFEx\0"), Encoding::Utf16Le);
    assert_eq!(Encoding::detect(b"\xFE\xFF\0x"), Encoding::Utf16Be);
    assert_eq!(Encoding::detect(b"plain"), Encoding::Utf8);
    assert_eq!(Encoding::detect(b"\xEF\xBB"), Encoding::Utf8);
    for e in [
        Encoding::Utf8,
        Encoding::Utf8Bom,
        Encoding::Utf16Le,
        Encoding::Utf16Be,
    ] {
        assert_eq!(Encoding::detect(e.bom()), e);
    }
}

fn utf16(text: &str, little_endian: bool) -> Vec<u8> {
    text.encode_utf16()
        .flat_map(|u| {
            if little_endian {
                u.to_le_bytes()
            } else {
                u.to_be_bytes()
            }
        })
        .collect()
}

fn decode_in_parts(input: &[u8], little_endian: bool, part: usize) -> (Vec<u8>, bool) {
    let mut decoder = Utf16Decoder::new(little_endian);
    let mut out = Vec::new();
    for piece in input.chunks(part.max(1)) {
        decoder.decode(piece, &mut out);
    }
    let ok = decoder.finish(&mut out);
    (out, ok)
}

#[test]
fn utf16_round_trip_across_part_boundaries() {
    let sample = "a\u{e9}\u{4e2d}\u{1F600}\r\n\u{10FFFF}z";
    for le in [true, false] {
        let bytes = utf16(sample, le);
        for part in 1..=bytes.len() {
            let (decoded, ok) = decode_in_parts(&bytes, le, part);
            assert!(ok);
            assert_eq!(decoded, sample.as_bytes());
            let mut encoder = Utf16Encoder::new(le);
            let mut encoded = Vec::new();
            for piece in decoded.chunks(part) {
                encoder.encode(piece, &mut encoded).unwrap();
            }
            encoder.finish().unwrap();
            assert_eq!(encoded, bytes);
        }
    }
}

#[test]
fn malformed_utf16_is_reported() {
    for le in [true, false] {
        let unit = |u: u16| if le { u.to_le_bytes() } else { u.to_be_bytes() };
        let lone_high: Vec<u8> = [unit(0xD800), unit(0x0041)].concat();
        let lone_low: Vec<u8> = [unit(0xDC00)].concat();
        let trailing_high: Vec<u8> = [unit(0x0041), unit(0xD83D)].concat();
        let mut odd = unit(0x0041).to_vec();
        odd.push(0x42);
        let mut high_then_odd = unit(0xD800).to_vec();
        high_then_odd.push(0x41);
        for (input, expected) in [
            (lone_high, "\u{FFFD}A"),
            (lone_low, "\u{FFFD}"),
            (trailing_high, "A\u{FFFD}"),
            (odd, "A\u{FFFD}"),
            (high_then_odd, "\u{FFFD}\u{FFFD}"),
        ] {
            for part in 1..=input.len() {
                let (out, ok) = decode_in_parts(&input, le, part);
                assert!(!ok);
                assert_eq!(out, expected.as_bytes());
            }
        }
    }
}

#[test]
fn utf16_encoder_rejects_invalid_utf8() {
    let mut encoder = Utf16Encoder::new(true);
    assert_eq!(encoder.encode(b"a\xFFb", &mut Vec::new()), Err(InvalidUtf8));
    let mut truncated = Utf16Encoder::new(true);
    truncated.encode(b"a\xE4\xB8", &mut Vec::new()).unwrap();
    assert_eq!(truncated.finish(), Err(InvalidUtf8));
}

#[test]
fn large_insert_spans_leaves() {
    let big: Vec<u8> = (0..10 * LEAF_MAX)
        .map(|i| if i % 9 == 0 { b'\n' } else { b'x' })
        .collect();
    let s = Snapshot::from_bytes(b"start end");
    let (after, _) = s
        .apply(&Transaction::single(Edit::insert(6, big.clone())))
        .unwrap();
    let mut model = b"start ".to_vec();
    model.extend_from_slice(&big);
    model.extend_from_slice(b"end");
    check(&after, &model);
}

#[test]
fn sequential_typing_coalesces_leaves() {
    let mut s = Snapshot::from_bytes(&[b'a'; 10 * LEAF_MAX]);
    let mut model = vec![b'a'; 10 * LEAF_MAX];
    let at = 5 * LEAF_MAX + 3;
    for i in 0..500usize {
        let byte = [b'0' + (i % 10) as u8];
        s = s
            .apply(&Transaction::single(Edit::insert(
                (at + i) as u64,
                &byte[..],
            )))
            .unwrap()
            .0;
        model.insert(at + i, byte[0]);
    }
    check(&s, &model);
    let leaves = tree::check_invariants(s.root());
    // Without coalescing every keystroke would add a leaf.
    assert!(leaves < 10 + 500 / 4, "leaves: {leaves}");
}

const ALPHABET: &[u8] = b"ab \n\r\xC3\xA9";

fn bytes(max: usize) -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(prop::sample::select(ALPHABET), 0..max)
}

#[derive(Clone, Debug)]
enum Op {
    Replace {
        at: prop::sample::Index,
        len: usize,
        text: Vec<u8>,
    },
    Multi {
        points: Vec<prop::sample::Index>,
        text: Vec<u8>,
    },
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (any::<prop::sample::Index>(), 0..40usize, bytes(40)).prop_map(|(at, len, text)| Op::Replace { at, len, text }),
        1 => (any::<prop::sample::Index>(), 0..20usize, bytes(400)).prop_map(|(at, len, text)| Op::Replace { at, len, text }),
        1 => (prop::collection::vec(any::<prop::sample::Index>(), 1..8), bytes(5)).prop_map(|(points, text)| Op::Multi { points, text }),
    ]
}

fn to_transaction(op: &Op, len: usize) -> Transaction {
    match op {
        Op::Replace { at, len: n, text } => {
            let start = at.index(len + 1);
            let end = (start + n).min(len);
            Transaction::single(Edit::new(start as u64..end as u64, text.clone()))
        }
        Op::Multi { points, text } => {
            let mut starts: Vec<usize> = points.iter().map(|p| p.index(len + 1)).collect();
            starts.sort_unstable();
            starts.dedup();
            // Each caret replaces one byte when available, like overtyping.
            let mut edits = Vec::new();
            let mut prev_end = 0;
            for s in starts {
                if s < prev_end {
                    continue;
                }
                let end = (s + 1).min(len);
                edits.push(Edit::new(s as u64..end as u64, text.clone()));
                prev_end = end.max(s + 1);
            }
            Transaction::new(edits)
        }
    }
}

fn apply_model(model: &[u8], tx: &Transaction) -> Vec<u8> {
    let mut out = model.to_vec();
    for e in tx.edits().iter().rev() {
        out.splice(
            e.range.start as usize..e.range.end as usize,
            e.text.iter().copied(),
        );
    }
    out
}

fn initial() -> impl Strategy<Value = (Vec<u8>, Vec<usize>)> {
    (bytes(600), prop::collection::vec(1..200usize, 0..6))
}

fn build_initial(model: &[u8], cuts: &[usize]) -> Snapshot {
    let mut builder = TreeBuilder::new();
    let mut rest = model;
    for &cut in cuts {
        let (block, tail) = rest.split_at(cut.min(rest.len()));
        builder.push_block(Arc::from(block));
        rest = tail;
    }
    builder.push_block(Arc::from(rest));
    builder.finish()
}

proptest! {
    #[test]
    fn batch_apply_matches_model(model in bytes(800), points in prop::collection::vec((any::<prop::sample::Index>(), 0..3usize, bytes(6)), 1..30)) {
        let s = Snapshot::from_bytes(&model);
        let mut spots: Vec<(usize, usize, Vec<u8>)> = points
            .iter()
            .map(|(at, len, text)| {
                let start = at.index(model.len() + 1);
                (start, (start + len).min(model.len()), text.clone())
            })
            .collect();
        spots.sort_by_key(|(start, _, _)| *start);
        let mut edits = Vec::new();
        let mut prev_end = 0;
        for (start, end, text) in spots {
            let duplicate_insertion = edits.last().is_some_and(|e: &Edit| e.range.is_empty() && start == end && e.range.start == start as u64);
            if start < prev_end || duplicate_insertion {
                continue;
            }
            edits.push(Edit::new(start as u64..end as u64, text));
            prev_end = end;
        }
        let tx = Transaction::new(edits);
        let expected = apply_model(&model, &tx);
        if let Some(root) = tree::apply_batch(s.root_arc(), tx.edits()) {
            let batched = Snapshot::from_root(root, Rev(1));
            check(&batched, &expected);
        }
        check(&s.apply(&tx).unwrap().0, &expected);
    }
}

#[test]
fn batch_apply_covers_local_edits() {
    let model: Vec<u8> = (0..2000u32).map(|i| b"ab\n"[(i % 3) as usize]).collect();
    let s = Snapshot::from_bytes(&model);
    let edits: Vec<Edit> = (0..200u64)
        .map(|i| Edit::insert(i * 10 + 1, &b"Z"[..]))
        .collect();
    let tx = Transaction::new(edits);
    let root = tree::apply_batch(s.root_arc(), tx.edits()).expect("all edits are inside leaves");
    check(
        &Snapshot::from_root(root, Rev(1)),
        &apply_model(&model, &tx),
    );
}

proptest! {
    #[test]
    fn matches_model((model, cuts) in initial(), ops in prop::collection::vec(op(), 1..40), probes in prop::collection::vec(any::<prop::sample::Index>(), 8)) {
        let mut model = model;
        let mut s = build_initial(&model, &cuts);
        check(&s, &model);
        let mut history = vec![(s.clone(), model.clone())];
        for op in &ops {
            let tx = to_transaction(op, model.len());
            let (next, _) = s.apply(&tx).unwrap();
            model = apply_model(&model, &tx);
            s = next;
            check(&s, &model);
            for p in &probes {
                let offset = p.index(model.len() + 2);
                prop_assert_eq!(s.line_of(offset as u64), model_line_of(&model, offset));
            }
            history.push((s.clone(), model.clone()));
        }
        // Old snapshots are unaffected by later edits.
        for (snapshot, expected) in &history {
            prop_assert_eq!(text(snapshot), expected.clone());
        }
    }

    #[test]
    fn change_set_maps_unchanged_bytes(model in bytes(300), op in op()) {
        let s = Snapshot::from_bytes(&model);
        let tx = to_transaction(&op, model.len());
        let (after, map) = s.apply(&tx).unwrap();
        let new = text(&after);
        let edited = |p: u64| tx.edits().iter().any(|e| e.range.start <= p && p < e.range.end);
        for p in 0..model.len() as u64 {
            if edited(p) {
                continue;
            }
            // A byte outside every edit keeps its value at its right-biased position.
            let q = map.map(p, Bias::Right);
            let r = map.map(p, Bias::Left);
            prop_assert!(r <= q);
            prop_assert_eq!(new[q as usize], model[p as usize]);
        }
        prop_assert_eq!(map.map(model.len() as u64, Bias::Right), new.len() as u64);
        prop_assert_eq!(map.map(0, Bias::Left), 0);
    }
}
