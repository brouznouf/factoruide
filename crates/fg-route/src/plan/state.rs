//! What a route is made of and what replaying it produces: stops, the character's state, the trace of events, the time breakdown.

use crate::model::Loc;
use crate::world::Pos;
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Accept,
    Objective(u8),
    TurnIn,
    /// Visit a flight master (`Stop::index` is the taxi node).
    LearnFlight,
    /// Bind the hearthstone (`Stop::index` is the inn).
    Bind,
    /// Run a dungeon with a group (`Stop::index` is the dungeon).
    Dungeon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stop {
    /// Quest index for quest stops, taxi node or inn index for visits.
    pub index: u32,
    pub kind: Kind,
}

impl Stop {
    pub fn is_quest(&self) -> bool {
        matches!(self.kind, Kind::Accept | Kind::Objective(_) | Kind::TurnIn)
    }

    pub fn quest(index: u32, kind: Kind) -> Self {
        Self { index, kind }
    }
}

/// How a trip is made.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Leg {
    Walk,
    Fly(usize, usize),
    Link(usize, usize),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Via {
    pub(crate) hearth: bool,
    pub(crate) leg: Leg,
}

/// What happened while replaying a route, for export.
#[derive(Debug, Clone)]
pub enum Event {
    Hearth {
        inn: usize,
    },
    Fly {
        from: usize,
        to: usize,
    },
    /// Transport links, boarding `first` and leaving `last` (same link when direct).
    Link {
        first: usize,
        last: usize,
    },
    LearnFlight {
        node: usize,
    },
    /// Class training: `since` is the level of the previous training.
    Train {
        trainer: usize,
        since: i64,
    },
    /// Quest reward picked among the choices (item ID).
    Reward {
        quest: usize,
        item: i64,
    },
    Grind {
        to_level: i64,
        seconds: f64,
        near: Pos,
        zone: i64,
    },
    /// Died on purpose, respawned at the spirit healer near `pos`.
    DeathSkip {
        pos: Pos,
        zone: i64,
    },
    Stop {
        stop: Stop,
        loc: Loc,
    },
    /// Quest given up: it can no longer be finished (its dungeon is done or outleveled).
    Abandon {
        quest: usize,
    },
    /// Normal mobs killed on the way to the next stop (`Params::farm_on_way`).
    Farm {
        kills: u32,
        seconds: f64,
    },
    /// Mobs of an objective killed on the way to the next stop (`finished`: it is done).
    Along {
        quest: usize,
        objective: u8,
        kills: f64,
        finished: bool,
    },
}

#[derive(Debug, Clone)]
pub struct Timed {
    pub event: Event,
    pub time: f64,
    pub level: i64,
    pub xp: i64,
    /// Power of the character (level plus what gear and spells add, see `power`), and the part
    /// of it its gear brings (levels).
    pub power: f64,
    pub gear: f64,
}

#[derive(Clone)]
pub(crate) struct State {
    pub(crate) pos: Pos,
    pub(crate) zone: i64,
    pub(crate) time: f64,
    pub(crate) level: i64,
    pub(crate) xp: i64,
    /// Rested XP left: kills give double up to it.
    pub(crate) rested: i64,
    pub(crate) known: u128,
    pub(crate) bind: usize,
    pub(crate) hearth_ready: f64,
    pub(crate) trained: i64,
    pub(crate) dungeons: u128,
    pub(crate) accepted: Vec<bool>,
    /// Quests out of play: turned in, or given up (those are also in `dropped`).
    pub(crate) turned: Vec<bool>,
    /// Quests given up (`Event::Abandon`): never turned in, so they open no follow-up.
    pub(crate) dropped: Vec<u32>,
    pub(crate) objectives: Vec<u32>,
    /// Objectives of the log whose mobs can be killed on the way to other stops, with the
    /// kills already done: (quest, objective, kills).
    pub(crate) along: Vec<(u32, u8, f64)>,
    /// Quests the route accepts (bit 1) and turns in (bit 2): taken and turned in on the spot
    /// when their NPC is at hand.
    pub(crate) planned: Rc<Planned>,
    /// Index in the route of the stop being applied (for `Planned::accept_at`).
    pub(crate) at: u32,
    /// Levels of power the class quests give (a pet, a form; `Quest::power`), part of `bonus`;
    /// `powers` holds the class powers already gained.
    pub(crate) class_bonus: f64,
    pub(crate) powers: u32,
    /// Gear worn (quest rewards, dungeon loot) and the levels of power gear and class spells
    /// add to the level (see `power`).
    pub(crate) gear: crate::power::Gear,
    pub(crate) bonus: f64,
    /// The part of `bonus` the gear brings.
    pub(crate) gear_bonus: f64,
    /// Level and dungeons done at the last check for quests to give up.
    pub(crate) abandon_check: (i64, u128),
    /// Explorable areas already discovered.
    pub(crate) explored: Vec<bool>,
    pub(crate) log: usize,
    /// XP grinding and farming on the way gave in the current level (see `Params::grind_cap`).
    pub(crate) grind_used: i64,
    pub(crate) spent: Breakdown,
}

impl State {
    /// Quest `i` was turned in (not given up): it counts as a prerequisite.
    pub(crate) fn rewarded(&self, i: usize) -> bool {
        self.turned[i] && !self.dropped.contains(&(i as u32))
    }
}

/// Where the time goes.
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct Breakdown {
    pub travel: f64,
    pub flights: f64,
    pub fighting: f64,
    pub grinding: f64,
    pub overhead: f64,
    #[serde(default)]
    pub training: f64,
    #[serde(default)]
    pub hearths: u32,
    #[serde(default)]
    pub missing_class_quests: u32,
    #[serde(default)]
    pub dungeons: f64,
    #[serde(default)]
    pub dungeon_runs: u32,
    /// Real play time (the optimizer score adds penalties on top).
    #[serde(default)]
    pub total: f64,
    pub quest_xp: i64,
    pub mob_xp: i64,
    pub grind_xp: i64,
    /// Mobs killed on the way (see `Params::farm_on_way`): time and XP.
    #[serde(default)]
    pub farming: f64,
    #[serde(default)]
    pub farm_xp: i64,
    /// Grinding beyond `Params::grind_cap` (part of `grinding`).
    #[serde(default)]
    pub grind_over: f64,
    /// XP from discovering areas.
    #[serde(default)]
    pub explore_xp: i64,
    /// Copper spent on flights.
    #[serde(default)]
    pub flight_cost: i64,
    /// Forced dungeons the route does not run.
    #[serde(default)]
    pub missing_dungeons: u32,
    /// Times the character went into another zone.
    #[serde(default)]
    pub zone_changes: u32,
}

/// Quests a route accepts (bit 1) and turns in (bit 2), and where it first accepts each.
#[derive(Default)]
pub(crate) struct Planned {
    pub(crate) flags: Vec<u8>,
    pub(crate) accept_at: Vec<u32>,
}

/// Same quest (any of its stops), or the same visit.
pub(crate) fn same_item(a: Stop, b: Stop) -> bool {
    if a.is_quest() && b.is_quest() {
        a.index == b.index
    } else {
        a == b
    }
}

pub fn fmt_time(seconds: f64) -> String {
    if !seconds.is_finite() {
        return "invalid".into();
    }
    let m = (seconds / 60.0).round() as i64;
    format!("{}h{:02}", m / 60, m % 60)
}
