//! Reputation goals: the points the character starts with, from the addon's record of a
//! character met in game, else its race's in the game's Faction table.

use super::start::StartState;
use crate::model::Profile;
use crate::params::ReputationGoal;
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

/// The points with each goal's faction at the start of the guide: the addon's record of the
/// character, else the race's (a character the addon saw before it recorded reputations: then
/// a note says so).
pub fn starting(
    conn: &Connection,
    goals: &[ReputationGoal],
    profile: &Profile,
    start: Option<&StartState>,
    notes: &mut Vec<String>,
) -> Result<Vec<f64>> {
    let mut out = Vec::with_capacity(goals.len());
    for goal in goals {
        let seen = start.and_then(|s| s.reputation.iter().find(|r| r.id == goal.faction));
        let points = if let Some(r) = seen {
            r.value
        } else {
            let points = race_base(conn, goal.faction, profile.race_bit(), profile.class_bit())?.unwrap_or(0);
            notes.push(format!(
                "reputation {}: starting value {points} is an estimated race/class baseline; no recorded standing for this faction",
                goal.faction
            ));
            points
        };
        out.push(points as f64);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_faction_missing_from_partial_snapshot_is_labeled_estimated() {
        let conn = Connection::open_in_memory().unwrap();
        let profile = crate::testing::profile(crate::faction::Faction::Horde, 20, 21);
        let start = StartState {
            reputation: vec![crate::job::Standing { id: 68, value: 1217 }],
            ..StartState::default()
        };
        let mut notes = Vec::new();
        let values = starting(
            &conn,
            &[ReputationGoal {
                faction: 67,
                target: 100,
            }],
            &profile,
            Some(&start),
            &mut notes,
        )
        .unwrap();
        assert_eq!(values, vec![0.0]);
        assert!(
            notes.iter().any(|n| n.contains("67") && n.contains("estimated")),
            "{notes:?}"
        );
    }

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
