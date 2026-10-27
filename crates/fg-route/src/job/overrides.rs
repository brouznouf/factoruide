//! The configuration files (`overrides/`): races, routing parameters, class quests, passes, links.

use crate::model::DungeonDef;
use crate::params::Params;
use crate::world::LinkDef;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaceDef {
    pub id: i64,
    pub faction: crate::faction::Faction,
    pub start_npc: i64,
    /// Flight paths a new character knows (TaxiNodes IDs): the game gives the capital's.
    #[serde(default)]
    pub start_flights: Vec<i64>,
}

/// Everything configurable on disk in `overrides/`.
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub races: BTreeMap<String, RaceDef>,
    pub class_quests: HashMap<String, Vec<String>>,
    /// Class quests that make the character stronger: quest name -> power gained, by class.
    pub class_powers: HashMap<String, Vec<(String, f64)>>,
    /// How each class fares while leveling (class_power.toml), by class.
    pub class_power: HashMap<String, ClassPower>,
    /// Class trainer spells that count for power, by class (spells.toml).
    pub spells: HashMap<String, BTreeMap<String, crate::power::SpellWeight>>,
    pub dungeons: Vec<DungeonDef>,
    /// Objectives done only once others of their quest are (`[[order]]` of objectives.toml).
    pub objective_orders: Vec<crate::model::ObjectiveOrder>,
    pub quest_overrides: Vec<crate::model::QuestOverride>,
    pub links: Vec<LinkDef>,
    /// Class teleports quests teach (`[[teleport]]` of travel.toml).
    pub teleports: Vec<crate::model::TeleportDef>,
    pub passes: Vec<crate::world::PassDef>,
    /// Areas left only by boat, portal or lift (`[[enclosed]]` of passes.toml).
    pub enclosed: Vec<crate::world::EnclosedDef>,
    /// Caves the terrain does not hold (`[[cave]]` of passes.toml).
    pub caves: Vec<crate::world::CaveDef>,
    /// Ways the game keeps closed (`[[closed]]` of passes.toml).
    pub closed: Vec<crate::world::ClosedDef>,
    pub params: Params,
}

/// A class's `[class]` of class_power.toml: levels of power over the average class by character
/// level, and how much gear counts for it.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ClassPower {
    #[serde(default)]
    pub curve: BTreeMap<String, f64>,
    pub gear: Option<f64>,
}

impl ClassPower {
    /// The (level, levels of power) points of the curve, by level.
    pub fn points(&self) -> Result<Vec<(i64, f64)>> {
        let mut points: Vec<(i64, f64)> = (self.curve.iter())
            .map(|(k, v)| {
                Ok((
                    k.parse::<i64>()
                        .with_context(|| format!("class_power.toml: level {k}"))?,
                    *v,
                ))
            })
            .collect::<Result<_>>()?;
        points.sort_by_key(|p| p.0);
        Ok(points)
    }

    /// The curve at every level from 0 to `top` (see `curve_by_level`).
    pub fn by_level(&self, top: i64) -> Result<Vec<f64>> {
        Ok(curve_by_level(&self.points()?, top))
    }
}

/// A curve given by (level, value) points at every level from 0 to `top`: straight lines
/// between the points, the first and last values beyond them (empty without points).
pub(crate) fn curve_by_level(points: &[(i64, f64)], top: i64) -> Vec<f64> {
    let mut points = points.to_vec();
    points.sort_by_key(|p| p.0);
    points.dedup_by_key(|p| p.0);
    if points.is_empty() {
        return Vec::new();
    }
    (0..=top)
        .map(|level| match points.iter().position(|p| p.0 >= level) {
            Some(0) => points[0].1,
            None => points[points.len() - 1].1,
            Some(i) => {
                let ((x0, y0), (x1, y1)) = (points[i - 1], points[i]);
                y0 + (y1 - y0) * (level - x0) as f64 / (x1 - x0) as f64
            }
        })
        .collect()
}

pub(super) fn parse_toml<T: serde::de::DeserializeOwned + Default>(
    file: &impl Fn(&str) -> Option<String>,
    name: &str,
) -> Result<T> {
    match file(name) {
        Some(text) => toml::from_str(&text).with_context(|| format!("parsing {name}")),
        None => Ok(T::default()),
    }
}

/// The text of `own` merged into `common` (TOML), None when one does not parse (then reported
/// on the two texts one after the other).
fn merged(common: &str, own: &str) -> Option<String> {
    let mut common: toml::Table = toml::from_str(common).ok()?;
    merge(&mut common, toml::from_str(own).ok()?);
    toml::to_string(&common).ok()
}

