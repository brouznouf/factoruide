//! Experience rules of each game version.

use serde::{Deserialize, Serialize};

/// Game version a guide is made for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edition {
    /// WoW Forever (Classic rules on the modern client, new races).
    #[default]
    Forever,
    /// Classic Era and Hardcore (1.15).
    Classic,
    /// The Burning Crusade (Anniversary realms, 2.5).
    Tbc,
}

impl Edition {
    pub const ALL: [Edition; 3] = [Edition::Forever, Edition::Classic, Edition::Tbc];

    pub fn key(self) -> &'static str {
        match self {
            Edition::Forever => "forever",
            Edition::Classic => "classic",
            Edition::Tbc => "tbc",
        }
    }

    pub fn from_key(key: &str) -> Option<Edition> {
        Self::ALL.into_iter().find(|e| e.key().eq_ignore_ascii_case(key))
    }

    /// Database file in data/.
    pub fn db_file(self) -> &'static str {
        match self {
            Edition::Forever => "forever.sqlite",
            Edition::Classic => "classic.sqlite",
            Edition::Tbc => "tbc.sqlite",
        }
    }

    /// QuestieDB flavor.
    pub fn questie_flavor(self) -> &'static str {
        match self {
            Edition::Forever => "Forever",
            Edition::Classic => "Vanilla",
            Edition::Tbc => "TBC",
        }
    }

    /// Battle.net product of the game client.
    pub fn product(self) -> &'static str {
        match self {
            Edition::Forever => "wow_classic_beta",
            Edition::Classic => "wow_classic_era",
            Edition::Tbc => "wow_anniversary",
        }
    }

    /// Client folder in the WoW install.
    pub fn client_folder(self) -> &'static str {
        match self {
            Edition::Forever => "_classic_beta_",
            Edition::Classic => "_classic_era_",
            Edition::Tbc => "_anniversary_",
        }
    }

    /// Overrides folder (`overrides/<key>/`), its files replacing those of `overrides/`.
    pub fn overrides_subdir(self) -> Option<&'static str> {
        match self {
            Edition::Forever => None,
            e => Some(e.key()),
        }
    }

    /// Quests the log holds.
    pub fn quest_log_size(self) -> usize {
        match self {
            Edition::Forever => 40,
            Edition::Classic => 20,
            Edition::Tbc => 25,
        }
    }

    /// Level of the first riding skill (60% mount).
    pub fn mount_level(self) -> i64 {
        match self {
            Edition::Forever | Edition::Classic => 40,
            Edition::Tbc => 30,
        }
    }

    pub fn rules(self) -> &'static Rules {
        match self {
            Edition::Forever => &FOREVER,
            Edition::Classic => &CLASSIC,
            Edition::Tbc => &TBC,
        }
    }
}

/// XP needed to go from level `l` to `l + 1` (index 0 = level 1), by version.
const CLASSIC_XP: [i64; 59] = [
    400, 900, 1400, 2100, 2800, 3600, 4500, 5400, 6500, 7600, 8800, 10100, 11400, 12900, 14400, 16000, 17700, 19400,
    21300, 23200, 25200, 27300, 29400, 31700, 34000, 36400, 38900, 41400, 44300, 47400, 50800, 54500, 58600, 62800,
    67100, 71600, 76100, 80800, 85700, 90700, 95800, 101000, 106300, 111800, 117500, 123200, 129100, 135100, 141200,
    147500, 153900, 160400, 167100, 173900, 180800, 187900, 195000, 202300, 209800,
];

/// The Burning Crusade 2.4 (CMaNGOS `player_xp_for_level`, from sniffs).
const TBC_XP: [i64; 69] = [
    400, 900, 1400, 2100, 2800, 3600, 4500, 5400, 6500, 7600, 8700, 9800, 11000, 12300, 13600, 15000, 16400, 17800,
    19300, 20800, 22400, 24000, 25500, 27200, 28900, 30500, 32200, 33900, 36300, 38800, 41600, 44600, 48000, 51400,
    55000, 58700, 62400, 66200, 70200, 74300, 78500, 82800, 87100, 91600, 96300, 101000, 105800, 110700, 115700,
    120900, 126100, 131500, 137000, 142500, 148200, 154000, 159900, 165800, 172000, 494000, 574700, 614400, 650300,
    682300, 710200, 734100, 753700, 768900, 779700,
];

