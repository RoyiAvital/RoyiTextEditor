//! Ensures `assets/` is complete before anything embeds or reads it: runs
//! `tools/Get-Fonts.ps1`, which downloads missing files from pinned releases
//! and verifies their SHA-256 hashes (a no-op when everything is present).

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let script = root.join("tools").join("Get-Fonts.ps1");
    let fonts = root.join("assets").join("fonts");
    println!("cargo:rerun-if-changed={}", script.display());
    println!("cargo:rerun-if-changed={}", fonts.display());

    let before = font_files(&fonts);
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script)
        .output();
    match output {
        Ok(out) if out.status.success() => {}
        Ok(out) => panic!(
            "{} failed:\n{}\n{}",
            script.display(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
        Err(e) => panic!("cannot run powershell.exe for {}: {e}", script.display()),
    }
    let added = font_files(&fonts).len().saturating_sub(before.len());
    if added > 0 {
        println!("cargo:warning=downloaded {added} font files into assets/fonts");
    }
}

fn font_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(families) = std::fs::read_dir(dir) else {
        return out;
    };
    for family in families.flatten() {
        if let Ok(files) = std::fs::read_dir(family.path()) {
            out.extend(
                files
                    .flatten()
                    .map(|f| f.path())
                    .filter(|p| p.extension().is_some_and(|e| e == "ttf")),
            );
        }
    }
    out
}
