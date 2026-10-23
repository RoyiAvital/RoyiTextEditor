//! Embeds the application manifest and icon into the executable: MinGW
//! `windres` compiles a resource script into a COFF object linked into the
//! binary. The icon is rendered from `Meta/Icon.svg` (64 px and up) and
//! `Meta/IconSmall.svg` (48 px and below, where the names become bars).
//! Fails when `Meta/AppMeta.json` and the Cargo package version differ.
//!
//! `RTE_SEGMENT_HEAP=1` adds `<heapType>SegmentHeap</heapType>` (used to
//! measure it; see Plan/Editing.md). `WINDRES` overrides the tool path.

use std::path::PathBuf;
use std::process::Command;

use resvg::tiny_skia::{Pixmap, Transform};

const SEGMENT_HEAP: &str = "<heapType xmlns=\"http://schemas.microsoft.com/SMI/2020/WindowsSettings\">SegmentHeap</heapType>";
const APP_META: &str = "../../Meta/AppMeta.json";
const ICON: &str = "../../Meta/Icon.svg";
const ICON_SMALL: &str = "../../Meta/IconSmall.svg";
/// Sizes in the `.ico`; Windows picks the nearest for each view and DPI.
const ICON_SIZES: [u32; 10] = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256];
/// Largest size drawn from `IconSmall.svg`.
const ICON_SMALL_MAX: u32 = 48;

fn main() {
    println!("cargo:rerun-if-changed=RoyiTextEditor.exe.manifest");
    println!("cargo:rerun-if-changed={APP_META}");
    println!("cargo:rerun-if-changed={ICON}");
    println!("cargo:rerun-if-changed={ICON_SMALL}");
    println!("cargo:rerun-if-env-changed=RTE_SEGMENT_HEAP");
    println!("cargo:rerun-if-env-changed=WINDRES");
    check_version();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let source = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"))
        .join("RoyiTextEditor.exe.manifest");
    let template = std::fs::read_to_string(&source).expect("read the application manifest");
    let heap = if std::env::var("RTE_SEGMENT_HEAP").as_deref() == Ok("1") {
        SEGMENT_HEAP
    } else {
        ""
    };
    std::fs::write(
        out.join("app.manifest"),
        template.replace("<!--HEAP-->", heap),
    )
    .expect("write manifest");
    std::fs::write(out.join("app.ico"), icon()).expect("write app.ico");
    // Resource 1 of type RT_MANIFEST (24) and icon group 1 (the executable icon).
    std::fs::write(
        out.join("resources.rc"),
        "1 24 \"app.manifest\"\n1 ICON \"app.ico\"\n",
    )
    .expect("write resources.rc");

    let windres = std::env::var("WINDRES").unwrap_or_else(|_| "windres".to_owned());
    let status = Command::new(&windres)
        .current_dir(&out)
        .args([
            "--input",
            "resources.rc",
            "--output",
            "resources.o",
            "--output-format=coff",
        ])
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => panic!("{windres} failed ({s}) compiling the application manifest"),
        Err(e) => panic!(
            "{windres} not found ({e}); install MinGW-w64 binutils or set WINDRES to its path"
        ),
    }
    println!(
        "cargo:rustc-link-arg-bins={}",
        out.join("resources.o").display()
    );
}

/// Checks the version against Cargo and exports the name as `RTE_APP_NAME`.
fn check_version() {
    let text = std::fs::read_to_string(APP_META).expect("read AppMeta.json");
    let info: serde_json::Value = serde_json::from_str(&text).expect("AppMeta.json is JSON");
    let name = info["name"].as_str().expect("AppMeta.json has a name");
    println!("cargo:rustc-env=RTE_APP_NAME={name}");
    let version = info["version"]
        .as_str()
        .expect("AppMeta.json has a version");
    let cargo = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    assert_eq!(
        version, cargo,
        "AppMeta.json and the workspace version in Cargo.toml differ"
    );
}

fn svg_tree(path: &str) -> resvg::usvg::Tree {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    resvg::usvg::Tree::from_str(&text, &resvg::usvg::Options::default())
        .unwrap_or_else(|e| panic!("parse {path}: {e}"))
}

/// An `.ico` with one PNG image per size in `ICON_SIZES`.
fn icon() -> Vec<u8> {
    let large = svg_tree(ICON);
    let small = svg_tree(ICON_SMALL);
    let images: Vec<(u32, Vec<u8>)> = ICON_SIZES
        .iter()
        .map(|&size| {
            let tree = if size <= ICON_SMALL_MAX {
                &small
            } else {
                &large
            };
            let scale = size as f32 / tree.size().width();
            let mut pixmap = Pixmap::new(size, size).expect("icon size");
            resvg::render(
                tree,
                Transform::from_scale(scale, scale),
                &mut pixmap.as_mut(),
            );
            (size, pixmap.encode_png().expect("encode icon PNG"))
        })
        .collect();

    let mut ico = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len();
    for (size, png) in &images {
        // Width and height 0 mean 256.
        let side = if *size >= 256 { 0 } else { *size as u8 };
        ico.extend_from_slice(&[side, side, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
        ico.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += png.len();
    }
    for (_, png) in &images {
        ico.extend_from_slice(png);
    }
    ico
}
