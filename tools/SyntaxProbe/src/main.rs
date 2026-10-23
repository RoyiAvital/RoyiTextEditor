//! Measures tree-sitter parse time and syntax-tree memory on generated files
//! (the size-limit table in Plan/Architecture.md). Not part of the build.
//!
//! cargo run --release --manifest-path tools/SyntaxProbe/Cargo.toml -- <kind> <MiB>
//!
//! `kind`: json, json-min, py, c, md. `md` also parses the injection layers
//! (inline grammar, fenced code); `SEPARATE=1` gives each region its own tree
//! instead of one combined tree.

use std::time::Instant;
use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

const MIB: usize = 1 << 20;

fn mem() -> (f64, f64) {
    let mut c: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    c.cb = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    (c.PagefileUsage as f64 / MIB as f64, c.PeakPagefileUsage as f64 / MIB as f64)
}

fn repeat(prefix: &str, block: &str, size: usize) -> Vec<u8> {
    let mut out = prefix.as_bytes().to_vec();
    for n in 0.. {
        let next = block.replace("$N", &n.to_string());
        if out.len() + next.len() > size {
            break;
        }
        out.extend_from_slice(next.as_bytes());
    }
    out
}

const PY: &str = include_str!("py.txt");
const C: &str = include_str!("c.txt");
const MD: &str = include_str!("md.txt");
const JSON_ITEM: &str = r#"{"id": $N, "name": "item \"$N\"\n", "price": 12.5e2, "tags": ["a", "b"], "active": true, "parent": null, "nested": {"depth": [1, 2, {"x": -0.5}]}}"#;

fn json(size: usize, compact: bool) -> Vec<u8> {
    let mut out = b"[".to_vec();
    for n in 0.. {
        let mut t = JSON_ITEM.replace("$N", &n.to_string());
        if compact {
            t = t.replace(": ", ":").replace(", ", ",");
        } else {
            t = format!("\n  {t}");
        }
        if out.len() + t.len() + 3 > size {
            break;
        }
        if n > 0 {
            out.push(b',');
        }
        out.extend_from_slice(t.as_bytes());
    }
    out.extend_from_slice(b"\n]\n");
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let kind = args[1].as_str();
    let size = args[2].parse::<usize>().unwrap() * MIB;
    let (bytes, lang): (Vec<u8>, tree_sitter::Language) = match kind {
        "json" => (json(size, false), tree_sitter_json::LANGUAGE.into()),
        "json-min" => (json(size, true), tree_sitter_json::LANGUAGE.into()),
        "py" => (repeat("", PY, size), tree_sitter_python::LANGUAGE.into()),
        "c" => (repeat("", C, size), tree_sitter_c::LANGUAGE.into()),
        "md" => (repeat("", MD, size), tree_sitter_md::LANGUAGE.into()),
        _ => panic!("kind"),
    };
    let (before, _) = mem();
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&lang).unwrap();
    let t = Instant::now();
    let tree = parser.parse(&bytes, None).unwrap();
    let parse = t.elapsed().as_secs_f64();
    let (after, peak) = mem();
    let mut cursor = tree.walk();
    let mut nodes = 0u64;
    'walk: loop {
        nodes += 1;
        if cursor.goto_first_child() || cursor.goto_next_sibling() {
            continue;
        }
        loop {
            if !cursor.goto_parent() {
                break 'walk;
            }
            if cursor.goto_next_sibling() {
                break;
            }
        }
    }
    let file = bytes.len() as f64 / MIB as f64;
    if kind == "md" {
        // Injection layers: inline grammar over all inline nodes, code grammars over fences.
        let mut inline = Vec::new();
        let mut code: [(Vec<tree_sitter::Range>, tree_sitter::Language); 3] = [
            (Vec::new(), tree_sitter_python::LANGUAGE.into()),
            (Vec::new(), tree_sitter_c::LANGUAGE.into()),
            (Vec::new(), tree_sitter_json::LANGUAGE.into()),
        ];
        let mut c = tree.walk();
        let mut lang_of_fence = 0usize;
        'w: loop {
            let n = c.node();
            match n.kind() {
                "inline" => inline.push(n.range()),
                "language" => {
                    lang_of_fence = match &bytes[n.byte_range()] {
                        b"python" => 0,
                        b"c" => 1,
                        _ => 2,
                    }
                }
                "code_fence_content" => code[lang_of_fence].0.push(n.range()),
                _ => {}
            }
            if c.goto_first_child() || c.goto_next_sibling() {
                continue;
            }
            loop {
                if !c.goto_parent() {
                    break 'w;
                }
                if c.goto_next_sibling() {
                    break;
                }
            }
        }
        let mut layers = vec![(inline, tree_sitter::Language::from(tree_sitter_md::INLINE_LANGUAGE))];
        layers.extend(code.into_iter());
        let t = Instant::now();
        let (base, _) = mem();
        let mut keep = Vec::new();
        for (ranges, lang) in layers {
            let lt = Instant::now();
            let count = ranges.len();
            let mut p = tree_sitter::Parser::new();
            p.set_language(&lang).unwrap();
            if std::env::var("SEPARATE").is_ok() {
                for r in &ranges {
                    p.set_included_ranges(std::slice::from_ref(r)).unwrap();
                    keep.push(p.parse(&bytes, None).unwrap());
                }
            } else {
                p.set_included_ranges(&ranges).unwrap();
                keep.push(p.parse(&bytes, None).unwrap());
            }
            println!("  layer ranges {count:>8}  parse {:7.2} s", lt.elapsed().as_secs_f64());
        }
        let (all, peak) = mem();
        println!(
            "md layers: parse {:6.2} s, layer trees {:8.1} MiB ({:5.1} B/B), block+layers {:8.1} MiB ({:5.1} B/B), peak {peak:8.1} MiB",
            t.elapsed().as_secs_f64(),
            all - base,
            (all - base) / file,
            all - before,
            (all - before) / file
        );
    }
    println!(
        "{kind:<9} file {file:7.1} MiB  parse {parse:6.2} s ({:5.1} MiB/s)  tree {:8.1} MiB ({:5.1} B/B)  peak {peak:8.1} MiB  nodes {nodes} ({:.1} B/node) error={}",
        file / parse,
        after - before,
        (after - before) / file,
        (after - before) * MIB as f64 / nodes as f64,
        tree.root_node().has_error()
    );
}
