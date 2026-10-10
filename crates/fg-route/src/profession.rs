//! Professions: the skill a character leveling them should have at each level. The route
//! does not plan them (no trainer visit, no practice time): the addon shows whether the skill
//! is ahead or behind this curve, and the profession's quests are planned when the curve
//! reaches their skill (optional in the guide).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Gathering,
    Crafting,
    Secondary,
}

pub struct ProfessionDef {
    pub key: &'static str,
    /// The game's skill line (quests requiring the profession name it).
    pub skill_id: i64,
    pub name: &'static str,
    pub kind: Kind,
    /// Default skill to have at the character levels of `CURVE_LEVELS`.
    pub curve: [i64; 8],
}

/// Character levels of the default curves' points.
pub const CURVE_LEVELS: [i64; 8] = [10, 15, 20, 25, 30, 40, 50, 60];

/// Level at which a profession is usually taken (the apprentice rank needs level 5): the curve
/// starts from 0 there.
pub const START_LEVEL: i64 = 5;

/// Default curves, from classic leveling guides (icy-veins, warcrafttavern, wowhead): where
/// the materials of each skill range are found. The profession is taken around level 5, so the
/// curves start slowly and catch up between levels 10 and 20.
/// - Gathering runs ahead of 5 x level: zone nodes stay orange/yellow well above their
///   requirement (herbs: Briarthorn 70 and Mageroyal 50 in 10-20 zones, Liferoot 150 in
///   20-30 zones, Sungrass 230 in 40-50 zones; ores: Tin 65, Iron 125, Mithril 175,
///   Thorium 245). Skinning follows mob level x 5, slightly ahead below level 20.
/// - Crafting lags its materials: cloth by mob level (linen 5-15, wool 15-25, silk 25-35,
///   mageweave 35-45, runecloth 45+), bars from the ores mined, enchanting dust from the
///   greens found (rare early, so it starts slowly).
/// - Fishing: 1 catch per point up to ~100, then up to 10 per point near 300, and the
///   good fishing waters (STV, Tanaris, Feralas) need 130-205 skill: fast early, slow late.
/// - Cooking and First Aid: cheap early (vendor recipes, linen), slower late (artisan quests,
///   rarer meat and cloth).
pub const PROFESSIONS: [ProfessionDef; 12] = [
    ProfessionDef {
        key: "alchemy",
        skill_id: 171,
        name: "Alchemy",
        kind: Kind::Crafting,
        curve: [25, 70, 115, 145, 170, 215, 265, 300],
    },
    ProfessionDef {
        key: "blacksmithing",
        skill_id: 164,
        name: "Blacksmithing",
        kind: Kind::Crafting,
        curve: [20, 55, 100, 125, 150, 205, 260, 300],
    },
    ProfessionDef {
        key: "enchanting",
        skill_id: 333,
        name: "Enchanting",
        kind: Kind::Crafting,
        curve: [5, 35, 75, 110, 140, 195, 250, 300],
    },
    ProfessionDef {
        key: "engineering",
        skill_id: 202,
        name: "Engineering",
        kind: Kind::Crafting,
        curve: [20, 60, 100, 125, 150, 200, 255, 300],
    },
    ProfessionDef {
        key: "leatherworking",
        skill_id: 165,
        name: "Leatherworking",
        kind: Kind::Crafting,
        curve: [20, 60, 105, 130, 150, 200, 250, 300],
    },
    ProfessionDef {
        key: "tailoring",
        skill_id: 197,
        name: "Tailoring",
        kind: Kind::Crafting,
        curve: [20, 60, 105, 135, 160, 215, 265, 300],
    },
    ProfessionDef {
        key: "herbalism",
        skill_id: 182,
        name: "Herbalism",
        kind: Kind::Gathering,
        curve: [40, 90, 135, 165, 190, 240, 285, 300],
    },
    ProfessionDef {
        key: "mining",
        skill_id: 186,
        name: "Mining",
        kind: Kind::Gathering,
        curve: [30, 75, 115, 145, 170, 225, 275, 300],
    },
    ProfessionDef {
        key: "skinning",
        skill_id: 393,
        name: "Skinning",
        kind: Kind::Gathering,
        curve: [35, 75, 110, 135, 160, 210, 260, 300],
    },
    ProfessionDef {
        key: "cooking",
        skill_id: 185,
        name: "Cooking",
        kind: Kind::Secondary,
        curve: [35, 80, 125, 150, 175, 225, 265, 300],
    },
    ProfessionDef {
        key: "fishing",
        skill_id: 356,
        name: "Fishing",
        kind: Kind::Secondary,
        curve: [45, 95, 140, 165, 190, 230, 265, 300],
    },
    ProfessionDef {
        key: "first_aid",
        skill_id: 129,
        name: "First Aid",
        kind: Kind::Secondary,
        curve: [30, 75, 120, 150, 180, 240, 285, 300],
    },
];

