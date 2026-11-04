//! The game's maps for the guide pages: the world map images and the minimap tiles.

#![expect(
    clippy::needless_pass_by_value,
    reason = "tauri commands take their arguments by value"
)]

use super::{AppState, CmdResult, Edition, HashMap, OnceLock, State, embedded};

// Maps -----------------------------------------------------------------------------

/// The maps of the edition: the world's and the dungeons' floors (`fg_app::map_index`).
#[tauri::command(async)]
pub(crate) fn map_index(state: State<'_, AppState>) -> CmdResult<serde_json::Value> {
    fg_app::map_index(&state.open_db()?)
}

/// A map image as a data URL.
#[tauri::command]
pub(crate) fn map_image(state: State<'_, AppState>, id: i64) -> CmdResult<tauri::ipc::Response> {
    let name = format!("{}/{id}.webp", state.edition().key());
    let (_, bytes) = embedded::MAPS
        .iter()
        .find(|(n, _)| *n == name)
        .ok_or_else(|| format!("no map image {name}"))?;
    Ok(tauri::ipc::Response::new(bytes.to_vec()))
}

/// An edition's minimap pack (`fg minimap`): its index, `(map, level, x, y)` -> (offset, length)
/// in the images after it.
pub(crate) struct Minimap {
    tiles: HashMap<(i64, u32, u32, u32), (usize, usize)>,
    images: &'static [u8],
}

pub(crate) fn minimap(edition: Edition) -> Option<&'static Minimap> {
    static PACKS: OnceLock<HashMap<&'static str, Minimap>> = OnceLock::new();
    let packs = PACKS.get_or_init(|| {
        embedded::MINIMAPS
            .iter()
            .filter_map(|&(edition, pack)| {
                let len = u32::from_le_bytes(pack.get(4..8)?.try_into().ok()?) as usize;
                if pack.get(..4)? != b"FGMM" {
                    return None;
                }
                let index: Vec<(i64, u32, u32, u32, usize, usize)> =
                    serde_json::from_slice(pack.get(8..8 + len)?).ok()?;
                let tiles = index
                    .into_iter()
                    .map(|(m, l, x, y, o, n)| ((m, l, x, y), (o, n)))
                    .collect();
                Some((
                    edition,
                    Minimap {
                        tiles,
                        images: &pack[8 + len..],
                    },
                ))
            })
            .collect()
    });
    packs.get(edition.key())
}

/// The minimap's images of the current game version: `[map, level, x, y]` of each (none
/// without a minimap).
#[tauri::command(async)]
pub(crate) fn minimap_index(state: State<'_, AppState>) -> Vec<(i64, u32, u32, u32)> {
    let mut tiles: Vec<_> = minimap(state.edition())
        .map(|m| m.tiles.keys().copied().collect())
        .unwrap_or_default();
    tiles.sort_unstable();
    tiles
}

/// A minimap image (WebP): of game map `map`, at `level` (2^level ADT tiles a side), the
/// `x`-th from the west and `y`-th from the north.
#[tauri::command(async)]
pub(crate) fn minimap_image(
    state: State<'_, AppState>,
    map: i64,
    level: u32,
    x: u32,
    y: u32,
) -> CmdResult<tauri::ipc::Response> {
    let m = minimap(state.edition()).ok_or("no minimap")?;
    let &(offset, len) = m
        .tiles
        .get(&(map, level, x, y))
        .ok_or_else(|| format!("no minimap image {map}/{level}/{x}/{y}"))?;
    let bytes = m.images.get(offset..offset + len).ok_or("minimap pack truncated")?;
    Ok(tauri::ipc::Response::new(bytes.to_vec()))
}
