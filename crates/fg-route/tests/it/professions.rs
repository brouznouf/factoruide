//! Secondary Expert ranks come from books, not ordinary trainers.

use crate::common::repo;
use fg_route::export::{self, Arg, Step, StepKind};
use fg_route::fixture::Fixture;
use fg_route::job::{self, Overrides, PlanRequest};
use fg_route::plan::{Breakdown, Event, Planner, Timed};
use fg_route::xp::Edition;
use serde_json::json;

#[test]
fn secondary_expert_books_select_the_factions_seller_and_export_the_item() {
    for edition in Edition::ALL {
        let mut fixture = Fixture::new(edition).toml(
            r#"
            [[npc]]
            id = 3028
            name = "Kah Mistrunner"
            subname = "Fishing Trainer"
            npc_flags = 16
            friendly_to = "H"
            at = [{ zone = 1638, x = 56.13, y = 46.38 }]
        "#,
        );
        for (id, name, faction, zone) in [
            (12033, "Wulan", "H", 405),
            (3955, "Shandrina", "A", 331),
            (2626, "Old Man Heming", "AH", 33),
            (13476, "Balai Lok'Wein", "H", 15),
            (2805, "Deneb Walker", "A", 45),
        ] {
            fixture = fixture.toml(&format!(
                r#"
                [[npc]]
                id = {id}
                name = "{name}"
                friendly_to = "{faction}"
                at = [{{ zone = {zone}, x = 50.0, y = 50.0 }}]
            "#
            ));
        }
        let conn = fixture.open();
        let overrides = Overrides::load_edition(&repo().join("overrides"), edition).unwrap();
        for (race, sellers) in [("orc", [12033, 2626, 13476]), ("human", [3955, 2626, 2805])] {
            let request: PlanRequest = serde_json::from_value(json!({
                "race":race, "class":"warrior", "from_level":20, "to_level":30,
                "params":{"edition":edition,"profession_trainers":true,"effort":0},
                "professions":[{"key":"cooking","target":225},{"key":"fishing","target":225},{"key":"first_aid","target":225}]
            })).unwrap();
            let prepared = job::prepare(&conn, &overrides, &request, &|_| {}).unwrap();
            assert_eq!(prepared.params.edition, edition);
            let sources = &prepared.model.profession_trainers;
            let kah = sources.iter().find(|s| s.loc.id == 3028);
            assert_eq!(kah.map(|s| s.cap), (race == "orc").then_some(150));
            let planner = Planner::new(&prepared.model, &prepared.world, &prepared.params, &prepared.profile);
            for (profession, (line, item, book_name)) in [
                (185, 16072, "Expert Cookbook"),
                (356, 16083, "Expert Fishing - The Bass and You"),
                (129, 16084, "Expert First Aid - Under Wraps"),
            ]
            .into_iter()
            .enumerate()
            {
                let books: Vec<_> = sources
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.line == line && s.book.is_some())
                    .collect();
                assert_eq!(books.len(), 1, "one seller for {race}, line {line}, {edition:?}");
                let (trainer, source) = books[0];
                assert_eq!((source.loc.id, source.cap), (sellers[profession], 225));
                assert_eq!(source.book.unwrap().item, item);
                let trace = [Timed {
                    event: Event::ProfTrain {
                        profession,
                        trainer,
                        ranks: 0b100,
                        outbound: 0.0,
                    },
                    time: 30.0,
                    level: 20,
                    xp: 0,
                    power: 20.0,
                    gear: 0.0,
                    party: vec![],
                }];
                let route = export::build_traced(&planner, &prepared.profile, &[], &trace, 30.0, Breakdown::default());
                let step = route.steps.iter().find(|s| s.kind == StepKind::Profession).unwrap();
                assert_eq!((step.line, step.skill), (Some(line), Some(225)));
                assert_eq!(step.target.as_ref().unwrap().id, sellers[profession]);
                assert_eq!(step.say[0].args["item"], Arg::Item(item, book_name.to_owned()));
                assert!(step.text.starts_with(&format!("Buy {book_name} from ")));
                let saved = serde_json::to_value(&route).unwrap();
                let loaded: fg_route::Route = serde_json::from_value(saved).unwrap();
                let lua = export::to_lua(&[loaded]);
                assert!(lua.contains(book_name) && lua.contains(&format!("line = {line}")) && lua.contains("sk = 225"));
            }
        }
    }
}

