//! The items' icons for the guide's panels: the packs `fg icons` writes (an
//! index as JSON, then the pictures).

#![expect(
    clippy::needless_pass_by_value,
    reason = "tauri commands take their arguments by value"
)]

use super::{AppState, CmdResult, Edition, HashMap, OnceLock, State, embedded};

/// An edition's icons (`fg icons`): each item at its picture, into the pack's
/// pictures.
pub(crate) struct Icons {
    items: HashMap<i64, (usize, usize)>,
    pictures: &'static [u8],
}

/// The icons of an edition, when it has a pack: `FGIC`, the index's length
/// (u32 LE), the index (JSON `item` -> `(offset, length)`), the pictures.
pub(crate) fn icons(edition: Edition) -> Option<&'static Icons> {
    static PACKS: OnceLock<HashMap<&'static str, Icons>> = OnceLock::new();
    let packs = PACKS.get_or_init(|| {
        embedded::ICONS
            .iter()
            .filter_map(|&(edition, pack)| {
                let len = u32::from_le_bytes(pack.get(4..8)?.try_into().ok()?) as usize;
                if pack.get(..4)? != b"FGIC" {
                    return None;
                }
                let index: Vec<(i64, usize, usize)> = serde_json::from_slice(pack.get(8..8 + len)?).ok()?;
                let items = index.into_iter().map(|(i, o, n)| (i, (o, n))).collect();
                Some((
                    edition,
                    Icons {
                        items,
                        pictures: &pack[8 + len..],
                    },
                ))
            })
            .collect()
    });
    packs.get(edition.key())
}

/// The icon of item `item` (a PNG), of the current game version's edition.
#[tauri::command(async)]
pub(crate) fn item_icon(state: State<'_, AppState>, item: i64) -> CmdResult<tauri::ipc::Response> {
    let icons = icons(state.edition()).ok_or("no icons")?;
    let &(offset, len) = icons
        .items
        .get(&item)
        .ok_or_else(|| format!("no icon of item {item}"))?;
    let bytes = icons.pictures.get(offset..offset + len).ok_or("icons pack truncated")?;
    Ok(tauri::ipc::Response::new(bytes.to_vec()))
}
