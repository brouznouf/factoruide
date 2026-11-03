//! Profession trainer stops (`Params::profession_trainers`): once the guide is planned, a stop to
//! learn each profession and, before the skill reaches the cap of its rank, the next rank. They
//! go only where the route already passes a trainer, so they cost the route no time; a rank whose
//! trainer the route does not pass has no stop.

use super::phrase::{Arg, Phrase};
use super::types::{RouteProfession, Step, StepKind, Target, TargetKind};
use super::writer::StepWriter;
use crate::model::ProfessionTrainer;

/// A stop to add: after `steps[after]`, the trainer teaches `rank` of `professions[profession]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stop {
    after: usize,
    profession: usize,
    rank: usize,
    trainer: usize,
}

/// A rank of a profession (apprentice to artisan): what the game asks to learn it, the skill cap it
/// gives, and the skill the curve may reach without it.
struct Rank {
    /// Character level and skill to learn it (the classic ranks; the data has no others).
    level: i64,
    skill: i64,
    cap: i64,
    /// Needed once the curve passes this: the cap of the rank before, and for the first rank
    /// the skill at which the second can be learned (the profession is learned by then).
    due: i64,
}

const RANKS: [Rank; 4] = [
    Rank {
        level: 1,
        skill: 0,
        cap: 75,
        due: 49,
    },
    Rank {
        level: 10,
        skill: 50,
        cap: 150,
        due: 75,
    },
    Rank {
        level: 20,
        skill: 125,
        cap: 225,
        due: 150,
    },
    Rank {
        level: 35,
        skill: 200,
        cap: 300,
        due: 225,
    },
];

/// A step passes by a trainer when it is this close (yards): the same town.
const NEAR: f64 = 150.0;

/// How far the route is from a trainer at `step`: None on another continent or without a place.
fn yards(step: &Step, trainer: &ProfessionTrainer) -> Option<f64> {
    let [continent, x, y] = step.world?;
    let pos = &trainer.loc.pos;
    (continent as i64 == pos.continent).then(|| (x - pos.x).hypot(y - pos.y))
}

/// The stops to add, in route order: for each rank of each profession the curve goes past (the
/// skill is above what the rank before allows), the first step in the span where the game lets
/// it be learned, and the curve has not passed the rank before, that stands by a trainer
/// teaching it.
fn stops(steps: &[Step], professions: &[RouteProfession], trainers: &[ProfessionTrainer]) -> Vec<Stop> {
    let mut out = Vec::new();
    for (p, profession) in professions.iter().enumerate() {
        let skill_at = |step: &Step| {
            profession
                .curve
                .get(step.level.clamp(0, 60) as usize)
                .copied()
                .unwrap_or(0)
        };
        for (r, rank) in RANKS.iter().enumerate() {
            // Needed from the first step the curve passes the rank before.
            let Some(due) = steps.iter().position(|s| skill_at(s) > rank.due) else {
                continue;
            };
            let open = steps
                .iter()
                .position(|s| s.level >= rank.level.max(profession.start) && skill_at(s) >= rank.skill);
            let Some(open) = open else {
                continue;
            };
            let teaching = |t: &ProfessionTrainer| t.line == profession.skill_line && t.cap >= rank.cap;
            let by = |i: usize| {
                // Not between a dungeon run and the quests done inside it.
                if steps[i].kind == StepKind::Dungeon || steps.get(i + 1).is_some_and(|s| s.inside) {
                    return None;
                }
                (trainers.iter().enumerate())
                    .filter(|(_, t)| teaching(t))
                    .filter_map(|(k, t)| Some((yards(&steps[i], t).filter(|d| *d <= NEAR)?, k)))
                    .min_by(|a, b| a.0.total_cmp(&b.0))
            };
            if let Some((after, (_, trainer))) = (open..=due.max(open)).find_map(|i| Some((i, by(i)?))) {
                out.push(Stop {
                    after,
                    profession: p,
                    rank: r,
                    trainer,
                });
            }
        }
    }
    out.sort_by_key(|s| (s.after, s.profession, s.rank));
    out
}

