//! Reputation a kill gives (`m_npc_reputation`): the creatures the character kills before a
//! quest asking for a standing it does not have yet.

use crate::faction::Faction;
use crate::world::{Pos, World};
use anyhow::Result;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

/// The bottom of each standing, Hated to Exalted, in points from Neutral's bottom.
pub const STANDING_FLOORS: [f64; 8] = [-42000.0, -6000.0, -3000.0, 0.0, 3000.0, 9000.0, 21000.0, 42000.0];

/// The points a kill given while the standing is at most `max_standing` brings the character
/// to: the bottom of the standing above (Friendly: Honored's 9000).
pub fn cap(max_standing: i64) -> f64 {
    usize::try_from(max_standing + 1)
        .ok()
        .and_then(|k| STANDING_FLOORS.get(k).copied())
        .unwrap_or(42999.0)
}

/// What one kill does to a faction's reputation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KillRep {
    pub faction: i64,
    /// Points given (negative: taken), the human's Diplomacy counted on gains.
    pub value: f64,
    /// Points a gain stops at (`cap`).
    pub cap: f64,
}

impl KillRep {
    /// The points after `kills` kills from `have`: gains stop at the cap (none above it).
    pub fn after(&self, have: f64, kills: f64) -> f64 {
        if self.value > 0.0 {
            if have >= self.cap {
                have
            } else {
                (have + self.value * kills).min(self.cap)
            }
        } else {
            have + self.value * kills
        }
    }
}

/// The creatures to kill for a faction's reputation, and where they stand.
#[derive(Debug, Clone)]
pub struct RepCamp {
    pub npc: i64,
    pub name: String,
    pub level: i64,
    /// What a kill gives the faction.
    pub gain: KillRep,
    /// Everything a kill does (the faction's gain and the rival's loss).
    pub effects: Vec<KillRep>,
    pub places: Vec<(Pos, i64)>,
}

/// The kill reputation of every creature, for the character's side (none in an older database).
pub(crate) fn load(conn: &Connection, faction: Faction, rate: f64) -> Result<HashMap<i64, Vec<KillRep>>> {
    let has_table: bool = conn.query_row(
        "SELECT count(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'm_npc_reputation'",
        [],
        |r| r.get(0),
    )?;
    let mut out: HashMap<i64, Vec<KillRep>> = HashMap::new();
    if !has_table {
        return Ok(out);
    }
    let mut stmt = conn.prepare(
        "SELECT npc_id, faction, value, max_standing FROM m_npc_reputation
         WHERE coalesce(side, '') IN ('', ?1)",
    )?;
    let rows = stmt.query_map([faction.letter()], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, i64>(3)?,
        ))
    })?;
    for row in rows {
        let (npc, faction, value, max) = row?;
        let value = if value > 0 { value as f64 * rate } else { value as f64 };
        out.entry(npc).or_default().push(KillRep {
            faction,
            value,
            cap: cap(max),
        });
    }
    Ok(out)
}

/// The creatures whose kills give reputation with `faction`, standing on a continent the
/// character can reach.
pub(crate) fn camps(
    conn: &Connection,
    world: &World,
    kills: &HashMap<i64, Vec<KillRep>>,
    faction: i64,
    reachable: &HashSet<i64>,
) -> Result<Vec<RepCamp>> {
    let mut out = Vec::new();
    let mut places = conn.prepare("SELECT zone_id, x, y FROM m_spawn WHERE entity_type = 'npc' AND entity_id = ?1")?;
    let mut info =
        conn.prepare("SELECT coalesce(name, ''), coalesce(max_level, min_level, 1) FROM m_npc WHERE id = ?1")?;
    let mut npcs: Vec<_> = kills.iter().collect();
    npcs.sort_unstable_by_key(|(id, _)| **id);
    for (&npc, effects) in npcs {
        let Some(gain) = effects.iter().find(|k| k.faction == faction && k.value > 0.0) else {
            continue;
        };
        let spots: Vec<(Pos, i64)> = places
            .query_map([npc], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?, r.get::<_, f64>(2)?))
            })?
            .filter_map(std::result::Result::ok)
            .filter_map(|(zone, x, y)| world.to_world(zone, x, y).map(|p| (p, zone)))
            .filter(|(p, _)| reachable.contains(&p.continent))
            .collect();
        if spots.is_empty() {
            continue;
        }
        let Ok((name, level)) = info.query_row([npc], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))) else {
            continue;
        };
        out.push(RepCamp {
            npc,
            name,
            level,
            gain: *gain,
            effects: effects.clone(),
            places: spots,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gain_stops_at_the_bottom_of_the_standing_above_its_last() {
        let friendly = KillRep {
            faction: 92,
            value: 20.0,
            cap: cap(4),
        };
        assert_eq!(friendly.cap, 9000.0);
        assert_eq!(friendly.after(8990.0, 3.0), 9000.0);
        assert_eq!(friendly.after(9500.0, 3.0), 9500.0, "none above it");
        let loss = KillRep {
            faction: 93,
            value: -100.0,
            cap: cap(4),
        };
        assert_eq!(loss.after(0.0, 3.0), -300.0);
    }
}
