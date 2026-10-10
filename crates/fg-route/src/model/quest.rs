//! The quest put together: its levels, XP, givers, objectives, prerequisites and what it means
//! for the character's class.

use super::builder::QuestBuilder;
use super::chains::class_sort;
use super::givers::Givers;
use super::selection::QuestSkill;
use super::sources::{LinkKind, QuestRow};
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
        let class_quest = r.sort == Some(-class_sort(self.profile.class_id));
        let (pre_all, pre_any) = self.prerequisites(r.id, &givers.item_pre);
        let guard = |locs: &[super::types::Loc]| {
            locs.iter()
                .map(|l| self.sources.guards.around(&l.pos))
                .fold(super::types::Guard::default(), super::types::Guard::max)
        };
        let (start_guard, end_guard) = (guard(&givers.starts), guard(&givers.ends));
        Quest {
            id: r.id,
            name: r.name.clone(),
            level,
            min_level: self.min_level(r, &givers),
            xp: self.xp(r, level),
            xp_observed: r.xp_observed && r.xp > 0,
            starts: givers.starts,
            ends: givers.ends,
            objectives,
            pre_all,
            pre_any,
            exclusive: self.links(r.id, LinkKind::Exclusive),
            breadcrumb_for: r.breadcrumb_for.filter(|b| *b > 0),
            start_kills: givers.start_work.0,
            start_mob_level: givers.start_work.1,
            start_uses: givers.start_work.2,
            mandatory: class_quest && self.profile.required_class_quests.iter().any(|n| n == &r.name),
            skill: skill.flatten(),
            power: class_quest
                .then(|| self.profile.class_powers.iter().position(|(n, _)| n == &r.name))
                .flatten()
                .map(|k| (k as u8, self.profile.class_powers[k].1)),
            start_guard,
            end_guard,
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
        min_level.max(self.params.level_for(need, 0.0))
    }

    /// Quests of the other classes give their XP to the other player.
    fn xp(&self, r: &QuestRow, level: i64) -> i64 {
        if self.other_class(r) {
            0
        } else if r.xp > 0 {
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
