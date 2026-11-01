//! Planner behavior on the Classic database: reproductions of the problems met on real guides.

use crate::common::{database, database_of, leveling, repo};
use fg_route::job::{self, Bind, LogObjective, LogQuest, MapPos, Overrides, PlanRequest, Prepared, StartState};
use fg_route::model::{CommonLoot, EntityKind, Guard, Initial, Objective, Spots};
use fg_route::params::ReputationGoal;
use fg_route::plan::{Edit, Event, Kind, Outcome, Planner, Stop};
use fg_route::testing::{At, Obj, Q, Scenario};
use fg_route::xp::Edition;
use rusqlite::Connection;
use std::sync::OnceLock;

fn prepare(race: &str, class: &str, to_level: i64, params: &[&str]) -> Prepared {
    prepare_from(race, class, to_level, params, None)
}

/// The same for a character met in game (`start`).
fn prepare_from(race: &str, class: &str, to_level: i64, params: &[&str], start: Option<StartState>) -> Prepared {
    prepare_group(race, class, to_level, params, start, vec![])
}

/// The same leveling with other players (`mates`).
fn prepare_group(
    race: &str,
    class: &str,
    to_level: i64,
    params: &[&str],
    start: Option<StartState>,
    mates: Vec<StartState>,
) -> Prepared {
    prepare_request(&PlanRequest {
        mates,
        start,
        ..request(race, class, to_level, params)
    })
}

/// A player as its addon would record it: race ID, class, level, nothing done.
fn player(name: &str, race: i64, class: &str, level: i64) -> StartState {
    StartState {
        name: name.into(),
        race,
        class: class.to_uppercase(),
        level,
        ..StartState::default()
    }
}

fn overrides() -> Overrides {
    Overrides::load_edition(&repo().join("overrides"), Edition::Classic).unwrap()
}

fn request(race: &str, class: &str, to_level: i64, params: &[&str]) -> PlanRequest {
    let params: Vec<String> = params.iter().map(|p| (*p).to_owned()).collect();
    PlanRequest {
        race: race.into(),
        class: class.into(),
        from_level: 1,
        to_level,
        name: None,
        params: Some(overrides().params.with_overrides(&params).unwrap()),
        required_class_quests: None,
        required_quests: vec![],
        professions: vec![],
        locale: None,
        mates: vec![],
        mate_professions: vec![],
        start: None,
    }
}

fn prepare_request(request: &PlanRequest) -> Prepared {
    job::prepare(&database(), &overrides(), request, &|_| {}).unwrap()
}

/// Orc warlock to 30, cautious: shared by the tests below.
fn orc_warlock() -> &'static Prepared {
    static PREPARED: OnceLock<Prepared> = OnceLock::new();
    PREPARED.get_or_init(|| prepare("orc", "warlock", 30, &["progression=cautious"]))
}

fn human_warrior() -> &'static Prepared {
    static PREPARED: OnceLock<Prepared> = OnceLock::new();
    PREPARED.get_or_init(|| prepare("human", "warrior", 30, &["progression=cautious"]))
}

/// A leveling scenario to 30, shared by the tests that look at any route of it.
fn generic() -> &'static Prepared {
    static PREPARED: OnceLock<Prepared> = OnceLock::new();
    PREPARED.get_or_init(|| {
        let mut s = leveling(30)
            // A follow-up quest.
            .quest(Q::new(500, "First").level(3).giver(1, 30.0, 40.0).kill("Beast", 3, 4, 1, 32.0, 70.0))
            .quest(Q::new(501, "Second").level(4).after_any(&[500]).giver(1, 31.0, 40.0).kill("Beast", 4, 4, 1, 33.0, 70.0));
        // A level where a dozen quests open: a checkpoint.
        for j in 0..12 {
            let x = 20.0 + 3.0 * f64::from(j);
            s = s.quest(Q::new(600 + i64::from(j), &format!("Burst {j}")).level(24).min_level(24).giver(2, x, 20.0).kill("Beast", 24, 4, 2, x, 80.0));
        }
        s.build()
    })
}

fn planner(p: &Prepared) -> Planner<'_> {
    Planner::new(&p.model, &p.world, &p.params, &p.profile)
}

fn quest(p: &Prepared, id: i64) -> Option<u32> {
    p.model.index.get(&id).map(|&i| i as u32)
}

/// Level 1 human warrior with only `n` quests of its own: taken and turned in at the start NPC,
/// nothing to do, no XP (each test sets what it checks). Nothing done on the way.
fn quests_at_start(n: usize) -> Prepared {
    quests_at_start_as("human", "warrior", n)
}

/// `quests_at_start` for another race and class (the race sets the side).
fn quests_at_start_as(race: &str, class: &str, n: usize) -> Prepared {
    let _ = class;
    let faction = if ["human", "dwarf", "gnome", "nightelf"].contains(&race) {
        fg_route::faction::Faction::Alliance
    } else {
        fg_route::faction::Faction::Horde
    };
    // Made-up quests anywhere for a level 1: the zones too strong for it do not count here.
    let mut scenario = Scenario::new()
        .levels(1, 2)
        .faction(faction)
        .zone(12, "Elwynn Forest")
        .zone(1537, "Ironforge")
        .params(|p| {
            (p.zone_gap, p.camp_radius, p.farm_on_way, p.flight_learn_radius) = (0.0, 0.0, false, 0.0);
        });
    for i in 0..n {
        scenario = scenario.quest(
            Q::new(900_000 + i as i64, &format!("quest {i}"))
                .xp(0)
                .observed()
                .giver(1, 50.0, 50.0),
        );
    }
    scenario.build()
}

/// `quests_at_start` on the real data of Classic (its world, its teleports, its recall): for the tests of what
/// the data holds.
fn quests_at_start_on_the_data_as(race: &str, class: &str, n: usize) -> Prepared {
    // Made-up quests anywhere for a level 1: the zones too strong for it do not count here.
    let mut p = prepare(race, class, 2, &["zone_gap=0"]);
    let here = p.model.start.clone();
    let template = p.model.quests[0].clone();
    p.model.quests = (0..n)
        .map(|i| {
            let mut q = template.clone();
            (q.id, q.name, q.level, q.min_level, q.xp, q.xp_observed) =
                (900_000 + i as i64, format!("quest {i}"), 1, 1, 0, true);
            (q.starts, q.ends) = (vec![here.clone()], vec![here.clone()]);
            q.objectives.clear();
            (q.pre_all, q.pre_any, q.exclusive, q.breadcrumb_for) = (vec![], vec![], vec![], None);
            (q.start_kills, q.start_uses, q.mandatory, q.skill, q.power) = (0.0, 0.0, false, None, None);
            (q.start_guard, q.end_guard) = (Guard::default(), Guard::default());
            (q.reputation, q.zone_forced) = (vec![], false);
            q
        })
        .collect();
    p.model.index = p.model.quests.iter().enumerate().map(|(i, q)| (q.id, i)).collect();
    (p.model.chain_top, p.model.unlocks, p.model.rewards) = (vec![1; n], vec![0.0; n], vec![(vec![], vec![]); n]);
    p.model.initial = Initial::default();
    p.model.power = None;
    p.model.trainers.clear();
    p.model.explore.clear();
    p.model.explore_by_zone.clear();
    (p.params.camp_radius, p.params.farm_on_way, p.params.flight_learn_radius) = (0.0, false, 0.0);
    p
}

/// The leveling scenario with a dungeon (the Deadmines, area 900) and three quests done inside it.
fn with_deadmines(scenario: Scenario) -> Scenario {
    let mut s = scenario.dungeon("The Deadmines", 1, 70.0, 70.0);
    for (id, name) in [
        (300, "Red Silk Bandanas"),
        (301, "Collecting Memories"),
        (302, "The Defias Brotherhood"),
    ] {
        s = s.quest(
            Q::new(id, name)
                .level(18)
                .giver(1, 40.0 + id as f64 - 300.0, 40.0)
                .objective(
                    Obj::kill(
                        "Defias",
                        18,
                        6,
                        At {
                            zone: 1,
                            x: 70.0,
                            y: 70.0,
                        },
                    )
                    .inside(0),
                ),
        );
    }
    s
}

/// An objective's mobs are killed once, at its stop.
#[test]
fn objective_finished_on_the_way_is_not_done_twice() {
    let p = &orc_valley().build();
    let planner = planner(p);
    let boars = quest(p, 788).unwrap();
    let accept = Stop::quest(boars, Kind::Accept);
    let objective = Stop::quest(boars, Kind::Objective(0));
    let (_, once) = planner.simulate_full(&[accept, objective], None).unwrap();
    let kills = p.model.quests[boars as usize].objectives[0].kills;
    let one_kill = p
        .params
        .kill_time(1, 0.0, p.model.quests[boars as usize].objectives[0].mob_level);
    assert!(
        once.fighting < kills * one_kill * 1.6,
        "{} s of fighting for {kills} kills of {one_kill:.0} s",
        once.fighting
    );
}

/// Elite camps need a few more levels than a lone elite, not a multiple of the level.
#[test]
fn elite_camp_needs_a_few_levels_more() {
    let p = human_warrior();
    let planner = planner(p);
    let tharilzun = quest(p, 19).unwrap();
    let q = &p.model.quests[tharilzun as usize];
    let (k, o) = q.objectives.iter().enumerate().find(|(_, o)| o.elite).unwrap();
    let need = planner.combat().need(q, o);
    assert!(
        need <= f64::from(o.mob_level as i32) + 2.0 + 2.5,
        "Tharil'zun needs {need}"
    );
    assert!(
        planner
            .combat()
            .level(q, k as u8, fg_route::params::Might { other: 0.0, gear: 0.0 })
            <= 28
    );
}

/// An item only another quest's turn-in hands (the bound chapters of The Green Hills of
/// Stranglethorn) is got there once that quest is turned in: the book is planned, and its
/// chapter cannot be counted before.
#[test]
fn item_handed_by_another_quest_waits_for_its_turn_in() {
    let p = leveling(30)
        .quest(
            Q::new(583, "Jungle")
                .level(5)
                .giver(1, 30.0, 40.0)
                .kill("Beast", 5, 3, 1, 32.0, 70.0),
        )
        .quest(
            Q::new(338, "The Green Hills")
                .level(6)
                .after(&[583])
                .giver(1, 31.0, 40.0)
                .objective(
                    Obj::visit(
                        "Chapter IV",
                        At {
                            zone: 1,
                            x: 31.0,
                            y: 40.0,
                        },
                    )
                    .handed_by(&[342]),
                ),
        )
        .quest(
            Q::new(342, "Chapter IV")
                .level(6)
                .after(&[583])
                .giver(1, 31.0, 41.0)
                .kill("Page", 6, 2, 1, 35.0, 70.0),
        )
        .build();
    let planner = planner(&p);
    let (book, chapter, jungle) = (
        quest(&p, 338).unwrap(),
        quest(&p, 342).unwrap(),
        quest(&p, 583).unwrap(),
    );
    let k = p.model.quests[book as usize]
        .objectives
        .iter()
        .position(|o| o.handed_by == [342])
        .expect("an objective handed by Chapter IV") as u8;
    let take = [
        Stop::quest(jungle, Kind::Accept),
        Stop::quest(jungle, Kind::Objective(0)),
        Stop::quest(jungle, Kind::TurnIn),
        Stop::quest(book, Kind::Accept),
        Stop::quest(chapter, Kind::Accept),
    ];
    let pages: Vec<Stop> = (0..p.model.quests[chapter as usize].objectives.len() as u8)
        .map(|j| Stop::quest(chapter, Kind::Objective(j)))
        .collect();
    let bound = Stop::quest(book, Kind::Objective(k));
    let early: Vec<Stop> = take.iter().copied().chain([bound]).collect();
    assert!(
        planner.simulate_full(&early, None).is_none(),
        "chapter counted before it is bound"
    );
    let route: Vec<Stop> = take
        .iter()
        .copied()
        .chain(pages)
        .chain([Stop::quest(chapter, Kind::TurnIn), bound])
        .collect();
    assert!(
        planner.simulate_full(&route, None).is_some(),
        "{:?}",
        planner.diagnose(&route)
    );
}
/// The item starting a quest drops from a mob the character must be able to take on (a
/// warlock, stronger than most classes in its twenties, a level earlier).
#[test]
fn item_start_waits_for_its_mob() {
    let p = orc_warlock();
    let owatanka = quest(p, 884).unwrap();
    assert!(p.model.quests[owatanka as usize].min_level >= 21);
}

/// The accept step of a quest started by an item a mob drops names that mob, and gives it to the
/// addon's target macro: the place alone shows nothing to loot.
#[test]
fn item_start_names_its_mob() {
    let p = orc_warlock();
    let owatanka = &p.model.quests[quest(p, 884).unwrap() as usize];
    let Some((mob, name)) = owatanka.start_mob.clone() else {
        panic!("Owatanka's Tailspike drops from a mob")
    };
    let planner = planner(p);
    let route = planner.construct(|_| {});
    let guide = fg_route::export::build(&planner, &p.profile, &route);
    let looted: Vec<_> = guide
        .steps
        .iter()
        .filter(|s| s.kind == fg_route::export::StepKind::Accept)
        .filter_map(|s| Some((s, p.model.quests[p.model.index[&s.quest?]].start_mob.clone()?)))
        .collect();
    assert!(!looted.is_empty(), "a quest started by a dropped item in the route");
    for (step, (id, mob)) in looted {
        assert!(step.text.starts_with(&format!("Kill {mob} and loot ")), "{}", step.text);
        assert_eq!((&step.mobs, &step.mob_ids), (&vec![mob.clone()], &vec![id]));
    }
    // Mobs living in several places: the others, for when nothing is at the step's.
    assert!(
        guide
            .steps
            .iter()
            .any(|s| s.kind == fg_route::export::StepKind::Objective && !s.spots.is_empty()),
        "objectives with other places"
    );
    assert!(mob > 0 && !name.is_empty());
}

/// A quest started by an item dropping less than 10% of the time is luck, not a plan.
#[test]
fn rare_drop_start_is_left_out() {
    assert!(
        quest(human_warrior(), 136).is_none(),
        "Captain Sanders' Hidden Treasure"
    );
}

/// A reputation goal brings in the quests giving that reputation, even worth nothing else (no
/// XP); a quest of another faction stays out, as every one does without goals.
#[test]
fn reputation_goal_takes_its_factions_quests() {
    let taken = |goals: Vec<ReputationGoal>| {
        let mut p = leveling(2)
            .quest(
                Q::new(800, "For Stormwind")
                    .level(1)
                    .xp(0)
                    .reputation(72, 250.0)
                    .giver(1, 30.0, 40.0),
            )
            .quest(
                Q::new(801, "For Ironforge")
                    .level(1)
                    .xp(0)
                    .reputation(69, 250.0)
                    .giver(1, 31.0, 40.0),
            )
            .build();
        p.model.initial.reputation = vec![4000.0; goals.len()];
        p.params.reputation_goals = goals;
        let (a, b) = (quest(&p, 800).unwrap(), quest(&p, 801).unwrap());
        let planner = planner(&p);
        let route = planner.construct(|_| {});
        let turned = |i| route.iter().any(|s| s.index == i && s.kind == Kind::TurnIn);
        ((turned(a), turned(b)), planner.reputation_reached(&route))
    };
    assert_eq!(
        taken(vec![ReputationGoal {
            faction: 72,
            target: 9000
        }]),
        ((true, false), vec![4250.0])
    );
    assert_eq!(taken(vec![]), ((false, false), vec![]));
}

/// A character starts with its race's reputation (Stormwind: a human's 4000).
#[test]
fn a_human_starts_with_stormwinds_reputation() {
    let goal = r#"reputation_goals=[{"faction":72,"target":9000}]"#;
    assert_eq!(
        prepare("human", "warrior", 2, &[goal]).model.initial.reputation,
        vec![4000.0]
    );
}
/// A forced zone's quests are all required: those filed under it or a subzone (Shadowglen's in
/// Teldrassil), class quests given there (the night elf warrior's Simple Sigil), not those
/// taken after the target level nor another zone's.
#[test]
fn forced_zone_quests_are_required() {
    let p = prepare("nightelf", "warrior", 8, &["forced_zones=[141]"]);
    let forced = |id| {
        quest(&p, id).is_some_and(|i| {
            let q = &p.model.quests[i as usize];
            q.zone_forced && q.mandatory
        })
    };
    assert!(forced(456) && forced(916), "Shadowglen's");
    assert!(forced(2459), "Teldrassil's");
    assert!(forced(3116), "a class quest given in Shadowglen");
    assert!(!forced(941), "taken from level 9");
    assert!(!forced(983), "Darkshore's");
    let none = prepare("nightelf", "warrior", 8, &[]);
    assert!(none.model.quests.iter().all(|q| !q.zone_forced));
}

/// The guide goes on past the target level until the quests of the forced zones are done: a
/// quest after the level is reached counts, where a class quest there does not.
#[test]
fn forced_zone_goes_past_the_target_level() {
    let missing = |zone: bool| {
        let mut p = quests_at_start(2);
        p.model.quests[0].xp = 10_000;
        p.model.quests[1].mandatory = true;
        p.model.quests[1].zone_forced = zone;
        let planner = planner(&p);
        let route: Vec<Stop> = [
            (0, Kind::Accept),
            (0, Kind::TurnIn),
            (1, Kind::Accept),
            (1, Kind::TurnIn),
        ]
        .into_iter()
        .map(|(i, k)| Stop::quest(i, k))
        .collect();
        let (_, b) = planner.simulate_full(&route, None).unwrap();
        (b.missing_class_quests, b.missing_zone_quests)
    };
    assert_eq!(missing(true), (0, 0));
    assert_eq!(missing(false), (1, 0));
}

/// A reputation goal not met at the target level keeps the guide going: the reputation of a
/// quest done after the level is reached counts, as long as the goal is not met.
#[test]
fn reputation_goal_goes_past_the_target_level() {
    let reached = |target: i64| {
        let mut p = quests_at_start(3);
        p.model.quests[0].xp = 10_000;
        p.model.quests[1].reputation = vec![(72, 250.0)];
        p.model.quests[2].reputation = vec![(72, 250.0)];
        p.model.initial.reputation = vec![4000.0];
        p.params.reputation_goals = vec![ReputationGoal { faction: 72, target }];
        let planner = planner(&p);
        let route: Vec<Stop> = (0..3)
            .flat_map(|i| [Stop::quest(i, Kind::Accept), Stop::quest(i, Kind::TurnIn)])
            .collect();
        planner.reputation_reached(&route)[0]
    };
    assert_eq!(reached(4100), 4250.0, "the guide ends once the goal is met");
    assert_eq!(reached(9000), 4500.0, "every quest after the level counts towards it");
}

