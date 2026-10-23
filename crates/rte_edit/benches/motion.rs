//! Worst-case caret movement latency (one keystroke, before layout and
//! drawing). Run: `cargo bench -p rte_edit`.

use divan::{Bencher, black_box};
use rte_edit::{column_at, next_boundary, prev_boundary, vertical, word_at, word_left, word_right};
use rte_text::Snapshot;

fn main() {
    divan::main();
}

const LONG: usize = 4 << 20;

fn word() -> Snapshot {
    Snapshot::from_bytes(&vec![b'a'; LONG])
}

#[divan::bench]
fn word_right_in_4mib_word(bencher: Bencher) {
    let s = word();
    bencher.bench_local(|| word_right(&s, black_box(0)));
}

#[divan::bench]
fn word_left_in_4mib_word(bencher: Bencher) {
    let s = word();
    bencher.bench_local(|| word_left(&s, black_box(s.len())));
}

#[divan::bench]
fn word_at_in_4mib_word(bencher: Bencher) {
    let s = word();
    bencher.bench_local(|| word_at(&s, black_box(LONG as u64 / 2)));
}

#[divan::bench]
fn word_right_over_4mib_whitespace(bencher: Bencher) {
    let s = Snapshot::from_bytes(&vec![b' '; LONG]);
    bencher.bench_local(|| word_right(&s, black_box(0)));
}

#[divan::bench]
fn right_and_left_over_200k_marks(bencher: Bencher) {
    let mut text = "e".to_owned();
    text.push_str(&"\u{301}".repeat(200_000));
    let s = Snapshot::from_bytes(text.as_bytes());
    bencher.bench_local(|| {
        (
            next_boundary(&s, black_box(0)),
            prev_boundary(&s, black_box(s.len())),
        )
    });
}

#[divan::bench]
fn column_near_start_of_4mib_line(bencher: Bencher) {
    let s = word();
    bencher.bench_local(|| column_at(&s, black_box(10), 4));
}

#[divan::bench]
fn down_between_4mib_lines(bencher: Bencher) {
    let mut text = vec![b'a'; LONG];
    text.push(b'\n');
    text.extend(std::iter::repeat_n(b'b', LONG));
    let s = Snapshot::from_bytes(&text);
    bencher.bench_local(|| vertical(&s, black_box(70_000), None, 1, 4));
}