pub struct Rules {
    pub edition_name: &'static str,
    pub max_level: i64,
    to_level: &'static [i64],
    /// Base XP of a kill is `level * 5 + base`, `base` by the expansion of the mob's zone
    /// (Azeroth, Outland...).
    kill_base: &'static [f64],
    /// Factor on the XP of the mobs killed in a 5-man dungeon.
    pub dungeon_kill_xp: f64,
    /// Factor on the XP of the quests done in a 5-man dungeon (their database value is Classic's).
    pub dungeon_quest_xp: f64,
}

static CLASSIC: Rules = Rules {
    edition_name: "Classic",
    max_level: 60,
    to_level: &CLASSIC_XP,
    kill_base: &[45.0],
    dungeon_kill_xp: 1.0,
    dungeon_quest_xp: 1.0,
};
/// Classic rules, except dungeons: their kills give much less XP and their quests much more, so
/// that players level in the open world (beta, October 2026, values still being tuned):
/// - kills: 25% of Classic (measured in the Deadmines: 28 → 7 XP, elites 80 → 20), then +20%
///   announced on October 8;
/// - quests: Classic XP + a bonus of 1.375 times it since October 1 (it was 2.75; Edwin
///   VanCleef: 2600 XP in Classic, 6175 in Forever).
static FOREVER: Rules = Rules {
    edition_name: "WoW Forever",
    dungeon_kill_xp: 0.25 * 1.2,
    dungeon_quest_xp: 2.375,
    ..CLASSIC
};
static TBC: Rules = Rules {
    edition_name: "The Burning Crusade",
    max_level: 70,
    to_level: &TBC_XP,
    kill_base: &[45.0, 235.0],
    dungeon_kill_xp: 1.0,
    dungeon_quest_xp: 1.0,
};

impl Rules {
    /// Whether the level curve is known (the edition can be planned).
    pub fn known(&self) -> bool {
        self.to_level.len() as i64 >= self.max_level - 1
    }

    pub fn to_next_level(&self, level: i64) -> i64 {
        if level >= self.max_level {
            return i64::MAX;
        }
        self.to_level
            .get((level.max(1) - 1) as usize)
            .copied()
            .unwrap_or(i64::MAX)
    }

    /// Highest level of the content gray for a character of `level` (no XP).
    pub fn gray_level(level: i64) -> i64 {
        match level {
            l if l <= 5 => 0,
            l if l <= 39 => l - 5 - l / 10,
            l if l <= 59 => l - 1 - l / 5,
            l => l - 9,
        }
    }

    fn zero_difference(level: i64) -> f64 {
        match level {
            l if l <= 7 => 5.0,
            8..=9 => 6.0,
            10..=11 => 7.0,
            12..=15 => 8.0,
            16..=19 => 9.0,
            20..=29 => 11.0,
            30..=39 => 12.0,
            40..=44 => 13.0,
            45..=49 => 14.0,
            50..=54 => 15.0,
            55..=59 => 16.0,
            _ => 17.0,
        }
    }

    /// XP for killing a normal mob of `mob` level, in a zone of expansion `content` (0 =
    /// Azeroth, 1 = Outland...).
    pub fn mob_xp(&self, level: i64, mob: i64, content: usize) -> f64 {
        let base = (level * 5) as f64 + self.kill_base[content.min(self.kill_base.len() - 1)];
        if mob >= level {
            base * (1.0 + 0.05 * (mob - level).min(4) as f64)
        } else if mob <= Self::gray_level(level) {
            0.0
        } else {
            base * (1.0 - (level - mob) as f64 / Self::zero_difference(level))
        }
    }

    /// XP for discovering an area of `area` level at `level` (as the game computes it: capped
    /// above, reduced by 5% per level beyond 5 levels over the area).
    pub fn exploration_xp(&self, level: i64, area: i64) -> i64 {
        if level >= self.max_level || area <= 0 {
            return 0;
        }
        let base = |l: i64| EXPLORATION_BASE[l.clamp(0, 60) as usize];
        let diff = level - area;
        if diff < -5 {
            base(level + 5)
        } else if diff > 5 {
            base(area) * (100 - (diff - 5) * 5).clamp(0, 100) / 100
        } else {
            base(area)
        }
    }
}

