//! XP options through route replay: acquisition, imported completion, kills and quest rewards.

use super::{planner, quests_at_start};
use crate::job::Prepared;
use crate::model::MateStart;
use crate::plan::{Edit, Kind, Stop};
use crate::xp::{Edition, SLEEPING_BAG_QUEST};

fn prepared() -> Prepared {
    let mut p = quests_at_start(2);
    p.params.edition = Edition::Forever;
    p.profile.from_level = 3;
    p.profile.to_level = 5;
    p.model.quests[0].id = SLEEPING_BAG_QUEST;
    for q in &mut p.model.quests {
        q.level = 3;
        q.xp = 100;
    }
    p.model.index = p.model.quests.iter().enumerate().map(|(i, q)| (q.id, i)).collect();
    p
}

fn stop(index: u32, kind: Kind) -> Edit {
    Edit::Stop {
        stop: Stop::quest(index, kind),
        farm: false,
        spot: None,
    }
}

#[test]
fn food_increases_kills_but_not_quests() {
    for (rested, kills) in [(0, [63, 63, 63]), (100, [126, 100, 63])] {
        let mut p = prepared();
        p.model.initial.rested = rested;
        let edits = [
            Edit::Grind { mobs: 1 },
            stop(1, Kind::Accept),
            stop(1, Kind::TurnIn),
            Edit::Grind { mobs: 1 },
            Edit::Grind { mobs: 1 },
        ];
        let base = planner(&p).edit(&edits);
        p.params.food_xp = true;
        let report = planner(&p).edit(&edits);
        assert_eq!(report.errors(), 0);
        assert_eq!([report.steps[0].xp, report.steps[3].xp, report.steps[4].xp], kills);
        assert_eq!(report.steps[2].xp, 100);
        assert_eq!(report.steps[0].end, base.steps[0].end, "no food upkeep time");
    }
}

#[test]
fn sleeping_bag_starts_after_its_reward_and_survives_profile_import() {
    for (food, before, after) in [(false, 60, 62), (true, 63, 65)] {
        let mut p = prepared();
        p.params.sleeping_bag = true;
        p.params.food_xp = food;
        let report = planner(&p).edit(&[
            Edit::Grind { mobs: 1 },
            stop(0, Kind::Accept),
            stop(0, Kind::TurnIn),
            Edit::Grind { mobs: 1 },
            stop(1, Kind::Accept),
            stop(1, Kind::TurnIn),
        ]);
        assert_eq!(report.errors(), 0);
        assert_eq!(report.steps[0].xp, before, "no bag before the quest");
        assert_eq!(report.steps[2].xp, 100, "the reward does not boost itself");
        assert_eq!(report.steps[3].xp, after);
        assert_eq!(report.steps[5].xp, 103);
        assert_eq!(
            report.steps[3].end - report.steps[3].start,
            report.steps[0].end - report.steps[0].start
        );
        p.model.initial.completed = Some([SLEEPING_BAG_QUEST].into());
        let imported = planner(&p).edit(&[Edit::Grind { mobs: 1 }, stop(1, Kind::Accept), stop(1, Kind::TurnIn)]);
        assert_eq!(imported.steps[0].xp, after);
        assert_eq!(imported.steps[2].xp, 103);
    }
}

#[test]
fn buffed_grinding_reaches_the_same_level_faster_and_consumes_rested_xp() {
    let mut p = prepared();
    p.profile.to_level = 4;
    let (_, unrested) = planner(&p).simulate_full(&[], None).unwrap();
    p.model.initial.rested = 200;
    let (_, base) = planner(&p).simulate_full(&[], None).unwrap();
    let end = planner(&p).end_of(&[]).unwrap();
    assert!((unrested.grinding / base.grinding - 7.0 / 6.0).abs() < 1e-9);
    p.params.food_xp = true;
    p.params.sleeping_bag = true;
    p.model.initial.completed = Some([SLEEPING_BAG_QUEST].into());
    let (_, boosted) = planner(&p).simulate_full(&[], None).unwrap();
    let buffed_end = planner(&p).end_of(&[]).unwrap();
    assert!((base.grinding / boosted.grinding - 1.08).abs() < 1e-9);
    assert_eq!((end.level, end.xp, end.rested), (4, 0, 0));
    assert_eq!((buffed_end.level, buffed_end.xp, buffed_end.rested), (4, 0, 0));
}

