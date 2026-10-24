// Temporary profiling probe (removed after the investigation).
#![allow(unsafe_code, unused_variables)]
use std::time::Instant;

use rte_text::Snapshot;

use crate::by_id;
use crate::session::{Painter, Session, Work};

const SAMPLES: &[(&str, &str)] = &[
    ("bash", "#!/bin/bash\n# comment $N\nfunction build_$N() {\n  local dir=\"${1:-/tmp}\"\n  for f in \"$dir\"/*.txt; do\n    if [[ -f \"$f\" && $count -gt 3 ]]; then\n      echo \"file: $f\" | grep -E '^[a-z]+' > /dev/null\n    fi\n  done\n  return 0\n}\n\n"),
    ("c", "#include <stdio.h>\n/* Block comment $N */\nstatic int sum_$N(const int *v, size_t n) {\n    int total = 0; // line\n    for (size_t k = 0; k < n; ++k) { total += v[k] * 3; }\n    if (total > 10) { printf(\"total=%d\\n\", total); }\n    return total;\n}\n\n"),
    ("cpp", "#include <vector>\nnamespace ns$N {\ntemplate <typename T>\nclass Box$N : public Base {\npublic:\n    explicit Box$N(T v) : value_(std::move(v)) {}\n    auto get() const -> const T& { return value_; } // getter\nprivate:\n    T value_{};\n};\n}\n\n"),
    ("csharp", "using System;\nnamespace App$N {\n    /// <summary>Doc</summary>\n    public sealed class Item$N : IItem {\n        public int Id { get; init; } = $N;\n        public async Task<string?> LoadAsync(string path) {\n            var text = await File.ReadAllTextAsync(path);\n            return text.Length > 3 ? $\"{text[..3]} {Id}\" : null; // c\n        }\n    }\n}\n\n"),
    ("css", "/* comment $N */\n.card-$N > .title:hover,\n#main a[href^=\"http\"] {\n  color: #1e90ff;\n  margin: 0 auto 1.5rem;\n  font: 600 14px/1.4 \"Segoe UI\", sans-serif;\n  transition: transform 0.2s ease-in-out;\n}\n@media (max-width: 600px) { .card-$N { display: none !important; } }\n\n"),
    ("go", "package main\n\n// Item$N is a type.\ntype Item$N struct {\n\tName string `json:\"name\"`\n\tSize int\n}\n\nfunc (i *Item$N) Load(path string) (string, error) {\n\tdata, err := os.ReadFile(path)\n\tif err != nil {\n\t\treturn \"\", fmt.Errorf(\"load: %w\", err)\n\t}\n\treturn string(data[:3]), nil\n}\n\n"),
    ("html", "<!-- comment $N -->\n<section class=\"card\" id=\"s$N\">\n  <h2 title=\"Heading\">Title &amp; more</h2>\n  <p>Text with <a href=\"https://example.com/$N\">a link</a> and <b>bold</b>.</p>\n  <ul>\n    <li><input type=\"checkbox\" checked> item</li>\n  </ul>\n</section>\n\n"),
    ("java", "package app;\n\n/** Doc $N. */\n@Deprecated\npublic final class Item$N implements Comparable<Item$N> {\n    private final int id = $N;\n    @Override\n    public int compareTo(Item$N other) {\n        // comment\n        return Integer.compare(id, other.id) + \"x\".length();\n    }\n}\n\n"),
    ("javascript", "import { readFile } from \"node:fs/promises\";\n\n/** Doc $N */\nexport class Point$N extends Base {\n  #x = 0;\n  constructor(x, y = 1.5e-3) { super(); this.#x = x; this.y = y; }\n  get norm() { return Math.sqrt(this.#x ** 2 + this.y ** 2); } // c\n}\n\nexport async function load$N(path, ...rest) {\n  const text = await readFile(path, \"utf8\");\n  return /^\\d+$/.test(text) ? `${text.slice(0, 3)} ${rest.length}` : null;\n}\n\n"),
    ("json", "  {\"id\": $N, \"name\": \"item \\\"$N\\\"\", \"price\": 12.5e2, \"tags\": [\"a\", \"b\"],\n   \"active\": true, \"parent\": null, \"nested\": {\"depth\": [1, 2, {\"x\": -0.5}]}},\n"),
    ("julia", "# comment $N\nfunction norm_$N(p::Point{T}; scale::Real = 2) where {T<:Real}\n    s = sum(x -> x^2, p.coords)\n    msg = \"value $(round(s, digits=3))\"\n    return s > 10 ? sqrt(s) * scale : nothing\nend\n\nstruct Point$N{T}\n    coords::Vector{T}\nend\n\n"),
    ("lua", "-- comment $N\nlocal Point$N = {}\nPoint$N.__index = Point$N\n\nfunction Point$N.new(x, y)\n  local self = setmetatable({}, Point$N)\n  self.x, self.y = x or 0, y or 1.5\n  return self\nend\n\nfunction Point$N:norm() return math.sqrt(self.x ^ 2 + self.y ^ 2) end\n\n"),
    ("markdown", "# Heading $N\n\nSome *emphasis*, **strong**, `code` and a [link](https://example.com/$N).\n\n- item one\n- item two\n\n```python\ndef f(x):\n    return x * 2\n```\n\n> Quote with `inline` code.\n\n"),
    ("matlab", "function y = norm_$N(x, scale)\n% NORM comment $N\n    if nargin < 2\n        scale = 2;\n    end\n    s = sum(x .^ 2);\n    msg = sprintf('value %.3f', s);\n    y = sqrt(s) * scale;\nend\n\n"),
    ("octave", "function y = norm_$N(x, scale)\n# NORM comment $N\n    if nargin < 2\n        scale = 2;\n    endif\n    s = sum(x .^ 2);\n    msg = sprintf(\"value %.3f\", s);\n    y = sqrt(s) * scale;\nendfunction\n\n"),
    ("powershell", "# comment $N\nfunction Get-Item$N {\n    [CmdletBinding()]\n    param([Parameter(Mandatory)][string]$Path, [int]$Count = 3)\n    $items = Get-ChildItem -Path $Path -Filter '*.txt' | Where-Object { $_.Length -gt 0 }\n    foreach ($i in $items) { Write-Host \"item: $($i.Name)\" }\n    return $items.Count\n}\n\n"),
    ("python", "@dataclass(frozen=True)\nclass Point$N(Base):\n    \"\"\"Docstring.\"\"\"\n    x: float = 1.5e-3\n\n    def norm(self, scale: float = 2) -> float:\n        # comment $N\n        value = (self.x ** 2) ** 0.5\n        return f\"{value:.3f}\" if value > 10 else None\n\n"),
    ("rust", "/// Doc $N.\n#[derive(Debug, Clone)]\npub struct Point$N<T: Copy> {\n    x: T,\n    y: f64,\n}\n\nimpl<T: Copy + Into<f64>> Point$N<T> {\n    pub fn norm(&self) -> f64 {\n        let x: f64 = self.x.into(); // comment\n        (x * x + self.y.powi(2)).sqrt()\n    }\n}\n\n"),
    ("sql", "-- comment $N\nSELECT u.id, u.name, COUNT(o.id) AS orders_$N\nFROM users AS u\nLEFT JOIN orders o ON o.user_id = u.id AND o.total > 10.5\nWHERE u.created_at >= '2026-01-01' AND u.name LIKE 'A%'\nGROUP BY u.id, u.name\nHAVING COUNT(o.id) > 3\nORDER BY orders_$N DESC;\n\n"),
    ("toml", "# comment $N\n[server_$N]\nhost = \"localhost\"\nport = 8080\nratio = 1.5e-3\nenabled = true\ntags = [\"a\", \"b\"]\ncreated = 2026-10-24T10:00:00Z\n\n[server_$N.limits]\nmax = { soft = 10, hard = 20 }\n\n"),
    ("tsx", "import React from \"react\";\n\ninterface Props$N { title: string; count?: number }\n\nexport function Card$N({ title, count = 0 }: Props$N): JSX.Element {\n  const [open, setOpen] = React.useState<boolean>(false);\n  return (\n    <div className=\"card\" onClick={() => setOpen(!open)}>\n      <h2>{title}</h2> {count > 0 && <span>{`${count}`}</span>}\n    </div>\n  );\n}\n\n"),
    ("typescript", "/** Shape $N */\nexport interface Shape$N<T extends object = {}> {\n  readonly id: number;\n  meta: Record<string, T>;\n}\n\nexport class Point$N implements Shape$N {\n  readonly id = $N;\n  meta: Record<string, object> = {};\n  constructor(private x: number, public y = 1.5e-3) {}\n  get norm(): number { return Math.sqrt(this.x ** 2 + this.y ** 2); } // c\n}\n\n"),
    ("xml", "<!-- comment $N -->\n<item id=\"$N\" type=\"book\">\n  <title lang=\"en\">Title &amp; more</title>\n  <price currency=\"USD\">12.50</price>\n  <tags><tag>a</tag><tag>b</tag></tags>\n  <![CDATA[raw <text>]]>\n</item>\n\n"),
    ("yaml", "# comment $N\nservice_$N:\n  image: \"nginx:1.25\"\n  ports:\n    - \"8080:80\"\n  environment:\n    DEBUG: true\n    RATIO: 1.5e-3\n  command: >\n    run --flag\n\n"),
];