#[test]
fn a_traced_training_return_flight_exports_its_departure_and_skip_boundary() {
    let conn = Fixture::new(Edition::Forever)
        .toml(
            r#"
        [[npc]]
        id = 3028
        name = "Kah Mistrunner"
        subname = "Fishing Trainer"
        npc_flags = 16
        friendly_to = "H"
        at = [{ zone = 1638, x = 56.13, y = 46.38 }]
    "#,
        )
        .open();
    let overrides = Overrides::load_edition(&repo().join("overrides"), Edition::Forever).unwrap();
    let request: PlanRequest = serde_json::from_value(json!({
        "race":"orc", "class":"warrior", "from_level":27, "to_level":28,
        "params":{"edition":"forever", "profession_trainers":true, "effort":0},
        "professions":[{"key":"fishing", "target":150}],
        "start":{"race":2, "class":"WARRIOR", "level":27,
                 "professions":[{"line":356, "rank":50, "cap":75}]}
    }))
    .unwrap();
    let prepared = job::prepare(&conn, &overrides, &request, &|_| {}).unwrap();
    let nodes = &prepared.world.taxi_nodes;
    let sun_rock = nodes.iter().position(|n| n.id == 29).unwrap();
    let thunder_bluff = nodes.iter().position(|n| n.id == 22).unwrap();
    let trainer = prepared
        .model
        .profession_trainers
        .iter()
        .position(|t| t.loc.id == 3028)
        .unwrap();
    assert_eq!(prepared.model.profession_trainers[trainer].cap, 150);
    let trace: Vec<_> = [
        Event::Fly {
            from: sun_rock,
            to: thunder_bluff,
        },
        Event::ProfTrain {
            profession: 0,
            trainer,
            outbound: 0.0,
            ranks: 0b10,
        },
        Event::Fly {
            from: thunder_bluff,
            to: sun_rock,
        },
    ]
    .into_iter()
    .enumerate()
    .map(|(i, event)| Timed {
        event,
        time: (i + 1) as f64 * 60.0,
        level: 27,
        xp: 0,
        power: 27.0,
        gear: 0.0,
        party: vec![],
    })
    .collect();
    let planner = Planner::new(&prepared.model, &prepared.world, &prepared.params, &prepared.profile);
    let mut route = export::build_traced(&planner, &prepared.profile, &[], &trace, 180.0, Breakdown::default());
    assert_eq!(route.steps.len(), 3);
    assert_eq!(route.steps[0].from_taxi, Some(29));
    assert_eq!(route.steps[0].target.as_ref().unwrap().id, 22);
    assert_eq!(route.steps[1].kind, StepKind::Profession);
    assert_eq!((route.steps[1].line, route.steps[1].skill), (Some(356), Some(150)));
    assert_eq!(route.steps[2].from_taxi, Some(22));
    assert_eq!(route.steps[2].target.as_ref().unwrap().id, 29);
    // An edit before the trip shifts its indexes; the departure survives saving.
    route.steps.insert(0, Step::bare(StepKind::Accept, 27, 0.0));
    let saved = serde_json::to_value(&route).unwrap();
    assert_eq!(saved["steps"][1]["from_taxi"], 29);
    let loaded: fg_route::Route = serde_json::from_value(saved).unwrap();
    let lua = export::to_lua(&[loaded]);
    assert!(lua.contains("pte = 4"));
    assert_eq!(
        lua.matches("pte = ").count(),
        1,
        "only the outbound flight can bypass the excursion"
    );
    // Travel with another purpose must stay in the guide after an edit.
    route.steps.insert(2, Step::bare(StepKind::TurnIn, 27, 0.0));
    assert!(!export::to_lua(&[route]).contains("pte = "));
}
