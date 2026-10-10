//! Where the guide has the character grind: planned grinds and checkpoints are moved back to the
//! last place of the route where mobs of its level live (a city has none), as an XP target
//! there, rather than a trip to grind and back.

use super::types::{Checkpoint, Step, StepKind};
use super::writer::StepWriter;
use crate::world::Pos;

/// A step is a place to grind when this many mobs of the character's level live within
/// `RADIUS` yards of it.
const RADIUS: f64 = 250.0;
const MIN_MOBS: usize = 8;

/// Steps of the guide with the XP the route has after each one (in its level).
pub(super) struct Guide<'s> {
    pub(super) steps: &'s mut Vec<Step>,
    pub(super) xps: &'s mut Vec<i64>,
    pub(super) checkpoints: &'s mut Vec<Checkpoint>,
}

impl Guide<'_> {
    fn insert(&mut self, at: usize, step: Step, xp: i64) {
        self.steps.insert(at, step);
        self.xps.insert(at, xp);
        for c in self.checkpoints.iter_mut().filter(|c| c.step >= at) {
            c.step += 1;
        }
    }

    fn remove(&mut self, at: usize) {
        self.steps.remove(at);
        self.xps.remove(at);
        for c in self.checkpoints.iter_mut().filter(|c| c.step > at) {
            c.step -= 1;
        }
    }

    /// First step after the last grind before `at`: grinds are not moved past another one.
    fn floor(&self, at: usize) -> usize {
        self.steps[..at]
            .iter()
            .rposition(|s| s.kind == StepKind::Grind)
            .map_or(0, |k| k + 1)
    }
}

/// Planned grinds and checkpoints placed where mobs of the character's level live.
pub(super) fn place(writer: &StepWriter<'_>, guide: &mut Guide<'_>) {
    move_grinds(writer, guide);
    for n in 0..guide.checkpoints.len() {
        let (at, level) = (guide.checkpoints[n].step, guide.checkpoints[n].level);
        // Be where the route is at the last place to grind before the level.
        if let Some(k) = spot(writer, guide.steps, guide.floor(at), at, level - 1) {
            let xp = guide.xps[k];
            let step = writer.checkpoint(&guide.steps[k], level, level - 1, xp);
            guide.insert(k + 1, step, xp);
        } else {
            let step = writer.checkpoint(&guide.steps[at - 1], level, level, 0);
            guide.insert(at, step, 0);
            guide.checkpoints[n].step = at;
        }
    }
}

/// Planned grinds where no mob of the character's level lives (a city, a flight path) are
/// done earlier at the last place of the route where some do, to the XP that leaves the route
/// on schedule.
fn move_grinds(writer: &StepWriter<'_>, guide: &mut Guide<'_>) {
    let rules = writer.planner.rules();
    let total = |level: i64, xp: i64| (1..level).map(|l| rules.to_next_level(l)).sum::<i64>() + xp;
    let split = |mut xp: i64| {
        let mut level = 1;
        while level < rules.max_level && xp >= rules.to_next_level(level) {
            xp -= rules.to_next_level(level);
            level += 1;
        }
        (level, xp)
    };
    let mut i = 1;
    while i < guide.steps.len() {
        let (grind, before) = (&guide.steps[i], &guide.steps[i - 1]);
        let level = before.level;
        if grind.kind != StepKind::Grind || grind.xp.is_some() || grindable(writer, grind, level) {
            i += 1;
            continue;
        }
        let Some(k) = spot(writer, guide.steps, guide.floor(i), i, level) else {
            i += 1;
            continue;
        };
        let gained = total(grind.level, guide.xps[i]) - total(level, guide.xps[i - 1]);
        let (to, xp) = split(total(guide.steps[k].level, guide.xps[k]) + gained);
        let minutes = ((grind.time - before.time) / 60.0).ceil();
        let step = writer.grind_before(grind, &guide.steps[k], to, xp, minutes);
        guide.remove(i);
        guide.insert(k + 1, step, xp);
        i += 1;
    }
}

/// Last step of `floor..at` still at `level` where mobs of that level live (not a flight, a
/// boat or the hearthstone: the character is no longer where they start).
fn spot(writer: &StepWriter<'_>, steps: &[Step], floor: usize, at: usize, level: i64) -> Option<usize> {
    (floor..at).rev().take_while(|&k| steps[k].level == level).find(|&k| {
        !matches!(steps[k].kind, StepKind::Fly | StepKind::Link | StepKind::Hearth)
            && grindable(writer, &steps[k], level)
    })
}

/// Whether mobs a character of `level` grinds live around a step.
fn grindable(writer: &StepWriter<'_>, step: &Step, level: i64) -> bool {
    let params = writer.planner.params;
    step.world.is_some_and(|[continent, x, y]| {
        let p = Pos {
            continent: continent as i64,
            x,
            y,
            zone: 0,
        };
        writer
            .planner
            .model
            .farm
            .around(&p, RADIUS, level - params.farm_below..=level + params.farm_above)
            >= MIN_MOBS
    })
}