/// Creatures acted on without being killed (`talk` objectives: healed, talked to) are visited,
/// not fought: no kills, one use each among their places, with the step's own text (Forever's
/// Rediscovering the Light, GitHub #17). Here Kobold Camp Cleanup's vermin, made such a target
/// in a copy of the objectives that shadows the database's.
#[test]
fn talked_to_creatures_are_not_killed() {
    let conn = database();
    conn.execute_batch(
        "CREATE TEMP TABLE m_quest_objective AS SELECT * FROM main.m_quest_objective;
         UPDATE temp.m_quest_objective SET type = 'talk', text = 'Heal Kobold Vermin' WHERE quest_id = 7;",
    )
    .unwrap();
    let p = job::prepare(&conn, &overrides(), &request("human", "warrior", 6, &[]), &|_| {}).unwrap();
    let o = &p.model.quests[quest(&p, 7).unwrap() as usize].objectives;
    assert_eq!(o.len(), 1);
    assert_eq!((o[0].kills, o[0].uses, o[0].count), (0.0, 10.0, 10.0));
    assert_eq!((o[0].loc.kind, o[0].loc.id), (EntityKind::Npc, 6));
    assert_eq!(o[0].text, "Heal Kobold Vermin");
}

/// Bitter Rivals has two child quests (the guarded barrel, Distracting Jarven): each is offered
/// while it is in the log.
#[test]
fn a_child_quest_is_taken_while_its_parent_is_in_the_log() {
    let conn = database();
    let p = job::prepare(&conn, &overrides(), &request("dwarf", "warrior", 10, &[]), &|_| {}).unwrap();
    for child in [308, 403] {
        let q = &p.model.quests[quest(&p, child).unwrap() as usize];
        assert_eq!(q.pre_held, [310], "{}", q.name);
    }
}

/// The Lazy Peons are woken with the Foreman's Blackjack, Sentinel Shaya healed: NPCs of the
/// character's own side are acted on, worded as the quest says, never "Kill".
#[test]
fn own_side_npcs_are_acted_on_not_killed() {
    let conn = database();
    let p = job::prepare(&conn, &overrides(), &request("orc", "warrior", 8, &[]), &|_| {}).unwrap();
    let o = &p.model.quests[quest(&p, 5441).unwrap() as usize].objectives;
    assert_eq!(o[0].text, "Use Foreman's Blackjack on Lazy Peon");
    assert_eq!((o[0].kills, o[0].loc.id), (0.0, 10556));
    conn.execute_batch(
        "CREATE TEMP TABLE m_quest AS SELECT * FROM main.m_quest;
         UPDATE temp.m_quest SET requirements = '[\"Heal and fortify Sentinel Shaya\"]' WHERE id = 5621;",
    )
    .unwrap();
    let p = job::prepare(&conn, &overrides(), &request("nightelf", "priest", 10, &[]), &|_| {}).unwrap();
    let o = &p.model.quests[quest(&p, 5621).unwrap() as usize].objectives;
    assert_eq!(
        (o[0].text.as_str(), o[0].kills, o[0].uses),
        ("Heal and fortify Sentinel Shaya", 0.0, 1.0)
    );
}

/// The Aged Envelope lies in a camp of Kul Tiras marines, who like only the Alliance (GitHub #83):
/// for a Horde character the item is guarded, so the quest it starts waits for a level that can
/// face them, not for the first one.
#[test]
fn an_item_lying_in_a_camp_of_the_other_side_is_guarded() {
    let p = prepare("orc", "hunter", 12, &["progression=cautious"]);
    let guard = p.model.quests[quest(&p, 830).unwrap() as usize].start_guard;
    assert!(guard.level >= 6 && guard.count >= 3, "{guard:?}");
}

/// The Deadmines' Collecting Memories and Oh Brother... are filed under the dungeon but their
/// items drop from elites in the quarry beside its entrance (GitHub #82): done with the dungeon's
/// group, at that place; Red Silk Bandanas also drops inside and stays a dungeon objective.
#[test]
fn quests_of_a_dungeon_done_beside_its_entrance_keep_their_place() {
    let conn = database_of("forever");
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let p = job::prepare(&conn, &overrides, &request("human", "warrior", 24, &[]), &|_| {}).unwrap();
    let objective = |id| &p.model.quests[quest(&p, id).unwrap() as usize].objectives[0];
    for id in [168, 167] {
        let o = objective(id);
        assert!(
            o.dungeon.is_some() && o.loc.kind == EntityKind::Item,
            "{id}: {:?}",
            o.loc.kind
        );
    }
    assert_eq!(objective(214).loc.kind, EntityKind::Dungeon);
}

/// The warlock's imp is taken first: it makes every fight that follows easier.
#[test]
fn pet_quest_comes_early() {
    let request = request("orc", "warlock", 30, &["progression=cautious", "effort=0"]);
    let route = job::plan(&database(), &overrides(), &request, &|_| {}).unwrap().route;
    let at = route
        .steps
        .iter()
        .position(|s| s.kind == fg_route::export::StepKind::TurnIn && s.quest == Some(1485))
        .unwrap();
    assert!(at < 60, "imp turned in at step {at}");
}

/// A class quest is required for the spell it teaches, not for its name: the undead warlock gets
/// the Imp from Piercing the Veil (or the orc's Vile Familiars: either is enough), the second
/// Vile Familiars (talk to Zureetha Fargaze in Durotar, the orc's chain after the Imp) teaches
/// nothing. It is neither required nor worth the Imp's power: the guide does not cross to Durotar
/// for it, and Piercing the Veil is worth the Imp's power.
#[test]
fn class_quest_is_required_for_its_spell_not_its_name() {
    let p = prepare("undead", "warlock", 10, &["progression=cautious"]);
    let q = |id| &p.model.quests[quest(&p, id).unwrap() as usize];
    let (veil, imp, talk) = (q(1470), q(1485), q(1499));
    assert!(veil.mandatory && imp.mandatory, "both teach the Imp");
    assert!(!talk.mandatory && talk.power.is_none(), "teaches nothing");
    assert!(veil.power.is_some() && veil.power == imp.power, "the same Imp");
    let route = planner(&p).construct(|_| {});
    let talk = quest(&p, 1499).unwrap();
    assert!(!route.iter().any(|s| s.kind == Kind::TurnIn && s.index == talk));
}

/// The log never overflows (a bug seen on an orc warlock to 60): given-up quests (gone gray, or
/// their dungeon outleveled) are given up before the next accept, and the construction's replay
/// (`settle`) goes where the route goes next, as the simulation does. Checked with construction
/// weights that once ended with "quest log full" at an accept.
#[test]
fn constructed_route_keeps_its_log_in_bounds() {
    for w in [
        [4.502, 2.569, 1.073, 0.475, 0.58, 0.547],
        [2.899, 5.096, 1.277, 0.732, 0.04, 0.352],
    ] {
        let p = leveling(30)
            .params(|p| {
                p.zone_stay = w[0];
                p.continent_stay = w[1];
                p.dungeon_quest_weight = w[2];
                p.fit_penalty = w[3];
                p.fit_above_penalty = w[4];
                p.efficiency_power = w[5];
                p.construction_random = false;
            })
            .build();
        let planner = planner(&p);
        let route = planner.construct(|_| {});
        assert!(
            planner.simulate(&route, None).is_some(),
            "{w:?}: {:?}",
            planner.diagnose(&route)
        );
    }
}

/// A stopped search (the user stopped the generation) ends at once, however long its budget,
/// and hands back a valid route.
#[test]
fn stopped_search_ends_at_once() {
    let p = leveling(30).build();
    let planner = planner(&p);
    let route = planner.construct(|_| {});
    let begun = std::time::Instant::now();
    let improved = planner.improve_while(
        route,
        fg_route::plan::Effort::Time(std::time::Duration::from_secs(600)),
        || false,
        |_| {},
        |_, _, _| {},
    );
    assert!(
        begun.elapsed() < std::time::Duration::from_secs(30),
        "took {:?}",
        begun.elapsed()
    );
    assert!(
        planner.simulate(&improved, None).is_some(),
        "{:?}",
        planner.diagnose(&improved)
    );
}

/// An aborted construction (the generation stopped or cancelled) gives up at once with an
/// unfinished route; without abort it reaches the target level.
#[test]
fn aborted_construction_gives_up() {
    let p = leveling(30).build();
    let abort = || true;
    let given_up = planner(&p).abortable(&abort).construct(|_| {});
    let built = planner(&p).construct(|_| {});
    assert!(
        given_up.len() < built.len() / 10,
        "{} stops of {}",
        given_up.len(),
        built.len()
    );
}

/// A job stopped before its optimization (a huge effort) still ends with a guide: the first
/// routes are built and the best one is kept.
#[test]
fn stopped_job_keeps_a_guide() {
    let interrupt = job::Interrupt::default();
    interrupt.stop();
    let begun = std::time::Instant::now();
    let outcome = leveling(12)
        .params(|p| p.effort = 100.0)
        .job(&|_| {}, &interrupt, None)
        .unwrap();
    assert!(
        begun.elapsed() < std::time::Duration::from_secs(300),
        "took {:?}",
        begun.elapsed()
    );
    assert!(outcome.route.total_time.is_finite());
    assert_eq!(outcome.route.to_level, 12);
}

/// A job given the walking memory saves the paths it searched with its outcome (kept through
/// JSON, as in the app's files and the shared guides), and computing it again searches none.
#[test]
fn job_saves_its_walks() {
    let walks = std::sync::Arc::new(fg_route::terrain::Walks::default());
    let interrupt = job::Interrupt::default();
    interrupt.stop();
    let request = request("orc", "warlock", 12, &["effort=100"]);
    let first =
        job::plan_interruptible(&database(), &overrides(), &request, &|_| {}, &interrupt, Some(&walks)).unwrap();
    let memory = first.walks.clone().expect("a memory with the outcome");
    assert!(!memory.is_empty());
    let saved = job::PlanOutcome::from_json(&serde_json::to_string(&first).unwrap()).unwrap();
    assert_eq!(saved.walks.as_ref(), Some(&memory));
    let known = walks.len();
    let again =
        job::plan_interruptible(&database(), &overrides(), &request, &|_| {}, &interrupt, Some(&walks)).unwrap();
    assert_eq!(walks.len(), known, "paths searched again");
    assert_eq!(again.walks.map(|m| m.len()), Some(memory.len()));
}
/// A search that panics ends the job with its panic instead of hanging it: the thread reporting
/// the progress stops with the searches (the app then shows the error).
#[test]
fn panicking_search_ends_the_job() {
    let (sent, received) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let scenario = leveling(12).params(|p| (p.threads, p.effort) = (1, 0.05));
        // The search's first message, from its own thread.
        let progress = |m: &str| assert!(!m.starts_with("start:"), "search failed");
        let interrupt = job::Interrupt::default();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            scenario.job(&progress, &interrupt, None)
        }));
        let _ = sent.send(outcome.is_err());
    });
    let panicked = received
        .recv_timeout(std::time::Duration::from_secs(300))
        .expect("the job hung after its search panicked");
    assert!(panicked);
}

/// The same request gives the same guide again, whichever search ends or reaches a meeting
/// first: the race, the rounds and the final count tries, not time.
#[test]
fn the_tournament_repeats_its_guide() {
    let scenario = || {
        leveling(12).params(|p| {
            (p.threads, p.candidates, p.final_routes) = (4, 8, 2);
            (p.qualify_tries, p.round_tries, p.final_tries, p.share_every) = (0.5, 1.0, 3.0, 50);
        })
    };
    let interrupt = job::Interrupt::default();
    let (first, again) = (
        scenario().job(&|_| {}, &interrupt, None).unwrap(),
        scenario().job(&|_| {}, &interrupt, None).unwrap(),
    );
    assert_eq!(
        serde_json::to_value(&first.plan).unwrap(),
        serde_json::to_value(&again.plan).unwrap()
    );
}

/// A search that panics leaves the meetings: the others do not wait for it, the job ends with
/// the panic.
#[test]
fn panicking_search_does_not_hold_the_meetings() {
    let (sent, received) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // A round of two routes, then a final of one route improved by both searches.
        let scenario = leveling(12).params(|p| {
            (p.threads, p.candidates, p.final_routes) = (2, 2, 1);
            (p.round_tries, p.final_tries, p.share_every) = (1.0, 20.0, 1);
        });
        // The first search of the final, at its first message (the other one waits at a meeting).
        let starts = std::sync::atomic::AtomicUsize::new(0);
        let progress = |m: &str| {
            let final_start = m.starts_with("start:") && starts.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 1;
            assert!(!final_start, "search failed");
        };
        let interrupt = job::Interrupt::default();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            scenario.job(&progress, &interrupt, None)
        }));
        let _ = sent.send(outcome.is_err());
    });
    let panicked = received
        .recv_timeout(std::time::Duration::from_secs(300))
        .expect("the job hung after a sharing search panicked");
    assert!(panicked);
}

/// A cancelled job fails with `Cancelled` instead of returning a route (the app saves nothing).
#[test]
fn cancelled_job_returns_no_route() {
    let interrupt = job::Interrupt::default();
    interrupt.cancel();
    let outcome = leveling(12).params(|p| p.effort = 100.0).job(&|_| {}, &interrupt, None);
    let error = outcome.expect_err("a cancelled job has no route");
    assert!(matches!(error, job::JobError::Cancelled), "{error:#}");
}
/// The local search hands back a valid route (cutting the stops after the target level once
/// made the start of a route invalid: a quest it plans later decides what is taken early), and
/// its incremental evaluations match a full replay (quests taken at hand look ahead in the
/// route; a dungeon quest added moves its run).
#[test]
fn improved_route_stays_valid() {
    let p = generic();
    let planner = planner(p).verified();
    let route = planner.construct(|_| {});
    let improved = planner.improve(route, std::time::Duration::from_secs(2), |_| {}, |_, _, _| {});
    assert!(
        planner.simulate(&improved, None).is_some(),
        "{:?}",
        planner.diagnose(&improved)
    );
}

/// Farming on the way replaces grinding: it gives XP, but never more than its share of each level.
#[test]
fn farm_on_the_way_stays_under_its_cap() {
    let p = generic();
    let planner = planner(p);
    let route = planner.construct(|_| {});
    let (_, spent) = planner.simulate_full(&route, None).unwrap();
    let rules = p.params.edition.rules();
    let levels: i64 = (1..30).map(|l| rules.to_next_level(l)).sum();
    assert!(spent.farm_xp > 0, "nothing farmed on the way");
    assert!(
        spent.farm_xp as f64 <= p.params.farm_cap * levels as f64 * 1.01,
        "{} XP farmed for {levels} XP of levels",
        spent.farm_xp
    );
}

/// Quest objects lying among the mobs of another quest are gathered between the kills: on real
/// quests, doing both together never takes longer than one after the other. Every objective of
/// both quests is done, so that both ways do the same work (an objective done along with
/// another would be extra work).
#[test]
fn gathering_with_kills_never_takes_longer() {
    let p = orc_warlock();
    let alone = p.params.with_overrides(&["gather_overlap=0".to_owned()]).unwrap();
    let (together, apart) = (planner(p), Planner::new(&p.model, &p.world, &alone, &p.profile));
    let quests = &p.model.quests;
    let free = |q: &fg_route::model::Quest| {
        q.pre_all.is_empty()
            && q.pre_any.is_empty()
            && q.level <= 15
            && q.objectives.iter().all(|o| o.dungeon.is_none())
    };
    let mut compared = 0;
    for (a, qa) in quests.iter().enumerate().filter(|(_, q)| free(q)) {
        let Some(ka) = qa.objectives.iter().position(|o| o.kills > 0.0 && o.dungeon.is_none()) else {
            continue;
        };
        for (b, qb) in quests.iter().enumerate().filter(|&(b, q)| b != a && free(q)) {
            let Some(kb) = qb.objectives.iter().position(|o| {
                o.kills <= 0.0
                    && o.uses > 0.0
                    && o.spots.is_some()
                    && o.loc.pos.continent == qa.objectives[ka].loc.pos.continent
                    && o.loc.pos.dist(&qa.objectives[ka].loc.pos) <= p.params.gather_radius
            }) else {
                continue;
            };
            let (a, b) = (a as u32, b as u32);
            let objectives = |q: u32, first: usize| {
                let n = quests[q as usize].objectives.len();
                (0..n).map(move |k| Stop::quest(q, Kind::Objective(((first + k) % n) as u8)))
            };
            let route: Vec<Stop> = [Stop::quest(a, Kind::Accept), Stop::quest(b, Kind::Accept)]
                .into_iter()
                .chain(objectives(a, ka))
                .chain(objectives(b, kb))
                .collect();
            let (Some((_, with)), Some((_, without))) =
                (together.simulate_full(&route, None), apart.simulate_full(&route, None))
            else {
                continue;
            };
            assert!(
                with.fighting <= without.fighting,
                "{} and {}: {:.0} s together, {:.0} s apart",
                qa.name,
                qb.name,
                with.fighting,
                without.fighting
            );
            compared += 1;
        }
    }
    assert!(compared > 0, "no kill and gathering objectives close together");
}

/// Objects lying among the mobs to kill are gathered between the kills, while resting and
/// looking for the next mob: both take less time than one after the other.
#[test]
fn gathering_next_to_kills_overlaps_them() {
    let mut p = quests_at_start(2);
    for (i, kills, uses, dx) in [(0, 6.0, 0.0, 80.0), (1, 0.0, 3.0, 90.0)] {
        let mut loc = p.model.start.clone();
        loc.pos.x += dx;
        let point = (loc.pos.x, loc.pos.y);
        p.model.quests[i].objectives = vec![Objective {
            loc: loc.clone(),
            text: format!("objective {i}"),
            kills,
            mob_level: 1,
            uses,
            extra: 0.0,
            dungeon: None,
            elite: false,
            count: kills + uses,
            mobs: vec![],
            pull: 1.0,
            spawns: 0,
            pack: 1.0,
            spots: Some(Spots {
                continent: loc.pos.continent,
                min: point,
                max: point,
                points: vec![point],
            }),
            guard: Guard::default(),
            beside_giver: false,
            item: false,
            alts: vec![],
            after: 0,
            handed_by: vec![],
            vendors: vec![],
            auction: vec![],
            slot: None,
            lasts: None,
            stock: None,
        }];
    }
    p.model.initial.accepted = vec![0, 1];
    let route = [Stop::quest(0, Kind::Objective(0)), Stop::quest(1, Kind::Objective(0))];
    let fighting = |p: &Prepared| planner(p).simulate_full(&route, None).unwrap().1.fighting;
    let together = fighting(&p);
    p.params.gather_overlap = 0.0;
    let apart = fighting(&p);
    assert!(together < apart, "{together:.0} s together, {apart:.0} s apart");
}