/// Zones of the Outland map (530) that are Azeroth content: the blood elf and draenei starting
/// zones and their cities (the game files them under a virtual Azeroth map).
const AZEROTH_ON_OUTLAND_MAP: [i64; 6] = [
    3430, // Eversong Woods
    3433, // Ghostlands
    3487, // Silvermoon City
    3524, // Azuremyst Isle
    3525, // Bloodmyst Isle
    3557, // The Exodar
];

/// Expansion of the content at a place of the open world (game map and zone), for kill XP: 1 in
/// Outland, 0 in Azeroth.
pub fn content_at(map: i64, zone: i64) -> usize {
    usize::from(map == 530 && !AZEROTH_ON_OUTLAND_MAP.contains(&zone))
}

/// Quest XP after the level difference penalty.
pub fn quest_xp(base: i64, quest_level: i64, level: i64) -> i64 {
    let factor = match level - quest_level {
        d if d <= 5 => 1.0,
        6 => 0.8,
        7 => 0.6,
        8 => 0.4,
        9 => 0.2,
        _ => 0.1,
    };
    // The game rounds rewards to multiples of 5.
    ((base as f64 * factor / 5.0).round() * 5.0) as i64
}

/// Base XP for discovering an area, by area level (the game's `exploration_basexp` table).
const EXPLORATION_BASE: [i64; 61] = [
    0, 5, 15, 25, 35, 45, 55, 65, 70, 80, 85, 90, 90, 90, 100, 105, 115, 125, 135, 145, 155, 165, 175, 185, 195, 200,
    210, 220, 230, 240, 245, 250, 255, 265, 270, 275, 280, 285, 285, 300, 315, 330, 345, 360, 375, 390, 405, 420, 440,
    455, 470, 490, 510, 530, 540, 560, 580, 600, 620, 640, 660,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn penalties() {
        let r = Edition::Classic.rules();
        assert_eq!(quest_xp(1000, 10, 15), 1000);
        assert_eq!(quest_xp(1000, 10, 17), 600);
        assert_eq!(quest_xp(1000, 10, 25), 100);
        assert!((r.mob_xp(10, 10, 0) - 95.0).abs() < 1e-9);
        assert!(r.mob_xp(10, 3, 0).abs() < 1e-9);
    }

    #[test]
    fn forever_dungeons() {
        let (forever, classic) = (Edition::Forever.rules(), Edition::Classic.rules());
        assert!((forever.dungeon_kill_xp - 0.3).abs() < 1e-9);
        assert!((forever.dungeon_quest_xp - 2.375).abs() < 1e-9);
        assert!((classic.dungeon_kill_xp - 1.0).abs() < 1e-9);
        assert_eq!(forever.to_next_level(30), classic.to_next_level(30));
    }

    #[test]
    fn tbc() {
        let r = Edition::Tbc.rules();
        assert!(r.known());
        assert_eq!(r.to_next_level(60), 494000);
        assert_eq!(r.to_next_level(70), i64::MAX);
        assert!((r.mob_xp(62, 62, 1) - 545.0).abs() < 1e-9);
    }

    /// The blood elf and draenei starting zones are on the Outland map, but their mobs give the
    /// XP of Azeroth's: 50 for a level 1 mob at level 1, not 240.
    #[test]
    fn starting_zones_on_the_outland_map() {
        let r = Edition::Tbc.rules();
        let eversong = content_at(530, 3430);
        assert!((r.mob_xp(1, 1, eversong) - 50.0).abs() < 1e-9);
        let azuremyst = content_at(530, 3524);
        assert!((r.mob_xp(10, 10, azuremyst) - 95.0).abs() < 1e-9);
        let hellfire = content_at(530, 3483);
        assert!((r.mob_xp(60, 60, hellfire) - 535.0).abs() < 1e-9);
        assert_eq!(content_at(0, 12), 0);
    }
}
