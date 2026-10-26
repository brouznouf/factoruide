//! Starting a route from a character met in game: what the addon's profile says (`/fg profile`,
//! saved at each logout) becomes the planner's initial state, so that the guide goes on from
//! where the character stands.

use crate::model::{Initial, Model, Objective, Quest};
use crate::params::Params;
use crate::world::World;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// A character as the addon saw it (its `FactoruideDB.profiles` entry).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StartState {
    /// "Name-Realm".
    pub name: String,
    /// ChrRaces ID and class file name (MAGE...).
    pub race: i64,
    pub class: String,
    pub faction: String,
    pub level: i64,
    /// XP into the level, needed for the next one, and rested XP left (doubles kill XP).
    pub xp: i64,
    pub xp_max: i64,
    pub rested: i64,
    /// In an inn or a city (rested XP builds up faster while logged out).
    pub resting: bool,
    /// Quests turned in (IDs).
    pub completed: Vec<i64>,
    /// Quests in the log.
    pub log: Vec<LogQuest>,
    /// Where the character stands (none inside an instance).
    pub position: Option<MapPos>,
    pub instance: bool,
    /// Where the hearthstone takes the character.
    pub bind: Option<Bind>,
    /// Flight paths known (TaxiNodes IDs).
    pub flights: Vec<i64>,
    /// `flights` is the final list (the app deduced the towns' flight paths from the quests and
    /// the player ticked or unticked them): not deduced again when planning.
    pub flights_final: bool,
    pub professions: Vec<Skill>,
    /// Riding skill (0: no mount).
    pub riding: i64,
    /// Items worn and spells learned (IDs).
    pub gear: Vec<i64>,
    pub spells: Vec<i64>,
    /// Copper.
    pub money: i64,
    /// Points with each faction the game lists for the character (none: recorded by an older
    /// addon).
    pub reputation: Vec<Standing>,
    /// When it was recorded (unix seconds).
    pub time: i64,
}

/// Points with a faction (Faction ID), counted from the bottom of Neutral.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Standing {
    pub id: i64,
    pub value: i64,
}

/// A quest of the log.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LogQuest {
    pub id: i64,
    /// Every objective done: only the turn-in is left.
    pub complete: bool,
    pub objectives: Vec<LogObjective>,
}

/// An objective as the quest log shows it ("Kobold Vermin slain: 3/10").
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LogObjective {
    pub text: String,
    pub done: i64,
    pub need: i64,
    pub finished: bool,
}

/// A point of a game map (uiMapID, percent coordinates).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct MapPos {
    pub map: i64,
    pub x: f64,
    pub y: f64,
}

/// The hearthstone's place: its name, and where the character stood when binding it (known
/// when the addon saw the binding).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Bind {
    pub name: String,
    pub position: Option<MapPos>,
    /// AreaTable IDs whose name in the player's language is `name` (the addon looks them up:
    /// the data only knows English names).
    pub areas: Vec<i64>,
}

/// A profession skill (skill line ID, rank).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Skill {
    pub line: i64,
    pub rank: i64,
}

/// An inn the hearthstone is bound to is this close to where the binding was seen (yards).
const BIND_RADIUS: f64 = 60.0;

impl StartState {
    /// Professions of the request following the character: the curve goes through its skill
    /// now; one it does not have yet starts from its level.
    pub fn anchor_professions(&self, goals: &mut [crate::profession::ProfessionGoal]) {
        for goal in goals {
            let Some(def) = crate::profession::find(&goal.key) else {
                continue;
            };
            let rank = self
                .professions
                .iter()
                .find(|s| s.line == def.skill_id)
                .map_or(0, |s| s.rank);
            let start = goal.start_level.unwrap_or(crate::profession::START_LEVEL);
            if rank <= 0 {
                goal.start_level = Some(start.max(self.level));
                continue;
            }
            let base = if goal.milestones.is_empty() {
                crate::profession::default_milestones(def.key)
            } else {
                goal.milestones.clone()
            };
            goal.start_level = Some((self.level - 1).clamp(1, start.max(1)));
            goal.milestones = std::iter::once((self.level, rank))
                .chain(base.into_iter().filter(|&(l, s)| l > self.level && s > rank))
                .collect();
        }
    }

