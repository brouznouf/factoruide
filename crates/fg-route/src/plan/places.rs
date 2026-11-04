//! Where each stop of a route takes place: the nearest giver or turn-in NPC, the objective
//! area, the flight master, the innkeeper, the dungeon entrance, the profession trainer.

use super::state::{Kind, State, Stop};
use crate::model::{EntityKind, Loc, Model};
use crate::world::{Pos, World};

/// The dungeon stop `stop` takes place in from state `s` (`Places::inside`).
pub(crate) fn stop_inside(model: &Model, s: &State, stop: Stop) -> Option<usize> {
    let nearest = |locs: &[Loc]| {
        locs.iter()
            .min_by(|a, b| s.pos.dist(&a.pos).total_cmp(&s.pos.dist(&b.pos)))
            .and_then(|l| l.inside)
    };
    let i = stop.index as usize;
    match stop.kind {
        Kind::Accept => nearest(&model.quests[i].starts),
        Kind::TurnIn => nearest(&model.quests[i].ends),
        Kind::Objective(k) => model.quests[i].objectives[k as usize].nearest(&s.pos).loc.inside,
        Kind::Dungeon => return Some(i),
        Kind::LearnFlight | Kind::Bind | Kind::Hearth => None,
    }
    .map(usize::from)
}

/// Zone of a hearthstone stop (`Places::zone`): none.
const HEARTH_ZONE: i64 = -1;

#[derive(Clone, Copy)]
pub struct Places<'p> {
    pub(crate) model: &'p Model,
    pub(crate) world: &'p World,
}

impl Places<'_> {
    /// Where a stop takes place: the closest quest giver/turn-in NPC, objective area (where it
    /// is done in several: `Planner::spot` chooses when doing it), the flight master or the
    /// innkeeper.
    pub(crate) fn loc(&self, s: &State, stop: Stop) -> Loc {
        let pick = |locs: &[Loc]| {
            locs.iter()
                .map(|l| (l, s.pos.dist(&l.pos)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .expect("a place has locations")
                .0
                .clone()
        };
        match stop.kind {
            Kind::Accept => pick(&self.model.quests[stop.index as usize].starts),
            Kind::TurnIn => pick(&self.model.quests[stop.index as usize].ends),
            Kind::Objective(k) => self.model.quests[stop.index as usize].objectives[k as usize]
                .nearest(&s.pos)
                .loc
                .clone(),
            Kind::LearnFlight => {
                let n = &self.world.taxi_nodes[stop.index as usize];
                Loc {
                    pos: n.pos,
                    zone: n.pos.zone,
                    kind: EntityKind::Taxi,
                    id: n.id,
                    name: n.name.clone(),
                    inside: None,
                    floor: None,
                }
            }
            Kind::Bind => self.model.inns[stop.index as usize].clone(),
            Kind::Hearth => self.model.inns[s.bind].clone(),
            Kind::Dungeon => self.model.dungeons[stop.index as usize].entrance.clone(),
        }
    }

    /// The dungeon a stop takes place in (`Loc::inside`), as `loc` chooses its place.
    pub(crate) fn inside(&self, s: &State, stop: Stop) -> Option<usize> {
        stop_inside(self.model, s, stop)
    }

    /// Position of a stop, cheap (no allocation) for distance heuristics.
    pub(crate) fn pos(&self, s: &State, stop: Stop) -> Pos {
        let nearest = |locs: &[Loc]| {
            locs.iter()
                .map(|l| (l.pos, s.pos.dist(&l.pos)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .expect("a place has locations")
                .0
        };
        match stop.kind {
            Kind::Accept => nearest(&self.model.quests[stop.index as usize].starts),
            Kind::TurnIn => nearest(&self.model.quests[stop.index as usize].ends),
            Kind::Objective(k) => {
                self.model.quests[stop.index as usize].objectives[k as usize]
                    .nearest(&s.pos)
                    .loc
                    .pos
            }
            Kind::LearnFlight => self.world.taxi_nodes[stop.index as usize].pos,
            Kind::Bind => self.model.inns[stop.index as usize].pos,
            Kind::Hearth => self.model.inns[s.bind].pos,
            Kind::Dungeon => self.model.dungeons[stop.index as usize].entrance.pos,
        }
    }

    pub(crate) fn zone(&self, s: Stop) -> i64 {
        let i = s.index as usize;
        match s.kind {
            Kind::Accept => self.model.quests[i].starts[0].zone,
            Kind::Objective(k) => self.model.quests[i].objectives[k as usize].loc.zone,
            Kind::TurnIn => self.model.quests[i].ends[0].zone,
            Kind::LearnFlight => self.world.taxi_nodes[i].pos.zone,
            Kind::Bind => self.model.inns[i].zone,
            // Not a place of its own (the bound inn): a stay by itself.
            Kind::Hearth => HEARTH_ZONE,
            Kind::Dungeon => self.model.dungeons[i].entrance.zone,
        }
    }

    /// The dungeon a stop usually takes place in (as `usual_pos`).
    pub(crate) fn usual_inside(&self, s: Stop) -> Option<usize> {
        let i = s.index as usize;
        match s.kind {
            Kind::Accept => self.model.quests[i].starts[0].inside,
            Kind::Objective(k) => self.model.quests[i].objectives[k as usize].loc.inside,
            Kind::TurnIn => self.model.quests[i].ends[0].inside,
            Kind::Dungeon => return Some(i),
            Kind::LearnFlight | Kind::Bind | Kind::Hearth => None,
        }
        .map(usize::from)
    }

    /// The dungeons a route enters: its dungeon stops and those of its stops inside one.
    pub(crate) fn entered(&self, route: &[Stop]) -> std::collections::BTreeSet<u32> {
        route
            .iter()
            .filter_map(|&s| self.usual_inside(s))
            .map(|d| d as u32)
            .collect()
    }

    /// Usual position of a stop (quest giver, objective area, turn-in NPC, flight master...),
    /// to draw a route on a map.
    pub(crate) fn usual_pos(&self, s: Stop) -> Pos {
        let i = s.index as usize;
        match s.kind {
            Kind::Accept => self.model.quests[i].starts[0].pos,
            Kind::Objective(k) => self.model.quests[i].objectives[k as usize].loc.pos,
            Kind::TurnIn => self.model.quests[i].ends[0].pos,
            Kind::LearnFlight => self.world.taxi_nodes[i].pos,
            Kind::Bind => self.model.inns[i].pos,
            Kind::Hearth => self.model.inns[self.model.start_inn].pos,
            Kind::Dungeon => self.model.dungeons[i].entrance.pos,
        }
    }

    /// The route as a light polyline: [continent, x, y] points, nearby points merged.
    pub(crate) fn polyline(&self, route: &[Stop], max_points: usize) -> Vec<[f64; 3]> {
        let mut out: Vec<[f64; 3]> = Vec::new();
        // The inn a hearthstone goes to depends on the binds before: the line jumps to the stop
        // after it instead.
        for s in route.iter().filter(|s| s.kind != Kind::Hearth) {
            let p = self.usual_pos(*s);
            let pt = [
                p.continent as f64,
                (p.x * 10.0).round() / 10.0,
                (p.y * 10.0).round() / 10.0,
            ];
            if out
                .last()
                .is_some_and(|l| l[0] as i64 == p.continent && (l[1] - pt[1]).hypot(l[2] - pt[2]) < 60.0)
            {
                continue;
            }
            out.push(pt);
        }
        if out.len() > max_points {
            let step = out.len() as f64 / max_points as f64;
            out = (0..max_points).map(|k| out[(k as f64 * step) as usize]).collect();
        }
        out
    }
}