/// Default milestones of a profession: its curve at the levels of `CURVE_LEVELS`.
pub fn default_milestones(key: &str) -> Vec<(i64, i64)> {
    find(key)
        .map(|d| CURVE_LEVELS.iter().copied().zip(d.curve).collect())
        .unwrap_or_default()
}

/// French name of a profession.
pub fn french_name(key: &str) -> Option<&'static str> {
    Some(match key {
        "alchemy" => "Alchimie",
        "blacksmithing" => "Forge",
        "enchanting" => "Enchantement",
        "engineering" => "Ingénierie",
        "herbalism" => "Herboristerie",
        "leatherworking" => "Travail du cuir",
        "mining" => "Minage",
        "skinning" => "Dépeçage",
        "tailoring" => "Couture",
        "cooking" => "Cuisine",
        "fishing" => "Pêche",
        "first_aid" => "Secourisme",
        _ => return None,
    })
}

pub fn find(key: &str) -> Option<&'static ProfessionDef> {
    PROFESSIONS.iter().find(|p| p.key == key)
}

/// A profession to level along the route.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfessionGoal {
    pub key: String,
    /// Skill to reach (up to 300).
    pub target: i64,
    /// Character level at which the profession is taken (default `START_LEVEL`).
    #[serde(default)]
    pub start_level: Option<i64>,
    /// Skill to reach by character level: (level, skill) points; default: the profession curve.
    #[serde(default)]
    pub milestones: Vec<(i64, i64)>,
}

/// Skill to have reached at a character level: nothing before `start` (the level the
/// profession is taken), then the goal's milestones (level, skill) linearly interpolated from 0
/// at `start`, or 5 skill per level since `start` without milestones (gathering, fishing and
/// the materials crafting needs all scale with zone level). Never above the goal.
pub fn target_at(goal_target: i64, milestones: &[(i64, i64)], start: i64, level: i64) -> i64 {
    if level <= start {
        return 0;
    }
    let curve = if milestones.is_empty() {
        5 * (level - start)
    } else {
        let mut sorted = milestones.to_vec();
        sorted.sort_unstable();
        let points: Vec<(i64, i64)> = sorted.iter().copied().filter(|(l, _)| *l > start).collect();
        match points.iter().position(|(l, _)| *l >= level) {
            Some(0) => points[0].1 * (level - start) / (points[0].0 - start).max(1),
            Some(i) => {
                let ((l0, s0), (l1, s1)) = (points[i - 1], points[i]);
                s0 + (s1 - s0) * (level - l0) / (l1 - l0).max(1)
            }
            None => sorted[sorted.len() - 1].1,
        }
    };
    curve.clamp(0, goal_target.min(300))
}

/// Skill line IDs, used by the addon to read the character's skill.
pub fn skill_line(key: &str) -> i64 {
    match key {
        "alchemy" => 171,
        "blacksmithing" => 164,
        "enchanting" => 333,
        "engineering" => 202,
        "herbalism" => 182,
        "leatherworking" => 165,
        "mining" => 186,
        "skinning" => 393,
        "tailoring" => 197,
        "cooking" => 185,
        "fishing" => 356,
        "first_aid" => 129,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_before_the_profession_is_taken() {
        assert_eq!(target_at(300, &[], 5, 4), 0);
        assert_eq!(target_at(300, &[], 5, 15), 50);
        assert_eq!(target_at(150, &[], 5, 60), 150);
        let mining = default_milestones("mining");
        assert_eq!(target_at(300, &mining, START_LEVEL, 5), 0);
        assert_eq!(target_at(300, &mining, START_LEVEL, 10), 30);
        assert_eq!(target_at(300, &mining, START_LEVEL, 20), 115);
    }

    #[test]
    fn milestones_interpolate() {
        let m = [(10, 75), (20, 150)];
        assert_eq!(target_at(300, &m, 0, 5), 37);
        assert_eq!(target_at(300, &m, 0, 15), 112);
        assert_eq!(target_at(300, &m, 0, 40), 150);
        // Taken at level 12: from 0 there to the next milestone.
        assert_eq!(target_at(300, &m, 12, 16), 75);
    }
}
