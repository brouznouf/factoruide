//! Profession trainer visits (`Params::profession_trainers`): the planner learns each rank of a
//! profession when the character passes a trainer, or by a trip when the skill needs it.

use super::*;
use crate::model::{ProfessionPlan, ProfessionTrainer};
use crate::plan::Event;

const LINE: i64 = 197;

/// A tailor taken at level 5: 5 skill per level from there, up to 300.
fn tailoring() -> ProfessionPlan {
    ProfessionPlan {
        key: "tailoring".to_owned(),
        name: "Tailoring".to_owned(),
        target: 300,
        start: 5,
        curve: (0..=60).map(|l: i64| (5 * (l - 5)).clamp(0, 300)).collect(),
        members: 1,
    }
}

/// A prepared route with a tailor and one tailoring trainer teaching up to `cap`, `away` yards east
/// of the start.
fn tailor_with_trainer(cap: i64, away: f64) -> Prepared {
    let mut p = quests_at_start(1);
    let mut loc = p.model.start.clone();
    loc.pos.x += away;
    p.model.professions = vec![tailoring()];
    p.model.profession_trainers = vec![ProfessionTrainer {
        line: LINE,
        cap,
        loc,
        book: None,
    }];
    p.params.profession_trainers = true;
    p.profile.to_level = 60;
    p
}

fn visits(planner: &Planner<'_>, s: &mut super::super::state::State) -> Vec<Event> {
    let mut events = Vec::new();
    planner.sim().train_professions(s, &mut |_, e| events.push(e));
    events
}

#[test]
fn a_trainer_close_by_teaches_the_first_rank_once() {
    let p = tailor_with_trainer(75, 50.0);
    let planner = planner(&p);
    let mut s = planner.sim().initial_state();
    s.level = 10;
    let before = s.time;
    let events = visits(&planner, &mut s);
    assert!(
        matches!(
            events[..],
            [Event::ProfTrain {
                profession: 0,
                trainer: 0,
                ranks: 0b1,
                ..
            }]
        ),
        "{events:?}"
    );
    assert!(s.time >= before + p.params.train_time, "the lesson takes time");
    assert_eq!(visits(&planner, &mut s).len(), 0, "learned once");
}

#[test]
fn a_far_trainer_waits_until_the_skill_needs_the_rank_then_gets_a_trip() {
    let p = tailor_with_trainer(75, 1500.0);
    let planner = planner(&p);
    let mut s = planner.sim().initial_state();
    // Skill 35: the apprentice rank is open and far, and not needed yet.
    s.level = 12;
    assert_eq!(visits(&planner, &mut s).len(), 0, "no trip before it is needed");
    // Skill 55: past what the apprentice rank allows (49), so a trip.
    s.level = 16;
    let before = s.time;
    let events = visits(&planner, &mut s);
    assert!(
        matches!(events[..], [Event::ProfTrain { ranks: 0b1, .. }]),
        "{events:?}"
    );
    assert!(s.time > before + p.params.train_time, "the trip takes time");
    assert!(
        s.pos.dist(&p.model.profession_trainers[0].loc.pos) < 1.0,
        "the character is at the trainer"
    );
}

#[test]
fn a_far_trainer_teaches_all_open_ranks_in_one_visit() {
    let p = tailor_with_trainer(150, 1500.0);
    let planner = planner(&p);
    let mut s = planner.sim().initial_state();
    // Skill 55: apprentice is overdue, journeyman is open but not needed yet.
    s.level = 16;
    assert_eq!(s.ranks[0], 0);
    let training = s.spent.training;
    let events = visits(&planner, &mut s);
    assert!(
        matches!(events[..], [Event::ProfTrain { ranks: 0b11, .. }]),
        "one visit learns both available ranks: {events:?}"
    );
    assert_eq!(s.ranks[0], 0b11);
    assert_eq!(s.spent.training - training, p.params.train_time);
    assert!(visits(&planner, &mut s).is_empty());
}

#[test]
fn ordinary_rank_visits_stop_at_the_requested_skill_cap() {
    for (level, cap) in [(30, 150), (45, 225), (15, 75)] {
        for away in [50.0, 1500.0] {
            let mut p = tailor_with_trainer(300, away);
            p.profile.from_level = level;
            p.model.professions[0].target = cap;
            for skill in &mut p.model.professions[0].curve {
                *skill = (*skill).min(cap);
            }
            p.model.initial.profession_caps = vec![(LINE, cap)];
            let planner = planner(&p);
            let mut s = planner.sim().initial_state();
            let (time, pos, ranks) = (s.time, s.pos, s.ranks);
            // At skills 125, 200 and 50 respectively, the next rank can be learned, but
            // the requested goal already fits within the character's learned cap.
            assert!(
                visits(&planner, &mut s).is_empty(),
                "goal {cap}, trainer {away} yards away"
            );
            assert_eq!(s.time, time, "no unnecessary training or travel time");
            assert_eq!(s.pos.dist(&pos), 0.0);
            assert_eq!(s.ranks, ranks);
        }
    }
}

