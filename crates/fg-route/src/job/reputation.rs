//! Reputation goals: the points the character starts with, from the addon's record of a
//! character met in game, else its race's in the game's Faction table.

use super::start::StartState;
use crate::model::Profile;
use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};

/// The starting points of a race and class with a faction: the first of the Faction row's four
/// entries whose race mask (two signed 32-bit halves, the Forever encoding of `race_bit`) and
/// class mask (0: any) take them, else Neutral's bottom. None when the faction is unknown.
pub fn race_base(conn: &Connection, faction: i64, race_bit: i64, class_bit: i64) -> Result<Option<i64>> {
    let columns = (0..4)
        .map(|k| {
            format!("ReputationRaceMasks{k}_0, ReputationRaceMasks{k}_1, ReputationClassMask_{k}, ReputationBase_{k}")
        })
        .collect::<Vec<_>>()
        .join(", ");
    let Ok(mut stmt) = conn.prepare(&format!("SELECT {columns} FROM client_faction WHERE ID = ?1")) else {
        return Ok(None);
    };
    let row = stmt
        .query_row([faction], |r| {
            (0..4)
                .map(|k| {
                    let half = |i: usize| {
                        r.get::<_, Option<i64>>(4 * k + i)
                            .map(|v| i64::from(v.unwrap_or(0) as u32))
                    };
                    Ok((
                        half(0)? | (half(1)? << 32),
                        r.get::<_, Option<i64>>(4 * k + 2)?.unwrap_or(0),
                        r.get::<_, Option<i64>>(4 * k + 3)?.unwrap_or(0),
                    ))
                })
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .optional()?;
    Ok(row.map(|entries| {
        entries
            .into_iter()
            .find(|&(races, classes, _)| races & race_bit != 0 && (classes == 0 || classes & class_bit != 0))
            .map_or(0, |(_, _, base)| base)
    }))
}

/// The points with each faction the route follows (`Model::reputation_factions`) at the start
/// of the guide: the addon's record of the character, else the race's (a character the addon
/// saw before it recorded reputations: then a note says so).
pub fn starting(
    conn: &Connection,
    factions: &[i64],
    profile: &Profile,
    start: Option<&StartState>,
    notes: &mut Vec<String>,
) -> Result<Vec<f64>> {
    let mut out = Vec::with_capacity(factions.len());
    for &faction in factions {
        let seen = start.and_then(|s| s.reputation.iter().find(|r| r.id == faction));
        let points = if let Some(r) = seen {
            r.value
        } else {
            if start.is_some_and(|s| s.reputation.is_empty()) {
                notes.push(format!(
                    "reputation {faction} of the character unknown (recorded by a newer addon): the race's start counted"
                ));
            }
            race_base(conn, faction, profile.race_bit(), profile.class_bit())?.unwrap_or(0)
        };
        out.push(points as f64);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A race starts with the points of the first entry taking its race and class, others at
    /// Neutral's bottom (the client's Darnassus: humans, dwarves and gnomes 3100, the Horde
    /// -42000, night elves 4000, the Alliance's Skyborne 500 in the high half).
    #[test]
    fn race_base_reads_the_faction_entries() {
        let conn = Connection::open_in_memory().unwrap();
        let mut columns = vec!["ID".to_owned()];
        for k in 0..4 {
            columns.extend([
                format!("ReputationRaceMasks{k}_0"),
                format!("ReputationRaceMasks{k}_1"),
                format!("ReputationClassMask_{k}"),
                format!("ReputationBase_{k}"),
            ]);
        }
        conn.execute_batch(&format!(
            "CREATE TABLE client_faction ({});
             INSERT INTO client_faction VALUES (69, 69, 0, 0, 3100, 1309324210, -1440044374, 0, -42000, 8, 0, 0, 4000, 0, 1, 0, 500),
                                               (70, 0, 0, 0, 0, -1, -1, 4, 900, 0, 0, 0, 0, 0, 0, 0, 0);",
            columns.join(", ")
        ))
        .unwrap();
        let base = |faction, race_id, class_id| {
            race_base(&conn, faction, crate::model::race_bit(race_id), 1i64 << (class_id - 1)).unwrap()
        };
        assert_eq!(base(69, 1, 1), Some(3100)); // human
        assert_eq!(base(69, 4, 1), Some(4000)); // night elf
        assert_eq!(base(69, 2, 1), Some(-42000)); // orc
        assert_eq!(base(69, 95, 1), Some(500)); // Skyborne
        assert_eq!(base(70, 2, 3), Some(900)); // any race, hunters only
        assert_eq!(base(70, 2, 1), Some(0));
        assert_eq!(base(71, 1, 1), None);
    }
}
