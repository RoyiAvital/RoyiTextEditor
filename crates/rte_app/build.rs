//! Embeds the application manifest and icon into the executable: MinGW
//! `windres` compiles a resource script into a COFF object linked into the
//! binary. The icon is rendered from `Meta/Icon.svg` (64 px and up) and
//! `Meta/IconSmall.svg` (48 px and below, where the names become bars).
//! Fails when `Meta/AppMeta.json` and the Cargo package version differ, or when
//! its `file name` is not the name of the `[[bin]]` target.
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
/// Side of the About panel icon (`about_icon.bin`: premultiplied `0xAARRGGBB`, little-endian).
const ABOUT_ICON: u32 = 128;

fn main() {
    println!("cargo:rerun-if-changed=RoyiTextEditor.exe.manifest");
    println!("cargo:rerun-if-changed={APP_META}");
    println!("cargo:rerun-if-changed={ICON}");
    println!("cargo:rerun-if-changed={ICON_SMALL}");
    println!("cargo:rerun-if-env-changed=RTE_SEGMENT_HEAP");
    println!("cargo:rerun-if-env-changed=WINDRES");
    read_meta();
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    std::fs::write(out.join("about_icon.bin"), about_icon()).expect("write about_icon.bin");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
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

/// Checks `AppMeta.json` against Cargo and exports the display name, file name
/// and links as `RTE_APP_NAME`, `RTE_FILE_NAME`, `RTE_REPOSITORY_URL` and
/// `RTE_DOCUMENTATION_URL`.
fn read_meta() {
    let text = std::fs::read_to_string(APP_META).expect("read AppMeta.json");
    let info: serde_json::Value = serde_json::from_str(&text).expect("AppMeta.json is JSON");
    let name = info["name"].as_str().expect("AppMeta.json has a name");
    println!("cargo:rustc-env=RTE_APP_NAME={name}");
    let file_name = info["file name"]
        .as_str()
        .expect("AppMeta.json has a file name");
    assert_eq!(
        Some(file_name),
        bin_name().as_deref(),
        "AppMeta.json `file name` must equal the [[bin]] name in crates/rte_app/Cargo.toml"
    );
    println!("cargo:rustc-env=RTE_FILE_NAME={file_name}");
    for (key, env) in [
        ("repository", "RTE_REPOSITORY_URL"),
        ("documentation", "RTE_DOCUMENTATION_URL"),
    ] {
        let url = info[key]
            .as_str()
            .unwrap_or_else(|| panic!("AppMeta.json has a {key} URL"));
        assert!(
            url.starts_with("https://"),
            "AppMeta.json {key} must be an https URL"
        );
        println!("cargo:rustc-env={env}={url}");
    }
    let version = info["version"]
        .as_str()
        .expect("AppMeta.json has a version");
    let cargo = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    assert_eq!(
        version, cargo,
        "AppMeta.json and the workspace version in Cargo.toml differ"
    );
}

/// `name` of the `[[bin]]` target in this package's `Cargo.toml`.
fn bin_name() -> Option<String> {
    let text = std::fs::read_to_string("Cargo.toml").ok()?;
    let mut in_bin = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_bin = line == "[[bin]]";
        } else if in_bin && let Some(value) = line.strip_prefix("name") {
            let value = value.trim_start().strip_prefix('=')?;
            return Some(value.trim().trim_matches('"').to_owned());
        }
    }
    None
}

fn render(tree: &resvg::usvg::Tree, size: u32) -> Pixmap {
    let scale = size as f32 / tree.size().width();
    let mut pixmap = Pixmap::new(size, size).expect("icon size");
    resvg::render(
        tree,
        Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap
}

/// Pixels of the About panel icon.
fn about_icon() -> Vec<u8> {
    let pixmap = render(&svg_tree(ICON), ABOUT_ICON);
    pixmap
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&p| {
            let [r, g, b, a] = p.map(u32::from);
            ((a << 24) | (r << 16) | (g << 8) | b).to_le_bytes()
        })
        .collect()
}

fn svg_tree(path: &str) -> resvg::usvg::Tree {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    assert!(
        !text.starts_with("version https://git-lfs"),
        "{path} is a Git LFS pointer; fetch it with `git lfs pull` (or `lfs: true` in actions/checkout)"
    );
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
            (
                size,
                render(tree, size).encode_png().expect("encode icon PNG"),
            )
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