fn text_of(lang: &str, block: &str, lines: usize) -> String {
    let per = block.matches('\n').count();
    let mut out = String::new();
    match lang {
        "json" => out.push_str("[\n"),
        "xml" => out.push_str("<?xml version=\"1.0\"?>\n<root>\n"),
        _ => {}
    }
    for n in 0..lines / per {
        out.push_str(&block.replace("$N", &n.to_string()));
    }
    match lang {
        "json" => out.push_str("  {}\n]\n"),
        "xml" => out.push_str("</root>\n"),
        _ => {}
    }
    out
}

#[test]
#[ignore]
fn probe_languages() {
    eprintln!("lang | KiB | query ms | parse ms | errors | first screen paint ms");
    for (lang, block) in SAMPLES {
        let text = text_of(lang, block, 10_000);
        let snapshot = Snapshot::from_bytes(text.as_bytes());
        let l = by_id(lang).unwrap();
        let t = Instant::now();
        let hl = crate::highlighter(l.grammar).unwrap();
        let query = t.elapsed().as_secs_f64() * 1000.0;
        let mut parse = Vec::new();
        let mut errors = false;
        for _ in 0..3 {
            let mut parser = tree_sitter::Parser::new();
            parser.set_language(&hl.language).unwrap();
            let t = Instant::now();
            let tree = parser.parse(text.as_bytes(), None).unwrap();
            parse.push(t.elapsed().as_secs_f64() * 1000.0);
            errors = tree.root_node().has_error();
        }
        parse.sort_by(f64::total_cmp);
        let mut session = Session::new(l, snapshot.clone(), true).unwrap();
        session.set_interest(0..4000);
        while !session.is_complete() || session.work(&|| false) != Work::Idle {
            session.work(&|| false);
        }
        let view = session.view();
        let screen = snapshot.line_start(60).unwrap_or(snapshot.len());
        let mut out = Vec::new();
        let t = Instant::now();
        Painter::default().paint(&view, &snapshot, 0..screen, None, &mut out);
        let paint = t.elapsed().as_secs_f64() * 1000.0;
        eprintln!(
            "{lang} | {} | {query:.1} | {:.1} | {errors} | {paint:.2}",
            text.len() / 1024,
            parse[1],
        );
    }
}

