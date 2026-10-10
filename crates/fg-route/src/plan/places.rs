//! Where each stop of a route takes place: the nearest giver or turn-in NPC, the objective
//! area, the flight master, the innkeeper, the dungeon entrance, the profession trainer.

use super::state::{Kind, State, Stop};
use crate::model::{EntityKind, Loc, Model};
use crate::world::{Pos, World};

#[derive(Clone, Copy)]
pub struct Places<'p> {
    pub(crate) model: &'p Model,
    pub(crate) world: &'p World,
}

impl Places<'_> {
    /// Where a stop takes place: the closest quest giver/turn-in NPC, the objective area,
    /// the flight master or the innkeeper.
    pub(crate) fn loc(&self, s: &State, stop: Stop) -> Loc {
        let pick = |locs: &[Loc]| {
            locs.iter()
                .min_by(|a, b| s.pos.dist(&a.pos).total_cmp(&s.pos.dist(&b.pos)))
                .unwrap()
                .clone()
        };
        match stop.kind {
            Kind::Accept => pick(&self.model.quests[stop.index as usize].starts),
            Kind::TurnIn => pick(&self.model.quests[stop.index as usize].ends),
            Kind::Objective(k) => self.model.quests[stop.index as usize].objectives[k as usize]
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
                }
            }
            Kind::Bind => self.model.inns[stop.index as usize].clone(),
            Kind::Dungeon => self.model.dungeons[stop.index as usize].entrance.clone(),
        }
    }

    /// Position of a stop, cheap (no allocation) for distance heuristics.
    pub(crate) fn pos(&self, s: &State, stop: Stop) -> Pos {
        let nearest = |locs: &[Loc]| {
            locs.iter()
                .map(|l| l.pos)
                .min_by(|a, b| s.pos.dist(a).total_cmp(&s.pos.dist(b)))
                .unwrap()
        };
        match stop.kind {
            Kind::Accept => nearest(&self.model.quests[stop.index as usize].starts),
            Kind::TurnIn => nearest(&self.model.quests[stop.index as usize].ends),
            Kind::Objective(k) => self.model.quests[stop.index as usize].objectives[k as usize].loc.pos,
            Kind::LearnFlight => self.world.taxi_nodes[stop.index as usize].pos,
            Kind::Bind => self.model.inns[stop.index as usize].pos,
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
            Kind::Dungeon => self.model.dungeons[i].entrance.zone,
        }
    }

    /// Usual position of a stop (quest giver, objective area, turn-in NPC, flight master...),
    /// to draw a route on a map.
    pub fn usual_pos(&self, s: &Stop) -> Pos {
        let i = s.index as usize;
        match s.kind {
            Kind::Accept => self.model.quests[i].starts[0].pos,
            Kind::Objective(k) => self.model.quests[i].objectives[k as usize].loc.pos,
            Kind::TurnIn => self.model.quests[i].ends[0].pos,
            Kind::LearnFlight => self.world.taxi_nodes[i].pos,
            Kind::Bind => self.model.inns[i].pos,
            Kind::Dungeon => self.model.dungeons[i].entrance.pos,
        }
    }

    /// The route as a light polyline: [continent, x, y] points, nearby points merged.
    pub fn polyline(&self, route: &[Stop], max_points: usize) -> Vec<[f64; 3]> {
        let mut out: Vec<[f64; 3]> = Vec::new();
        for s in route {
            let p = self.usual_pos(s);
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