/// Common loot a quest asks for by the stack (cloth for a donation) comes from the route's kills
/// where it drops: after killing there, only what the bags lack is gathered or bought.
#[test]
fn common_loot_comes_from_the_kills_before() {
    let mut p = quests_at_start(2);
    // Quest 0 kills 100 mobs at the start, quest 1 asks for 20 cloth (a minute each to gather
    // or buy), and a mob there drops one cloth in ten.
    p.model.quests[0].objectives = vec![kills_east_of_start(&p, 0.0, 100.0, 1)];
    let mut cloth = kills_east_of_start(&p, 0.0, 0.0, 1);
    (cloth.count, cloth.extra, cloth.stock) = (20.0, 20.0 * 60.0, Some(0));
    p.model.quests[1].objectives = vec![cloth];
    p.model.loot = CommonLoot {
        items: vec![2589],
        ..CommonLoot::default()
    };
    p.model.loot.by_zone.insert(p.model.start.zone, vec![(0, 0.1)]);
    p.model.initial.accepted = vec![0, 1];
    // The kills' XP must not end the route before the cloth.
    p.profile.to_level = 10;
    let fighting = |route: &[Stop]| planner(&p).simulate_full(route, None).unwrap().1.fighting;
    let (kill, gather) = (Stop::quest(0, Kind::Objective(0)), Stop::quest(1, Kind::Objective(0)));
    let first = fighting(&[gather, kill]);
    let after = fighting(&[kill, gather]);
    assert!(
        (first - after - 10.0 * 60.0).abs() < 1.0,
        "{first:.0} s gathering first, {after:.0} s after the kills"
    );
}

/// Each checkpoint has a step where a character behind the route grinds: at the last place
/// before it where mobs of its level live (to the XP the route has there), else at the
/// checkpoint itself.
#[test]
fn checkpoints_wait_for_their_level() {
    use fg_route::export::StepKind;
    let p = generic();
    let planner = planner(p);
    let route = planner.construct(|_| {});
    let guide = fg_route::export::build(&planner, &p.profile, &route);
    assert!(!guide.checkpoints.is_empty(), "no checkpoint up to level 30");
    for c in &guide.checkpoints {
        let sync = guide.steps[..=c.step]
            .iter()
            .rev()
            .find(|s| s.kind == StepKind::Grind && s.keep)
            .unwrap();
        let before = sync.level == c.level - 1 && sync.xp.is_some_and(|xp| xp > 0);
        assert!(before || sync.level == c.level, "{sync:?}");
    }
}

/// Only flight masters are flight paths: not the nodes of scripted flights (quests, towers), nor
/// those shown to the other faction only.
#[test]
fn flight_paths_are_flight_masters() {
    let world = crate::common::world(Edition::Classic, fg_route::faction::Faction::Horde);
    let has = |name: &str| world.taxi_nodes.iter().any(|n| n.name.starts_with(name));
    assert!(has("Ratchet") && has("Crossroads") && has("Hammerfall"));
    assert!(!has("Quest Path"), "a shaman quest's flight");
    assert!(!has("Plaguewood Tower"), "an Eastern Plaguelands tower");
    assert!(!has("Northshire Abbey"));
}

/// Zone maps overlap (Durotar's covers Ratchet and the Crossroads): flight paths are placed in
/// their own zone (a city's own map in a city), and towns are named.
#[test]
fn flight_paths_are_in_their_zone() {
    let world = crate::common::world(Edition::Classic, fg_route::faction::Faction::Horde);
    let zone_of = |town: &str| {
        let n = world.taxi_nodes.iter().find(|n| n.name.starts_with(town)).unwrap();
        world.zone_name(n.pos.zone).to_owned()
    };
    assert_eq!(zone_of("Ratchet"), "The Barrens");
    assert_eq!(zone_of("Crossroads"), "The Barrens");
    assert_eq!(zone_of("Orgrimmar"), "Orgrimmar");
    assert_eq!(zone_of("Thunder Bluff"), "Thunder Bluff");
    let ratchet = world.taxi_nodes.iter().find(|n| n.name.starts_with("Ratchet")).unwrap();
    assert_eq!(world.place_name(&ratchet.pos), Some("Ratchet"));
}

/// Marla's Last Wish: Samuel's Remains are looted from Samuel Fipps, then buried at Marla's
/// grave. The guide gets them first; burying them before is not a valid route.
#[test]
fn item_used_by_an_objective_is_got_first() {
    let mut p = leveling(10)
        .quest(
            Q::new(800, "Last Wish")
                .level(5)
                .giver(1, 30.0, 40.0)
                .objective(Obj::kill(
                    "Fipps",
                    5,
                    1,
                    At {
                        zone: 1,
                        x: 40.0,
                        y: 70.0,
                    },
                ))
                .objective(
                    Obj::visit(
                        "Bury the remains",
                        At {
                            zone: 1,
                            x: 60.0,
                            y: 70.0,
                        },
                    )
                    .after(&[0]),
                ),
        )
        .build();
    let marla = quest(&p, 800).unwrap();
    p.model.initial.accepted = vec![marla as usize];
    let planner = planner(&p);
    let objective = |k: u8| Stop::quest(marla, Kind::Objective(k));
    let turn_in = Stop::quest(marla, Kind::TurnIn);
    assert!(planner.simulate(&[objective(0), objective(1), turn_in], None).is_some());
    assert!(planner.simulate(&[objective(1), objective(0), turn_in], None).is_none());
}
/// Rut'theran Village lies at the foot of Teldrassil: no walk goes up to Dolanaar nor across the
/// sea to Auberdine, the portal and the boat do. Darnassus is walked to from Dolanaar.
#[test]
fn rut_theran_is_left_by_portal_or_boat() {
    let w = crate::common::world(Edition::Classic, fg_route::faction::Faction::Alliance);
    let at = |zone, x, y| w.to_world(zone, x, y).unwrap();
    let (village, dolanaar) = (at(141, 55.5, 90.0), at(141, 55.6, 59.8));
    assert!(w.walk_distance(&village, &dolanaar).is_infinite());
    assert!(w.walk_distance(&village, &at(148, 37.0, 44.0)).is_infinite());
    assert!(w.walk_distance(&dolanaar, &at(1657, 40.0, 50.0)).is_finite());
    let portal = w
        .links
        .iter()
        .find(|l| l.name == "Portal Rut'theran Village - Darnassus")
        .unwrap();
    assert!(w.walk_distance(&village, &portal.from).is_finite());
    assert!(portal.seconds < 30.0, "a portal is taken at once");
}

/// The walking world of each edition for a faction (`job::walk_world`).
fn walk_worlds(faction: fg_route::faction::Faction) -> Vec<(&'static str, &'static fg_route::world::World)> {
    [
        ("forever", Edition::Forever),
        ("classic", Edition::Classic),
        ("tbc", Edition::Tbc),
    ]
    .into_iter()
    .map(|(name, edition)| (name, crate::common::world(edition, faction)))
    .collect()
}

/// The least distance (yards) from a point to a walk drawn (`World::walk_path`), its segments.
fn distance_to_path(path: &[fg_route::world::Pos], p: &fg_route::world::Pos) -> f64 {
    path.windows(2)
        .map(|w| {
            let (ax, ay, bx, by) = (w[0].x, w[0].y, w[1].x, w[1].y);
            let (dx, dy) = (bx - ax, by - ay);
            let len = dx * dx + dy * dy;
            let t = if len > 0.0 {
                (((p.x - ax) * dx + (p.y - ay) * dy) / len).clamp(0.0, 1.0)
            } else {
                0.0
            };
            (p.x - ax - t * dx).hypot(p.y - ay - t * dy)
        })
        .fold(f64::INFINITY, f64::min)
}

/// Valley of Trials is walled by mountains: walks out of it take its road east to Durotar's
/// plain (the floors hold the road, and no way over the ridges), never a straight line over the
/// ridges, even to the plain just behind its north wall (GitHub #43). Walks inside it do not
/// take the road.
#[test]
fn valley_of_trials_is_left_by_its_road() {
    for (edition, w) in walk_worlds(fg_route::faction::Faction::Horde) {
        let at = |x, y| w.to_world(14, x, y).unwrap();
        let (north_end, behind) = (at(44.5, 56.5), at(42.0, 50.5));
        let (gornek, razor_hill) = (at(42.1, 68.4), at(51.9, 43.5));
        // Where the road leaves the valley.
        let road = at(47.4, 67.7);
        for (a, b) in [(north_end, behind), (behind, north_end), (gornek, razor_hill)] {
            let path = w.walk_path(&a, &b, 0);
            let near = distance_to_path(&path, &road);
            assert!(near < ROAD, "{edition}: {near} yd from the road, {path:?}");
            let length: f64 = path.windows(2).map(|p| p[0].dist(&p[1])).sum();
            assert!(
                (w.walk_distance(&a, &b) - length).abs() < 0.2 * length,
                "{edition}: priced as drawn"
            );
        }
        assert!(
            w.walk_distance(&north_end, &behind) > 4.0 * north_end.dist(&behind),
            "{edition}"
        );
        let sarkoth = at(40.6, 62.6);
        assert!(
            distance_to_path(&w.walk_path(&gornek, &sarkoth, 0), &road) > ROAD,
            "{edition}"
        );
    }
}

/// How near a walk drawn goes by a passage's point when it takes the road or the tunnel there
/// (yards): its turns are cells, or the tiles' gates on a long walk.
const ROAD: f64 = 120.0;

/// Burning Blade Coven opens on Valley of Trials: Yarrog Baneshadow, deep inside under the
/// ridge (nearer the plain behind it than the valley), is walked to from the valley through the
/// cave's mouth (the floors hold the cave), and from Razor Hill by the valley's road.
#[test]
fn burning_blade_coven_is_reached_from_the_valley() {
    for (edition, w) in walk_worlds(fg_route::faction::Faction::Horde) {
        let at = |x, y| w.to_world(14, x, y).unwrap();
        let (yarrog, zureetha, razor_hill) = (at(42.7, 53.0), at(42.8, 69.0), at(51.9, 43.5));
        let (mouth, road) = (at(45.3, 56.4), at(47.4, 67.7));
        let from_valley = w.walk_distance(&zureetha, &yarrog);
        assert!(from_valley < 1.5 * zureetha.dist(&yarrog), "{edition}: {from_valley}");
        assert!(w.walk_distance(&razor_hill, &yarrog) > from_valley, "{edition}");
        let path = w.walk_path(&razor_hill, &yarrog, 0);
        let near = distance_to_path(&path, &road);
        assert!(near < ROAD, "{edition}: {near} yd from the road, {path:?}");
        let inside = w.walk_path(&zureetha, &yarrog, 0);
        let near = distance_to_path(&inside, &mouth);
        assert!(near < ROAD, "{edition}: {near} yd from the mouth, {inside:?}");
    }
}

/// Coldridge Valley is left by Coldridge Pass, the tunnel to Kharanos, in every edition.
#[test]
fn coldridge_valley_is_left_by_its_tunnel() {
    for (edition, w) in walk_worlds(fg_route::faction::Faction::Alliance) {
        let at = |x, y| w.to_world(1, x, y).unwrap();
        // Every cell walked over (the turns are the tiles' gates of a long walk).
        let (a, b) = (at(29.0, 66.0), at(46.0, 52.0));
        let path: Vec<_> = w.terrain.cell_path(&a, &b).into_iter().map(|(p, _)| p).collect();
        assert!(!path.is_empty(), "{edition}: no way");
        // By the tunnel's mouth in the valley (the floors hold the tunnel).
        let near = distance_to_path(&path, &at(33.0, 69.3));
        assert!(near < ROAD, "{edition}: {near} yd from the tunnel");
        assert!(w.walk_distance(&a, &b) < 2.0 * a.dist(&b), "{edition}: no way round");
    }
}

/// WoW Forever's Mount Hyjal is walled by cliffs: walks up from Felwood or Winterspring take
/// Darkwhisper Gorge, even from Felwood just below its western rim.
#[test]
fn mount_hyjal_is_reached_by_darkwhisper_gorge() {
    let w = crate::common::world(Edition::Forever, fg_route::faction::Faction::Horde);
    let at = |zone, x, y| w.to_world(zone, x, y).unwrap();
    let gorge = at(618, 56.5, 71.6);
    let (west, foothills) = (at(616, 12.8, 52.4), at(616, 55.1, 82.6));
    for (a, b) in [(at(361, 62.0, 40.0), west), (at(618, 61.0, 38.0), foothills)] {
        let path = w.walk_path(&a, &b, 0);
        let near = distance_to_path(&path, &gorge);
        assert!(near < ROAD, "{near} yd from the gorge, {path:?}");
        // No shorter way round: at least the straight lines to the gorge and on.
        let least = a.dist(&gorge) + gorge.dist(&b);
        assert!(
            w.walk_distance(&a, &b) > 0.9 * least,
            "{} < {least}",
            w.walk_distance(&a, &b)
        );
    }
}

/// The passages of the Eastern Kingdoms' south (Stormwind, Elwynn) and north (Dun Morogh,
/// Silverpine) do not meet: going from one to the other takes the tram, a flight or a boat, never
/// a walk in a straight line over the mountains.
#[test]
fn no_walk_between_the_south_and_north_of_the_eastern_kingdoms() {
    let w = crate::common::world(Edition::Classic, fg_route::faction::Faction::Alliance);
    let at = |zone, x, y| w.to_world(zone, x, y).unwrap();
    let stormwind = at(1519, 77.0, 30.3);
    assert!(w.walk_distance(&stormwind, &at(130, 58.2, 69.9)).is_infinite());
    assert!(w.walk_distance(&at(12, 42.0, 65.0), &at(38, 35.0, 46.0)).is_infinite());
    assert!(w.walk_distance(&stormwind, &at(40, 56.0, 47.0)).is_finite());
    assert!(w.walk_distance(&at(1, 47.0, 52.0), &at(38, 35.0, 46.0)).is_finite());
}

/// A level 17 does not walk into a zone whose mobs are far above it (Stranglethorn Vale, to the
/// boat of Booty Bay), nor across one on its way (Arathi, behind the Burning Steppes), but walks
/// to Ashenvale and the Barrens; a level 50 walks to all of them.
#[test]
fn walks_do_not_go_into_zones_far_above_the_character() {
    let w = crate::common::world(Edition::Classic, fg_route::faction::Faction::Alliance);
    let at = |zone, x, y| w.to_world(zone, x, y).unwrap();
    let westfall = at(40, 40.0, 70.0);
    let booty_bay = at(33, 26.1, 73.4);
    let (weak, strong) = (w.walk_level(17), w.walk_level(50));
    assert!(w.walk_distance_at(&westfall, &booty_bay, weak).is_infinite());
    assert!(w.walk_distance_at(&westfall, &booty_bay, strong).is_finite());
    let darkshore = at(148, 36.0, 45.0);
    assert!(w.walk_distance_at(&darkshore, &at(331, 34.0, 50.0), weak).is_finite());
    assert!(w.walk_distance_at(&darkshore, &at(17, 63.0, 37.0), weak).is_finite());
    // Out of a zone too strong for the character: yes.
    assert!(w.walk_distance_at(&booty_bay, &westfall, weak).is_finite());
}

/// The mountain skip from Dun Morogh into the Wetlands is a drop: taken that way only, and a step
/// of the guide (it is not on any road).
#[test]
fn mountain_skip_is_one_way_and_a_step_of_the_guide() {
    // Every zone may be walked in, whatever the character's level.
    let mut o = overrides();
    o.params.zone_gap = 0.0;
    let world = job::game_world(&database(), &o, fg_route::faction::Faction::Alliance, 3).unwrap();
    let w = &world;
    let mut p = quests_at_start_as("dwarf", "warrior", 1);
    let at = |zone, x, y| w.to_world(zone, x, y).unwrap();
    let (kharanos, menethil) = (at(1, 47.0, 52.0), at(11, 10.5, 56.0));
    let skip = w.passes.iter().position(|q| q.name.contains("mountain skip")).unwrap();
    assert_eq!(w.walk_shortcuts(&kharanos, &menethil), [skip]);
    assert_eq!(w.walk_shortcuts(&menethil, &kharanos), Vec::<usize>::new());
    assert!(w.walk_distance(&menethil, &kharanos) > w.walk_distance(&kharanos, &menethil));
    // The character starts in Kharanos, with a quest to do in the Wetlands.
    let mut o = one_kill_east_of_start(&p, 0.0);
    (o.loc.pos, o.loc.zone) = (menethil, 11);
    p.model.quests[0].objectives = vec![o];
    p.model.initial.accepted = vec![0];
    p.model.initial.pos = Some((kharanos, 1));
    let planner = || Planner::new(&p.model, w, &p.params, &p.profile);
    let mut trace = Vec::new();
    planner()
        .simulate_full(&[Stop::quest(0, Kind::Objective(0))], Some(&mut trace))
        .unwrap();
    assert!(
        trace
            .iter()
            .any(|t| matches!(t.event, Event::Shortcut { pass } if pass == skip))
    );
    // In the guide editor, the shortcut is part of the walk to the step, not a step of its own.
    let edited = planner().explicit().edit(&[Edit::Stop {
        stop: Stop::quest(0, Kind::Objective(0)),
        farm: false,
        spot: None,
    }]);
    assert_eq!(edited.steps[0].outcome, Outcome::Done);
}

/// In the guide editor, grinding up several levels is one step even when it moves from camp to
/// camp (each level at the camp of its mobs).
#[test]
fn editor_grind_over_several_camps_is_one_step() {
    let p = leveling(30).build();
    let edited = planner(&p).explicit().edit(&[Edit::GrindTo { level: 12 }]);
    let grinds = edited
        .trace
        .iter()
        .filter(|t| matches!(t.event, Event::Grind { .. }))
        .count();
    assert!(grinds > 1, "one camp only: nothing checked");
    assert_eq!(edited.steps[0].outcome, Outcome::Done);
}

