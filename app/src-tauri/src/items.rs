//! An item's details as the game's tooltip shows them (`fg-app`), for the guide's panels: the
//! best gear's rows, the steps that hand one over.

#![expect(
    clippy::needless_pass_by_value,
    reason = "tauri commands take their arguments by value"
)]

use super::{AppState, CmdResult};
use fg_app::ItemInfo;
use tauri::State;

/// An item's details (None: the game's data has none of it, the page shows what it has).
#[tauri::command(async)]
pub(crate) fn item_info(state: State<'_, AppState>, item: i64) -> CmdResult<Option<ItemInfo>> {
    Ok(fg_app::item_info(&state.open_db()?, item))
}
