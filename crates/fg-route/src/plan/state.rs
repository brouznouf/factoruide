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
    /// Back to the inn the hearthstone is bound to: the hearthstone (`Stop::index` 0) or the
    /// shaman's Astral Recall (1). Nothing when it is not ready yet, or not learned.
    Hearth,
    /// Run a dungeon with a group (`Stop::index` is the dungeon).
    Dungeon,
}

/// How the last step of an edited guide left the character (see `State::landed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Landed {
    #[default]
    No,
    /// Off a flight: the walk on counts as flying.
    Flight,
    /// Off a boat or a teleport: the walk on counts as travel.
    Transport,
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
pub enum Leg {
    Walk,
    /// A flight between two flight paths.
    Fly(usize, usize),
    /// Boats, zeppelins or portals: boarding the first link, leaving the last.
    Link(usize, usize),
    /// A flight to a port, then its boats (flight paths, then links).
    FlyLink(usize, usize, usize, usize),
    /// Boats, then a flight from near where they land (links, then flight paths).
    LinkFly(usize, usize, usize, usize),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Via {
    /// A class teleport first (index in `Model::teleports`).
    pub(crate) teleport: Option<usize>,
    /// Or dying first, to respawn at this spirit healer (index in `Model::graveyards`).
    pub(crate) death: Option<usize>,
    pub(crate) leg: Leg,
}

/// What happened while replaying a route, for export.
#[derive(Debug, Clone)]
pub enum Event {
    Hearth {
        inn: usize,
        /// Astral Recall rather than the hearthstone.
        recall: bool,
    },
    /// A class teleport (index in `Model::teleports`).
    Teleport {
        to: usize,
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
    /// An off-road shortcut taken on a walk (index in `World::passes`).
    Shortcut {
        pass: usize,
    },
    /// Class training: `since` is the level of the previous training.
    Train {
        trainer: usize,
        since: i64,
        abilities: u64,
    },
    /// The bags' loot sold at a vendor (`Model::vendors`).
    Sell {
        vendor: usize,
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
    /// Mobs of the character's level killed where it stands (a step of an edited route).
    GrindMobs {
        mobs: u32,
        seconds: f64,
        near: Pos,
        zone: i64,
    },
    /// In a group further apart than `Params::group_sync_max`, the players behind (`members`,
    /// a bit each in `Profile` order) kill mobs of their level on their own, the others
    /// waiting, until they are within `Params::group_sync_window`.
    CatchUp {
        members: u8,
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
    /// Normal mobs killed on the way to the next stop (`Params::farm_on_way`), and the XP they
    /// gave (rested bonus included).
    Farm {
        kills: u32,
        xp: i64,
        seconds: f64,
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
    /// The players of a group guide, in `Profile` order: level and XP into it (empty alone).
    pub party: Vec<(i64, i64)>,
}

/// Objective stops done one after the other within `gather_radius` of the first one: objects
/// gathered between the kills (resting, looking for the next mob) take `gather_overlap` of the
/// kills' time. Their place, the time of their kills and gatherings, and what that saved. Only
/// consecutive stops: the route says what is done together, each stop can be moved apart.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Together {
    pub(crate) at: Option<Pos>,
    pub(crate) kills: f64,
    pub(crate) gathers: f64,
    pub(crate) saved: f64,
}

/// A flag per quest (or per area), a bit each: a state is kept before every stop of a route.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Flags(Vec<u64>);

impl Flags {
    pub(crate) fn new(n: usize) -> Self {
        Self(vec![0; n.div_ceil(64)])
    }

    pub(crate) fn set(&mut self, i: usize, on: bool) {
        if on {
            self.0[i / 64] |= 1 << (i % 64);
        } else {
            self.0[i / 64] &= !(1 << (i % 64));
        }
    }

    /// The flags set here and not in `other`, in order.
    pub(crate) fn and_not(&self, other: &Self) -> Vec<usize> {
        let mut out = Vec::new();
        for (w, (a, b)) in self.0.iter().zip(&other.0).enumerate() {
            let mut bits = a & !b;
            while bits != 0 {
                out.push(w * 64 + bits.trailing_zeros() as usize);
                bits &= bits - 1;
            }
        }
        out
    }
}

impl std::ops::Index<usize> for Flags {
    type Output = bool;

    fn index(&self, i: usize) -> &bool {
        if self.0[i / 64] >> (i % 64) & 1 != 0 {
            &true
        } else {
            &false
        }
    }
}

/// Most other players of a group guide (`Profile::mates`).
pub(crate) const MATES: usize = 4;

/// Where another player of a group guide stands: it does the same stops as the character, and
/// gets the XP of the quests still for it (`Initial::need`) and of the kills at its own level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct MateXp {
    pub(crate) level: i64,
    pub(crate) xp: i64,
    pub(crate) rested: i64,
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
    /// The other players of a group guide (the first `Initial::mates.len()`).
    pub(crate) mates: [MateXp; MATES],
    /// The member (0: the character planned for, then `Profile::mates`) in each place: the
    /// character's own (`level`, `xp`, `rested`), then `mates`. The lowest is always first, so
    /// that the levels of quests and mobs are those it can take on.
    pub(crate) order: [u8; MATES + 1],
    /// The group has been within `Params::group_sync_window` (from then on, never further
    /// apart than `group_sync_max`).
    pub(crate) synced: bool,
    /// Common loot in the bags (`Model::loot`), gathered by the kills so far, less what
    /// objectives took.
    pub(crate) stock: Vec<f32>,
    /// Bag slots the loot to sell takes, and those the quests' items take (`Params::bag_weight`).
    pub(crate) bags: f32,
    pub(crate) held: u16,
    pub(crate) known: u128,
    pub(crate) bind: usize,
    pub(crate) hearth_ready: f64,
    pub(crate) recall_ready: f64,
    pub(crate) trained: i64,
    /// The class teleports learned (bit per `Model::teleports`): each at the trainer of its city.
    pub(crate) taught: u16,
    pub(crate) abilities: u64,
    pub(crate) dungeons: u128,
    pub(crate) accepted: Flags,
    /// Quests out of play: turned in, or given up (those are also in `dropped`).
    pub(crate) turned: Flags,
    /// Quests given up (`Event::Abandon`): never turned in, so they open no follow-up.
    pub(crate) dropped: Vec<u32>,
    /// Objectives done of each quest, a bit each.
    pub(crate) objectives: Vec<u8>,
    /// Timed quests of the log: (quest, time by which the route must turn it in).
    pub(crate) timers: Vec<(u32, f64)>,
    /// Objectives started before the route (a character met in game), with the share already
    /// done (0 to 1): (quest, objective, share). A share, not kills: the places of an objective
    /// may need different kills (their drop rates differ).
    pub(crate) started: Vec<(u32, u8, f64)>,
    /// The objective stops done one after the other at the same place (see `Together`).
    pub(crate) together: Together,
    /// The place of the route's next stop, when the route is known (a replay): an objective
    /// with several places is done at the one on the way to it (`Planner::spot`). Set before
    /// each stop, not part of what the state carries on.
    pub(crate) next: Option<Pos>,
    /// The place imposed on an objective (quest, objective, place: 0 its own, then its
    /// `alts`): a guide replayed where it was computed (`Edit::Stop`).
    pub(crate) spot: Option<(u32, u8, u8)>,
    /// Just off a flight, boat or teleport step of an edited guide (`Edit::Fly`, `Edit::Link`,
    /// `Edit::Teleport`): the walk to the next stop ends that trip, as when a trip takes it by
    /// itself (no mob farmed, the zone changed on arriving).
    pub(crate) landed: Landed,
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
    /// Quality of the gear for what the character takes on (`Power::gear_quality`).
    pub(crate) gear_quality: f64,
    /// Level and dungeons done at the last check for quests to give up.
    pub(crate) abandon_check: (i64, u128),
    /// Explorable areas already discovered.
    pub(crate) explored: Flags,
    pub(crate) log: usize,
    /// XP grinding and farming on the way gave in the current level (see `Params::grind_cap`
    /// and `Params::farm_cap`).
    pub(crate) grind_used: i64,
    pub(crate) farm_used: i64,
    /// Points with the faction of each reputation goal (`Params::reputation_goals`; none
    /// without goals).
    pub(crate) reputation: Vec<f32>,
    /// Quests of the forced zones not done yet (nor closed by an exclusive one): the guide
    /// goes on past the target level until they are.
    pub(crate) forced_left: u32,
    pub(crate) required_left: u32,
    pub(crate) spent: Breakdown,
}

impl State {
    /// The group's players in `Profile` order: level and XP into it (empty alone).
    pub(crate) fn party(&self) -> Vec<(i64, i64)> {
        let n = self.mates.iter().take_while(|m| m.level > 0).count();
        if n == 0 {
            return Vec::new();
        }
        let mut party = vec![(0, 0); n + 1];
        party[usize::from(self.order[0])] = (self.level, self.xp);
        for (k, m) in self.mates[..n].iter().enumerate() {
            party[usize::from(self.order[k + 1])] = (m.level, m.xp);
        }
        party
    }

    /// What the character takes on depends on besides its level (`Params::reach`).
    pub(crate) fn might(&self) -> crate::params::Might {
        crate::params::Might {
            other: self.bonus - self.gear_bonus,
            gear: self.gear_quality,
        }
    }

    /// Quest `i` was turned in (not given up): it counts as a prerequisite.
    pub(crate) fn rewarded(&self, i: usize) -> bool {
        self.turned[i] && !self.dropped.contains(&(i as u32))
    }

    /// The class teleport `i` is learned: at its city's trainer (a mage learns Teleport: Ironforge
    /// in Ironforge), or on taking one of the quests teaching it.
    pub(crate) fn knows(&self, i: usize, tp: &crate::model::Teleport) -> bool {
        tp.level <= self.trained
            && if tp.quests.is_empty() {
                self.taught & (1 << i) != 0
            } else {
                tp.quests.iter().any(|&q| self.accepted[q] || self.turned[q])
            }
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
    /// The objectives' fixed time on top of their kills and uses (part of `fighting`): waiting
    /// for respawns, for an event or for the other players, buying, exploring.
    #[serde(default)]
    pub waiting: f64,
    /// Copper spent on flights.
    #[serde(default)]
    pub flight_cost: i64,
    /// What that gold weighs for the optimizer, each flight at the level it is taken
    /// (`Params::gold_weight_at`), in seconds.
    #[serde(skip)]
    pub flight_gold: f64,
    /// What the loot left behind with full bags weighs for the optimizer
    /// (`Params::bag_weight`), in seconds.
    #[serde(skip)]
    pub bag_overflow: f64,
    /// Forced dungeons the route does not run.
    #[serde(default)]
    pub missing_dungeons: u32,
    /// Times the character went into another zone.
    #[serde(default)]
    pub zone_changes: u32,
    /// Time spent outside the preferred zones (none preferred: 0).
    #[serde(default)]
    pub outside_preferred: f64,
    /// Quests the route took and gave up (see `Params::abandon_weight`).
    #[serde(default)]
    pub abandoned: u32,
    /// Quests of the forced zones the route does not do (see `Params::forced_zones`).
    #[serde(default)]
    pub missing_zone_quests: u32,
    /// Quests of the leveled professions left out (see `Params::profession_quest_weight`).
    #[serde(default)]
    pub missing_profession_quests: u32,
    /// Reputation points the goals still miss at the end (see `Params::reputation_goals`).
    #[serde(default)]
    pub reputation_missing: f64,
    /// A group's levels apart beyond `Params::group_sync_window`, by the time they stay so
    /// (level seconds): what the group waits to play together.
    #[serde(default)]
    pub desync: f64,
    /// The longest the group was out of `Params::group_sync_window` from the start (seconds):
    /// the time it takes to get its levels together.
    #[serde(default)]
    pub sync_time: f64,
    /// Time the players behind spent killing mobs on their own to catch up (part of
    /// `grinding`, see `Event::CatchUp`).
    #[serde(default)]
    pub catch_up: f64,
}

/// Quests a route accepts (bit 1), turns in (bit 2) or does an objective of (bit 4), and where
/// it first accepts each.
#[derive(Default)]
pub(crate) struct Planned {
    pub(crate) flags: Vec<u8>,
    pub(crate) accept_at: Vec<u32>,
}

impl Planned {
    /// No objective nor turn-in of quest `i`: one in the log at the start is given up.
    pub(crate) fn left(&self, i: usize) -> bool {
        self.flags[i] & 6 == 0
    }
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
