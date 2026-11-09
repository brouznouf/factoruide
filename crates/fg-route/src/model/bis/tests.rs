use crate::{
    fixture::Fixture,
    job::{self, Overrides},
    xp::Edition,
};
use std::fmt::Write;

/// Runtime fixtures use the real residual selector, without requiring item-stat fixtures
/// for every independent scheduling case. Production has no test-only construction API.
pub(crate) fn select_test_sources(p: &mut job::Prepared, roots: &[u32], starts: &[&job::StartState]) {
    let mut selector = super::Selector::new(
        &p.model.quests,
        &p.model.dungeons,
        &p.model.skipped,
        &p.profile,
        &p.params,
        &p.model.known_class_prerequisites,
        starts,
    );
    let mut bis = super::Bis {
        requires: vec![false; p.model.quests.len()],
        ..super::Bis::default()
    };
    for &root in roots {
        let (plan, closure) = selector.select(root).unwrap();
        selector.committed = closure;
        bis.required_roots.push(root);
        bis.required_plans.push(plan);
    }
    let graph = selector.finalize().unwrap();
    bis.source_actions = graph.actions;
    bis.source_edges = graph.edges;
    bis.source_quests = graph.quests;
    bis.source_visits = graph.visits;
    for &a in &bis.source_actions {
        if let super::BisQuestAction::TurnIn(i) = a {
            bis.requires[i as usize] = true;
        }
    }
    for (i, q) in p.model.quests.iter_mut().enumerate() {
        q.mandatory |= bis.requires(i);
        if bis.requires(i) {
            q.mandatory_origins.insert(super::super::MandatoryOrigins::GEAR);
        }
    }
    p.model.bis = bis;
}

fn prepared(completed: bool) -> job::Prepared {
    prepared_with(completed, "required", "")
}