/// A link kept to some races (Forever's Skyborne portals) is only in their guides.
#[test]
fn link_kept_to_a_race() {
    let mut o = overrides();
    let mut tram = (o.links.iter())
        .find(|l| l.name.starts_with("Deeprun Tram"))
        .unwrap()
        .clone();
    (tram.name, tram.races) = ("Gnomes only".into(), vec!["gnome".into()]);
    o.links.push(tram);
    let has = |race: &str| {
        o.links_of_race(o.races[race].id)
            .iter()
            .any(|l| l.name == "Gnomes only")
    };
    assert!(has("gnome"));
    assert!(!has("human"));
}
/// A link a quest opens (Forever's Skyborne leave their island by the teleport the giver of its
/// last quest offers) is not taken before that quest is.
#[test]
fn link_opened_by_a_quest() {
    let mut p = quests_at_start(2);
    let point = |zone, x, y| fg_route::world::ZonePoint { zone, x, y, z: None };
    let link = fg_route::world::LinkDef {
        name: "Opened by quest 1".into(),
        faction: None,
        from: point(12, 48.17, 42.94),
        to: point(1537, 50.0, 50.0),
        seconds: 5.0,
        one_way: true,
        instant: true,
        races: vec![],
        after_quest: Some(p.model.quests[1].id),
        while_held: false,
    };
    let (faction, speed, wait) = (
        p.profile.faction,
        p.params.run_speed / p.params.detour,
        p.params.link_wait,
    );
    p.world.add_links(&[link], faction, speed, wait);
    let mut o = one_kill_east_of_start(&p, 0.0);
    (o.loc.pos, o.loc.zone) = (p.world.to_world(1537, 50.0, 50.0).unwrap(), 1537);
    p.model.quests[0].objectives = vec![o];
    let takes_it = |p: &mut Prepared, accepted: Vec<usize>| {
        p.model.initial.accepted = accepted;
        let mut trace = Vec::new();
        planner(p)
            .simulate_full(&[Stop::quest(0, Kind::Objective(0))], Some(&mut trace))
            .unwrap();
        trace
            .iter()
            .any(|t| matches!(t.event, Event::Link { first, .. } if p.world.links[first].name == "Opened by quest 1"))
    };
    assert!(!takes_it(&mut p, vec![0]), "quest 1 not taken: closed");
    assert!(takes_it(&mut p, vec![0, 1]), "quest 1 taken: open");
}

/// A link open only while a quest is in the log (Vesprystus flies the character to Auberdine
/// with Flight to Auberdine, the flight path known or not) is closed before it is taken and
/// once it is turned in.
#[test]
fn link_open_while_a_quest_is_held() {
    let mut p = quests_at_start(2);
    let point = |zone, x, y| fg_route::world::ZonePoint { zone, x, y, z: None };
    let link = fg_route::world::LinkDef {
        name: "While quest 1 is held".into(),
        faction: None,
        from: point(12, 48.17, 42.94),
        to: point(1537, 50.0, 50.0),
        seconds: 5.0,
        one_way: true,
        instant: true,
        races: vec![],
        after_quest: Some(p.model.quests[1].id),
        while_held: true,
    };
    let (faction, speed, wait) = (
        p.profile.faction,
        p.params.run_speed / p.params.detour,
        p.params.link_wait,
    );
    p.world.add_links(&[link], faction, speed, wait);
    let mut o = one_kill_east_of_start(&p, 0.0);
    (o.loc.pos, o.loc.zone) = (p.world.to_world(1537, 50.0, 50.0).unwrap(), 1537);
    p.model.quests[0].objectives = vec![o];
    let takes_it = |p: &mut Prepared, accepted: Vec<usize>, turned: Vec<usize>| {
        p.model.initial.accepted = accepted;
        p.model.initial.turned = turned;
        // Quest 1 handed in after when in the log: kept there (one the route does nothing with
        // is given up at the start).
        let mut route = vec![Stop::quest(0, Kind::Objective(0))];
        if p.model.initial.accepted.contains(&1) {
            route.push(Stop::quest(1, Kind::TurnIn));
        }
        let mut trace = Vec::new();
        planner(p).simulate_full(&route, Some(&mut trace)).unwrap();
        trace.iter().any(
            |t| matches!(t.event, Event::Link { first, .. } if p.world.links[first].name == "While quest 1 is held"),
        )
    };
    assert!(!takes_it(&mut p, vec![0], vec![]), "quest 1 not taken: closed");
    assert!(takes_it(&mut p, vec![0, 1], vec![]), "quest 1 in the log: open");
    assert!(!takes_it(&mut p, vec![0], vec![1]), "quest 1 turned in: closed");
}

/// A quest given up is not turned in: it opens no follow-up. Fire Hardened Mail, left in the log
/// when Razorfen Kraul is not run, does not give Furen's Armor (it used to).
#[test]
fn quest_given_up_opens_no_follow_up() {
    let mut p = leveling(10)
        .dungeon("Razorfen Kraul", 1, 70.0, 70.0)
        .params(|p| p.dungeons = false)
        .quest(
            Q::new(800, "Mail").level(4).giver(1, 30.0, 40.0).objective(
                Obj::kill(
                    "Boss",
                    4,
                    1,
                    At {
                        zone: 1,
                        x: 70.0,
                        y: 70.0,
                    },
                )
                .inside(0),
            ),
        )
        .quest(
            Q::new(801, "Armor")
                .level(5)
                .after(&[800])
                .giver(1, 31.0, 40.0)
                .kill("Beast", 5, 3, 1, 33.0, 70.0),
        )
        .build();
    let (mail, armor) = (quest(&p, 800).unwrap(), quest(&p, 801).unwrap());
    let first = Stop::quest(quest(&p, 100).unwrap(), Kind::Accept);
    let route = [first, Stop::quest(armor, Kind::Accept)];
    p.model.initial.turned = vec![mail as usize];
    assert!(
        planner(&p).simulate(&route, None).is_some(),
        "turned in, it opens the follow-up"
    );
    p.model.initial.turned.clear();
    p.model.initial.accepted = vec![mail as usize];
    // In the log, and turned in by the route if it could be.
    let kept = [first, Stop::quest(mail, Kind::TurnIn)];
    let mut trace = Vec::new();
    assert!(planner(&p).simulate(&kept, Some(&mut trace)).is_some());
    assert!(
        trace
            .iter()
            .any(|t| matches!(t.event, Event::Abandon { quest } if quest == mail as usize)),
        "Mail is given up"
    );
    assert!(
        planner(&p).simulate(&[kept[0], kept[1], route[1]], None).is_none(),
        "given up, it does not"
    );
}
/// Grinding takes place at a camp of mobs of the character's level, gone to first: not in a
/// town, where no mob lives.
#[test]
fn grinding_goes_to_a_camp() {
    let mut p = leveling(4).zone(4, "Town").build();
    let town = p.world.to_world(4, 50.0, 50.0).unwrap();
    p.model.initial.pos = Some((town, 4));
    let planner = planner(&p);
    let mut trace = Vec::new();
    let (_, spent) = planner.simulate_full(&[], Some(&mut trace)).unwrap();
    let camps: Vec<_> = (trace.iter())
        .filter_map(|t| match t.event {
            Event::Grind { near, zone, .. } => Some((near, zone)),
            _ => None,
        })
        .collect();
    assert_ne!(camps, []);
    for (near, zone) in &camps {
        assert_ne!(*zone, 4, "grinding in town");
        assert!(p.model.farm.around(near, 150.0, 1..=5) >= 8, "no camp at {near:?}");
    }
    assert!(spent.travel > 0.0, "no trip to the camp");
    assert!(spent.grinding > 0.0);
}
/// The addon gets how many items an item objective asks for (its items to keep) and the guide's
/// reputation goals (its reputation tab).
#[test]
fn addon_gets_item_counts_and_reputation_goals() {
    let goal = r#"reputation_goals=[{"faction":72,"target":9000}]"#;
    let p = prepare("human", "warrior", 6, &[goal]);
    let planner = planner(&p);
    let route = planner.construct(|_| {});
    let guide = fg_route::export::build(&planner, &p.profile, &route);
    let lua = fg_route::export::to_lua(std::slice::from_ref(&guide));
    let item = guide
        .steps
        .iter()
        .find(|s| {
            s.target
                .as_ref()
                .is_some_and(|t| t.kind == fg_route::export::TargetKind::Item)
        })
        .expect("an item objective");
    let line = lua
        .lines()
        .find(|l| l.contains(&format!("t = {:?}", item.text)))
        .unwrap();
    assert!(line.contains(", c = "), "{line}");
    assert!(lua.contains("reputations = { { id = 72, name = "), "{}", &lua[..300]);
}

/// The mobs farmed on the way to a step are a count of their own (the addon counts them),
/// not words of its text.
#[test]
fn mobs_on_the_way_are_counted_apart() {
    let p = generic();
    let planner = planner(p);
    let route = planner.construct(|_| {});
    let guide = fg_route::export::build(&planner, &p.profile, &route);
    let farmed: Vec<_> = guide.steps.iter().filter(|s| s.farm.is_some()).collect();
    assert!(!farmed.is_empty(), "no mobs farmed on the way");
    assert!(
        farmed.iter().all(|s| !s.text.contains("on the way)")),
        "{}",
        farmed[0].text
    );
    assert!(fg_route::export::to_lua(std::slice::from_ref(&guide)).contains(", fk = "));
}

/// The addon counts the XP of the mobs on the way: each step farmed to has the XP the planner
/// expects from them (rested bonus included), all together the XP farmed on the way.
#[test]
fn mobs_on_the_way_carry_their_xp() {
    let p = generic();
    let planner = planner(p);
    let route = planner.construct(|_| {});
    let (_, spent) = planner.simulate_full(&route, None).unwrap();
    let guide = fg_route::export::build(&planner, &p.profile, &route);
    let farmed: Vec<_> = guide.steps.iter().filter(|s| s.farm.is_some()).collect();
    assert!(!farmed.is_empty(), "no mobs farmed on the way");
    assert!(farmed.iter().all(|s| s.farm_xp.is_some_and(|xp| xp > 0)));
    let total: i64 = farmed.iter().filter_map(|s| s.farm_xp).sum();
    assert_eq!(total, spent.farm_xp);
    let lua = fg_route::export::to_lua(std::slice::from_ref(&guide));
    let step = farmed[0];
    let line = lua
        .lines()
        .find(|l| l.contains(&format!("t = {:?}", step.text)) && l.contains(", fk = "))
        .unwrap();
    assert!(line.contains(&format!(", fx = {}", step.farm_xp.unwrap())), "{line}");
}

/// The addon gets the IDs of the mobs a step is done on, to leave their kills out of the mobs on
/// the way.
#[test]
fn addon_knows_the_mobs_of_each_step() {
    let p = generic();
    let planner = planner(p);
    let route = planner.construct(|_| {});
    let guide = fg_route::export::build(&planner, &p.profile, &route);
    let lua = fg_route::export::to_lua(std::slice::from_ref(&guide));
    let kill = guide
        .steps
        .iter()
        .find(|s| s.kind == fg_route::export::StepKind::Objective && !s.mob_ids.is_empty())
        .expect("an objective on mobs");
    let line = lua
        .lines()
        .find(|l| l.contains(&format!("t = {:?}", kill.text)))
        .unwrap();
    let ids: Vec<String> = kill.mob_ids.iter().map(ToString::to_string).collect();
    assert!(line.contains(&format!("mi = {{ {} }}", ids.join(", "))), "{line}");
}

/// A dungeon run is a step that takes the player in, then a step per quest of the route done
/// or started inside (marked `inside`, in the run's time), each turned in later.
#[test]
fn dungeon_quests_are_steps_of_their_own() {
    use fg_route::export::StepKind;
    let p = with_deadmines(leveling(25))
        .params(|p| p.forced_dungeons = vec![900])
        .build();
    let planner = planner(&p);
    let route = planner.construct(|_| {});
    let guide = fg_route::export::build(&planner, &p.profile, &route);
    let steps = &guide.steps;
    assert!(steps.iter().any(|s| s.kind == StepKind::Dungeon), "the forced run");
    assert!(steps.iter().any(|s| s.inside), "no quest step in the runs");
    for (i, s) in steps.iter().enumerate().filter(|(_, s)| s.inside) {
        let before = &steps[i - 1];
        assert!(
            before.kind == StepKind::Dungeon || before.inside,
            "{} not after a run ({:?} {})",
            s.text,
            before.kind,
            before.text
        );
        // Objectives, and quests started by a boss's drop (Glowing Shard).
        assert!(matches!(s.kind, StepKind::Objective | StepKind::Accept), "{}", s.text);
        // The first routes may leave one in the log (its turn-in guarded beyond the character's
        // reach): the optimizer gives those up. One turned in is turned in after the run.
        let q = s.quest.unwrap();
        let turnin = steps
            .iter()
            .position(|t| t.kind == StepKind::TurnIn && t.quest == Some(q));
        assert!(turnin.is_none_or(|t| t > i), "{} turned in before the run", s.text);
    }
    assert!(
        steps.windows(2).all(|w| w[0].time <= w[1].time + 1e-6),
        "time goes back"
    );
    // The addon tells them apart (the run step is then done on entering the dungeon).
    assert!(fg_route::export::to_lua(std::slice::from_ref(&guide)).contains("di = true"));
}

/// A timed quest reached after its time has failed: the guide gives it up instead of turning it
/// in (no reward); in time, it is turned in.
#[test]
fn late_timed_quest_is_given_up() {
    let mut p = leveling(30)
        .levels(22, 30)
        .quest(
            Q::new(800, "The Gauntlet")
                .level(24)
                .xp(2000)
                .timed(3600.0)
                .giver(3, 30.0, 40.0)
                .kill("Beast", 24, 4, 3, 34.0, 70.0),
        )
        .build();
    let i = p.model.quests.iter().position(|q| q.id == 800).expect("The Gauntlet");
    assert_eq!(p.model.quests[i].time_limit, Some(3600.0));
    let objectives = p.model.quests[i].objectives.len() as u8;
    let mut route = vec![Stop::quest(i as u32, Kind::Accept)];
    route.extend((0..objectives).map(|k| Stop::quest(i as u32, Kind::Objective(k))));
    route.push(Stop::quest(i as u32, Kind::TurnIn));
    for (limit, turned_in) in [(Some(1.0), false), (Some(36_000.0), true), (None, true)] {
        p.model.quests[i].time_limit = limit;
        let mut trace = Vec::new();
        let (_, spent) = planner(&p)
            .simulate_full(&route, Some(&mut trace))
            .expect("a valid route");
        let given_up = trace
            .iter()
            .any(|t| matches!(t.event, Event::Abandon { quest } if quest == i));
        let turned = trace
            .iter()
            .any(|t| matches!(t.event, Event::Stop { stop, .. } if stop.kind == Kind::TurnIn));
        assert_eq!((turned, given_up), (turned_in, !turned_in), "limit {limit:?}");
        assert_eq!(spent.quest_xp > 0, turned_in, "reward with limit {limit:?}");
    }
}
/// The Affray: Big Will only comes out once the grate is stepped on, so a route killing him first
/// is refused. Taken at level 30, too weak for him within the ten minutes, the quest is taken
/// once the character is strong enough and turned in, not given up.
#[test]
fn affray_grate_first_and_taken_strong_enough() {
    let p = leveling(40)
        .levels(30, 40)
        .quest(
            Q::new(800, "The Affray")
                .level(34)
                .timed(600.0)
                .giver(3, 30.0, 40.0)
                .objective(
                    Obj::kill(
                        "Big Will",
                        38,
                        1,
                        At {
                            zone: 3,
                            x: 40.0,
                            y: 70.0,
                        },
                    )
                    .after(&[1]),
                )
                .objective(Obj::visit(
                    "Grate",
                    At {
                        zone: 3,
                        x: 39.0,
                        y: 70.0,
                    },
                )),
        )
        .build();
    let i = quest(&p, 800).unwrap();
    let (will, grate) = (0, 1);
    let route = |first: u8, second: u8| {
        [
            Stop::quest(i, Kind::Accept),
            Stop::quest(i, Kind::Objective(first)),
            Stop::quest(i, Kind::Objective(second)),
            Stop::quest(i, Kind::TurnIn),
        ]
    };
    assert!(
        planner(&p).simulate(&route(will, grate), None).is_none(),
        "Big Will first"
    );
    let mut trace = Vec::new();
    planner(&p)
        .simulate(&route(grate, will), Some(&mut trace))
        .expect("the grate first");
    let given_up = trace
        .iter()
        .any(|t| matches!(t.event, Event::Abandon { quest } if quest == i as usize));
    let level = |kind: Kind| {
        trace.iter().find_map(|t| match t.event {
            Event::Stop { stop, .. } if stop.kind == kind => Some(t.level),
            _ => None,
        })
    };
    assert!(!given_up && level(Kind::TurnIn).is_some(), "turned in in time");
    assert!(
        level(Kind::Accept).is_some_and(|l| l > 30),
        "taken stronger: {:?}",
        level(Kind::Accept)
    );
}
/// Dungeon mobs keep their level: a character far above them gets no XP from the run (they used
/// to follow the character's level).
#[test]
fn outleveled_dungeon_gives_no_mob_xp() {
    let mut p = with_deadmines(leveling(40)).levels(32, 40).build();
    // Run that late only with levels chosen up to it, its mobs around level 18.
    (
        p.model.dungeons[0].min_level,
        p.model.dungeons[0].max_level,
        p.model.dungeons[0].mob_level,
    ) = (14, 40, 18);
    let run = Stop {
        index: 0,
        kind: Kind::Dungeon,
    };
    let (_, spent) = planner(&p).simulate_full(&[run], None).unwrap();
    assert_eq!(spent.dungeon_runs, 1);
    assert_eq!(spent.mob_xp, 0);
}

/// A dungeon is never run past the levels chosen for it, even when the player asked for it.
#[test]
fn a_dungeon_is_not_run_past_its_levels() {
    let mut p = with_deadmines(leveling(40)).levels(32, 40).build();
    (p.model.dungeons[0].min_level, p.model.dungeons[0].max_level) = (14, 28);
    let run = Stop {
        index: 0,
        kind: Kind::Dungeon,
    };
    assert!(planner(&p).simulate(&[run], None).is_none());
    p.params.forced_dungeons = vec![p.model.dungeons[0].area];
    assert!(planner(&p).simulate(&[run], None).is_none());
    // Within them, it is run.
    p.profile.from_level = 25;
    assert!(planner(&p).simulate(&[run], None).is_some());
}

