//! Which quests the planner considers: those of the race and class of a member of the group (the
//! character alone) and of its levels, then the rules that leave quests out.

use super::builder::{QuestBuilder, Skip};
use super::sources::QuestRow;

/// The profession a quest needs: None for no profession, Some(None) for one the profile does not
/// level, Some(Some((profession index, skill))) otherwise.
pub(super) type QuestSkill = Option<Option<(usize, i64)>>;

impl QuestBuilder<'_> {
    pub(super) fn concerns(&self, r: &QuestRow) -> bool {
        let profile = self.profile;
        let level = r.level.unwrap_or(1).max(1);
        let min_level = r.min_level.unwrap_or(1).max(1);
        // A quest of a member's own race and class (in a group, the others' too: done together,
        // their XP goes to those it is for).
        profile.members_of(r.races, r.classes) != 0
            && (!profile.required_quests.is_empty()
                || profile.within_levels(level, min_level, self.params.max_quest_above))
    }

    /// Profession quests: kept when the profile levels that profession, accepted once the skill
    /// is reached.
    pub(super) fn skill(&self, r: &QuestRow) -> QuestSkill {
        r.skill.filter(|s| *s != 0).map(|id| {
            self.profile
                .professions
                .iter()
                .filter_map(|g| crate::profession::find(&g.key))
                .position(|d| d.skill_id == id)
                .map(|p| (p, r.skill_value.unwrap_or(0)))
        })
    }

    pub(super) fn check_rules(&self, r: &QuestRow, skill: QuestSkill) -> Result<(), Skip> {
        if self.excluded.contains(&r.id) {
            return Err("excluded by configuration");
        }
        let excluded_sort = r.sort.is_some_and(|s| s < 0 && self.excluded_sorts.contains(&-s));
        if excluded_sort && !matches!(skill, Some(Some(_))) {
            return Err("holiday/profession/event category");
        }
        if self.sources.pvp.contains(&r.id) && !self.params.pvp_quests {
            return Err("PvP (battleground) quest");
        }
        if r.special.unwrap_or(0) & 1 != 0 {
            return Err("repeatable");
        }
        // A negative spell is one the character must not know yet: the spell the quest teaches
        // (Camping 101's Camp Tent), never known before taking it.
        if matches!(skill, Some(None)) || r.rep.unwrap_or(0) != 0 || r.spell.unwrap_or(0) > 0 {
            return Err("needs a profession, reputation or spell");
        }
        Ok(())
    }
}
