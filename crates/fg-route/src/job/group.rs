//! A group guide: one route for several players, each starting from what its addon recorded. It
//! is planned for the one with the least XP (`PlanRequest::members`); the others follow the same
//! stops, each getting the XP of the quests still for it.

use super::StartState;
use crate::model::{MateStart, Model};
use crate::world::World;
use std::collections::HashSet;

/// The other members joined to the character's initial state (already in `model.initial`):
/// which quests are still for whom, the log they share, the flights they all know, whether their
/// hearthstones are bound to the same inn.
pub(super) fn join(
    model: &mut Model,
    world: &World,
    character: &StartState,
    mates: &[&StartState],
    notes: &mut Vec<String>,
) {
    let init = &mut model.initial;
    init.mates = mates
        .iter()
        .map(|m| MateStart {
            level: m.level,
            xp: m.xp,
            rested: m.rested,
            completed: m.completed.iter().map(|q| q.0).collect(),
        })
        .collect();
    for m in mates {
        init.known &= m.known_flights(world);
    }
    // A shared profession rank is known only as far as every player leveling it. When some
    // caps were recorded, infer each older profile's cap from its own skill, rather than letting
    // the leading character's skill override another player's reported lower cap.
    init.profession_caps = model
        .professions
        .iter()
        .filter_map(|plan| {
            let line = crate::profession::find(&plan.key)?.skill_id;
            let skills: Vec<_> = std::iter::once(character)
                .chain(mates.iter().copied())
                .enumerate()
                .filter(|(member, _)| plan.members & (1 << member) != 0)
                .map(|(_, member)| member.professions.iter().find(|skill| skill.line == line))
                .collect();
            if !skills
                .iter()
                .any(|skill| skill.is_some_and(|skill| skill.cap.is_some()))
            {
                return None;
            }
            skills
                .into_iter()
                .map(|skill| {
                    skill.map_or(0, |skill| {
                        skill.cap.unwrap_or_else(|| {
                            crate::plan::RANKS
                                .iter()
                                .enumerate()
                                .filter(|(index, rank)| {
                                    if *index == 0 {
                                        skill.rank > 0
                                    } else {
                                        skill.rank > rank.due
                                    }
                                })
                                .map(|(_, rank)| rank.cap)
                                .max()
                                .unwrap_or(0)
                        })
                    })
                })
                .min()
                .map(|cap| (line, cap))
        })
        .collect();
    let bind = character.bind_inn(model, world);
    if bind.is_none() || mates.iter().any(|m| m.bind_inn(model, world) != bind) {
        model.initial.unbound = true;
        notes.push("hearthstones bound to different inns: the group binds them together first".into());
    }
    let init = &model.initial;
    let need: Vec<u8> = (model.quests.iter())
        .map(|q| {
            (0..=mates.len())
                .filter(|&m| {
                    let done = |id: &i64| init.done_by(m, *id);
                    q.members & (1 << m) != 0
                        && !done(&q.id)
                        && !q.exclusive.iter().any(done)
                        && !q.breadcrumb_for.as_ref().is_some_and(done)
                })
                .fold(0, |need, m| need | 1 << m)
        })
        .collect();
    // In the log of everyone it is for: the group carries on with it, as far as the character
    // got. The others are taken again by the group (who has one already just goes along).
    let logs: Vec<HashSet<i64>> = mates.iter().map(|m| m.log.iter().map(|q| q.id.0).collect()).collect();
    let shared = |i: usize| {
        let id = model.quests[i].id;
        need[i] & 1 != 0 && (0..mates.len()).all(|k| need[i] & (2 << k) == 0 || logs[k].contains(&id))
    };
    let init = &mut model.initial;
    init.accepted.retain(|&i| shared(i));
    init.progress.retain(|p| init.accepted.contains(&p.0));
    let index = &model.index;
    init.log.retain(|(id, _)| {
        index
            .get(id)
            .is_none_or(|&i| init.accepted.contains(&i) || need[i] == 0)
    });
    init.turned = (0..need.len()).filter(|&i| need[i] == 0).collect();
    init.need = need;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::Skill;
    use crate::model::ProfessionPlan;
    use crate::profession::ProfessionGoal;

    #[test]
    fn profession_caps_require_every_owner_but_ignore_other_members() {
        let player = |(cap, rank)| StartState {
            level: 25,
            professions: vec![Skill { line: 185, rank, cap }],
            ..StartState::default()
        };
        for (leader, mate, owners, expected) in [
            ((Some(225), 150), (Some(150), 150), 0b11, vec![(185, 150)]),
            ((Some(150), 150), (Some(225), 150), 0b11, vec![(185, 150)]),
            ((Some(225), 125), (Some(225), 125), 0b11, vec![(185, 225)]),
            ((Some(225), 125), (None, 150), 0b11, vec![(185, 150)]),
            ((None, 200), (Some(150), 150), 0b11, vec![(185, 150)]),
            ((None, 150), (None, 150), 0b11, vec![]),
            ((None, 150), (Some(225), 125), 0b10, vec![(185, 225)]),
            ((Some(225), 125), (None, 150), 0b01, vec![(185, 225)]),
        ] {
            let mut prepared = crate::testing::Scenario::new().start(player(leader)).build();
            let mut plan = ProfessionPlan::new(&ProfessionGoal {
                key: "cooking".into(),
                target: 225,
                start_level: None,
                milestones: vec![],
            })
            .unwrap();
            plan.members = owners;
            prepared.model.professions = vec![plan];
            join(
                &mut prepared.model,
                &prepared.world,
                &player(leader),
                &[&player(mate)],
                &mut Vec::new(),
            );
            assert_eq!(prepared.model.initial.profession_caps, expected);
        }
    }
}
