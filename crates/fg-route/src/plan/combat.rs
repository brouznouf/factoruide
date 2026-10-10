//! How long fights take and what the character can take on: kill times, the power an
//! objective requires, the level it is done at, the XP of its mobs, grinding.

use crate::model::{Dungeon, EntityKind, Objective, Quest};
use crate::params::Params;
use crate::world::Pos;
use crate::xp;

#[derive(Clone, Copy)]
pub struct Combat<'a> {
    pub(crate) params: &'a Params,
    pub(crate) dungeons: &'a [Dungeon],
}

impl Combat<'_> {
    fn rules(self) -> &'static xp::Rules {
        self.params.edition.rules()
    }

    /// Kill time of one mob of an objective (elites take `elite_kill_factor` times longer, more
    /// in a camp where they come several at a time: longer rests).
    pub fn kill_time(self, level: i64, bonus: f64, o: &Objective) -> f64 {
        let t = self.params.kill_time(level, bonus, o.mob_level);
        if o.elite {
            t * self.params.elite_kill_factor * (1.0 + 0.25 * (o.pull - 1.0))
        } else {
            t
        }
    }

    /// Power an objective requires: its quest's level (where its mobs live), and its mobs: their
    /// level, one less for a lone target (one or two kills: a named mob), `pack_extra` more for a
    /// pack (three kills or more), two more for an elite, plus `elite_pull_levels` per extra
    /// elite expected in each pull (a camp of elites). Exploring an area or escorting requires
    /// the quest's level; talking and delivering nothing.
    pub fn need(self, q: &Quest, o: &Objective) -> f64 {
        if o.kills <= 0.0 && o.uses <= 0.0 && o.loc.kind != EntityKind::Area {
            return 0.0;
        }
        let mut need = q.level as f64;
        if o.kills > 0.0 {
            let mut mob = o.mob_level as f64 + if o.kills <= 2.0 { -1.0 } else { self.params.pack_extra() };
            if o.elite {
                mob += 2.0 + self.params.elite_pull_levels * (o.pull - 1.0).max(0.0);
            }
            need = need.max(mob);
        }
        need
    }

    /// Lowest level at which the character, with `bonus` levels of power, takes on objective `k`
    /// of quest `q` (0 when it requires nothing).
    pub fn level(self, q: &Quest, k: u8, bonus: f64) -> i64 {
        let need = self.need(q, &q.objectives[k as usize]);
        if need <= 0.0 {
            0
        } else {
            self.params.level_for(need, bonus)
        }
    }

    /// XP of one mob of an objective (elites give twice the XP), the character's share in a group.
    pub fn mob_xp(self, level: i64, o: &Objective) -> f64 {
        let mut x = self.rules().mob_xp(level, o.mob_level, self.content(o)) * self.params.group_xp_share();
        if o.elite {
            x *= 2.0;
        }
        if o.dungeon.is_some() {
            x *= self.rules().dungeon_kill_xp;
        }
        x
    }

    /// Expansion of the content of an objective's mobs (Azeroth, Outland), for their XP.
    fn content(self, o: &Objective) -> usize {
        o.dungeon.map_or_else(
            || xp::content_at(o.loc.pos.continent, o.loc.pos.zone),
            |d| self.dungeons[d].content,
        )
    }

    /// XP per second grinding mobs of the character's level around `at`, `bonus` levels of
    /// power above it.
    pub fn grind_rate(self, level: i64, bonus: f64, at: &Pos) -> f64 {
        // Group: each kill is faster, its XP shared.
        let content = xp::content_at(at.continent, at.zone);
        self.rules().mob_xp(level, level, content) * self.params.group_xp_share()
            / (self.params.grind_kill_time * crate::params::power_speed(bonus) / self.params.group_damage())
    }
}
