//! Production tree limits on multi-megabyte documents.

use std::sync::Arc;

use rte_text::{Edit, Snapshot, Transaction, TreeBuilder};

fn lcg(seed: &mut u64) -> u64 {
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *seed >> 33
}

#[test]
fn random_edits_on_large_document() {
    let mut seed = 7;
    let mut model: Vec<u8> = (0..6 * 1024 * 1024)
        .map(|_| b"abc \n\r"[(lcg(&mut seed) % 6) as usize])
        .collect();
    let mut builder = TreeBuilder::new();
    builder.push_block(Arc::from(&model[..1024 * 1024]));
    builder.push_block(Arc::from(&model[1024 * 1024..]));
    let mut s = builder.finish();
    let original = s.clone();
    let original_model = model.clone();

    for _ in 0..2000 {
        let len = model.len() as u64;
        let start = lcg(&mut seed) % (len + 1);
        let end = (start + lcg(&mut seed) % 64).min(len);
        let text: Vec<u8> = (0..lcg(&mut seed) % 3000)
            .map(|i| b"xy\n"[(i % 3) as usize])
            .collect();
        let tx = Transaction::single(Edit::new(start..end, text.clone()));
        s = s.apply(&tx).unwrap().0;
        model.splice(start as usize..end as usize, text);
    }

    assert_eq!(s.to_vec(0..s.len()), model);
    let newlines: Vec<u64> = model
        .iter()
        .enumerate()
        .filter(|(_, b)| **b == b'\n')
        .map(|(i, _)| i as u64)
        .collect();
    assert_eq!(s.newlines(), newlines.len() as u64);
    for k in (0..newlines.len()).step_by(997) {
        assert_eq!(s.line_start(k as u64 + 1), Some(newlines[k] + 1));
        assert_eq!(s.line_of(newlines[k] + 1), k as u64 + 1);
    }
    assert_eq!(original.to_vec(0..original.len()), original_model);
}

#[test]
fn many_carets_in_one_transaction() {
    let model = vec![b'a'; 1024 * 1024];
    let s = Snapshot::from_bytes(&model);
    let edits: Vec<Edit> = (0..10_000u64)
        .map(|i| Edit::insert(i * 100, &b"\n"[..]))
        .collect();
    let (after, map) = s.apply(&Transaction::new(edits)).unwrap();
    assert_eq!(after.len(), model.len() as u64 + 10_000);
    assert_eq!(after.newlines(), 10_000);
    assert_eq!(map.map(150, rte_text::Bias::Left), 152);
}