    /// The planner's initial state (its log: see `Initial::log`); `notes` says what could not
    /// be placed.
    pub fn initial(&self, model: &Model, world: &World, params: &mut Params, notes: &mut Vec<String>) -> Initial {
        let mut init = Initial {
            xp: self.xp.max(0),
            rested: self.rested.max(0),
            gear: self.gear.clone(),
            completed: Some(self.completed.iter().copied().collect()),
            ..Initial::default()
        };
        let in_log: HashSet<i64> = self.log.iter().map(|q| q.id).collect();
        init.turned = self
            .completed
            .iter()
            .filter(|id| !in_log.contains(id))
            .filter_map(|id| model.index.get(id).copied())
            .collect();
        // The quests of the log are taken, as far along as the log says: the guide finishes
        // those it wants (only the turn-in left for one complete), the others are abandoned at
        // its start.
        for q in &self.log {
            // A quest with nothing done is left aside: the guide takes it, or not, as any other (its
            // giver's step is done in a moment when it is already in the log), instead of being tied
            // to a quest the character has not begun. Done or begun ones are finished or given up.
            let begun = q.complete || q.objectives.iter().any(|o| o.finished || o.done > 0);
            if !begun {
                continue;
            }
            if let Some(&i) = model.index.get(&q.id) {
                init.accepted.push(i);
                for (k, share) in objective_progress(&model.quests[i], q) {
                    init.progress.push((i, k, share));
                }
            }
        }
        init.log = self
            .log
            .iter()
            .map(|q| {
                let name = model.index.get(&q.id).map(|&i| model.quests[i].name.clone());
                (q.id, name.unwrap_or_default())
            })
            .collect();
        init.pos = self.position.and_then(|p| at(world, p)).or_else(|| {
            self.bind_inn(model, world)
                .map(|i| (model.inns[i].pos, model.inns[i].zone))
        });
        if init.pos.is_none() {
            notes.push("character position unknown: the guide starts at the race's starting place".into());
        }
        init.bind = self.bind_inn(model, world);
        if init.bind.is_none() && self.bind.is_some() {
            notes.push("hearthstone place not found: bind it again in game to record it".into());
        }
        init.known = self.known_flights(world);
        init.trained = model
            .power
            .as_ref()
            .filter(|p| p.has_spells() && !self.spells.is_empty())
            .and_then(|p| p.trained_level(&self.spells.iter().copied().collect(), self.level));
        if self.riding > 0 {
            params.mount_level = params.mount_level.min(self.level);
        }
        init
    }

    /// The character where `end` leaves it, as its addon would record it (a player made up from
    /// a guide, to plan the next one from there: a group's players in tests). `self` is the
    /// character at the start of the guide (the quests done before it are kept).
    #[must_use]
    pub fn after(&self, model: &Model, world: &World, rules: &crate::xp::Rules, end: &crate::plan::End) -> Self {
        let mut completed: Vec<i64> = self.completed.clone();
        for &i in &end.turned {
            let id = model.quests[i].id;
            if !completed.contains(&id) {
                completed.push(id);
            }
        }
        let map = |zone: i64, pos: &crate::world::Pos| {
            world.to_map(zone, pos).map(|p| MapPos {
                map: p.ui_map,
                x: p.x,
                y: p.y,
            })
        };
        let inn = &model.inns[end.bind];
        let mut spells = self.spells.clone();
        for (i, ability) in model.abilities.entries.iter().enumerate() {
            if end.abilities & (1 << i) != 0 {
                spells.extend(&ability.spells);
            }
        }
        spells.sort_unstable();
        spells.dedup();
        Self {
            level: end.level,
            xp: end.xp,
            xp_max: rules.to_next_level(end.level),
            rested: end.rested,
            spells,
            resting: false,
            completed,
            log: (end.log.iter())
                .map(|&(i, complete)| LogQuest {
                    id: model.quests[i].id,
                    complete,
                    objectives: vec![],
                })
                .collect(),
            position: map(end.zone, &end.pos),
            instance: false,
            bind: Some(Bind {
                name: inn.name.clone(),
                position: map(inn.zone, &inn.pos),
                areas: vec![],
            }),
            flights: (world.taxi_nodes.iter().enumerate().take(128))
                .filter(|(n, _)| end.known & (1u128 << n) != 0)
                .map(|(_, node)| node.id)
                .collect(),
            ..self.clone()
        }
    }

    /// The flight paths known, a bit per taxi node.
    pub(super) fn known_flights(&self, world: &World) -> u128 {
        let flights: HashSet<i64> = self.flights.iter().copied().collect();
        (world.taxi_nodes.iter().enumerate().take(128))
            .filter(|(_, node)| flights.contains(&node.id))
            .fold(0, |known, (n, _)| known | 1u128 << n)
    }

