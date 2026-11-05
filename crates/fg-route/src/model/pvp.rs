//! Battleground quests.

use super::sources::{ObjectiveKind, ObjectiveRows, QuestRow};
use anyhow::Result;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

/// Battleground (PvP) quests: filed under a battleground, asking for what battleground quests
/// ask for (marks of honor), or rewarding a faction almost only battleground quests reward.
pub(super) fn pvp_quests(conn: &Connection, rows: &[QuestRow], objectives: &ObjectiveRows) -> Result<HashSet<i64>> {
    let mut areas: HashSet<i64> = conn
        .prepare(
            "SELECT a.ID FROM client_areatable a JOIN client_map m ON m.ID = a.ContinentID WHERE m.InstanceType = 3
             UNION SELECT AreaTableID FROM client_map WHERE InstanceType = 3",
        )?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    // A battleground map without an area of its own (Alterac Valley, Darkspear Islands) says 0:
    // no zone, the sort a quest filed nowhere has too (WoW Forever's library books).
    areas.remove(&0);
    let factions = |r: &QuestRow| -> Vec<i64> {
        r.reputations()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A quest filed under no zone (sort 0) is not a battleground quest because a battleground
    /// map has no area of its own; one filed under Warsong Gulch is.
    #[test]
    fn a_quest_of_no_zone_is_not_a_battleground_quest() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE client_map (ID, AreaTableID, InstanceType);
             CREATE TABLE client_areatable (ID, ContinentID);
             INSERT INTO client_map VALUES (30, 0, 3), (489, 3277, 3);",
        )
        .unwrap();
        let rows = [
            QuestRow {
                id: 1,
                sort: Some(0),
                ..Default::default()
            },
            QuestRow {
                id: 2,
                sort: Some(3277),
                ..Default::default()
            },
        ];
        let pvp = pvp_quests(&conn, &rows, &ObjectiveRows::default()).unwrap();
        assert_eq!(pvp, HashSet::from([2]));
    }
}
