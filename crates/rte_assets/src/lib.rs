//! Bundled assets. They are not stored in Git: this crate's build script
//! downloads missing files (see `tools/Get-Fonts.ps1`).

/// Folder holding one subfolder per font family (source tree).
pub const FONTS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts");

/// Default face, embedded so startup reads no font file.
pub static DEFAULT_FONT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/fonts/JetBrainsMono/JetBrainsMono-Regular.ttf"
));
