//! Text engine benchmarks with Ropey as the baseline. Run: `cargo bench -p rte_text`.

use std::sync::{Arc, LazyLock};

use divan::{Bencher, black_box};
use rte_text::{Edit, Snapshot, Transaction, TreeBuilder};

const DOC_BYTES: usize = 64 * 1024 * 1024;
const BLOCK: usize = 16 * 1024 * 1024;

/// ASCII code-like lines, so Ropey accepts the same bytes.
static DOC: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let line = b"    let value = compute(index, &table[offset..]); // comment\n";
    line.iter().copied().cycle().take(DOC_BYTES).collect()
});

static SNAPSHOT: LazyLock<Snapshot> = LazyLock::new(|| build(&DOC));
static ROPE: LazyLock<ropey::Rope> =
    LazyLock::new(|| ropey::Rope::from_str(std::str::from_utf8(&DOC).unwrap()));

fn build(doc: &[u8]) -> Snapshot {
    let mut builder = TreeBuilder::new();
    for block in doc.chunks(BLOCK) {
        builder.push_block(Arc::from(block));
    }
    builder.finish()
}

/// Deterministic pseudo-random offsets.
fn offsets(count: usize, len: usize) -> Vec<usize> {
    let mut x = 0x2545_F491_4F6C_DD1Du64;
    (0..count)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x % len as u64) as usize
        })
        .collect()
}

fn main() {
    divan::main();
}

mod load {
    use super::*;

    #[divan::bench]
    fn rte_text_blocks() -> Snapshot {
        build(black_box(&DOC))
    }

    #[divan::bench]
    fn ropey_from_str() -> ropey::Rope {
        ropey::Rope::from_str(std::str::from_utf8(black_box(&DOC)).unwrap())
    }
}

mod edits {
    use super::*;

    const EDITS: usize = 1000;

    #[divan::bench]
    fn rte_text_scattered_inserts(bencher: Bencher) {
        let at = offsets(EDITS, DOC_BYTES);
        bencher.bench(|| {
            let mut s = SNAPSHOT.clone();
            for &o in &at {
                s = s
                    .apply(&Transaction::single(Edit::insert(o as u64, &b"x"[..])))
                    .unwrap()
                    .0;
            }
            s
        });
    }

    #[divan::bench]
    fn ropey_scattered_inserts(bencher: Bencher) {
        let at = offsets(EDITS, DOC_BYTES);
        bencher.bench(|| {
            let mut rope = ROPE.clone();
            for &o in &at {
                rope.insert(rope.byte_to_char(o), "x");
            }
            rope
        });
    }

    #[divan::bench]
    fn rte_text_typing_one_place(bencher: Bencher) {
        let start = DOC_BYTES / 2;
        bencher.bench(|| {
            let mut s = SNAPSHOT.clone();
            for i in 0..EDITS {
                s = s
                    .apply(&Transaction::single(Edit::insert(
                        (start + i) as u64,
                        &b"x"[..],
                    )))
                    .unwrap()
                    .0;
            }
            s
        });
    }

    #[divan::bench]
    fn rte_text_10k_carets(bencher: Bencher) {
        let mut at = offsets(10_000, DOC_BYTES);
        at.sort_unstable();
        at.dedup();
        let tx = Transaction::new(
            at.iter()
                .map(|&o| Edit::insert(o as u64, &b"x"[..]))
                .collect(),
        );
        bencher.bench(|| SNAPSHOT.apply(black_box(&tx)).unwrap());
    }

    #[divan::bench]
    fn ropey_10k_carets(bencher: Bencher) {
        let mut at = offsets(10_000, DOC_BYTES);
        at.sort_unstable();
        at.dedup();
        bencher.bench(|| {
            let mut rope = ROPE.clone();
            for &o in at.iter().rev() {
                rope.insert(rope.byte_to_char(o), "x");
            }
            rope
        });
    }
}

mod queries {
    use super::*;

    const QUERIES: usize = 10_000;

    #[divan::bench]
    fn rte_text_line_start(bencher: Bencher) {
        let lines = offsets(QUERIES, SNAPSHOT.newlines() as usize);
        bencher.bench(|| {
            lines
                .iter()
                .map(|&l| SNAPSHOT.line_start(l as u64).unwrap())
                .sum::<u64>()
        });
    }

    #[divan::bench]
    fn ropey_line_to_byte(bencher: Bencher) {
        let lines = offsets(QUERIES, ROPE.len_lines() - 1);
        bencher.bench(|| lines.iter().map(|&l| ROPE.line_to_byte(l)).sum::<usize>());
    }

    #[divan::bench]
    fn rte_text_line_of(bencher: Bencher) {
        let at = offsets(QUERIES, DOC_BYTES);
        bencher.bench(|| at.iter().map(|&o| SNAPSHOT.line_of(o as u64)).sum::<u64>());
    }

    #[divan::bench]
    fn ropey_byte_to_line(bencher: Bencher) {
        let at = offsets(QUERIES, DOC_BYTES);
        bencher.bench(|| at.iter().map(|&o| ROPE.byte_to_line(o)).sum::<usize>());
    }

    #[divan::bench]
    fn rte_text_viewport_100_lines(bencher: Bencher) {
        let line = SNAPSHOT.newlines() / 2;
        bencher.bench(|| {
            let start = SNAPSHOT.line_start(line).unwrap();
            let end = SNAPSHOT.line_start(line + 100).unwrap();
            SNAPSHOT.chunks(start..end).map(<[u8]>::len).sum::<usize>()
        });
    }
}
