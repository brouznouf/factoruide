//! Writes the money coin icons, the empty equipment slot pictures and the tooltip font out of an
//! installed client into the app's assets (`app/src/assets`, see NOTICE.md): any product has
//! them (`wowf` is one), and they never change between game versions, so this is run once for
//! good.
//!
//!   cargo run -p fg-casc --example extract-client-assets -- "F:\World of Warcraft"

use fg_casc::{Storage, blp};
use std::path::{Path, PathBuf};

fn main() {
    let install = std::env::args()
        .nth(1)
        .expect("the install path (its `World of Warcraft` folder)");
    let storage = Storage::open(Path::new(&install), "wowf")
        .expect("opening the install's storage (its `World of Warcraft` folder)");
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../app/src/assets");
    // The game has no empty picture of its own for the back (a garment's serves, as the game's
    // own sheet has none either) nor for the two-hand slot (a two-handed weapon sits in the main
    // hand): both take another slot's picture.
    let files = [
        ("Interface\\MoneyFrame\\UI-GoldIcon.blp", "coins/gold.png"),
        ("Interface\\MoneyFrame\\UI-SilverIcon.blp", "coins/silver.png"),
        ("Interface\\MoneyFrame\\UI-CopperIcon.blp", "coins/copper.png"),
        ("Interface\\PaperDoll\\UI-PaperDoll-Slot-Head.blp", "slots/head.png"),
        ("Interface\\PaperDoll\\UI-PaperDoll-Slot-Neck.blp", "slots/neck.png"),
        (
            "Interface\\PaperDoll\\UI-PaperDoll-Slot-Shoulder.blp",
            "slots/shoulder.png",
        ),
        ("Interface\\PaperDoll\\UI-PaperDoll-Slot-Shirt.blp", "slots/shirt.png"),
        ("Interface\\PaperDoll\\UI-PaperDoll-Slot-Chest.blp", "slots/chest.png"),
        ("Interface\\PaperDoll\\UI-PaperDoll-Slot-Waist.blp", "slots/waist.png"),
        ("Interface\\PaperDoll\\UI-PaperDoll-Slot-Legs.blp", "slots/legs.png"),
        ("Interface\\PaperDoll\\UI-PaperDoll-Slot-Feet.blp", "slots/feet.png"),
        ("Interface\\PaperDoll\\UI-PaperDoll-Slot-Wrists.blp", "slots/wrists.png"),
        ("Interface\\PaperDoll\\UI-PaperDoll-Slot-Hands.blp", "slots/hands.png"),
        ("Interface\\PaperDoll\\UI-PaperDoll-Slot-Finger.blp", "slots/finger.png"),
        (
            "Interface\\PaperDoll\\UI-PaperDoll-Slot-Trinket.blp",
            "slots/trinket.png",
        ),
        (
            "Interface\\PaperDoll\\UI-PaperDoll-Slot-MainHand.blp",
            "slots/main-hand.png",
        ),
        (
            "Interface\\PaperDoll\\UI-PaperDoll-Slot-SecondaryHand.blp",
            "slots/secondary-hand.png",
        ),
        ("Interface\\PaperDoll\\UI-PaperDoll-Slot-Ranged.blp", "slots/ranged.png"),
    ];
    for (name, file) in files {
        let bytes = storage
            .file_by_name(name)
            .expect("reading the manifest")
            .expect("the game's own file in the manifest");
        let png = blp::png(&bytes).expect("decoding the picture");
        std::fs::create_dir_all(out.join(file).parent().expect("a folder")).expect("making the folder");
        std::fs::write(out.join(file), png).expect("writing the png");
    }
    let font = storage
        .file_by_name("Fonts\\FRIZQT__.TTF")
        .expect("reading the manifest")
        .expect("the game's own file in the manifest");
    std::fs::write(out.join("friz-quadrata.ttf"), font).expect("writing the font");
}