/// `own` into `common`: tables key by key (`[human]` of races.toml, settings of routing.toml),
/// `own`'s values over `common`'s, and lists of tables (`[[link]]`, `[[pass]]`) one after the
/// other: an edition's file adds to the one of every game version.
fn merge(common: &mut toml::Table, own: toml::Table) {
    for (key, value) in own {
        match (common.get_mut(&key), value) {
            (Some(toml::Value::Table(c)), toml::Value::Table(o)) => merge(c, o),
            (Some(toml::Value::Array(c)), toml::Value::Array(o)) if c.iter().chain(&o).all(toml::Value::is_table) => {
                c.extend(o);
            }
            (_, value) => {
                common.insert(key, value);
            }
        }
    }
}

impl Overrides {
    /// The flight paths a character of race `race_id` (ChrRaces ID) knows from its creation.
    pub fn start_flights(&self, race_id: i64) -> &[i64] {
        (self.races.values())
            .find(|r| r.id == race_id)
            .map_or(&[], |r| &r.start_flights)
    }

    pub fn load(dir: &Path) -> Result<Self> {
        Self::parse(|name| std::fs::read_to_string(dir.join(name)).ok())
    }

    /// Overrides of a game version: `dir/<file>` (every version's) with `dir/<edition>/<file>`.
    pub fn load_edition(dir: &Path, edition: crate::xp::Edition) -> Result<Self> {
        Self::parse_edition(|name| std::fs::read_to_string(dir.join(name)).ok(), edition)
    }

    /// Overrides of a game version from files by relative path: each file of every version
    /// (`<file>`) with the version's own (`<edition>/<file>`) merged in (`merge`); the edition's
    /// rules set in the parameters.
    pub fn parse_edition(file: impl Fn(&str) -> Option<String>, edition: crate::xp::Edition) -> Result<Self> {
        let mut o = Self::parse(|name| {
            let own = file(&format!("{}/{name}", edition.overrides_subdir()));
            match (file(name), own) {
                (Some(common), Some(own)) => Some(merged(&common, &own).unwrap_or_else(|| format!("{common}\n{own}"))),
                (common, own) => common.or(own),
            }
        })?;
        o.params.edition = edition;
        o.params.quest_log_size = edition.quest_log_size();
        o.params.mount_level = edition.mount_level();
        Ok(o)
    }