    /// The inn the hearthstone is bound to: the closest to where the binding was seen, else
    /// one in a place of that name.
    pub(super) fn bind_inn(&self, model: &Model, world: &World) -> Option<usize> {
        let bind = self.bind.as_ref()?;
        if let Some(p) = bind.position.and_then(|p| at(world, p)) {
            let closest = model
                .inns
                .iter()
                .enumerate()
                .filter(|(_, inn)| inn.pos.continent == p.0.continent)
                .map(|(i, inn)| (i, inn.pos.dist(&p.0)))
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((i, d)) = closest
                && d <= BIND_RADIUS
            {
                return Some(i);
            }
        }
        let name = bind.name.trim();
        let named = model.inns.iter().position(|inn| {
            world.place_name(&inn.pos).is_some_and(|n| n.eq_ignore_ascii_case(name))
                || world.zone_name(inn.zone).eq_ignore_ascii_case(name)
        });
        if named.is_some() {
            return named;
        }
        // The area of the inn (a tavern is a subzone of its town): the inns of its zone, the
        // closest to where the character stands when several.
        let from = self.position.and_then(|p| at(world, p)).map(|p| p.0);
        bind.areas
            .iter()
            .flat_map(|&a| {
                let zone = world.area_zone(a);
                model
                    .inns
                    .iter()
                    .enumerate()
                    .filter(move |(_, inn)| world.area_zone(inn.zone) == zone)
                    .map(|(i, _)| i)
            })
            .min_by(|&a, &b| match &from {
                Some(p) if model.inns[a].pos.continent == p.continent && model.inns[b].pos.continent == p.continent => {
                    model.inns[a].pos.dist(p).total_cmp(&model.inns[b].pos.dist(p))
                }
                _ => a.cmp(&b),
            })
    }
}

/// World position of a map point.
fn at(world: &World, p: MapPos) -> Option<(crate::world::Pos, i64)> {
    let zone = world.zone_of_ui_map(p.map)?;
    Some((world.to_world(zone, p.x, p.y)?, zone))
}

/// What the log says of a quest's objectives: (objective, share done) for those started. The
/// log lists them in its own order and its own language: matched by the name of their target,
/// else by the amount asked when only one objective asks it, else the only objective. One left
/// unmatched counts as not started (the guide goes to do it, the game shows it done).
fn objective_progress(q: &Quest, log: &LogQuest) -> Vec<(u8, f64)> {
    let n = q.objectives.len().min(32);
    if log.complete {
        return (0..n).map(|k| (k as u8, 1.0)).collect();
    }
    let mut free = vec![true; n];
    let mut out = Vec::new();
    for lo in &log.objectives {
        let share = if lo.finished {
            1.0
        } else if lo.need > 0 {
            (lo.done as f64 / lo.need as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let text = lo.text.to_lowercase();
        let named = |o: &Objective| {
            o.mobs
                .iter()
                .any(|(_, m)| !m.is_empty() && text.contains(&m.to_lowercase()))
                || (!o.loc.name.is_empty() && text.contains(&o.loc.name.to_lowercase()))
        };
        let counted = |o: &Objective| lo.need > 0 && (o.count - lo.need as f64).abs() < 0.5;
        let candidates = || (0..n).filter(|&k| free[k]);
        let found = candidates()
            .find(|&k| named(&q.objectives[k]) && counted(&q.objectives[k]))
            .or_else(|| candidates().find(|&k| named(&q.objectives[k])))
            .or_else(|| {
                let mut c = candidates().filter(|&k| counted(&q.objectives[k]));
                c.next().filter(|_| c.next().is_none())
            })
            .or_else(|| (n == 1 && log.objectives.len() == 1 && free[0]).then_some(0));
        if let Some(k) = found {
            free[k] = false;
            if share > 0.0 {
                out.push((k as u8, share));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ProfessionPlan;
    use crate::profession::ProfessionGoal;

    fn goal(key: &str) -> ProfessionGoal {
        ProfessionGoal {
            key: key.into(),
            target: 300,
            start_level: None,
            milestones: vec![],
        }
    }

    /// The curve of a profession the character has goes through its skill now; one it does not
    /// have yet starts from its level.
    #[test]
    fn professions_follow_the_character() {
        let start = StartState {
            level: 30,
            professions: vec![Skill { line: 186, rank: 200 }],
            ..StartState::default()
        };
        let mut goals = vec![goal("mining"), goal("herbalism")];
        start.anchor_professions(&mut goals);
        let mining = ProfessionPlan::new(&goals[0]).unwrap();
        assert_eq!(mining.skill_at(30), 200, "ahead of the default curve");
        assert!(mining.skill_at(40) >= 200, "never goes back");
        let herbalism = ProfessionPlan::new(&goals[1]).unwrap();
        assert_eq!(herbalism.skill_at(30), 0);
        assert!(herbalism.skill_at(35) > 0);
    }
}
