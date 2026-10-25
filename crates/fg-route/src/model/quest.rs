//! The quest put together: its levels, XP, givers, objectives, prerequisites and what it means
//! for the character's class.

use super::builder::QuestBuilder;
use super::givers::Givers;
use super::selection::QuestSkill;
use super::sources::{LinkKind, ObjectiveKind, QuestRow, SourceKind};
use super::types::{Objective, Quest};

impl QuestBuilder<'_> {
    pub(super) fn assemble(
        &self,
        r: &QuestRow,
        skill: QuestSkill,
        givers: Givers,
        objectives: Vec<Objective>,
    ) -> Quest {
        let level = r.level.unwrap_or(1).max(1);
        let class_quest = r.of_class(self.profile.class_id);
        // The class quests the group requires are those of every class in it (the power they
        // give is the character's own class only).
        let group_class_quest = self.profile.class_ids().iter().any(|&c| r.of_class(c));
        let (pre_all, pre_any) = self.prerequisites(r.id, &givers.item_pre);
        let guard = |locs: &[super::types::Loc]| {
            locs.iter()
                .map(|l| self.sources.guards.around(&l.pos))
                .fold(super::types::Guard::default(), super::types::Guard::max)
        };
        let (start_guard, end_guard) = (guard(&givers.starts), guard(&givers.ends));
        let zone_forced = self.zone_forced(r, &givers, &objectives);
        Quest {
            id: r.id,
            name: r.name.clone(),
            level,
            min_level: self.min_level(r, &givers),
            xp: self.xp(r, level),
            xp_observed: r.xp_observed,
            time_limit: r.time_limit.filter(|t| *t > 0).map(|t| t as f64),
            starts: givers.starts,
            ends: givers.ends,
            objectives,
            pre_all,
            pre_any,
            exclusive: self.links(r.id, LinkKind::Exclusive),
            pre_held: self.pre_held(r),
            breadcrumb_for: r.breadcrumb_for.filter(|b| *b > 0),
            start_kills: givers.start_work.0,
            start_mob_level: givers.start_work.1,
            start_uses: givers.start_work.2,
            start_mob: givers.start_mob,
            start_spots: givers.start_spots,
            start_dungeon: givers.start_dungeon,
            held: givers.held,
            carried: r.source_item.is_some_and(|i| i > 0),
            mandatory: zone_forced
                || group_class_quest && self.profile.required_class_quests.iter().any(|n| n == &r.name),
            zone_forced,
            required: false,
            skill: skill.flatten(),
            power: class_quest
                .then(|| self.profile.class_powers.iter().position(|(n, _)| n == &r.name))
                .flatten()
                .map(|k| (k as u8, self.profile.class_powers[k].1)),
            reward_spell: r.reward_spell,
            start_guard,
            end_guard,
            reputation: r
                .reputations()
                .into_iter()
                .map(|(f, v)| (f, v as f64 * self.profile.reputation_rate()))
                .collect(),
            members: self.profile.members_of(r.races, r.classes),
        }
    }

    /// Whether the quest is one of a forced zone: filed under it (or a subzone), else given
    /// there (class and profession quests); taken before the target level, outside dungeons.
    fn zone_forced(&self, r: &QuestRow, givers: &Givers, objectives: &[Objective]) -> bool {
        let zones = &self.params.forced_zones;
        if zones.is_empty()
            || objectives.iter().any(|o| o.dungeon.is_some())
            || self.min_level(r, givers) > self.profile.to_level
        {
            return false;
        }
        match r.sort.filter(|s| *s > 0) {
            Some(area) => self.sources.in_zones(area, zones),
            None => givers.starts.iter().any(|l| self.sources.in_zones(l.zone, zones)),
        }
    }

    /// The quest's minimum level, and the level at which the character takes on the mob that
    /// drops the item starting it: a lone target (a named mob: Owatanka, level 24, is no quest
    /// for a level 11 character), a pack when farmed.
    fn min_level(&self, r: &QuestRow, givers: &Givers) -> i64 {
        let min_level = r.min_level.unwrap_or(1).max(1);
        let (kills, mob_level, _) = givers.start_work;
        if kills <= 0.0 {
            return min_level;
        }
        let need = mob_level as f64 + if kills <= 2.0 { -1.0 } else { self.params.pack_extra() };
        min_level.max(self.params.level_for(need, crate::params::Might::PLAIN))
    }

    /// The quest's XP (to each member it is for, see `Initial::need`).
    fn xp(&self, r: &QuestRow, level: i64) -> i64 {
        if r.xp > 0 || r.xp_observed {
            // The game's own XP, none included (an item handed in for a reward only).
            r.xp
        } else {
            self.sources.default_xp.get(&level).copied().unwrap_or(0)
        }
    }

    /// All of `pre_all` and one of `pre_any`. The quest handing the item that starts this one is
    /// one more prerequisite: any of them when several do.
    fn prerequisites(&self, id: i64, item_pre: &[i64]) -> (Vec<i64>, Vec<i64>) {
        let mut pre_all = self.links(id, LinkKind::PreAll);
        let mut pre_any = self.links(id, LinkKind::PreAny);
        if item_pre.len() == 1 {
            pre_all.extend(item_pre);
        } else if item_pre.len() > 1 {
            pre_any.extend(item_pre);
        }
        (pre_all, pre_any)
    }

    /// The quests to hold in the log when taking it: those giving a bag that holds an item it
    /// asks for, unless its own starting item gives it.
    fn pre_held(&self, r: &QuestRow) -> Vec<i64> {
        let own = |item: i64| {
            r.source_item.is_some_and(|s| {
                s == item || (self.sources.item_origins.get(&item)).is_some_and(|o| o.contains(&(SourceKind::Item, s)))
            })
        };
        let mut held: Vec<i64> = Vec::new();
        for o in self.sources.objectives.get(&r.id).into_iter().flatten() {
            if o.0 != ObjectiveKind::Item {
                continue;
            }
            let Some(item) = o.1.filter(|i| !own(*i)) else { continue };
            if let Some(q) = self.bag_quest(r.id, item)
                && !held.contains(&q)
            {
                held.push(q);
            }
        }
        held
    }

    fn links(&self, id: i64, kind: LinkKind) -> Vec<i64> {
        self.sources
            .links
            .get(&id)
            .into_iter()
            .flatten()
            .filter(|(k, _)| *k == kind)
            .map(|(_, o)| *o)
            .filter(|o| *o > 0)
            .collect()
    }
}