    /// Overrides from the text of each file (`races.toml`...), `None` when a file is absent.
    pub fn parse(file: impl Fn(&str) -> Option<String>) -> Result<Self> {
        #[derive(Deserialize, Default)]
        struct ClassQuests {
            #[serde(default)]
            required: Vec<String>,
            #[serde(default)]
            power: BTreeMap<String, f64>,
        }
        #[derive(Deserialize, Default)]
        struct Dungeons {
            #[serde(default)]
            dungeon: Vec<DungeonDef>,
        }
        #[derive(Deserialize, Default)]
        struct Objectives {
            #[serde(default)]
            order: Vec<crate::model::ObjectiveOrder>,
        }
        #[derive(Deserialize, Default)]
        struct Quests {
            #[serde(default)]
            quest: Vec<crate::model::QuestOverride>,
        }
        #[derive(Deserialize, Default)]
        struct Travel {
            #[serde(default)]
            link: Vec<LinkDef>,
            #[serde(default)]
            teleport: Vec<crate::model::TeleportDef>,
        }
        #[derive(Deserialize, Default)]
        struct Passes {
            #[serde(default)]
            pass: Vec<crate::world::PassDef>,
            #[serde(default)]
            enclosed: Vec<crate::world::EnclosedDef>,
            #[serde(default)]
            cave: Vec<crate::world::CaveDef>,
            #[serde(default)]
            closed: Vec<crate::world::ClosedDef>,
        }
        let class_quests: HashMap<String, ClassQuests> = parse_toml(&file, "class_quests.toml")?;
        let passes = parse_toml::<Passes>(&file, "passes.toml")?;
        let travel = parse_toml::<Travel>(&file, "travel.toml")?;
        Ok(Self {
            races: parse_toml(&file, "races.toml")?,
            class_powers: class_quests
                .iter()
                .map(|(k, v)| (k.clone(), v.power.clone().into_iter().collect()))
                .collect(),
            class_quests: class_quests.into_iter().map(|(k, v)| (k, v.required)).collect(),
            spells: parse_toml(&file, "spells.toml")?,
            class_power: parse_toml(&file, "class_power.toml")?,
            dungeons: parse_toml::<Dungeons>(&file, "dungeons.toml")?.dungeon,
            objective_orders: parse_toml::<Objectives>(&file, "objectives.toml")?.order,
            quest_overrides: parse_toml::<Quests>(&file, "quests.toml")?.quest,
            links: travel.link,
            teleports: travel.teleport,
            passes: passes.pass,
            enclosed: passes.enclosed,
            caves: passes.cave,
            closed: passes.closed,
            params: parse_toml(&file, "routing.toml")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ClassPower, Overrides};

    /// The class curve between the levels given is a straight line, flat beyond them; Forever's
    /// file changes some of the levels of a class and keeps the others.
    #[test]
    fn class_curve_by_level() {
        let power = ClassPower {
            curve: [("10".to_owned(), -1.0), ("20".to_owned(), 1.0)].into(),
            gear: None,
        };
        let curve = power.by_level(30).unwrap();
        assert_eq!(
            (curve[1], curve[10], curve[15], curve[20], curve[30]),
            (-1.0, -1.0, 0.0, 1.0, 1.0)
        );
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../overrides");
        let paladin = |e| {
            Overrides::load_edition(&dir, e).unwrap().class_power["paladin"]
                .by_level(60)
                .unwrap()
        };
        let (forever, classic) = (paladin(Edition::Forever), paladin(Edition::Classic));
        assert!(forever[15] > classic[15]);
        assert!((forever[55] - classic[55]).abs() < 1e-9);
    }
    use crate::xp::Edition;
    use std::path::Path;

    /// Forever has more quests: the shipped settings farm less on the way there than on Classic
    /// and TBC.
    #[test]
    fn farming_on_the_way_depends_on_the_edition() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../overrides");
        let cap = |e| Overrides::load_edition(&dir, e).unwrap().params.farm_cap;
        assert!(cap(Edition::Forever) < cap(Edition::Classic));
        assert!((cap(Edition::Classic) - cap(Edition::Tbc)).abs() < 1e-9);
    }

    /// The files of every game version hold what they share (races, the tram, the zeppelins);
    /// each version's folder adds its own (Forever's Skyborne, boats and Hall of Thanes, TBC's
    /// draenei and Dark Portal, Classic's straight Menethil Harbor boat), never seen by the others.
    #[test]
    fn each_edition_adds_its_own_to_the_shared_files() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../overrides");
        let load = |e| Overrides::load_edition(&dir, e).unwrap();
        let (forever, classic, tbc) = (load(Edition::Forever), load(Edition::Classic), load(Edition::Tbc));
        let link = |o: &Overrides, name: &str| o.links.iter().any(|l| l.name == name);
        for o in [&forever, &classic, &tbc] {
            assert!(o.races.contains_key("human") && o.races.contains_key("tauren"));
            assert!(link(o, "Deeprun Tram Ironforge - Stormwind"));
            assert_eq!(o.teleports.len(), 1);
        }
        assert!(forever.races.contains_key("skyborne") && !classic.races.contains_key("skyborne"));
        assert!(tbc.races.contains_key("draenei") && !forever.races.contains_key("draenei"));
        assert!(
            link(&forever, "Boat Stormwind Harbor - Auberdine") && !link(&classic, "Boat Stormwind Harbor - Auberdine")
        );
        assert!(
            !link(&forever, "Boat Menethil Harbor - Auberdine") && link(&classic, "Boat Menethil Harbor - Auberdine")
        );
        assert!(link(&tbc, "Dark Portal") && !link(&classic, "Dark Portal"));
        assert_eq!(classic.races.len(), 8);
        // Forever's new dungeon is not listed in the other versions (without quests nor levels there).
        let thanes = |o: &Overrides| o.dungeons.iter().any(|d| d.area == 16919);
        assert!(thanes(&forever) && !thanes(&classic) && !thanes(&tbc));
    }

    /// An edition's file adds its lists of tables to the shared one and sets its keys over it.
    #[test]
    fn edition_file_merged_into_the_shared_one() {
        let shared = "farm_cap = 0.1\nrun_speed = 7.0\n[[link]]\nname = \"a\"\n";
        let own = "farm_cap = 0.2\n[[link]]\nname = \"b\"\n";
        let merged: toml::Table = toml::from_str(&super::merged(shared, own).unwrap()).unwrap();
        assert_eq!(merged["farm_cap"].as_float(), Some(0.2));
        assert_eq!(merged["run_speed"].as_float(), Some(7.0));
        let names: Vec<&str> = merged["link"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["a", "b"]);
    }
}