#[test]
fn rested_xp_can_make_a_nearby_guarded_object_cheaper_than_a_distant_safe_one() {
    let mut p = super::camp_of_two_objectives();
    p.profile.from_level = 3;
    p.profile.to_level = 5;
    p.params.class_curve.clear();
    p.params.grind_kill_time = 60.0;
    p.params.detour = 1.0;
    p.model.initial.accepted = vec![0];
    p.model.quests[0].level = 3;
    let object = &mut p.model.quests[0].objectives[0];
    object.kills = 0.0;
    object.uses = 1.0;
    object.guard = crate::model::Guard {
        level: 7,
        count: 1,
        elite: false,
    };
    let near = object.loc.pos;
    let mut safe = object.clone();
    safe.guard = crate::model::Guard::default();
    safe.loc.pos.x += 4500.0;
    let far = safe.loc.pos;
    object.alts = vec![safe];
    for (rested, chosen) in [(0, far), (200, near)] {
        p.model.initial.rested = rested;
        let report = planner(&p).edit(&[stop(0, Kind::Objective(0))]);
        assert_eq!(report.errors(), 0);
        let visited = report.trace.iter().find_map(|t| match &t.event {
            crate::plan::Event::Stop { stop, loc } if stop.kind == Kind::Objective(0) => Some(loc.pos),
            _ => None,
        });
        assert_eq!(visited, Some(chosen));
    }
}

#[test]
fn bag_completion_stays_with_each_member_when_the_lowest_player_changes() {
    let mut p = prepared();
    p.params.sleeping_bag = true;
    p.params.group_size = 2;
    p.model.initial.completed = Some([SLEEPING_BAG_QUEST].into());
    p.model.initial.mates = vec![MateStart {
        level: 3,
        ..MateStart::default()
    }];
    p.model.initial.need = vec![3, 3];
    let report = planner(&p).edit(&[
        Edit::Grind { mobs: 1 },
        Edit::Grind { mobs: 1 },
        stop(1, Kind::Accept),
        stop(1, Kind::TurnIn),
    ]);
    assert_eq!(report.errors(), 0);
    let kills: Vec<_> = report
        .trace
        .iter()
        .filter(|t| matches!(t.event, crate::plan::Event::GrindMobs { .. }))
        .collect();
    assert_eq!(kills[1].party, [(3, 62), (3, 60)]);
    let reward = report
        .trace
        .iter()
        .find(|t| matches!(t.event, crate::plan::Event::Stop { stop, .. } if stop.kind == Kind::TurnIn))
        .unwrap();
    assert_eq!(reward.party, [(3, 165), (3, 160)]);
}

#[test]
fn forever_options_do_not_change_other_editions() {
    for edition in [Edition::Classic, Edition::Tbc] {
        let mut p = prepared();
        p.params.edition = edition;
        p.params.food_xp = true;
        p.params.sleeping_bag = true;
        p.model.initial.completed = Some([SLEEPING_BAG_QUEST].into());
        let report = planner(&p).edit(&[Edit::Grind { mobs: 1 }, stop(1, Kind::Accept), stop(1, Kind::TurnIn)]);
        assert_eq!(report.steps[0].xp, 60);
        assert_eq!(report.steps[2].xp, 100);
    }
}

#[test]
fn buffs_reduce_catch_up_time_for_the_member_who_has_them_and_spend_rested_xp() {
    for (food, bag, mates, factor, gained) in [
        (true, false, 1, 1.05, vec![26, 26]),
        (false, true, 1, 1.03, vec![25, 26]),
        (true, false, 2, 1.05, vec![20, 20, 20]),
        (false, true, 2, 1.0, vec![19, 20, 19]),
    ] {
        let mut p = super::duo(100);
        p.params.edition = Edition::Forever;
        p.params.group_size = 1 + mates as i64;
        (p.params.group_sync_window, p.params.group_sync_max) = (0.05, 0.1);
        p.model.initial.need = vec![1, 1];
        p.model.initial.mates = vec![
            MateStart {
                level: 1,
                rested: 50,
                ..MateStart::default()
            };
            mates
        ];
        let (_, base) = planner(&p).simulate_full(&super::BOTH, None).unwrap();
        p.params.food_xp = food;
        p.params.sleeping_bag = bag;
        p.model.initial.mates[0].completed.insert(SLEEPING_BAG_QUEST);
        let (_, boosted) = planner(&p).simulate_full(&super::BOTH, None).unwrap();
        assert!(base.catch_up > 0.0);
        assert!((base.catch_up / boosted.catch_up - factor).abs() < 1e-9);
        let mut edits: Vec<_> = super::BOTH.iter().map(|s| stop(s.index, s.kind)).collect();
        edits.push(Edit::Grind { mobs: 1 });
        let report = planner(&p).edit(&edits);
        assert_eq!(report.errors(), 0);
        let at = report
            .trace
            .iter()
            .position(|t| matches!(t.event, crate::plan::Event::GrindMobs { .. }))
            .unwrap();
        let before = report.trace[..at]
            .iter()
            .rev()
            .find(|t| matches!(t.event, crate::plan::Event::CatchUp { .. }))
            .unwrap();
        let gains: Vec<_> = before
            .party
            .iter()
            .zip(&report.trace[at].party)
            .map(|(&(level, xp), &(after_level, after_xp))| {
                assert_eq!(level, after_level);
                after_xp - xp
            })
            .collect();
        assert_eq!(gains, gained, "catch-up exhausted each rested pool before this kill");
    }
}