#[test]
fn even_a_one_point_goal_can_learn_apprentice_nearby() {
    let mut p = tailor_with_trainer(300, 50.0);
    p.model.professions[0].target = 1;
    for skill in &mut p.model.professions[0].curve {
        *skill = (*skill).min(1);
    }
    let planner = planner(&p);
    let mut s = planner.sim().initial_state();
    s.level = 10;
    assert!(matches!(
        visits(&planner, &mut s)[..],
        [Event::ProfTrain { ranks: 0b1, .. }]
    ));
}

#[test]
fn a_goal_above_the_learned_cap_can_upgrade_early_at_a_nearby_trainer() {
    let mut p = tailor_with_trainer(300, 50.0);
    p.profile.from_level = 30;
    p.model.professions[0].target = 151;
    for skill in &mut p.model.professions[0].curve {
        *skill = (*skill).min(151);
    }
    p.model.initial.profession_caps = vec![(LINE, 150)];
    let planner = planner(&p);
    let mut s = planner.sim().initial_state();
    assert!(matches!(
        visits(&planner, &mut s)[..],
        [Event::ProfTrain { ranks: 0b100, .. }]
    ));
}

#[test]
fn a_trainer_that_does_not_teach_the_rank_is_not_visited() {
    // Teaches up to the first rank only; at skill 130 the character needs the third.
    let p = tailor_with_trainer(75, 50.0);
    let planner = planner(&p);
    let mut s = planner.sim().initial_state();
    s.level = 5 + 26;
    let events = visits(&planner, &mut s);
    assert!(
        matches!(events[..], [Event::ProfTrain { ranks: 0b1, .. }]),
        "only the rank the trainer teaches: {events:?}"
    );
    assert_eq!(visits(&planner, &mut s).len(), 0, "no trip for ranks nobody teaches");
}

#[test]
fn nothing_happens_when_the_option_is_off() {
    let mut p = tailor_with_trainer(75, 50.0);
    p.model.profession_trainers.clear();
    let planner = planner(&p);
    let mut s = planner.sim().initial_state();
    s.level = 20;
    let before = s.time;
    assert_eq!(visits(&planner, &mut s).len(), 0);
    assert_eq!(s.time, before);
}

/// A character met in game at level 25 with its tailoring at 100 knows the first two ranks: the
/// trainers are not visited for them again; a new character, at level 1, knows none.
#[test]
fn a_character_met_in_game_knows_the_ranks_its_skill_needs() {
    let mut p = tailor_with_trainer(150, 50.0);
    p.profile.from_level = 25;
    let planner = planner(&p);
    let mut s = planner.sim().initial_state();
    assert_eq!(s.ranks[0], 0b11, "skill 100: apprentice and journeyman");
    assert_eq!(visits(&planner, &mut s).len(), 0, "nothing to learn again");
    p.profile.from_level = 1;
    assert_eq!(
        super::planner(&p).sim().initial_state().ranks[0],
        0,
        "a new character knows none"
    );
}

/// The nearby seller offers Expert Cooking at skill 125, including characters below level 20.
fn secondary_with_book(key: &str, skill: i64) -> Prepared {
    let mut p = quests_at_start(1);
    let def = crate::profession::find(key).unwrap();
    let book = *crate::profession::EXPERT_BOOKS
        .iter()
        .find(|book| book.line == def.skill_id)
        .unwrap();
    let mut loc = p.model.start.clone();
    loc.pos.x += 50.0;
    p.model.professions = vec![ProfessionPlan {
        key: key.into(),
        name: def.name.into(),
        target: 225,
        start: 5,
        curve: vec![skill; 61],
        members: 1,
    }];
    p.model.profession_trainers = vec![ProfessionTrainer {
        line: book.line,
        cap: book.cap,
        loc,
        book: Some(book),
    }];
    p.params.profession_trainers = true;
    p.profile.from_level = 15;
    p.profile.to_level = 60;
    p
}

#[test]
fn expert_cooking_book_needs_skill_125_but_not_level_twenty() {
    let below = secondary_with_book("cooking", 124);
    let below_planner = planner(&below);
    let mut below_state = below_planner.sim().initial_state();
    assert!(visits(&below_planner, &mut below_state).is_empty());

    let p = secondary_with_book("cooking", 125);
    let planner = planner(&p);
    let mut s = planner.sim().initial_state();
    assert_eq!(s.level, 15);
    assert_eq!(s.ranks[0], 0b11);
    let events = visits(&planner, &mut s);
    assert!(
        matches!(events[..], [Event::ProfTrain { ranks: 0b100, .. }]),
        "{events:?}"
    );
    assert_eq!(s.ranks[0], 0b111);
    assert!(visits(&planner, &mut s).is_empty(), "the book is read only once");
}