#[test]
#[ignore]
fn probe_query_cold() {
    let lang = std::env::var("PROBE_LANG").unwrap();
    let l = by_id(&lang).unwrap();
    let t = Instant::now();
    let language = l.grammar.language();
    let load = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    let _ = tree_sitter::Query::new(&language, crate::highlight::highlights_source(l.grammar))
        .unwrap();
    let query = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    let _ = crate::highlight::tags_source(l.grammar)
        .map(|s| tree_sitter::Query::new(&language, s).unwrap());
    let tags = t.elapsed().as_secs_f64() * 1000.0;
    eprintln!("COLD {lang} | {load:.2} | {query:.1} | {tags:.1}");
}

/// Blocks of `lang` (10,000 lines) split into `parts` texts at block boundaries.
fn chunks(lang: &str, block: &str, parts: usize) -> Vec<String> {
    let per = block.matches('\n').count();
    let blocks = 10_000 / per;
    (0..parts)
        .map(|p| {
            let mut out = String::new();
            for n in blocks * p / parts..blocks * (p + 1) / parts {
                out.push_str(&block.replace("$N", &n.to_string()));
            }
            out
        })
        .collect()
}

#[test]
#[ignore]
fn probe_parallel() {
    eprintln!("lang | 1 thread ms | 4 threads ms | 8 threads ms | query+parse parallel ms");
    for (lang, block) in SAMPLES {
        let l = by_id(lang).unwrap();
        let hl = crate::highlighter(l.grammar).unwrap();
        let mut row = Vec::new();
        for parts in [1usize, 4, 8] {
            let texts = chunks(lang, block, parts);
            let mut best = f64::MAX;
            for _ in 0..3 {
                let t = Instant::now();
                std::thread::scope(|s| {
                    for text in &texts {
                        let language = hl.language.clone();
                        s.spawn(move || {
                            let mut parser = tree_sitter::Parser::new();
                            parser.set_language(&language).unwrap();
                            parser.parse(text.as_bytes(), None).unwrap()
                        });
                    }
                });
                best = best.min(t.elapsed().as_secs_f64() * 1000.0);
            }
            row.push(format!("{best:.1}"));
        }
        // A fresh query compile while one thread parses.
        let source = crate::highlight::highlights_source(l.grammar);
        let text = chunks(lang, block, 1).remove(0);
        let t = Instant::now();
        std::thread::scope(|s| {
            let language = hl.language.clone();
            s.spawn(move || tree_sitter::Query::new(&language, source).unwrap());
            let mut parser = tree_sitter::Parser::new();
            parser.set_language(&hl.language).unwrap();
            parser.parse(text.as_bytes(), None).unwrap();
        });
        row.push(format!("{:.1}", t.elapsed().as_secs_f64() * 1000.0));
        eprintln!("{lang} | {}", row.join(" | "));
    }
}