/// Add the stops to the steps (`xps`: the XP the route has after each), each after the step that
/// passes the trainer.
pub(super) fn add(writer: &StepWriter<'_>, steps: &mut Vec<Step>, xps: &mut Vec<i64>) {
    let planner = writer.planner;
    let (model, params) = (planner.model, planner.params);
    if !params.profession_trainers || model.profession_trainers.is_empty() {
        return;
    }
    let professions: Vec<RouteProfession> = model.professions.iter().map(super::route_profession).collect();
    // The players of a group guide having the profession (a bit each).
    let everyone = ((1u16 << (1 + planner.profile.mates.len())) - 1) as u8;
    let found = stops(steps, &professions, &model.profession_trainers);
    // From the last, so that the places stay right; those after the same step in the order found.
    for stop in found.into_iter().rev() {
        let (plan, profession) = (&model.professions[stop.profession], &professions[stop.profession]);
        let trainer = &model.profession_trainers[stop.trainer].loc;
        let at = &steps[stop.after];
        let mut step = Step::bare(StepKind::Profession, at.level, at.time);
        step.line = Some(profession.skill_line);
        step.skill = Some(RANKS[stop.rank].cap);
        step.say.push(
            Phrase::new(if stop.rank == 0 {
                "profession.learn"
            } else {
                "profession.rank"
            })
            .arg(
                "profession",
                Arg::Profession(profession.key.clone(), profession.name.clone()),
            )
            .arg("who", Arg::Npc(trainer.id, trainer.name.clone())),
        );
        step.target = Some(Target {
            kind: TargetKind::Npc,
            id: trainer.id,
            name: trainer.name.clone(),
        });
        writer.put_at(&mut step, &trainer.pos, trainer.zone);
        step.members = (plan.members != everyone).then_some(plan.members);
        let xp = xps[stop.after];
        steps.insert(stop.after + 1, step);
        xps.insert(stop.after + 1, xp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::types::StepKind;
    use crate::model::{EntityKind, Loc};
    use crate::world::Pos;

    const LINE: i64 = 197;

    /// A route walking east, one step per level 1..=60 at x = level * 1000 on continent 1.
    fn route() -> Vec<Step> {
        (1..=60)
            .map(|l| {
                let mut s = Step::bare(StepKind::Accept, l, l as f64);
                s.world = Some([1.0, l as f64 * 1000.0, 0.0]);
                s
            })
            .collect()
    }

    /// The skill to have at each level 0..=60: 7 per level from 6, 5 per level from 16, up to 300.
    fn curve() -> Vec<i64> {
        (0..=60)
            .map(|l: i64| match l {
                0..=5 => 0,
                6..=15 => (l - 5) * 7,
                _ => (70 + (l - 15) * 5).min(300),
            })
            .collect()
    }

    fn profession(curve: Vec<i64>) -> RouteProfession {
        RouteProfession {
            key: "tailoring".to_owned(),
            name: "Tailoring".to_owned(),
            skill_line: LINE,
            target: 300,
            start: 5,
            curve,
        }
    }

    /// A trainer teaching up to `cap`, standing at x on continent 1 (id = cap).
    fn trainer(cap: i64, x: f64) -> ProfessionTrainer {
        ProfessionTrainer {
            line: LINE,
            cap,
            loc: Loc {
                pos: Pos {
                    continent: 1,
                    x,
                    y: 0.0,
                    zone: 0,
                    z: None,
                },
                zone: 0,
                kind: EntityKind::Npc,
                id: cap,
                name: format!("T{cap}"),
            },
        }
    }

    /// One trainer per rank, each in the town the route reaches at about the right level.
    fn every_rank() -> Vec<ProfessionTrainer> {
        vec![
            trainer(75, 6000.0),
            trainer(150, 16000.0),
            trainer(225, 26000.0),
            trainer(300, 41000.0),
        ]
    }

    fn learned(stops: &[Stop], trainers: &[ProfessionTrainer]) -> Vec<(usize, i64)> {
        stops.iter().map(|s| (s.rank, trainers[s.trainer].cap)).collect()
    }

    #[test]
    fn every_rank_is_learned_in_order_from_a_trainer_who_teaches_it() {
        let trainers = every_rank();
        let found = stops(&route(), &[profession(curve())], &trainers);
        assert_eq!(learned(&found, &trainers), [(0, 75), (1, 150), (2, 225), (3, 300)]);
    }

    #[test]
    fn a_rank_comes_after_the_game_allows_it_and_before_the_skill_passes_the_cap_before() {
        let (steps, curve) = (route(), curve());
        let found = stops(&steps, &[profession(curve.clone())], &every_rank());
        // (the level it is learned at, the skill the curve wants then)
        let at = |rank: usize| {
            let s = found.iter().find(|s| s.rank == rank).expect("a stop");
            let level = steps[s.after].level;
            (level, curve[level as usize])
        };
        // Learned by the time the curve passes the cap before it, never before the level it needs.
        assert!(at(0).0 >= 5 && at(0).1 <= 49);
        assert!(at(1).0 >= 10 && at(1).1 >= 50 && at(1).1 <= 75);
        assert!(at(2).0 >= 20 && at(2).1 >= 125 && at(2).1 <= 150);
        assert!(at(3).0 >= 35 && at(3).1 >= 200 && at(3).1 <= 225);
    }

    #[test]
    fn no_stop_where_the_route_passes_no_trainer() {
        // Journeyman's trainer is a continent away, Expert's 3000 yards off the road.
        let mut far = trainer(150, 16000.0);
        far.loc.pos.continent = 0;
        let mut off = trainer(225, 26000.0);
        off.loc.pos.y = 3000.0;
        let trainers = vec![trainer(75, 6000.0), far, off, trainer(300, 41000.0)];
        let found = stops(&route(), &[profession(curve())], &trainers);
        assert_eq!(learned(&found, &trainers), [(0, 75), (3, 300)]);
    }

    #[test]
    fn a_trainer_whose_title_stops_at_a_lower_rank_does_not_teach_the_next() {
        // The second trainer teaches no further than the first rank: Journeyman has only the third,
        // a few yards off the same road.
        let trainers = vec![trainer(75, 6000.0), trainer(75, 16000.0), trainer(150, 16100.0)];
        let found = stops(&route(), &[profession(curve())], &trainers);
        assert_eq!(learned(&found, &trainers), [(0, 75), (1, 150)]);
        assert_eq!(found[0].trainer, 0);
        assert_eq!(found[1].trainer, 2);
    }

    #[test]
    fn a_rank_the_curve_never_needs_has_no_stop() {
        // Skill stays at 60: apprentice is enough.
        let low: Vec<i64> = curve().into_iter().map(|c| c.min(60)).collect();
        let trainers = every_rank();
        let found = stops(&route(), &[profession(low)], &trainers);
        assert_eq!(learned(&found, &trainers), [(0, 75)]);
    }

    #[test]
    fn the_trainers_of_another_profession_are_not_used() {
        let mut other = every_rank();
        for t in &mut other {
            t.line = 164;
        }
        assert_eq!(stops(&route(), &[profession(curve())], &other), Vec::<Stop>::new());
    }

    #[test]
    fn the_stop_is_at_the_first_step_that_passes_the_trainer() {
        // Two steps by the apprentice trainer: the first one gets the stop.
        let mut steps = route();
        steps[7].world = Some([1.0, 6050.0, 20.0]);
        let found = stops(&steps, &[profession(curve())], &[trainer(75, 6000.0)]);
        assert_eq!(found.len(), 1);
        assert_eq!(steps[found[0].after].level, 6);
    }

    #[test]
    fn no_stop_between_a_dungeon_run_and_its_quests() {
        let mut steps = route();
        steps[5].kind = StepKind::Dungeon;
        steps[6].inside = true;
        let found = stops(&steps, &[profession(curve())], &[trainer(75, 6000.0)]);
        // Steps 5 (the run) and 4 (followed by nothing inside) are by the trainer's road: 4 is level 5.
        assert!(found.iter().all(|s| s.after != 5 && !steps[s.after + 1].inside));
    }

    #[test]
    fn nothing_is_added_without_professions_or_trainers() {
        assert_eq!(stops(&route(), &[], &every_rank()), Vec::<Stop>::new());
        assert_eq!(stops(&route(), &[profession(curve())], &[]), Vec::<Stop>::new());
    }
}
