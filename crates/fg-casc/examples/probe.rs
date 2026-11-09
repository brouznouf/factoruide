// Debugging the chain against the real install (not a library test: the
// client is only on this machine). Run with:
//   cargo run -p fg-casc --example probe -- <install> <product>
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let install = PathBuf::from(args.next().expect("the install path"));
    let product = args.next().expect("the product");
    let storage = fg_casc::Storage::open(&install, &product)?;
    println!("{product}: {} files in the manifest", storage.files());
    for name in [
        "DBFilesClient\\Item.db2",
        "DBFilesClient\\ItemDisplayInfo.db2",
        "DBFilesClient\\ItemAppearance.db2",
        "DBFilesClient\\TactKey.db2",
        "Interface\\Icons\\INV_Sword_04.blp",
    ] {
        match storage.file_by_name(name) {
            Ok(Some(bytes)) => println!(
                "  {name}: {} bytes, magic {:02x?}",
                bytes.len(),
                &bytes[..8.min(bytes.len())]
            ),
            Ok(None) => println!("  {name}: not in the manifest"),
            Err(err) => println!("  {name}: {err}"),
        }
    }
    Ok(())
}