/// A guide made with an XP buff setting tells the addon which buffs it counts on (it warns when one
/// is missing); without them it says nothing, and only WoW Forever has them.
#[test]
fn a_guide_tells_the_xp_buffs_it_counts_on() {
    use fg_route::xp::Edition;
    let buffs = |edition: Edition, food: bool, bag: bool| {
        let p = leveling(5)
            .params(move |p| (p.edition, p.food_xp, p.sleeping_bag) = (edition, food, bag))
            .build();
        let planner = planner(&p);
        let route = planner.construct(|_| {});
        fg_route::export::build(&planner, &p.profile, &route).buffs
    };
    assert_eq!(buffs(Edition::Forever, false, false), Vec::<String>::new());
    assert_eq!(buffs(Edition::Forever, true, false), ["food"]);
    assert_eq!(buffs(Edition::Forever, true, true), ["food", "bag"]);
    assert_eq!(buffs(Edition::Classic, true, true), Vec::<String>::new());
}

/// A route that leaves out a forced dungeon is not valid: the player asked for it.
#[test]
fn a_route_without_a_forced_dungeon_is_not_valid() {
    let p = with_deadmines(leveling(25))
        .params(|p| p.forced_dungeons = vec![900])
        .build();
    let planner = planner(&p);
    let route = planner.construct(|_| {});
    assert!(route.iter().any(|s| s.kind == Kind::Dungeon), "the forced run");
    planner.validate_required(&route).unwrap();
    let error = planner.validate_required(&[]).unwrap_err().to_string();
    assert!(error.contains("forced dungeon"), "{error}");
}

/// Where an entity of the model stands, as the addon records it.
fn map_pos(p: &Prepared, pos: &fg_route::world::Pos, zone: i64) -> MapPos {
    let m = p.world.to_map(zone, pos).unwrap();
    MapPos {
        map: m.ui_map,
        x: m.x,
        y: m.y,
    }
}

/// The valley of an orc: Gornek gives Cutting Teeth (788), then Sting of the Scorpid (789).
fn orc_valley() -> Scenario {
    leveling(30)
        .levels(1, 30)
        .quest(
            Q::new(788, "Cutting Teeth")
                .level(1)
                .giver(1, 60.0, 60.0)
                .kill("Boar", 1, 10, 1, 62.0, 62.0),
        )
        .quest(
            Q::new(789, "Sting of the Scorpid")
                .level(3)
                .after(&[788])
                .giver(1, 60.0, 60.0)
                .kill("Scorpid Worker", 3, 10, 1, 65.0, 65.0),
        )
}

/// An orc of level 4 standing next to Gornek, Cutting Teeth turned in and Sting of the Scorpid
/// in the log (with `objectives` as its log shows them).
fn orc_met_in_game(objectives: Vec<LogObjective>, complete: bool, rested: i64) -> StartState {
    StartState {
        name: "Brouz-Realm".into(),
        level: 4,
        xp: 300,
        rested,
        completed: vec![4641.into(), 788.into()],
        log: vec![LogQuest {
            id: 789.into(),
            complete,
            objectives,
        }],
        position: Some(MapPos {
            map: 1,
            x: 60.0,
            y: 60.0,
        }),
        ..StartState::default()
    }
}

/// A character met in game goes on from where it stands: its level, the quests it did open
/// their follow-ups, and a quest complete in its log is turned in next to it, without taking it
/// again.
#[test]
fn a_character_met_in_game_goes_on_from_where_it_stands() {
    let new = orc_valley().build();
    let scorpids = quest(&new, 789).unwrap();
    let accept = Stop::quest(scorpids, Kind::Accept);
    assert!(
        planner(&new).simulate(&[accept], None).is_none(),
        "a new character must do Cutting Teeth first"
    );

    let p = orc_valley().start(orc_met_in_game(vec![], true, 0)).build();
    assert_eq!(p.profile.from_level, 4);
    let scorpids = quest(&p, 789).unwrap();
    let turn_in = Stop::quest(scorpids, Kind::TurnIn);
    let mut trace = Vec::new();
    assert!(
        planner(&p).simulate(&[turn_in], Some(&mut trace)).is_some(),
        "in the log, complete"
    );
    let at = trace
        .iter()
        .find(|t| matches!(t.event, Event::Stop { stop, .. } if stop == turn_in))
        .unwrap();
    assert!(at.time < 30.0, "next to Gornek: no walk ({} s)", at.time);
    assert!(at.level >= 4);
}

/// The log's objectives done count, matched to the quest's whatever the game's language: the
/// route only does what is left.
#[test]
fn objectives_done_in_the_log_are_not_done_again() {
    let log = |done| {
        vec![LogObjective {
            text: "Queue de scorpide ouvrier : 0/10".into(),
            done,
            need: 10,
            finished: done == 10,
        }]
    };
    let mob_xp = |done| {
        let p = orc_valley()
            .params(|p| p.farm_on_way = false)
            .start(orc_met_in_game(log(done), false, 0))
            .build();
        let scorpids = quest(&p, 789).unwrap();
        // Nothing done: not held, the guide takes the quest itself.
        let mut route = vec![];
        if done == 0 {
            route.push(Stop::quest(scorpids, Kind::Accept));
        }
        route.push(Stop::quest(scorpids, Kind::Objective(0)));
        route.push(Stop::quest(scorpids, Kind::TurnIn));
        planner(&p).simulate_full(&route, None).unwrap().1.mob_xp
    };
    let (none, half, all) = (mob_xp(0), mob_xp(5), mob_xp(10));
    assert!(none > 0 && half < none && half > 0, "{none} then {half}");
    assert_eq!(all, 0);
}

/// Rested XP doubles the XP of kills until it runs out.
#[test]
fn rested_xp_doubles_kill_xp() {
    let objectives = || {
        vec![LogObjective {
            text: "Scorpid Worker Tail: 0/10".into(),
            need: 10,
            ..LogObjective::default()
        }]
    };
    let mob_xp = |rested| {
        let p = orc_valley()
            .params(|p| p.farm_on_way = false)
            .start(orc_met_in_game(objectives(), false, rested))
            .build();
        let scorpids = quest(&p, 789).unwrap();
        let route = [
            Stop::quest(scorpids, Kind::Accept),
            Stop::quest(scorpids, Kind::Objective(0)),
        ];
        planner(&p).simulate_full(&route, None).unwrap().1.mob_xp
    };
    let (plain, rested) = (mob_xp(0), mob_xp(100_000));
    assert!(plain > 0);
    assert!((rested - 2 * plain).abs() <= 10, "{rested} vs 2 x {plain}");
}

/// The quests of the log are taken: the guide finishes those it wants, and abandons the others
/// at its start (the quest the model does not have too).
#[test]
fn quests_of_the_log_the_guide_does_not_finish_are_abandoned_first() {
    // Scorpids begun (2 of 10), a quest the model does not have with nothing done.
    let begun = vec![LogObjective {
        text: "Scorpid Worker Tail: 2/10".into(),
        done: 2,
        need: 10,
        finished: false,
    }];
    let mut start = orc_met_in_game(begun, false, 0);
    start.log.push(LogQuest {
        id: 999_999.into(),
        ..LogQuest::default()
    });
    let p = orc_valley().start(start).build();
    let planner = planner(&p);
    let scorpids = quest(&p, 789).unwrap();
    let abandoned = |route: &[Stop]| -> Vec<i64> {
        let guide = fg_route::export::build(&planner, &p.profile, route);
        guide
            .steps
            .iter()
            .take_while(|s| s.kind == fg_route::export::StepKind::Abandon)
            .filter_map(|s| s.quest)
            .collect()
    };
    assert_eq!(abandoned(&[]), [789, 999_999]);
    let finished = [
        Stop::quest(scorpids, Kind::Objective(0)),
        Stop::quest(scorpids, Kind::TurnIn),
    ];
    assert_eq!(abandoned(&finished), [999_999]);
}

/// A quest complete in the log whose turn-in the route passes by later is kept rather than
/// given up: the construction puts its turn-in where the route goes there.
#[test]
fn quest_complete_in_the_log_is_turned_in_on_the_way() {
    let p = orc_valley()
        .levels(4, 8)
        .start(orc_met_in_game(vec![], true, 0))
        .build();
    let planner = planner(&p);
    let scorpids = quest(&p, 789).unwrap();
    let route = planner.construct(|_| {});
    assert!(route.contains(&Stop::quest(scorpids, Kind::TurnIn)), "{route:?}");
    assert!(!route.contains(&Stop::quest(scorpids, Kind::Accept)));
    let guide = fg_route::export::build(&planner, &p.profile, &route);
    assert!(
        !guide
            .steps
            .iter()
            .any(|s| s.kind == fg_route::export::StepKind::Abandon)
    );
}

/// The hearthstone goes where the character bound it, the flight paths it knows are known, and
/// the quests of its log leave the whole log to the guide (it abandons those it does not take).
#[test]
fn hearthstone_flight_paths_and_log_of_a_character_met_in_game() {
    let mut p = leveling(30)
        .inn("Barrens Inn", 3, 54.0, 54.0)
        .inn("Durotar Inn", 2, 54.0, 54.0)
        .build();
    let (inn, node) = (1, 1);
    let start = StartState {
        level: 12,
        log: vec![LogQuest {
            id: 999_999.into(),
            ..LogQuest::default()
        }],
        bind: Some(Bind {
            name: String::new(),
            areas: vec![],
            position: Some(map_pos(&p, &p.model.inns[inn].pos, p.model.inns[inn].zone)),
        }),
        flights: vec![p.world.taxi_nodes[node].id],
        ..StartState::default()
    };
    let log_size = p.params.quest_log_size;
    let mut notes = Vec::new();
    let initial = start.initial(&p.model, &p.world, &mut p.params, &mut notes);
    assert_eq!(initial.bind, Some(inn));
    assert_eq!(initial.known, 1 << node);
    assert_eq!(p.params.quest_log_size, log_size);
    assert_eq!(initial.log, [(999_999, String::new())]);
    assert_eq!(
        initial.pos.map(|(pos, _)| pos),
        Some(p.model.inns[inn].pos),
        "no position: at the hearthstone"
    );
}
/// The hearthstone is bound anew as the route moves on from the starting inn, each
/// bind worth it: the route without them takes longer. Built with fixed construction weights.
#[test]
fn hearthstone_follows_the_route() {
    let p = &leveling(30)
        .inn("Barrens Inn", 3, 54.0, 54.0)
        .inn("Durotar Inn", 2, 54.0, 54.0)
        .params(|p| {
            (p.zone_stay, p.continent_stay, p.dungeon_quest_weight) = (2.5, 1.0, 0.5);
            (p.fit_penalty, p.fit_above_penalty, p.efficiency_power) = (0.0, 0.0, 0.0);
        })
        .build();
    let planner = planner(p);
    let route = planner.construct(|_| {});
    let inns: Vec<&str> = route
        .iter()
        .filter(|s| s.kind == Kind::Bind)
        .map(|s| p.world.zone_name(p.model.inns[s.index as usize].zone))
        .collect();
    assert!(!inns.is_empty(), "bound at {inns:?}");
    let without: Vec<Stop> = route.iter().filter(|s| s.kind != Kind::Bind).copied().collect();
    let (with, without) = (planner.simulate(&route, None), planner.simulate(&without, None));
    // Without them the same stops take longer, or are no longer possible in that order (the
    // hearthstone's trips changed what was picked up on the way).
    assert!(
        with.is_some() && without.is_none_or(|w| with.unwrap() < w),
        "{with:?} with the binds, {without:?} without ({inns:?})"
    );
}
/// An objective `dx` yards east of the start, finished by killing one level 1 mob.
fn one_kill_east_of_start(p: &Prepared, dx: f64) -> Objective {
    kills_east_of_start(p, dx, 1.0, 1)
}

/// Back to town twice within the hour: the hearthstone is kept for the longer trip (from the
/// farther objective), not used on the first trip it would shorten.
#[test]
fn hearthstone_is_kept_for_the_longer_trip_back() {
    let mut p = quests_at_start(2);
    // As the crow flies: the objectives are points north of the start, wherever they fall.
    p.world.terrain = fg_route::terrain::Terrain::default();
    p.model.quests[0].objectives = vec![one_kill_east_of_start(&p, 1500.0)];
    p.model.quests[1].objectives = vec![one_kill_east_of_start(&p, 3000.0)];
    p.model.initial.accepted = vec![0, 1];
    let route = vec![
        Stop::quest(0, Kind::Objective(0)),
        Stop::quest(0, Kind::TurnIn),
        Stop::quest(1, Kind::Objective(0)),
        Stop::quest(1, Kind::TurnIn),
    ];
    let planner = planner(&p);
    let placed = planner.place_hearths(route.clone());
    let at = placed.iter().position(|s| s.kind == Kind::Hearth);
    assert_eq!(at, Some(3), "{placed:?}");
    assert!(planner.simulate(&placed, None) < planner.simulate(&route, None));
}

/// The hearthstone does not take the character back to an inn of an excluded zone (a character
/// gone to another continent, bound in its start zone it no longer wants).
#[test]
fn hearthstone_avoids_an_excluded_zone() {
    let mut p = quests_at_start(2);
    p.model.quests[0].objectives = vec![one_kill_east_of_start(&p, 1500.0)];
    p.model.quests[1].objectives = vec![one_kill_east_of_start(&p, 3000.0)];
    p.model.initial.accepted = vec![0, 1];
    let route = vec![
        Stop::quest(0, Kind::Objective(0)),
        Stop::quest(0, Kind::TurnIn),
        Stop::quest(1, Kind::Objective(0)),
        Stop::quest(1, Kind::TurnIn),
    ];
    p.params.excluded_zones = vec![p.model.inns[p.model.start_inn].zone];
    let placed = planner(&p).place_hearths(route);
    assert!(!placed.iter().any(|s| s.kind == Kind::Hearth), "{placed:?}");
}

/// Time spent outside the preferred zones counts in the route's score: the same route scores
/// worse when its zone is not the preferred one.
#[test]
fn time_outside_the_preferred_zones_counts() {
    let mut p = quests_at_start(1);
    p.model.quests[0].objectives = vec![one_kill_east_of_start(&p, 1500.0)];
    p.model.initial.accepted = vec![0];
    let route = [Stop::quest(0, Kind::Objective(0)), Stop::quest(0, Kind::TurnIn)];
    let zone = p.model.inns[p.model.start_inn].zone;
    p.params.preferred_zones = vec![zone];
    let inside = planner(&p).simulate(&route, None).unwrap();
    p.params.preferred_zones = vec![zone + 1];
    let outside = planner(&p).simulate(&route, None).unwrap();
    assert!(outside > inside, "{outside} outside, {inside} inside");
}

/// A mage's teleports on the real data: to the capitals of its side only, the first at level 20.
#[test]
fn mage_teleports_are_the_capitals_of_its_side() {
    let p = quests_at_start_on_the_data_as("human", "mage", 0);
    let names: Vec<(&str, i64)> = p.model.teleports.iter().map(|t| (t.name.as_str(), t.level)).collect();
    assert!(names.contains(&("Teleport: Ironforge", 20)), "{names:?}");
    assert!(!names.iter().any(|(n, _)| n.contains("Orgrimmar")), "{names:?}");
}

/// A mage trained in its teleports goes to a capital with one rather than across the world.
#[test]
fn mage_teleports_to_a_capital() {
    let mut p = Scenario::new()
        .levels(25, 30)
        .faction(fg_route::faction::Faction::Alliance)
        .zone(12, "Elwynn Forest")
        .zone(1537, "Ironforge")
        .teleport("Teleport: Ironforge", 20, 1537, 50.0, 50.0)
        .quest(
            Q::new(900_000, "quest 0")
                .level(27)
                .xp(0)
                .observed()
                .giver(1, 50.0, 50.0),
        )
        .build();
    let ironforge = p.model.teleports[0].to.clone();
    let mut o = one_kill_east_of_start(&p, 0.0);
    o.loc = ironforge;
    p.model.quests[0].objectives = vec![o];
    p.model.initial.accepted = vec![0];
    p.model.initial.trained = Some(25);
    let route = [Stop::quest(0, Kind::Objective(0))];
    // When the objective is done (the grinding to the target level comes after).
    let done = |p: &Prepared, teleport: bool| {
        let mut trace = Vec::new();
        planner(p).simulate_full(&route, Some(&mut trace)).unwrap();
        assert_eq!(
            trace.iter().any(|t| matches!(t.event, Event::Teleport { .. })),
            teleport
        );
        (trace.iter())
            .find(|t| matches!(t.event, Event::Stop { .. }))
            .unwrap()
            .time
    };
    let with = done(&p, true);
    p.model.teleports.clear();
    let without = done(&p, false);
    assert!(
        with * 3.0 < without,
        "{with:.0} s to the objective with the teleport, {without:.0} s without"
    );
}

/// A shaman has a second hearthstone from level 30.
#[test]
fn shaman_has_astral_recall() {
    let p = quests_at_start_on_the_data_as("orc", "shaman", 0);
    assert_eq!(p.model.recall, Some(30));
    assert!(p.model.teleports.is_empty());
}

/// Training starts from the spells the character learned: the first rank it lacks.
#[test]
fn last_training_from_the_spells_learned() {
    let power = orc_warlock().model.power.as_ref().unwrap();
    let up_to = |level: i64| power.ranks().filter(|r| r.level <= level).map(|r| r.id).collect();
    assert_eq!(power.trained_level(&up_to(12), 12), Some(12));
    let trained = power.trained_level(&up_to(6), 12).unwrap();
    assert!((6..12).contains(&trained), "trained at {trained}");
    assert_eq!(power.trained_level(&[1].into(), 12), None, "other data");
}

/// A turn-in among stronger mobs waits until the character can fight through them: Sven's
/// Revenge ends at a mound in a camp of level 25-27 Defias (a level 22 warrior died there).
#[test]
fn turn_in_among_strong_mobs_waits() {
    let p = human_warrior();
    let sven = quest(p, 95).unwrap();
    assert!(p.model.quests[sven as usize].end_guard.level >= 26);
    let route = [Stop::quest(sven, Kind::Accept), Stop::quest(sven, Kind::TurnIn)];
    let mut trace = Vec::new();
    assert!(planner(p).simulate(&route, Some(&mut trace)).is_some());
    let at = |stop: Stop| {
        trace
            .iter()
            .find(|t| matches!(t.event, Event::Stop { stop: s, .. } if s == stop))
            .unwrap()
    };
    assert!(at(route[0]).level < 24, "taken early (level {})", at(route[0]).level);
    assert!(at(route[1]).power >= 26.0, "turned in at power {}", at(route[1]).power);
}

