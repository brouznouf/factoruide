//! A new character's first spell: the guide sells the starting gear at the vendor, then trains at
//! level 1 where a guide of the game's own players is known to (overrides/forever/races.toml).

use crate::common::{database_of, repo};
use fg_route::export::{Route, Step, StepKind};
use fg_route::job::{self, Overrides, PlanRequest};
use fg_route::plan::{Event, Kind, Planner, Stop};
use fg_route::xp::Edition;

fn request(race: &str, class: &str) -> PlanRequest {
    serde_json::from_value(serde_json::json!({
        "race": race, "class": class, "from_level": 1, "to_level": 3,
        "required_class_quests": []
    }))
    .unwrap()
}

/// The first levels of a new character's guide on the Forever data (the first routes only).
fn guide(race: &str, class: &str) -> Route {
    let conn = database_of("forever");
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let mut req = request(race, class);
    let mut params = job::prepare(&conn, &overrides, &req, &|_| {}).unwrap().params;
    params.effort = 0.0;
    params.threads = 1;
    req.params = Some(params);
    job::plan(&conn, &overrides, &req, &|_| {}).unwrap().route
}

fn at_level_1(guide: &Route, kind: StepKind) -> Vec<(usize, &Step)> {
    (guide.steps.iter().enumerate())
        .filter(|(_, s)| s.kind == kind && s.level == 1)
        .collect()
}

#[test]
fn a_new_character_sells_its_starting_gear_then_trains_its_level_1_spell() {
    // Race, class, the spell and its ID (Battle Shout 6673, Rockbiter Weapon 8017, Immolate 348,
    // Arcane Intellect 1459, at rank 1).
    for (race, class, spell, id) in [
        ("orc", "warrior", "Battle Shout", 6673),
        ("human", "warrior", "Battle Shout", 6673),
        ("tauren", "shaman", "Rockbiter Weapon", 8017),
        ("undead", "warlock", "Immolate", 348),
        ("undead", "mage", "Arcane Intellect", 1459),
        ("dwarf", "shaman", "Rockbiter Weapon", 8017),
    ] {
        let guide = guide(race, class);
        let sells = at_level_1(&guide, StepKind::Sell);
        let trains = at_level_1(&guide, StepKind::Train);
        assert_eq!(trains.len(), 1, "{race} {class}: one training at level 1");
        assert_eq!(sells.len(), 1, "{race} {class}: one vendor stop at level 1");
        let ((sold, sell), (trained, train)) = (sells[0], trains[0]);
        assert!(sold < trained, "{race} {class}: the vendor comes before the trainer");
        assert!(
            sell.text.contains("starting gear"),
            "{race} {class}: the step names the gear: {}",
            sell.text
        );
        assert!(
            train.text.contains(spell) && train.spells.contains(&id),
            "{race} {class}: trains {spell}: {} {:?}",
            train.text,
            train.spells
        );
        // Nothing the trainer sells for free at level 1 is listed as new.
        assert_eq!(train.spells.len(), 1, "{race} {class}: {:?}", train.spells);
        // The two stops cost real time, and are near the start.
        let (selling, training) = (sell.time, train.time - sell.time);
        assert!(
            selling > 0.0 && training >= 30.0,
            "{race} {class}: {selling} s, {training} s"
        );
        assert!(
            train.time < 600.0,
            "{race} {class}: trained {} s into the guide",
            train.time
        );
        assert!(
            guide.breakdown.training >= 30.0,
            "{race} {class}: the training is in the totals"
        );
    }
}

#[test]
fn starts_no_guide_is_known_to_train_at_stay_as_they_were() {
    for (race, class, why) in [
        (
            "nightelf",
            "warrior",
            "RestedXP tells the Shadowglen warrior to save up",
        ),
        ("human", "mage", "not shown for Northshire"),
        ("orc", "hunter", "no level 1 spell that counts"),
        ("orc", "rogue", "no level 1 spell that counts"),
    ] {
        let guide = guide(race, class);
        assert!(
            at_level_1(&guide, StepKind::Train).is_empty(),
            "{race} {class} ({why}) trains at level 1"
        );
        assert!(
            (guide.steps.iter()).all(|s| !(s.kind == StepKind::Sell && s.text.contains("starting gear"))),
            "{race} {class} ({why}) sells its starting gear"
        );
    }
}

#[test]
fn only_classes_with_a_paid_level_1_spell_have_one_to_train() {
    let conn = database_of("forever");
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let mut with = Vec::new();
    for class in [
        "warrior", "paladin", "hunter", "rogue", "priest", "shaman", "mage", "warlock", "druid",
    ] {
        let race = if class == "paladin" { "human" } else { "orc" };
        let race = if matches!(class, "priest" | "druid") {
            "troll"
        } else {
            race
        };
        let race = if class == "druid" { "tauren" } else { race };
        let p = job::prepare(&conn, &overrides, &request(race, class), &|_| {}).unwrap();
        if p.model
            .power
            .as_ref()
            .is_some_and(fg_route::power::PowerModel::has_level_1_training)
        {
            with.push(class);
        }
    }
    assert_eq!(
        with,
        ["warrior", "paladin", "priest", "shaman", "mage", "warlock", "druid"],
        "hunters and rogues have no level 1 spell to train"
    );
}

#[test]
fn the_races_list_real_classes() {
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let classes = [
        "warrior", "paladin", "hunter", "rogue", "priest", "shaman", "mage", "warlock", "druid",
    ];
    let listed: Vec<(&String, &String)> = (overrides.races.iter())
        .flat_map(|(race, def)| def.start_training.iter().map(move |class| (race, class)))
        .collect();
    assert!(listed.len() >= 15, "{} starts listed", listed.len());
    for (race, class) in listed {
        assert!(classes.contains(&class.as_str()), "{race}: {class} is not a class");
    }
}

/// Without a vendor to sell the starting gear to, the character cannot pay: no training at level 1.
#[test]
fn no_vendor_in_reach_no_training_at_level_1() {
    let conn = database_of("forever");
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let mut p = job::prepare(&conn, &overrides, &request("orc", "warrior"), &|_| {}).unwrap();
    let cutting_teeth = p.model.index[&788] as u32;
    let route = [Stop::quest(cutting_teeth, Kind::Accept)];
    let events = |p: &job::Prepared| {
        let mut trace = Vec::new();
        Planner::new(&p.model, &p.world, &p.params, &p.profile)
            .simulate_full(&route, Some(&mut trace))
            .unwrap();
        trace.into_iter().map(|t| t.event).collect::<Vec<_>>()
    };
    let with = events(&p);
    assert!(with.iter().any(|e| matches!(e, Event::Sell { .. })), "sells first");
    assert!(with.iter().any(|e| matches!(e, Event::Train { .. })), "then trains");
    p.model.vendors.clear();
    let without = events(&p);
    assert!(
        (without.iter()).all(|e| !matches!(e, Event::Sell { .. } | Event::Train { .. })),
        "nothing to sell to: nothing trained"
    );
}
