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
    p.model.profession_trainers = vec![ProfessionTrainer { line: LINE, cap, loc }];
    p.params.profession_trainers = true;
    p.profile.to_level = 60;
    p
}

fn visits(planner: &Planner<'_>, s: &mut super::super::state::State) -> Vec<Event> {
    let mut events = Vec::new();
    planner.train_professions(s, &mut |_, e| events.push(e));
    events
}

#[test]
fn a_trainer_close_by_teaches_the_first_rank_once() {
    let p = tailor_with_trainer(75, 50.0);
    let planner = planner(&p);
    let mut s = planner.initial_state();
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
    let mut s = planner.initial_state();
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
fn a_trainer_that_does_not_teach_the_rank_is_not_visited() {
    // Teaches up to the first rank only; at skill 130 the character needs the third.
    let p = tailor_with_trainer(75, 50.0);
    let planner = planner(&p);
    let mut s = planner.initial_state();
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
    let mut s = planner.initial_state();
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
    let mut s = planner.initial_state();
    assert_eq!(s.ranks[0], 0b11, "skill 100: apprentice and journeyman");
    assert_eq!(visits(&planner, &mut s).len(), 0, "nothing to learn again");
    p.profile.from_level = 1;
    assert_eq!(
        super::planner(&p).initial_state().ranks[0],
        0,
        "a new character knows none"
    );
}