mod bump {
    use std::cell::RefCell;
    use std::ffi::c_void;

    thread_local! {
        static ARENA: RefCell<(Vec<Box<[u64]>>, usize)> = const { RefCell::new((Vec::new(), 0)) };
    }

    const CHUNK: usize = 1 << 22;

    pub unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
        let words = size.div_ceil(8) + 1;
        ARENA.with(|a| {
            let mut a = a.borrow_mut();
            if a.0.is_empty() || a.1 + words > a.0.last().unwrap().len() {
                a.0.push(vec![0u64; CHUNK.max(words)].into_boxed_slice());
                a.1 = 0;
            }
            let at = a.1;
            a.1 += words;
            let block = a.0.last_mut().unwrap();
            block[at] = size as u64;
            unsafe { block.as_mut_ptr().add(at + 1) as *mut c_void }
        })
    }

    pub unsafe extern "C" fn calloc(n: usize, size: usize) -> *mut c_void {
        let p = unsafe { malloc(n * size) };
        unsafe { std::ptr::write_bytes(p as *mut u8, 0, n * size) };
        p
    }

    pub unsafe extern "C" fn realloc(p: *mut c_void, size: usize) -> *mut c_void {
        let q = unsafe { malloc(size) };
        if !p.is_null() {
            let old = unsafe { *(p as *const u64).sub(1) } as usize;
            unsafe { std::ptr::copy_nonoverlapping(p as *const u8, q as *mut u8, old.min(size)) };
        }
        q
    }

    pub unsafe extern "C" fn free(_: *mut c_void) {}
}

#[test]
#[ignore]
fn probe_bump_allocator() {
    unsafe {
        tree_sitter::set_allocator(Some(tree_sitter::Allocator {
            malloc: bump::malloc,
            calloc: bump::calloc,
            realloc: bump::realloc,
            free: bump::free,
        }));
    }
    eprintln!("lang | parse ms with a bump allocator");
    for (lang, block) in SAMPLES {
        let l = by_id(lang).unwrap();
        let language = l.grammar.language();
        let text = chunks(lang, block, 1).remove(0);
        let mut best = f64::MAX;
        for _ in 0..3 {
            let mut parser = tree_sitter::Parser::new();
            parser.set_language(&language).unwrap();
            let t = Instant::now();
            let tree = parser.parse(text.as_bytes(), None).unwrap();
            best = best.min(t.elapsed().as_secs_f64() * 1000.0);
            std::mem::forget(tree);
        }
        eprintln!("{lang} | {best:.1}");
    }
}