/// On a crowded server, escorts wait for their turn and targets with few spawns are fought
/// over: Miran's escort (level 15: 90 % of the crowd) and Bellygrub (one spawn) take waiting,
/// the wolves of Northshire (many spawns) hardly any.
#[test]
fn crowded_server_waits_for_escorts_and_rare_spawns() {
    let quiet = human_warrior();
    let crowded = prepare("human", "warrior", 30, &["progression=cautious", "crowded=true"]);
    let wait = |id: i64| {
        let extra = |p: &Prepared| {
            p.model.quests[quest(p, id).unwrap() as usize]
                .objectives
                .iter()
                .map(|o| o.extra)
                .sum::<f64>()
        };
        extra(&crowded) - extra(quiet)
    };
    assert!(wait(309) >= 0.9 * 900.0 - 1e-6, "escort: {} s", wait(309));
    assert!(wait(34) >= 90.0, "Bellygrub: {} s", wait(34));
    assert!(wait(33) / 8.0 < wait(34) / 2.0, "wolves: {} s for 8 meats", wait(33));
}

/// The clean-up at the end of the search never makes the route worse: cutting the stops after
/// the target level would drop a turn-in planned there that was made at hand before, losing its
/// XP to grinding (it did).
#[test]
fn clean_up_keeps_a_quest_turned_in_at_hand() {
    let mut p = quests_at_start(1);
    (p.model.quests[0].xp, p.params.camp_radius) = (400, 80.0);
    let planner = planner(&p);
    let route = vec![Stop::quest(0, Kind::Accept), Stop::quest(0, Kind::TurnIn)];
    let (before, spent) = planner.simulate_full(&route, None).unwrap();
    assert_eq!(spent.quest_xp, 400, "turned in at hand at the first stop");
    let cleaned = planner.improve(route, std::time::Duration::ZERO, |_| {}, |_, _, _| {});
    let (after, spent) = planner.simulate_full(&cleaned, None).unwrap();
    assert_eq!(spent.quest_xp, 400);
    assert!(after <= before + 0.5, "{before} -> {after}");
}

/// A quest taken at hand needs what its own stop needs: the camp around its giver (three level
/// 25 mobs) is fought through at the same level, not at level 1 because another giver stands
/// nearby (it was).
#[test]
fn quest_at_hand_waits_for_its_camp() {
    let mut p = quests_at_start(2);
    p.params.camp_radius = 80.0;
    p.model.quests[1].start_guard = Guard {
        level: 25,
        count: 3,
        elite: false,
    };
    let planner = planner(&p);
    let taken_at = |route: &[Stop]| {
        let mut trace = Vec::new();
        planner.simulate_full(route, Some(&mut trace)).unwrap();
        trace.iter().find_map(|t| {
            matches!(t.event, Event::Stop { stop, .. } if stop == Stop::quest(1, Kind::Accept)).then_some(t.level)
        })
    };
    let own_stop = taken_at(&[Stop::quest(1, Kind::Accept)]).unwrap();
    assert!(own_stop > 20, "taken at level {own_stop}");
    let at_hand = taken_at(&[Stop::quest(0, Kind::Accept), Stop::quest(1, Kind::Accept)]).unwrap();
    assert_eq!(at_hand, own_stop);
}

/// Kill `kills` mobs of `mob_level` `dx` yards east of the start.
fn kills_east_of_start(p: &Prepared, dx: f64, kills: f64, mob_level: i64) -> Objective {
    let mut loc = p.model.start.clone();
    loc.pos.x += dx;
    Objective {
        loc,
        text: format!("kill x{kills} at {dx} yd"),
        kills,
        mob_level,
        uses: 0.0,
        extra: 0.0,
        dungeon: None,
        elite: false,
        count: kills,
        mobs: vec![],
        pull: 1.0,
        spawns: 0,
        pack: 1.0,
        spots: None,
        guard: Guard::default(),
        beside_giver: false,
        item: false,
        alts: vec![],
        after: 0,
        handed_by: vec![],
        vendors: vec![],
        auction: vec![],
        slot: None,
        lasts: None,
        stock: None,
    }
}

/// Where the objective is done: east of the start by how many yards.
fn done_at(p: &Prepared) -> f64 {
    let mut trace = Vec::new();
    planner(p)
        .simulate_full(&[Stop::quest(0, Kind::Objective(0))], Some(&mut trace))
        .unwrap();
    let loc = trace
        .iter()
        .find_map(|t| match &t.event {
            Event::Stop { loc, .. } => Some(loc.pos.x),
            _ => None,
        })
        .unwrap();
    (loc - p.model.start.pos.x).round()
}

/// Mobs living in several places: the objective is done at the closest one, unless its mobs are
/// too strong there (the levels to grind first cost more than the walk to the other).
#[test]
fn objective_is_done_at_its_best_place() {
    let mut p = quests_at_start(1);
    p.model.initial.accepted = vec![0];
    let mut far = kills_east_of_start(&p, 900.0, 8.0, 1);
    far.alts = vec![kills_east_of_start(&p, 100.0, 8.0, 1)];
    p.model.quests[0].objectives = vec![far.clone()];
    assert_eq!(done_at(&p), 100.0, "the closest place");
    far.alts = vec![kills_east_of_start(&p, 100.0, 8.0, 12)];
    p.model.quests[0].objectives = vec![far];
    assert_eq!(done_at(&p), 900.0, "the weaker mobs, further away");
}

/// In a group, the class quests required of each class are required, not only the
/// character's: a warrior leveling with a warlock goes for the felsteed.
#[test]
fn group_class_quests_are_required() {
    let (warrior, warlock) = (player("A", 1, "warrior", 10), player("B", 1, "warlock", 10));
    let p = prepare_group("human", "warrior", 40, &[], Some(warrior), vec![warlock]);
    let felsteed: Vec<bool> = p
        .model
        .quests
        .iter()
        .filter(|q| q.name == "Summon Felsteed")
        .map(|q| q.mandatory)
        .collect();
    assert!(!felsteed.is_empty() && felsteed.iter().all(|&m| m), "{felsteed:?}");
}

/// Moving a pet quest early (`power_first`, at the end of the search) may cost time, on purpose,
/// but never more than `class_power_slack`; without slack the clean-up never makes the route
/// worse.
#[test]
fn pet_quest_moves_early_within_its_slack() {
    let mut p = quests_at_start(2);
    p.world.terrain = fg_route::terrain::Terrain::default();
    p.model.quests[0].power = Some((0, 1.4));
    p.model.quests[0].ends[0].pos.x += 700.0;
    let route = vec![
        Stop::quest(0, Kind::Accept),
        Stop::quest(1, Kind::Accept),
        Stop::quest(1, Kind::TurnIn),
        Stop::quest(0, Kind::TurnIn),
    ];
    let cost = |p: &Prepared| {
        let planner = planner(p);
        let before = planner.simulate(&route, None).unwrap();
        let cleaned = planner.improve(route.clone(), std::time::Duration::ZERO, |_| {}, |_, _, _| {});
        planner.simulate(&cleaned, None).unwrap() - before
    };
    let slack = p.params.class_power_slack;
    let with_slack = cost(&p);
    assert!(with_slack <= slack + 0.5, "{with_slack} s for a slack of {slack} s");
    p.params.class_power_slack = 0.0;
    let strict = cost(&p);
    assert!(strict <= 0.5, "{strict} s without slack");
}

/// Each player keeps its own race: a night elf druid with a human warrior brings the druid's
/// Aquatic Form (a night elf quest), not the quests of races nobody plays.
#[test]
fn each_player_keeps_its_race() {
    let (warrior, druid) = (player("A", 1, "warrior", 10), player("B", 4, "druid", 10));
    let p = prepare_group("human", "warrior", 30, &[], Some(warrior), vec![druid]);
    let aquatic: Vec<bool> = (p.model.quests.iter())
        .filter(|q| q.name == "Aquatic Form")
        .map(|q| q.mandatory)
        .collect();
    assert_eq!(aquatic, [true]);
    // The druid's quest is for it only (the second member), the warrior's own for it only.
    let of = |name: &str| p.model.quests.iter().find(|q| q.name == name).map(|q| q.members);
    assert_eq!(of("Aquatic Form"), Some(0b10));
    // No dwarf, gnome or Horde race in the group: none of their own quests.
    let others = fg_route::model::race_bit(3) | fg_route::model::race_bit(7) | fg_route::model::race_bit(2);
    let conn = database();
    for q in &p.model.quests {
        let races: Option<i64> = conn
            .query_row("SELECT races_mask FROM m_quest WHERE id = ?1", [q.id], |r| r.get(0))
            .unwrap();
        let races = races.unwrap_or(0);
        assert!(
            races == 0 || races & !others != 0,
            "{} ({}): of a race nobody plays",
            q.name,
            q.id
        );
    }
}

/// The group's guide is planned for the player with the least XP, whoever made it.
#[test]
fn a_group_guide_is_planned_for_the_one_behind() {
    let (warrior, mage) = (player("A", 1, "warrior", 14), player("B", 1, "mage", 12));
    let p = leveling(20).levels(12, 20).start(warrior).mates(vec![mage]).build();
    assert_eq!(p.profile.class_name, "Mage");
    assert_eq!(p.profile.from_level, 12);
    assert_eq!(p.model.initial.mates.len(), 1);
    assert_eq!(p.model.initial.mates[0].level, 14);
}
/// A quest a player did already stays for the others; one everybody did is out of the guide.
#[test]
fn quests_done_by_some_stay_for_the_others() {
    let (both, one) = (109, 110);
    let mut a = player("A", 1, "warrior", 10);
    a.completed = vec![both.into(), one.into()];
    let mut b = player("B", 1, "warrior", 10);
    b.completed = vec![both.into()];
    let p = leveling(20).levels(10, 20).start(a).mates(vec![b]).build();
    let index = |id: i64| p.model.index[&id];
    assert!(!p.model.index.contains_key(&both) || p.model.initial.turned.contains(&index(both)));
    assert!(!p.model.initial.turned.contains(&index(one)));
    // For B only (A and B have the same XP: A, the guide's character, is planned for).
    assert_eq!(p.model.initial.need(index(one)), 0b10);
}
/// A guide is saved without a language: it is written in any of them when shown or installed.
/// The guide saved is the same whatever language it is made in, but for its texts.
#[test]
fn a_guide_is_written_in_any_language() {
    use fg_route::export::{StepKind, read_route, render};
    use fg_route::model::Names;
    let saved = |locale: &str| {
        let p = prepare_request(&PlanRequest {
            locale: Some(locale.into()),
            ..request("orc", "warlock", 4, &[])
        });
        let planner = planner(&p);
        let route = planner.construct(|_| {});
        let mut saved = serde_json::to_value(fg_route::export::build(&planner, &p.profile, &route)).unwrap();
        for step in saved["steps"].as_array_mut().unwrap() {
            step.as_object_mut().unwrap().remove("text");
        }
        saved.as_object_mut().unwrap().remove("locale");
        saved
    };
    let in_english = saved("enUS");
    assert_eq!(saved("frFR"), in_english);
    let guide = read_route(in_english).unwrap();
    let conn = database();
    let names = |locale: &str| Names::load(&conn, locale).unwrap();
    let accept = |g: &fg_route::Route| {
        g.steps
            .iter()
            .find(|s| s.kind == StepKind::Accept && s.quest == Some(788))
            .unwrap()
            .clone()
    };
    let en = render(&guide, &names("enUS"));
    let fr = render(&guide, &names("frFR"));
    let de = render(&guide, &names("deDE"));
    assert_eq!(accept(&en).quest_name.as_deref(), Some("Cutting Teeth"));
    assert!(
        accept(&en).text.starts_with("Accept Cutting Teeth from "),
        "{}",
        accept(&en).text
    );
    assert_eq!(accept(&fr).quest_name.as_deref(), Some("La dent tranchante"));
    assert!(
        accept(&fr).text.starts_with("Prendre La dent tranchante auprès de "),
        "{}",
        accept(&fr).text
    );
    assert!(
        accept(&de).text.starts_with("Scharfe Zähne bei "),
        "{}",
        accept(&de).text
    );
    for locale in [
        "enUS", "frFR", "deDE", "esES", "esMX", "ptBR", "ruRU", "koKR", "zhCN", "zhTW",
    ] {
        let g = render(&guide, &names(locale));
        assert_eq!(g.locale.as_deref(), Some(locale));
        for s in &g.steps {
            assert!(!s.text.is_empty() && !s.text.contains('{'), "{locale}: {s:?}");
        }
    }
}

/// Accept steps tell what their quest needs (quests done, level), so that the addon lists only
/// the quests that can be taken now.
#[test]
fn accept_steps_carry_what_the_quest_needs() {
    let p = generic();
    let planner = planner(p);
    let route = planner.construct(|_| {});
    let guide = fg_route::export::build(&planner, &p.profile, &route);
    let accepts: Vec<_> = guide
        .steps
        .iter()
        .filter(|s| s.kind == fg_route::export::StepKind::Accept)
        .collect();
    let follow_up = accepts
        .iter()
        .find(|s| !s.pre_any.is_empty())
        .expect("a follow-up quest in the route");
    let q = &p.model.quests[p.model.index[&follow_up.quest.unwrap()]];
    assert_eq!((&follow_up.pre, &follow_up.pre_any), (&q.pre_all, &q.pre_any));
    assert!(accepts.iter().all(|s| s.min_level.is_some()));
    let lua = fg_route::export::to_lua(std::slice::from_ref(&guide));
    let ids: Vec<String> = q.pre_any.iter().map(ToString::to_string).collect();
    assert!(
        lua.contains(&format!("pa = {{ {} }}", ids.join(", "))),
        "prerequisites in the addon's routes"
    );
}

/// Each step carries the time it is planned to take (the addon records it next to the time the
/// player took): the steps' times add up to the route's.
#[test]
fn steps_carry_their_planned_time() {
    let p = generic();
    let planner = planner(p);
    let route = planner.construct(|_| {});
    let guide = fg_route::export::build(&planner, &p.profile, &route);
    let lua = fg_route::export::to_lua(std::slice::from_ref(&guide));
    let durations: Vec<f64> = lua
        .lines()
        .filter_map(|line| line.split(", d = ").nth(1))
        .map(|rest| rest.split([',', ' ']).next().unwrap().parse().unwrap())
        .collect();
    assert!(durations.len() > guide.steps.len() / 2, "most steps take some time");
    let end = guide.steps.last().unwrap().time;
    let sum: f64 = durations.iter().sum();
    assert!(
        (sum - end).abs() <= guide.steps.len() as f64,
        "planned times {sum} s, route {end} s"
    );
}

/// Blackrock Menace (axes from Blackrock orcs, Redridge): its spot near the giver holds only a
/// few orcs standing in pairs, one point for a Grunt or an Outrunner taking turns. The pairs
/// make it harder than lone orcs, the ~22 kills wait for respawns, more on a crowded server, if
/// less than with fixed respawns (the game shortens them with players around).
#[test]
fn small_spot_in_pairs_is_harder_and_slower() {
    let p = human_warrior();
    let objective = |p: &Prepared| p.model.quests[quest(p, 20).unwrap() as usize].objectives[0].clone();
    let o = objective(p);
    assert!((5..=10).contains(&o.spawns), "{} spawns", o.spawns);
    assert!(o.pack >= 1.5, "{} mobs per pull", o.pack);
    let q = &p.model.quests[quest(p, 20).unwrap() as usize];
    let need = planner(p).combat().need(q, &o);
    assert!(need >= o.mob_level as f64 + p.params.pack_extra() + 0.5, "need {need}");
    assert!(o.extra > 0.0, "respawns waited for");
    let crowded = objective(&prepare("human", "warrior", 30, &["progression=cautious", "crowded=1"])).extra;
    let fixed = objective(&prepare(
        "human",
        "warrior",
        30,
        &["progression=cautious", "crowded=1", "dynamic_respawn=0"],
    ))
    .extra;
    assert!(crowded > 1.5 * o.extra, "{crowded} vs {}", o.extra);
    assert!(fixed > 3.0 * o.extra && fixed > 1.5 * crowded, "{fixed} vs {crowded}");
}

