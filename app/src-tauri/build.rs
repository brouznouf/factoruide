#![expect(clippy::unwrap_used, reason = "a build script fails by panicking")]

use std::fmt::Write as _;
use std::path::Path;

/// `(name, include_bytes!(path))` entries for the files of `dir` with one of `extensions`; with
/// `subdirs`, also those of its folders, at any depth, named `folder/file` (the addon's libraries
/// are two folders down).
fn files(dir: &Path, extensions: &[&str], skip: &[&str], subdirs: bool) -> String {
    fn walk(
        dir: &Path,
        prefix: &str,
        extensions: &[&str],
        skip: &[&str],
        subdirs: bool,
        out: &mut Vec<(String, std::path::PathBuf)>,
    ) {
        println!("cargo:rerun-if-changed={}", dir.display());
        for entry in std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .flatten()
        {
            let p = entry.path();
            let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
            if p.is_dir() {
                if subdirs {
                    walk(&p, &format!("{name}/"), extensions, skip, subdirs, out);
                }
            } else if p.extension().is_some_and(|e| extensions.iter().any(|x| e == *x))
                && !(prefix.is_empty() && skip.contains(&name.as_str()))
            {
                out.push((name, p));
            }
        }
    }
    let mut entries = Vec::new();
    walk(dir, "", extensions, skip, subdirs, &mut entries);
    entries.sort();
    entries.iter().fold(String::new(), |mut out, (name, p)| {
        let path = p.canonicalize().unwrap();
        let _ = writeln!(out, "    ({name:?}, include_bytes!({:?})),", path.to_str().unwrap());
        out
    })
}

/// Embeds the game databases (one per edition, from `fg --edition <e> bundle`) and their map
/// images, the overrides and the addon in the app.
fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = root.join("data");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut databases = String::new();
    for edition in ["forever", "classic", "tbc"] {
        let db = dir.join(format!("{edition}.sqlite.zst"));
        println!("cargo:rerun-if-changed={}", db.display());
        if let Ok(path) = db.canonicalize() {
            let _ = writeln!(
                databases,
                "    ({edition:?}, include_bytes!({:?})),",
                path.to_str().unwrap()
            );
        }
    }
    // Each edition's minimap (`fg minimap`), when it has one.
    let mut minimaps = String::new();
    for edition in ["forever", "classic", "tbc"] {
        let pack = dir.join(format!("maps/{edition}/minimap.pack"));
        println!("cargo:rerun-if-changed={}", pack.display());
        if let Ok(path) = pack.canonicalize() {
            let _ = writeln!(
                minimaps,
                "    ({edition:?}, include_bytes!({:?})),",
                path.to_str().unwrap()
            );
        }
    }
    // Each edition's item icons (`fg icons`), when it has one.
    let mut icons = String::new();
    for edition in ["forever", "classic", "tbc"] {
        let pack = dir.join(format!("icons/{edition}/icons.pack"));
        println!("cargo:rerun-if-changed={}", pack.display());
        if let Ok(path) = pack.canonicalize() {
            let _ = writeln!(
                icons,
                "    ({edition:?}, include_bytes!({:?})),",
                path.to_str().unwrap()
            );
        }
    }
    assert!(
        !databases.is_empty(),
        "no database in {}: run `fg bundle`",
        dir.display()
    );
    // The addon is installed from an addon site: the app only checks its version.
    let toc_path = root.join("addon/Factoruide_Profile/Factoruide_Profile.toc");
    println!("cargo:rerun-if-changed={}", toc_path.display());
    let toc = std::fs::read_to_string(&toc_path).unwrap();
    let field = |name: &str| {
        toc.lines()
            .find_map(|l| l.strip_prefix(name))
            .map(|v| v.trim().to_owned())
            .unwrap_or_default()
    };
    let addon_version = field("## Version:");
    let addon_interface = format!("## Interface: {}", field("## Interface:"));
    let out = format!(
        "/// (edition, zstd database) of every edition bundled.\n\
         pub const DATABASES: &[(&str, &[u8])] = &[\n{databases}];\n\
         /// (\"<edition>/<map id>.webp\", image) of every edition's maps.\n\
         pub const MAPS: &[(&str, &[u8])] = &[\n{}];\n\
         /// (edition, pack of its minimap's images) of the editions with one.\n\
         pub const MINIMAPS: &[(&str, &[u8])] = &[\n{minimaps}];\n\
         /// (edition, pack of its items' icons) of the editions with one.\n\
         pub const ICONS: &[(&str, &[u8])] = &[\n{icons}];\n\
         pub const OVERRIDES: &[(&str, &[u8])] = &[\n{}];\n\
         /// The oldest addon the app works with (the .toc version in this repository), and the\n\
         /// game versions its .toc declares.\n\
         pub const ADDON_VERSION: &str = {addon_version:?};\n\
         pub const ADDON_INTERFACE: &str = {addon_interface:?};\n",
        files(&dir.join("maps"), &["webp"], &[], true),
        files(&root.join("overrides"), &["toml"], &[], true),
    );
    std::fs::write(Path::new(&std::env::var("OUT_DIR").unwrap()).join("embedded.rs"), out).unwrap();
    tauri_build::build();
}