#[test]
fn a_far_cookbook_seller_is_visited_once_when_the_curve_passes_150() {
    let mut p = secondary_with_book("cooking", 150);
    p.model.profession_trainers[0].loc.pos.x = p.model.start.pos.x + 1500.0;
    p.model.professions[0].curve[16] = 151;
    let planner = planner(&p);
    let mut s = planner.sim().initial_state();
    let before = s.time;
    let travel = s.spent.travel;
    let training = s.spent.training;
    assert_eq!(s.ranks[0], 0b11);
    assert!(visits(&planner, &mut s).is_empty(), "no distant trip at skill 150");
    assert_eq!(s.time, before);
    assert_eq!(s.spent.travel, travel);

    s.level = 16;
    let events = visits(&planner, &mut s);
    assert!(
        matches!(events[..], [Event::ProfTrain { ranks: 0b100, .. }]),
        "one Expert purchase after the curve passes 150: {events:?}"
    );
    assert_eq!(s.ranks[0], 0b111);
    assert!(s.spent.travel > travel, "the vendor trip is counted");
    assert_eq!(s.spent.training - training, p.params.train_time);
    assert!(s.pos.dist(&p.model.profession_trainers[0].loc.pos) < 1.0);
    let after = s.time;
    assert!(visits(&planner, &mut s).is_empty(), "the book is not bought again");
    assert_eq!(s.time, after);
}

#[test]
fn the_expert_cooking_book_cannot_replace_learning_the_lower_ranks() {
    let p = secondary_with_book("cooking", 125);
    let planner = planner(&p);
    for known in [0, 0b1] {
        let mut s = planner.sim().initial_state();
        s.ranks[0] = known;
        assert!(visits(&planner, &mut s).is_empty());
        assert_eq!(s.ranks[0], known, "the book teaches neither apprentice nor journeyman");
    }
}

#[test]
fn a_reported_expert_cap_avoids_buying_the_book_at_skill_125() {
    for (cap, learns) in [(Some(225), false), (Some(150), true), (None, true)] {
        let mut p = secondary_with_book("cooking", 125);
        p.model.initial.profession_caps = cap.into_iter().map(|cap| (185, cap)).collect();
        let planner = planner(&p);
        let mut s = planner.sim().initial_state();
        assert_eq!(s.ranks[0], if learns { 0b11 } else { 0b111 });
        assert_eq!(!visits(&planner, &mut s).is_empty(), learns);
    }
}

#[test]
fn cooking_goals_that_stop_at_150_do_not_buy_the_book() {
    for target in [125, 150] {
        let mut p = secondary_with_book("cooking", 125);
        p.model.professions[0].target = target;
        let planner = planner(&p);
        let mut state = planner.sim().initial_state();
        assert!(visits(&planner, &mut state).is_empty());
        assert_eq!(state.ranks[0], 0b11);
    }
}

#[test]
fn fishing_expert_book_observes_the_editions_character_level_requirement() {
    for edition in Edition::ALL {
        let mut p = secondary_with_book("fishing", 125);
        p.params.edition = edition;
        p.profile.from_level = 19;
        let planner = planner(&p);
        let mut state = planner.sim().initial_state();
        let before = state.time;
        let events = visits(&planner, &mut state);
        if edition == Edition::Tbc {
            assert!(matches!(events[..], [Event::ProfTrain { ranks: 0b100, .. }]));
        } else {
            assert!(
                events.is_empty(),
                "Expert Fishing needs character level 20 in {edition:?}"
            );
            assert_eq!(state.time, before, "no trip for a book the character cannot read");
            state.level = 20;
            assert!(matches!(
                visits(&planner, &mut state)[..],
                [Event::ProfTrain { ranks: 0b100, .. }]
            ));
        }
    }
}

#[test]
fn first_aid_expert_book_needs_skill_125_without_a_character_level_twenty_gate() {
    for skill in [124, 125] {
        let p = secondary_with_book("first_aid", skill);
        let planner = planner(&p);
        let mut state = planner.sim().initial_state();
        assert_eq!(state.level, 15);
        let events = visits(&planner, &mut state);
        if skill == 124 {
            assert!(events.is_empty());
        } else {
            assert!(matches!(events[..], [Event::ProfTrain { ranks: 0b100, .. }]));
        }
    }
}

#[test]
fn secondary_books_respect_reported_caps_and_goals_ending_at_150() {
    for key in ["cooking", "fishing", "first_aid"] {
        for cap in [150, 225] {
            for target in [150, 225] {
                let mut p = secondary_with_book(key, 125);
                p.profile.from_level = 20;
                p.model.professions[0].target = target;
                p.model.initial.profession_caps = vec![(crate::profession::skill_line(key), cap)];
                let planner = planner(&p);
                let mut state = planner.sim().initial_state();
                assert_eq!(
                    !visits(&planner, &mut state).is_empty(),
                    cap == 150 && target > 150,
                    "{key}, learned cap {cap}, target {target}"
                );
            }
        }
    }
}