/// Settings saved before the crowd scale (`crowded: true`) read as a full crowd.
#[test]
fn crowded_on_reads_as_a_full_crowd() {
    let on: fg_route::params::Params = serde_json::from_str(r#"{"crowded": true}"#).unwrap();
    let half: fg_route::params::Params = serde_json::from_str(r#"{"crowded": 0.5}"#).unwrap();
    assert!((on.crowded - 1.0).abs() < 1e-9 && (half.crowded - 0.5).abs() < 1e-9);
}

/// The crowd thins out with the levels: all of it in the starting zones, half from level 40.
#[test]
fn crowd_thins_out_with_the_levels() {
    let p: fg_route::params::Params = serde_json::from_str(r#"{"crowded": 1}"#).unwrap();
    let at: Vec<f64> = [1, 10, 11, 25, 35, 45, 60, 70].iter().map(|&l| p.crowd_at(l)).collect();
    assert_eq!(at, [1.0, 1.0, 0.9, 0.8, 0.6, 0.5, 0.5, 0.5]);
}

/// Every level the route reaches has a step to catch up (grind) for a character behind it, at
/// a place of the level where mobs live, asking at most 90 % of the level: a character a few
/// percent behind goes on.
#[test]
fn every_level_has_a_step_to_catch_up() {
    let p = generic();
    let planner = planner(p);
    let route = planner.construct(|_| {});
    let guide = fg_route::export::build(&planner, &p.profile, &route);
    let rules = p.params.edition.rules();
    let gates: Vec<_> = guide
        .steps
        .iter()
        .filter(|s| s.say.first().is_some_and(|p| p.key == "catch_up"))
        .collect();
    let levels: std::collections::BTreeSet<i64> = gates.iter().map(|s| s.level).collect();
    assert!(levels.len() >= 15, "levels with a step: {levels:?}");
    for s in &gates {
        assert!(
            s.xp.unwrap() as f64 <= 0.9 * rules.to_next_level(s.level) as f64 + 1.0,
            "level {} asks {} xp",
            s.level,
            s.xp.unwrap()
        );
    }
}

/// Turn-in steps tell the XP they give, for the addon's grind steps to count the quests
/// complete in the log.
#[test]
fn turn_ins_carry_their_xp() {
    let p = generic();
    let planner = planner(p);
    let guide = fg_route::export::build(&planner, &p.profile, &planner.construct(|_| {}));
    let turn_ins: Vec<_> = guide
        .steps
        .iter()
        .filter(|s| s.kind == fg_route::export::StepKind::TurnIn)
        .collect();
    assert!(turn_ins.iter().all(|s| s.reward_xp.is_some()));
    assert!(turn_ins.iter().filter(|s| s.reward_xp.unwrap() > 0).count() * 10 >= turn_ins.len() * 9);
    assert!(fg_route::export::to_lua(std::slice::from_ref(&guide)).contains(", rx = "));
}

/// Every step tells the XP the route has once it is done, never going down from one step to
/// the next (planned grinds moved earlier included): the addon shows it on the XP bar.
#[test]
fn steps_carry_the_planned_xp() {
    let p = generic();
    let planner = planner(p);
    let guide = fg_route::export::build(&planner, &p.profile, &planner.construct(|_| {}));
    let rules = p.params.edition.rules();
    let total = |s: &fg_route::export::Step| {
        (1..s.level).map(|l| rules.to_next_level(l)).sum::<i64>() + s.route_xp.expect("planned xp")
    };
    for pair in guide.steps.windows(2) {
        assert!(
            total(&pair[1]) >= total(&pair[0]),
            "{} -> {}",
            pair[0].text,
            pair[1].text
        );
    }
}

/// The tauren druid's bear form: Body and Heart is required with its chain through Moonglade
/// (one possible prerequisite each), Moonglade is reached by the teleport the quest
/// "Moonglade" teaches, and Lunaclaw Spirit is met at the Horde's Moonkin Stone, in the Barrens.
#[test]
fn druid_bear_form_through_moonglade() {
    let p = prepare("tauren", "druid", 16, &[]);
    let at = |id: i64| &p.model.quests[p.model.index[&id]];
    for id in [5922, 5930, 5932, 6002] {
        assert!(at(id).mandatory, "{} ({id}) not required", at(id).name);
    }
    assert_eq!(at(6002).objectives[0].loc.zone, 17);
    let teleport = p
        .model
        .teleports
        .iter()
        .find(|t| t.name == "Teleport: Moonglade")
        .unwrap();
    assert_eq!(
        teleport.ability,
        p.model.abilities.bit(18960).expect("Moonglade ability")
    );
    assert!(
        p.model
            .teleports
            .iter()
            .all(|t| t.name != "Teleport: Moonglade" || t.level == 0)
    );
}

/// A dungeon whose quests are filed under it is listed even when the sources place nothing its
/// quests need inside (WoW Forever's Ruins of Lordaeron, GitHub #18): here The Deadmines with every
/// spawn inside removed.
#[test]
fn dungeons_with_quests_filed_under_them() {
    let path = std::env::temp_dir().join(format!("fg-dungeons-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    database().execute("VACUUM INTO ?1", [path.to_str().unwrap()]).unwrap();
    let conn = Connection::open(&path).unwrap();
    conn.execute(
        "DELETE FROM m_spawn WHERE zone_id IN (SELECT area FROM dungeon WHERE name = 'The Deadmines'
           UNION SELECT j.value FROM dungeon, json_each(coalesce(alt_areas, '[]')) j WHERE name = 'The Deadmines')",
        [],
    )
    .unwrap();
    let options = job::options(&conn, &overrides()).unwrap();
    drop(conn);
    std::fs::remove_file(&path).ok();
    assert!(options.dungeons.iter().any(|d| d.name == "The Deadmines"));
}

/// Dungeons whose zone the game files under the continent (Blackrock Depths, Blackrock Spire,
/// the Sunken Temple) are found through their instance map.
#[test]
fn dungeons_filed_under_the_continent() {
    let options = job::options(&database(), &overrides()).unwrap();
    for name in ["Blackrock Depths", "Blackrock Spire", "The Temple of Atal'Hakkar"] {
        assert!(options.dungeons.iter().any(|d| d.name == name), "{name} missing");
    }
}

/// City of Dalaran (Forever) is entered by a tunnel at the west edge of the Alterac Mountains
/// by the Horde, by the pipe in the Alliance's own quarter further in: each faction's guide
/// plans the dungeon at its own door.
#[test]
fn city_of_dalaran_has_a_door_for_each_faction() {
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let door = |race: &str| {
        let request = PlanRequest {
            params: Some(overrides.params.with_overrides(&[]).unwrap()),
            ..request(race, "mage", 40, &[])
        };
        let p = job::prepare(&database_of("forever"), &overrides, &request, &|_| {}).unwrap();
        let dalaran = p
            .model
            .dungeons
            .iter()
            .find(|d| d.area == 16544)
            .expect("City of Dalaran");
        let at = p.world.to_map(dalaran.entrance.zone, &dalaran.entrance.pos).unwrap();
        (dalaran.entrance.zone, at.x, at.y)
    };
    let close = |(zone, x, y): (i64, f64, f64), (z, ex, ey): (i64, f64, f64)| {
        zone == z && (x - ex).abs() < 0.1 && (y - ey).abs() < 0.1
    };
    let alliance = door("human");
    let horde = door("orc");
    assert!(close(alliance, (36, 14.7, 63.0)), "Alliance: {alliance:?}");
    assert!(close(horde, (36, 8.46, 59.21)), "Horde: {horde:?}");
}

/// A book for the library (Forever, GitHub #29): the librarian offers "The Dalaran Digest" only
/// while the book is in the bags. Looting it is the accept, the quest is taken and turned in
/// together at the librarian, never sent there before the book.
#[test]
fn library_book_is_looted_before_the_librarian() {
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let request = PlanRequest {
        // A level 1 walks to Silverpine for the book: the zones too strong for it count elsewhere.
        params: Some(overrides.params.with_overrides(&["zone_gap=0".to_owned()]).unwrap()),
        ..request("human", "mage", 30, &[])
    };
    let p = job::prepare(&database_of("forever"), &overrides, &request, &|_| {}).unwrap();
    let digest = quest(&p, 78127).expect("The Dalaran Digest planned");
    let q = &p.model.quests[digest as usize];
    assert_eq!(q.held, Some(209844), "offered for The Dalaran Digest, Vol. 23");
    assert!(
        q.starts.iter().all(|l| l.kind == EntityKind::Item && l.id == 209844),
        "accepted where the book lies, not at the librarian"
    );
    assert!(
        q.objectives.iter().all(|o| o.loc.id != 209844),
        "the book is not got again"
    );
    assert!(
        q.ends.iter().any(|l| l.kind == EntityKind::Npc && l.id == 211033),
        "Garion Wendell"
    );
    let planner = planner(&p);
    let route = [Stop::quest(digest, Kind::Accept), Stop::quest(digest, Kind::TurnIn)];
    assert!(
        planner.simulate_full(&route, None).is_some(),
        "{:?}",
        planner.diagnose(&route)
    );
    let guide = fg_route::export::build(&planner, &p.profile, &route);
    let steps: Vec<_> = guide.steps.iter().filter(|s| s.quest == Some(78127)).collect();
    let accept = steps
        .iter()
        .find(|s| s.kind == fg_route::export::StepKind::Accept)
        .unwrap();
    assert!(accept.held && accept.text.starts_with("Loot "), "{}", accept.text);
    assert_eq!(accept.zone.as_deref(), Some("Silverpine Forest"));
    let turnin = steps
        .iter()
        .find(|s| s.kind == fg_route::export::StepKind::TurnIn)
        .unwrap();
    assert!(
        turnin.text.starts_with("Accept The Dalaran Digest from "),
        "{}",
        turnin.text
    );
}

/// An item bought (Look to the Stars' Bronze Tube) names its vendor in Darkshire, with its limited
/// supply, then others of the character's side; never a vendor of the other side (Sovik in
/// Orgrimmar).
#[test]
fn bought_item_names_its_vendors() {
    let p = human_warrior();
    let stars = quest(p, 174).unwrap();
    let o = p.model.quests[stars as usize]
        .objectives
        .iter()
        .find(|o| !o.vendors.is_empty())
        .expect("a buy objective");
    let first = &o.vendors[0];
    assert_eq!(first.name, "Herble Baubbletump");
    assert_eq!(first.stock, Some((2, 10800.0)));
    assert!(o.vendors.len() > 3);
    assert!(o.vendors.iter().all(|v| v.id != 3413), "Sovik sells to the Horde only");
    assert!(o.loc.pos.dist(&first.pos) < 50.0, "the place is at the first vendor");
}

/// An item in limited supply is bought at the vendor on an empty server and at an auction house
/// on a crowded one (the vendor often out, many players selling it): the places cost so.
#[test]
fn limited_supply_follows_the_crowd() {
    let cost = |crowded: &str| {
        let p = prepare("human", "warrior", 25, &[crowded]);
        let stars = quest(&p, 174).unwrap();
        let o = p.model.quests[stars as usize]
            .objectives
            .iter()
            .find(|o| !o.vendors.is_empty())
            .expect("a buy objective")
            .clone();
        let auction = o
            .alts
            .iter()
            .find(|a| !a.auction.is_empty())
            .expect("an auction house")
            .extra;
        (o.extra, auction)
    };
    let (vendor, auction) = cost("crowded=0");
    assert!(
        vendor < auction,
        "empty server: {vendor} at the vendor, {auction} at the auction house"
    );
    let (vendor, auction) = cost("crowded=1");
    assert!(
        vendor > auction,
        "crowded server: {vendor} at the vendor, {auction} at the auction house"
    );
}

/// The Great Outdoors (Forever, GitHub #35): sitting at the campfire beside the quest's giver is
/// done in the camp at the level the quest is taken, not a level later for the quest's level;
/// only an object beside the giver counts, never kills.
#[test]
fn campfire_beside_the_giver_needs_no_power() {
    let p = leveling(10)
        .quest(
            Q::new(900, "Outdoors")
                .level(8)
                .min_level(5)
                .giver(1, 50.0, 40.0)
                .objective(Obj::visit(
                    "Sit by the Basic Campfire",
                    At {
                        zone: 1,
                        x: 50.5,
                        y: 40.0,
                    },
                )),
        )
        .quest(
            Q::new(901, "Far fire")
                .level(8)
                .min_level(5)
                .giver(1, 50.0, 40.0)
                .objective(Obj::visit(
                    "Sit by the other fire",
                    At {
                        zone: 1,
                        x: 80.0,
                        y: 40.0,
                    },
                )),
        )
        .quest(
            Q::new(902, "Beasts at the door")
                .level(8)
                .min_level(5)
                .giver(1, 50.0, 40.0)
                .kill("Wolf", 8, 3, 1, 50.5, 40.0),
        )
        .build();
    let planner = planner(&p);
    let might = fg_route::params::Might::PLAIN;
    let need = |id: i64| {
        let q = &p.model.quests[quest(&p, id).unwrap() as usize];
        (
            q.objectives[0].beside_giver,
            planner.combat().level(q, 0, might),
            q.min_level,
        )
    };
    let (beside, level, min) = need(900);
    assert!(beside && level <= min, "done at level {level}, taken at {min}");
    assert!(!need(901).0, "a fire far from the giver is not beside them");
    assert!(!need(902).0, "kills are never beside the giver");
}

/// The Forever data marks The Great Outdoors' campfire beside Kaga Wildhoof and the others.
#[test]
fn great_outdoors_campfires_are_beside_their_giver() {
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let request = PlanRequest {
        params: Some(
            overrides
                .params
                .with_overrides(&["progression=cautious".into()])
                .unwrap(),
        ),
        ..request("human", "warrior", 20, &[])
    };
    let p = job::prepare(&database_of("forever"), &overrides, &request, &|_| {}).unwrap();
    let q = &p.model.quests[quest(&p, 95998).unwrap() as usize];
    let o = &q.objectives[0];
    assert!(o.text.starts_with("Sit by the Basic Campfire"), "{}", o.text);
    assert!(o.beside_giver, "the campfire is beside {}", q.starts[0].name);
}

/// Camping 101 (Forever, GitHub #35): each profession's asks not to know the camp item it
/// teaches yet, which a character leveling never does. They are the profession's quests,
/// optional, after The Great Outdoors; left out without the profession.
#[test]
fn camping_101_is_a_profession_quest_after_the_great_outdoors() {
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let goal = |key: &str| fg_route::profession::ProfessionGoal {
        key: key.into(),
        target: 300,
        start_level: None,
        milestones: vec![],
    };
    let request = PlanRequest {
        params: Some(overrides.params.with_overrides(&[]).unwrap()),
        professions: vec![goal("skinning"), goal("leatherworking")],
        ..request("tauren", "druid", 20, &[])
    };
    let p = job::prepare(&database_of("forever"), &overrides, &request, &|_| {}).unwrap();
    // Camping 101: Skinning and Leatherworking at Kaga Wildhoof.
    for id in [97936, 97934] {
        let i = quest(&p, id).unwrap_or_else(|| panic!("Camping 101 {id} planned"));
        let q = &p.model.quests[i as usize];
        assert!(q.skill.is_some() && !q.mandatory, "{id}: optional, for the profession");
        assert!(q.pre_any.contains(&96605), "{id}: after The Great Outdoors");
    }
    assert!(quest(&p, 97935).is_none(), "Camping 101: Mining without Mining");
}

/// The Warsong Reports (Horde, Ashenvale): Kadrak gives the Bundle of Reports with the quest,
/// each scout takes the Warsong Report out of it and hands over the update Kadrak asks for. The
/// scouts' quests are children of The Warsong Reports: their updates are got at their hand-in,
/// neither bought nor looked for, nor the report in the bundle.
#[test]
fn items_a_child_hands_over_are_got_at_its_hand_in() {
    let p = orc_warlock();
    let reports = quest(p, 6543).expect("The Warsong Reports planned");
    let q = &p.model.quests[reports as usize];
    let handing: Vec<&Vec<i64>> = q
        .objectives
        .iter()
        .map(|o| &o.handed_by)
        .filter(|h| !h.is_empty())
        .collect();
    assert_eq!(handing.len(), 3, "the three scouts' updates: {handing:?}");
    assert!(
        q.objectives.iter().all(|o| o.vendors.is_empty()),
        "no update to buy: {:?}",
        q.objectives.iter().map(|o| &o.text).collect::<Vec<_>>()
    );
}

/// Places of known height stand on their floor where several lie over each other: Fargodeep
/// Mine's kobolds, Goldtooth and the area to scout are in the mine, not on the hill over it.
#[test]
fn fargodeep_mine_is_under_the_hill() {
    let p = human_warrior();
    let mut checked = 0;
    for name in [
        "The Fargodeep Mine",
        "Goldtooth",
        "Gold Dust Exchange",
        "Kobold Candles",
    ] {
        let q = (p.model.quests.iter().find(|q| q.name == name)).unwrap_or_else(|| panic!("{name} planned"));
        for o in &q.objectives {
            let (floors, on) = p.world.terrain.floors_at(&o.loc.pos).unwrap();
            assert!(o.loc.pos.z.is_some(), "{name}: {} has its height", o.loc.name);
            let top = *floors.last().unwrap();
            assert!(
                on.is_some_and(|h| h <= top - 20),
                "{name}: {} on {on:?} of {floors:?}",
                o.loc.name
            );
            checked += 1;
        }
    }
    assert!(checked >= 4);
}

/// Thendal Grove's watchtower (WoW Forever, Zephras Isle): Halaan Hawk-Eye stands on its top
/// (the height the quest data gives), walked to up its stairs from the ground below.
#[test]
fn zephras_watchtower_is_climbed() {
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let request = PlanRequest {
        params: Some(overrides.params.with_overrides(&[]).unwrap()),
        ..request("skyborne", "mage", 5, &[])
    };
    let p = job::prepare(&database_of("forever"), &overrides, &request, &|_| {}).unwrap();
    let q = &p.model.quests[quest(&p, 94414).expect("The Anchors of Zephras planned") as usize];
    let halaan = &q.starts[0].pos;
    assert_eq!(halaan.z, Some(1020));
    let below = fg_route::world::Pos { z: None, ..*halaan };
    let ground = p.world.to_world(16593, 43.0, 25.5).unwrap();
    let (up, foot) = (
        p.world.walk_distance(&ground, halaan),
        p.world.walk_distance(&ground, &below),
    );
    assert!(up > foot + 40.0, "up the stairs: {up} yd, {foot} yd to the foot");
}

/// A group takes on what its weakest class does at each level: its curve is never above the
/// curve of any of its classes.
#[test]
fn a_group_takes_on_what_its_weakest_class_does() {
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let curve = |class: &str| {
        let points = overrides.class_power[class].points().unwrap();
        job::curve_by_level(&points, 100)
    };
    let (warrior, mage) = (curve("warrior"), curve("mage"));
    let mut group = warrior.clone();
    overrides.weakest_curve(&mut group, &["mage".to_owned()], 100).unwrap();
    for (l, g) in group.iter().enumerate() {
        assert!(*g <= warrior[l] + 1e-9 && *g <= mage[l] + 1e-9, "level {l}");
    }
    assert!(group.iter().zip(&warrior).any(|(g, w)| g < w) || group.iter().zip(&mage).any(|(g, m)| g < m));
}
/// A group guide carries its players to the addon, each step where each of them is, and who
/// the steps not for everyone are for.
#[test]
fn a_group_guide_tells_the_addon_its_players() {
    let route = leveling(14)
        .levels(10, 14)
        .start(player("A-Realm", 1, "warrior", 10))
        .mates(vec![player("B-Realm", 1, "mage", 10)])
        .quest(Q::new(700, "Warrior only").level(11).members(0b01).giver(2, 30.0, 40.0))
        .job(&|_| {}, &job::Interrupt::default(), None)
        .unwrap()
        .route;
    let names: Vec<&str> = route.players.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["A-Realm", "B-Realm"]);
    assert!(
        route
            .steps
            .iter()
            .filter(|s| s.level > 0)
            .all(|s| s.party.len() == 2 || s.event.is_none())
    );
    let lua = fg_route::export::to_lua(std::slice::from_ref(&route));
    assert!(lua.contains(
        r#"players = { { n = "A-Realm", race = 1, class = 1, professions = {  } }, { n = "B-Realm", race = 1, class = 8, professions = {  } } }"#
    ));
    assert!(lua.contains("gp = { "));
    // A quest for one of them is theirs only.
    for s in route.steps.iter().filter(|s| s.quest == Some(700)) {
        assert_eq!(s.members, Some(0b01), "{:?}", s.quest_name);
    }
}
/// A step of an edited guide that only a zone too strong for the character makes possible is
/// done, with a warning (the planner itself keeps out of such zones): Gadgetzan's flight path
/// from Thousand Needles at level 34, Tanaris closed to it.
#[test]
fn an_edited_step_may_walk_into_a_closed_zone_with_a_warning() {
    use fg_route::plan::{Edit, Outcome};
    let probe = prepare_from("human", "warrior", 40, &["progression=cautious"], None);
    let needles = 400;
    let zherin = probe.world.to_world(needles, 81.63, 77.95).unwrap();
    let start = StartState {
        name: "Test-Realm".into(),
        race: 1,
        class: "WARRIOR".into(),
        faction: Some(fg_route::faction::Faction::Alliance),
        level: 34,
        xp_max: 10_000,
        position: Some(map_pos(&probe, &zherin, needles)),
        ..StartState::default()
    };
    let p = prepare_from("human", "warrior", 40, &["progression=cautious"], Some(start));
    let node = p
        .world
        .taxi_nodes
        .iter()
        .position(|n| n.name.starts_with("Gadgetzan"))
        .unwrap();
    let learn = Edit::Stop {
        stop: Stop {
            index: node as u32,
            kind: Kind::LearnFlight,
        },
        farm: false,
        spot: None,
    };
    // The planner's own walks cannot go there at that level.
    let gadgetzan = p.world.taxi_nodes[node].pos;
    assert!(
        p.world
            .walk_distance_at(&zherin, &gadgetzan, p.world.walk_level(34))
            .is_infinite()
    );
    let edited = planner(&p).explicit().edit(&[learn]);
    assert_eq!(edited.steps[0].outcome, Outcome::Done);
    assert!(edited.steps[0].warning.is_some());
    assert_eq!(edited.errors(), 0);
    // Once the character is strong enough, nothing to warn of.
    let strong = prepare_from(
        "human",
        "warrior",
        45,
        &["progression=cautious"],
        Some(StartState {
            level: 40,
            position: Some(map_pos(&probe, &zherin, needles)),
            name: "Test-Realm".into(),
            race: 1,
            class: "WARRIOR".into(),
            faction: Some(fg_route::faction::Faction::Alliance),
            xp_max: 10_000,
            ..StartState::default()
        }),
    );
    let edited = planner(&strong).explicit().edit(&[learn]);
    assert_eq!(edited.steps[0].outcome, Outcome::Done);
    assert!(edited.steps[0].warning.is_none());
}

/// A new character knows its capital's flight path, as the game gives it (GitHub #60): the
/// races' lists (the deduction for a character met in game is tested in `job::start`).
#[test]
fn a_new_character_knows_its_capitals_flight_path() {
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let (stormwind, auberdine, rutheran) = (2, 26, 27);
    assert_eq!(overrides.start_flights(1), [stormwind]);
    assert_eq!(overrides.start_flights(4), [auberdine, rutheran]);
}

/// WoW Forever's Ruins of Lordaeron is a level 15-20 dungeon though its quests say 21-22
/// (GitHub #48): the game's range sets its monsters' level, hence when a group may run it (3
/// levels below them in the normal profile).
#[test]
fn ruins_of_lordaeron_is_run_at_the_games_levels() {
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let request = PlanRequest {
        params: Some(
            overrides
                .params
                .with_overrides(&["progression=normal".to_owned()])
                .unwrap(),
        ),
        ..request("undead", "warrior", 30, &[])
    };
    let p = job::prepare(&database_of("forever"), &overrides, &request, &|_| {}).unwrap();
    let ruins = p
        .model
        .dungeons
        .iter()
        .find(|d| d.area == 16611)
        .expect("Ruins of Lordaeron");
    assert_eq!((ruins.mob_level, ruins.min_level, ruins.max_level), (18, 15, 21));
    // The app shows the same level.
    let options = job::options(&database_of("forever"), &overrides).unwrap();
    let listed = options.dungeons.iter().find(|d| d.id == 16611).unwrap();
    assert_eq!(listed.level, Some(18));
}

/// The levels chosen for the dungeons (`dungeon_from`, `dungeon_to`) are where the guide runs them,
/// never past them; a choice leaving no levels for a dungeon it could run is refused.
#[test]
fn dungeons_are_run_within_the_levels_chosen() {
    let scenario = |from: i64, to: i64| {
        leveling(30)
            .dungeon("The Deadmines", 1, 70.0, 70.0)
            .quest(
                Q::new(300, "Red Silk Bandanas")
                    .level(18)
                    .giver(1, 40.0, 40.0)
                    .objective(
                        Obj::kill(
                            "Defias",
                            18,
                            6,
                            At {
                                zone: 1,
                                x: 70.0,
                                y: 70.0,
                            },
                        )
                        .inside(0),
                    ),
            )
            .params(move |p| (p.dungeon_from, p.dungeon_to, p.effort) = (from, to, 0.0))
    };
    let p = scenario(0, 1).build();
    let route = scenario(0, 1)
        .job(&|_| {}, &job::Interrupt::default(), None)
        .unwrap()
        .route;
    let runs = route
        .steps
        .iter()
        .filter(|s| s.kind == fg_route::export::StepKind::Dungeon)
        .count();
    assert!(runs > 0, "the dungeon is run");
    for s in route
        .steps
        .iter()
        .filter(|s| s.kind == fg_route::export::StepKind::Dungeon)
    {
        let d = p.model.dungeons.iter().find(|d| s.text.contains(&d.name)).unwrap();
        assert!(
            d.min_level <= s.level && s.level <= d.max_level + 1,
            "{} at {}",
            d.name,
            s.level
        );
    }
    let err = scenario(3, 1).try_build().err().map(|e| e.to_string());
    assert!(err.is_some_and(|e| e.contains("leave none")));
}

/// A quest filed under a dungeon whose starting item no source places (Forever's Crest of
/// Lordaeron, from an object of Ruins of Lordaeron, GitHub #61) is started during the run.
#[test]
fn a_dungeon_quest_started_by_an_unplaced_item_starts_in_the_run() {
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let request = PlanRequest {
        params: Some(overrides.params.with_overrides(&[]).unwrap()),
        ..request("human", "warrior", 30, &[])
    };
    let p = job::prepare(&database_of("forever"), &overrides, &request, &|_| {}).unwrap();
    let ruins = p.model.dungeons.iter().position(|d| d.area == 16611).unwrap();
    for id in [95189, 92415] {
        let q = &p.model.quests[quest(&p, id).unwrap_or_else(|| panic!("{id} planned")) as usize];
        assert_eq!(q.start_dungeon, Some(ruins), "{}", q.name);
    }
}

/// Forever's Rare Books (GitHub #59): its books drop from elites, left out of a solo guide, but
/// a quest the player asks for is planned with its elites; a book whose name holds a colon is
/// one book, not the default count.
#[test]
fn rare_books_asked_for_are_planned_with_one_of_each_book() {
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let request = PlanRequest {
        params: Some(overrides.params.with_overrides(&[]).unwrap()),
        required_quests: vec![job::RequiredQuest::Id(91777)],
        ..request("human", "warrior", 12, &[])
    };
    let p = job::prepare(&database_of("forever"), &overrides, &request, &|_| {}).unwrap();
    let counts = |id: i64| {
        let q = &p.model.quests[quest(&p, id).unwrap_or_else(|| panic!("{id} planned")) as usize];
        q.objectives.iter().map(|o| (o.loc.id, o.count)).collect::<Vec<_>>()
    };
    assert_eq!(counts(91777), [(248003, 1.0), (248004, 1.0)]);
    assert!(counts(91775).contains(&(248002, 1.0)) && counts(91775).contains(&(248001, 6.0)));
}

/// Ishamuhale comes to a Fresh Zhevra Carcass, which rots in 30 minutes (GitHub #51): the fang
/// is got after the carcass, and the carcass no earlier than the kill can follow.
#[test]
fn a_carcass_that_rots_is_looted_right_before_ishamuhale() {
    let p = leveling(25)
        .levels(10, 25)
        .quest(
            Q::new(882, "Ishamuhale")
                .level(20)
                .giver(2, 30.0, 40.0)
                .objective(
                    Obj::kill(
                        "Zhevra",
                        20,
                        1,
                        At {
                            zone: 2,
                            x: 35.0,
                            y: 70.0,
                        },
                    )
                    .lasts(1800.0),
                )
                .objective(
                    Obj::visit(
                        "Fang",
                        At {
                            zone: 2,
                            x: 36.0,
                            y: 70.0,
                        },
                    )
                    .after(&[0]),
                ),
        )
        .build();
    let ishamuhale = quest(&p, 882).unwrap();
    let stop = |s: Stop| Edit::Stop {
        stop: s,
        farm: false,
        spot: None,
    };
    let run = |grind: Option<i64>| {
        let mut edits = vec![
            Edit::GrindTo { level: 20 },
            stop(Stop::quest(ishamuhale, Kind::Accept)),
            stop(Stop::quest(ishamuhale, Kind::Objective(0))),
        ];
        edits.extend(grind.map(|level| Edit::GrindTo { level }));
        edits.push(stop(Stop::quest(ishamuhale, Kind::Objective(1))));
        planner(&p).explicit().edit(&edits).steps.last().unwrap().outcome
    };
    assert_eq!(run(None), Outcome::Done);
    assert_ne!(run(Some(25)), Outcome::Done, "the carcass rotted while grinding");
}
/// Each player of a group guide levels its own professions, from its own profile: here A, with
/// skinning at 50, is not the one the guide is planned for (B has less XP). A profession quest
/// is for those leveling it, and each player's addon gets its own curve.
#[test]
fn a_group_guide_levels_each_players_professions() {
    let skinning = || fg_route::profession::ProfessionGoal {
        key: "skinning".into(),
        target: 150,
        start_level: None,
        milestones: vec![],
    };
    let mut a = player("A-Realm", 2, "warrior", 10);
    a.xp = 500;
    a.professions = vec![job::Skill { line: 393, rank: 50 }];
    let request = PlanRequest {
        start: Some(a),
        professions: vec![skinning()],
        mates: vec![player("B-Realm", 2, "warrior", 10)],
        mate_professions: vec![vec![]],
        ..request("orc", "warrior", 13, &["effort=0"])
    };
    let p = prepare_request(&request);
    assert!(p.profile.professions.is_empty(), "B's own: none");
    let plan = p
        .model
        .professions
        .iter()
        .find(|x| x.key == "skinning")
        .expect("A's skinning");
    assert_eq!((plan.members, plan.curve[10]), (0b10, 50));
    let leather = quest(&p, 768).expect("Gathering Leather planned");
    assert_eq!(p.model.quests[leather as usize].members, 0b10, "for A only");
    let route = job::plan(&database(), &overrides(), &request, &|_| {}).unwrap().route;
    let own = |name: &str| {
        let player = route.players.iter().find(|x| x.name == name).unwrap();
        player.professions.iter().map(|x| x.key.as_str()).collect::<Vec<_>>()
    };
    assert_eq!((own("A-Realm"), own("B-Realm")), (vec!["skinning"], vec![]));
    assert!(route.professions.is_empty(), "the guide's character is B");
}

/// Two new characters the addon never saw, an orc and a troll, level together from the Valley of
/// Trials: the guide starts there for both.
#[test]
fn new_characters_level_together_from_the_start() {
    let mut orc = player("Player 1", 2, "warrior", 1);
    orc.faction = Some(fg_route::faction::Faction::Horde);
    let mut troll = player("Player 2", 8, "mage", 1);
    troll.faction = Some(fg_route::faction::Faction::Horde);
    let request = PlanRequest {
        start: Some(orc),
        mates: vec![troll],
        mate_professions: vec![vec![]],
        ..request("orc", "warrior", 4, &["effort=0"])
    };
    let route = job::plan(&database(), &overrides(), &request, &|_| {}).unwrap().route;
    let names: Vec<&str> = route.players.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Player 1", "Player 2"]);
    assert!(
        route.steps.iter().any(|s| s.zone.as_deref() == Some("Durotar")),
        "starts in the Valley of Trials"
    );
}

/// A dungeon forced with its quests (`forced_dungeon_quests`, GitHub #46) is run with every quest
/// of it the character can take, which are required as class quests are.
#[test]
fn a_dungeon_forced_with_its_quests_takes_them_all() {
    const DEADMINES: i64 = 900;
    let mut scenario = leveling(25)
        .dungeon("The Deadmines", 1, 70.0, 70.0)
        .params(|p| p.forced_dungeon_quests = vec![DEADMINES]);
    for id in 300..304 {
        scenario = scenario.quest(
            Q::new(id, &format!("Mine quest {id}"))
                .level(18)
                .giver(1, 40.0, 40.0)
                .objective(
                    Obj::kill(
                        "Defias",
                        18,
                        6,
                        At {
                            zone: 1,
                            x: 70.0,
                            y: 70.0,
                        },
                    )
                    .inside(0),
                ),
        );
    }
    let (_, guide) = scenario.guide();
    assert!(
        guide
            .plan
            .iter()
            .any(|s| matches!(s, job::RouteStep::Dungeon { dungeon: DEADMINES }))
    );
    for id in 300..304 {
        assert!(
            guide
                .plan
                .iter()
                .any(|s| matches!(s, job::RouteStep::TurnIn { quest } if *quest == id)),
            "{id} turned in"
        );
    }
}
/// A forced dungeon stays within the levels of its window: the route that cannot run it there is
/// refused (GitHub #84, a forced Ragefire Chasm run at level 47). A dungeon a must-have quest
/// needs is run past them.
#[test]
fn a_forced_dungeon_is_held_to_its_levels() {
    let levels = |change: fn(&mut fg_route::Params)| {
        let p = with_deadmines(leveling(30)).params(change).build();
        (p.model.dungeons[0].min_level, p.model.dungeons[0].max_level)
    };
    let window = |p: &mut fg_route::Params| (p.dungeon_from, p.dungeon_to) = (-1, 3);
    let (min, window_end) = levels(window);
    assert!(window_end < 30, "{window_end}");
    assert_eq!(
        levels(|p| {
            (p.dungeon_from, p.dungeon_to, p.forced_dungeons) = (-1, 3, vec![900]);
        }),
        (min, window_end)
    );
    assert_eq!(
        levels(|p| {
            (p.dungeon_from, p.dungeon_to, p.forced_dungeon_quests) = (-1, 3, vec![900]);
        }),
        (min, window_end)
    );
}

/// A guide that starts past the levels of a forced dungeon cannot run it: the request is refused
/// at once, saying which dungeon.
#[test]
fn a_forced_dungeon_below_the_start_level_is_refused() {
    let scenario = |from: i64, forced: Vec<i64>| {
        with_deadmines(leveling(60))
            .levels(from, 60)
            .params(move |p| p.forced_dungeons = forced)
            .try_build()
    };
    let error = scenario(28, vec![900]).err().expect("refused").to_string();
    assert!(error.contains("The Deadmines") && error.contains("forced"), "{error}");
    assert!(scenario(28, vec![]).is_ok(), "not forced: left aside");
    assert!(scenario(20, vec![900]).is_ok(), "still within its levels");
}

/// A mage learns each city's teleport from that city's trainer (GitHub #75): the guide teleports
/// to a city only after training there.
#[test]
fn a_mage_teleports_only_to_cities_it_trained_in() {
    let p = leveling(26)
        .levels(20, 26)
        .trainer(3, 50.0, 50.0)
        .teleport("Teleport: Crossroads", 20, 3, 50.0, 50.0)
        .build();
    let edit = |edits: &[Edit]| planner(&p).explicit().edit(edits).steps.last().unwrap().outcome;
    let train = Edit::Train { trainer: 1 };
    let cast = Edit::Teleport { to: 0 };
    let up = Edit::GrindTo { level: 21 };
    assert_ne!(edit(&[up, cast]), Outcome::Done, "teleport before training in its city");
    assert_ne!(
        edit(&[up, Edit::Train { trainer: 0 }, cast]),
        Outcome::Done,
        "trained in another city"
    );
    assert_eq!(edit(&[up, train, cast]), Outcome::Done, "trained there");
}
/// A step that travels (hearthstone, flight, boat, teleport) holds the time of its trip, which is
/// A step that travels (hearthstone, flight, boat, teleport) holds the time of its trip, which is
/// not counted in the step after it (GitHub #68: the hearthstone showed no time at all).
#[test]
fn travel_steps_hold_their_own_time() {
    use fg_route::export::StepKind;
    use job::RouteStep;
    let (editor, _) = leveling(24).inn("Barrens Inn", 3, 54.0, 54.0).guide();
    let draft = [
        RouteStep::FlightMaster { node: 25 },
        RouteStep::Bind { inn: 600_001 },
        RouteStep::Fly { from: 25, to: 23 },
        RouteStep::Hearth { recall: false },
    ];
    let route = editor.save(&draft, vec![]).unwrap().route;
    let cast = fg_route::params::Params::default().hearth_cast;
    let (mut hearths, mut flights) = (0, 0);
    for (k, step) in route.steps.iter().enumerate().skip(1) {
        let took = step.time - route.steps[k - 1].time;
        match step.kind {
            StepKind::Hearth => {
                hearths += 1;
                assert!(took >= cast - 0.5, "the hearthstone at step {k} took {took} s");
            }
            StepKind::Fly => {
                flights += 1;
                assert!(took >= 20.0, "the flight at step {k} took {took} s");
            }
            _ => {}
        }
    }
    assert!(hearths > 0 && flights > 0, "{hearths} hearthstones, {flights} flights");
}
/// Death skips (`death_skips`, GitHub #54): off unless asked; on, the guide dies to respawn at the
/// nearest spirit healer when that is faster, never once a level and a half past 10 (no
/// resurrection sickness before 11), and the guide editor replays them as they were.
#[test]
fn death_skips_below_level_ten_and_a_half() {
    let scenario = |on: bool| {
        leveling(13)
            .graveyard(3, 50.0, 50.0)
            .params(move |p| (p.death_skips, p.effort) = (on, 0.0))
    };
    let deaths = |o: &job::PlanOutcome| {
        (o.route.steps.iter())
            .filter(|s| s.kind == fg_route::export::StepKind::DeathSkip)
            .count()
    };
    let interrupt = job::Interrupt::default();
    assert_eq!(deaths(&scenario(false).job(&|_| {}, &interrupt, None).unwrap()), 0);
    let (editor, outcome) = scenario(true).guide();
    assert!(deaths(&outcome) > 0);
    for (k, s) in outcome.route.steps.iter().enumerate() {
        if s.kind == fg_route::export::StepKind::DeathSkip {
            // The step before says where the character was: below 10 and a half.
            let before = outcome.route.steps[..k]
                .iter()
                .rev()
                .find(|x| x.level > 0)
                .map_or(1, |x| x.level);
            assert!(before <= 10, "a death skip at level {before}");
        }
    }
    let skips = outcome
        .plan
        .iter()
        .filter(|s| matches!(s, job::RouteStep::DeathSkip))
        .count();
    assert_eq!(skips, deaths(&outcome), "the plan has them as steps");
    let report = editor.simulate(&outcome.plan, &fg_route::model::Names::default());
    assert_eq!(
        report.errors,
        0,
        "{:?}",
        report.steps.iter().find(|s| s.status == "error").map(|s| s.reason)
    );
    assert!(
        (report.total_time - outcome.route.total_time).abs() < 60.0,
        "{} / {}",
        report.total_time,
        outcome.route.total_time
    );
}
