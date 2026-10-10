//! Battleground quests.

use super::sources::{ObjectiveKind, ObjectiveRows, QuestRow};
use anyhow::Result;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

/// Battleground (PvP) quests: filed under a battleground, asking for what battleground quests
/// ask for (marks of honor), or rewarding a faction almost only battleground quests reward.
pub(super) fn pvp_quests(conn: &Connection, rows: &[QuestRow], objectives: &ObjectiveRows) -> Result<HashSet<i64>> {
    let areas: HashSet<i64> = conn
        .prepare(
            "SELECT a.ID FROM client_areatable a JOIN client_map m ON m.ID = a.ContinentID WHERE m.InstanceType = 3
             UNION SELECT AreaTableID FROM client_map WHERE InstanceType = 3",
        )?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let factions = |r: &QuestRow| -> Vec<i64> {
        r.reputation
            .as_deref()
            .and_then(|j| serde_json::from_str::<Vec<(i64, i64)>>(j).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, v)| *v > 0)
            .map(|(f, _)| f)
            .collect()
    };
    let items = |id: i64| -> Vec<i64> {
        objectives
            .get(&id)
            .into_iter()
            .flatten()
            .filter(|(k, ..)| *k == ObjectiveKind::Item)
            .filter_map(|(_, t, ..)| *t)
            .collect()
    };
    let filed = |r: &QuestRow| r.sort.is_some_and(|s| areas.contains(&s));
    let mut marks = HashSet::new();
    let mut rewarded: HashMap<i64, (usize, usize)> = HashMap::new();
    for r in rows {
        let bg = filed(r);
        if bg {
            marks.extend(items(r.id));
        }
        for f in factions(r) {
            let e = rewarded.entry(f).or_default();
            e.0 += 1;
            e.1 += usize::from(bg);
        }
    }
    let pvp_factions: HashSet<i64> = rewarded
        .into_iter()
        .filter(|(_, (all, bg))| *bg > 0 && *bg * 5 >= *all * 4)
        .map(|(f, _)| f)
        .collect();
    Ok(rows
        .iter()
        .filter(|r| {
            filed(r)
                || items(r.id).iter().any(|i| marks.contains(i))
                || factions(r).iter().any(|f| pvp_factions.contains(f))
        })
        .map(|r| r.id)
        .collect())
}
