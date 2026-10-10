//! What the Factoruide addon recorded in game (its `Factoruide.lua` SavedVariables): quest
//! accepts and turn-ins with the NPC and position, objectives progress, NPCs seen...

#[cfg(feature = "local")]
use crate::lua::{self, Val};
#[cfg(feature = "local")]
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
#[cfg(feature = "local")]
use std::path::{Path, PathBuf};

/// One recorded event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub seq: Option<i64>,
    pub ts: Option<i64>,
    pub event: Option<String>,
    pub quest_id: Option<i64>,
    pub entity_type: Option<String>,
    pub entity_id: Option<i64>,
    pub ui_map_id: Option<i64>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub level: Option<i64>,
    /// Extra data of the event (objectives, rewards...).
    pub data: Option<serde_json::Value>,
}

/// The events of one character.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Character {
    /// "Name-Realm" when read locally; an opaque id in contributions.
    pub character: String,
    pub race: Option<String>,
    pub class: Option<String>,
    pub events: Vec<Event>,
}

#[cfg(feature = "local")]
/// Every `Factoruide.lua` SavedVariables file under the client's WTF folder (or, for an account
/// that has not run the renamed addon yet, its former `BrouzQuest.lua`).
pub fn find_saved_variables(client_dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(accounts) = std::fs::read_dir(client_dir.join("WTF/Account")) else {
        return found;
    };
    for account in accounts.flatten() {
        let dir = account.path().join("SavedVariables");
        if let Some(file) = ["Factoruide.lua", "BrouzQuest.lua"]
            .iter()
            .map(|n| dir.join(n))
            .find(|f| f.exists())
        {
            found.push(file);
        }
    }
    found
}

/// The characters and events of SavedVariables files.
#[cfg(feature = "local")]
pub fn read(files: &[PathBuf]) -> Result<Vec<Character>> {
    let mut out = Vec::new();
    for file in files {
        let lua = lua::new_lua();
        let global = if file.ends_with("BrouzQuest.lua") {
            "BrouzQuestDB"
        } else {
            "FactoruideDB"
        };
        let db = lua::load_globals(&lua, file, &[global])?
            .pop()
            .with_context(|| format!("{global} missing"))?;
        for (character, c) in db.field("characters").str_entries() {
            let events = c
                .field("events")
                .list()
                .into_iter()
                .map(|e| Event {
                    seq: e.get(1).int(),
                    ts: e.get(2).int(),
                    event: e.get(3).str().map(str::to_owned),
                    quest_id: e.get(4).int(),
                    entity_type: e.get(5).str().map(str::to_owned),
                    entity_id: e.get(6).int(),
                    ui_map_id: e.get(7).int(),
                    x: e.get(8).num(),
                    y: e.get(9).num(),
                    level: e.get(10).int(),
                    data: match e.get(11) {
                        Val::Table(_) => Some(e.get(11).to_json()),
                        _ => None,
                    },
                })
                .collect();
            out.push(Character {
                character: character.to_owned(),
                race: c.field("race").str().map(str::to_owned),
                class: c.field("class").str().map(str::to_owned),
                events,
            });
        }
    }
    Ok(out)
}
