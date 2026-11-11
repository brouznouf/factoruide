#![expect(clippy::unwrap_used, reason = "a build script fails by panicking")]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The `.toml` files of `dir` and of its folders, named by their path under it.
fn files(dir: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let p = entry.path();
        let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
        if p.is_dir() {
            files(&p, &format!("{name}/"), out);
        } else if p.extension().is_some_and(|e| e == "toml") {
            out.push((name, p));
        }
    }
}

/// Embeds the quest notes (`notes/`) in the library: every guide written gets them, whatever
/// runs it (desktop, web, command line).
fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut notes = Vec::new();
    files(&root.join("notes"), "", &mut notes);
    notes.sort();
    let list = notes.iter().fold(String::new(), |mut out, (name, p)| {
        let path = p.canonicalize().unwrap();
        let _ = writeln!(out, "    ({name:?}, include_str!({:?})),", path.to_str().unwrap());
        out
    });
    let out =
        format!("/// (file, text) of every notes file, by name.\npub const NOTES: &[(&str, &str)] = &[\n{list}];\n");
    std::fs::write(Path::new(&std::env::var("OUT_DIR").unwrap()).join("notes.rs"), out).unwrap();
}