fn prepared_with(completed: bool, mode: &str, sql: &str) -> job::Prepared {
    let mut text = String::from(
        r#"
[[npc]]
id = 990001
name = "Gear giver"
friendly_to = "AH"
at = [{ zone = 12, x = 48.0, y = 42.0 }]
[[quest]]
id = 990003
name = "Unsupported crown"
pre_all = [999999]
level = 1
min_level = 1
giver = 990001
rewards = [{ item = 990010 }]
[[quest]]
id = 990004
name = "Supported cap"
level = 1
min_level = 1
giver = 990001
rewards = [{ item = 990011 }]
"#,
    );
    for (id, strength) in [(990010, 10), (990011, 1)] {
        write!(text, r#"
[[item]]
id = {id}
name = "Cap {id}"
level = 1
class = 4
[[row]]
table = "m_item_stats"
values = {{ id = {id}, quality = 1, inventory_type = 1, class = 4, subclass = 1, allowable_class = 0, required_level = 1, str = {strength}.0, agi = 0.0, sta = 0.0, int = 0.0, spi = 0.0, armor = 0.0, ap = 0.0, rap = 0.0, sp = 0.0, heal = 0.0, crit = 0.0, spell_crit = 0.0, hit = 0.0, mp5 = 0.0, dps = 0.0, speed = 0.0 }}
"#).unwrap();
    }
    let conn = Fixture::new(Edition::Classic).toml(&text).open();
    conn.execute_batch(sql).unwrap();
    let overrides = Overrides::load_edition(
        &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../overrides"),
        Edition::Classic,
    )
    .unwrap();
    let request = serde_json::from_value(serde_json::json!({
        "race": "human", "class": "warrior", "from_level": 1, "to_level": 5,
        "required_class_quests": [],
        "start": {"level":1, "completed": if completed {vec![999999]} else {vec![]}},
        "params": {"edition":"classic", "bis":mode}
    }))
    .unwrap();
    job::prepare(&conn, &overrides, &request, &|_| {}).unwrap()
}

#[test]
fn alternate_giver_is_retained_and_modes_keep_their_contract() {
    let sql = "INSERT INTO m_quest_reward (quest_id,item_id,count,choice) VALUES (990004,990010,1,0);";
    let p = prepared_with(false, "required", sql);
    let i = p.model.index[&990004];
    assert_eq!(
        p.model.power.as_ref().unwrap().items[p.model.bis.entries[0].item as usize].id,
        990010
    );
    assert_eq!(p.model.bis.required_roots(), &[i as u32]);
    assert!(p.model.bis.requires(i) && p.model.quests[i].mandatory);
    assert!(!p.model.quests[i].required, "gear is not an exact quest request");
    assert_eq!(p.model.bis.fallback_notes().len(), 1);
    let p = prepared_with(false, "prefer", sql);
    assert!(p.model.bis.required_roots().is_empty() && p.model.bis.required_plans().is_empty());
    assert!(!p.model.quests[p.model.index[&990004]].mandatory);
    let p = prepared_with(false, "off", sql);
    assert!(p.model.bis.entries.is_empty() && p.model.bis.fallback_notes().is_empty());
}

#[test]
fn gear_only_class_category_uses_completion_membership_not_category_for_origin() {
    use crate::model::MandatoryOrigins as Origins;
    let sql = "UPDATE m_quest SET zone_or_sort=-81, classes_mask=1 WHERE id=990004;";
    for mode in ["required", "prefer", "off"] {
        let p = prepared_with(false, mode, sql);
        let q = &p.model.quests[p.model.index[&990004]];
        assert!(!q.mandatory_origins.contains(Origins::CLASS));
        assert_eq!(q.mandatory_origins.contains(Origins::GEAR), mode == "required");
    }
}

#[test]
fn no_supported_alternative_is_explained() {
    let p = prepared_with(
        false,
        "required",
        "INSERT INTO m_quest_link (quest_id,kind,other_id,sources) VALUES (990004,'pre_all',999998,'test');",
    );
    assert!(p.model.bis.entries.is_empty());
    assert_eq!(p.model.bis.required_roots(), &[] as &[u32]);
    assert_eq!(p.model.bis.fallback_notes().len(), 2);
    assert!(
        p.model
            .bis
            .fallback_notes()
            .iter()
            .all(|n| n.contains("no supported alternative"))
    );
}

#[test]
fn final_band_keeps_pre_target_source_and_rejects_target_floor() {
    let p = prepared_with(false, "required", "UPDATE m_quest SET min_level=4 WHERE id=990004;");
    assert_eq!(p.model.bis.entries[0].level, 5);
    let p = prepared_with(false, "required", "UPDATE m_quest SET min_level=5 WHERE id=990004;");
    assert!(p.model.bis.entries.is_empty());
    assert!(p.model.bis.fallback_notes().iter().any(|n| n.contains("level 5")));
}

#[test]
fn missing_predecessor_falls_back_but_recorded_completion_restores_best() {
    let p = prepared(false);
    let chosen = &p.model.bis.entries[0];
    assert_eq!(p.model.power.as_ref().unwrap().items[chosen.item as usize].id, 990011);
    assert!(
        p.notes
            .iter()
            .any(|n| n.contains("999999") && n.contains("Supported cap")),
        "{:?}",
        p.notes
    );
    let p = prepared(true);
    assert_eq!(
        p.model.power.as_ref().unwrap().items[p.model.bis.entries[0].item as usize].id,
        990010
    );
}

#[test]
fn round2_higher_reward_reentry_chain_falls_back_before_ranking() {
    use crate::testing::{Q, Scenario};
    let mut p = prepared(false);
    let inside = Scenario::new()
        .dungeon("Scholomance", 1, 50.0, 50.0)
        .quest(Q::new(5382, "Dungeon objective").kill("Boss", 1, 1, 1, 50.0, 60.0))
        .build();
    let mut objective = inside.model.quests[0].objectives[0].clone();
    objective.loc.inside = Some(0);
    p.model.dungeons = inside.model.dungeons;
    p.model.dungeons[0].min_level = 1;
    p.model.dungeons[0].max_level = 60;
    let high = p.model.index[&990003];
    p.model.quests[high].id = 5384;
    p.model.quests[high].name = "Kirtonos the Herald".into();
    p.model.quests[high].pre_all = vec![5515];
    p.model.quests[high].objectives = vec![objective];
    for (id, name, previous) in [
        (5382, "Doctor Theolen Krastinov, the Butcher", vec![]),
        (5515, "Krastinov's Bag of Horrors", vec![5382]),
    ] {
        let mut q = p.model.quests[high].clone();
        q.id = id;
        q.name = name.into();
        q.pre_all = previous;
        p.model.quests.push(q);
        p.model.rewards.push((vec![], vec![]));
    }
    for q in p.model.quests.iter_mut().filter(|q| [5382, 5515, 5384].contains(&q.id)) {
        for loc in q.starts.iter_mut().chain(&mut q.ends) {
            loc.id = 11216;
            loc.name = "Eva Sarkhoff".into();
            loc.inside = None;
        }
    }
    let conn = Fixture::new(Edition::Classic).open();
    let empty = std::collections::HashSet::new();
    let mut sources =
        super::super::sources::Sources::read(&conn, &p.world, &p.profile, &p.params, &empty, &empty, &empty).unwrap();
    sources.dungeons = p.model.dungeons.clone();
    let rank = |starts: &[&job::StartState]| {
        super::load(
            &conn,
            p.model.power.as_ref().unwrap(),
            &p.model.quests,
            &p.model.rewards,
            &p.profile,
            &p.world,
            &sources,
            &p.params,
            &[],
            &empty,
            starts,
        )
        .unwrap()
    };
    let bis = rank(&[]);
    assert_eq!(
        p.model.power.as_ref().unwrap().items[bis.entries[0].item as usize].id,
        990011
    );
    assert!(
        bis.fallback_notes()
            .iter()
            .any(|n| n.contains("Kirtonos") && n.contains("one continuous visit") && n.contains("Supported cap"))
    );
    let start = job::StartState {
        completed: vec![5382.into(), 5515.into()],
        ..Default::default()
    };
    let bis = rank(&[&start]);
    assert_eq!(
        p.model.power.as_ref().unwrap().items[bis.entries[0].item as usize].id,
        990010
    );
}
