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
}

/// Everything configurable on disk in `overrides/`.
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub races: BTreeMap<String, RaceDef>,
    pub class_quests: HashMap<String, Vec<String>>,
    /// Class quests that make the character stronger: quest name -> power gained, by class.
    pub class_powers: HashMap<String, Vec<(String, f64)>>,
    /// Class trainer spells that count for power, by class (spells.toml).
    pub spells: HashMap<String, BTreeMap<String, crate::power::SpellWeight>>,
    pub dungeons: Vec<DungeonDef>,
    pub links: Vec<LinkDef>,
    pub passes: Vec<crate::world::PassDef>,
    pub params: Params,
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

impl Overrides {
    pub fn load(dir: &Path) -> Result<Self> {
        Self::parse(|name| std::fs::read_to_string(dir.join(name)).ok())
    }

    /// Overrides of a game version: `dir/<edition>/<file>` when present, else `dir/<file>`.
    pub fn load_edition(dir: &Path, edition: crate::xp::Edition) -> Result<Self> {
        Self::parse_edition(|name| std::fs::read_to_string(dir.join(name)).ok(), edition)
    }

    /// Overrides of a game version from files by relative path: `<edition>/<file>` when
    /// present, else `<file>`; the edition's rules set in the parameters.
    pub fn parse_edition(file: impl Fn(&str) -> Option<String>, edition: crate::xp::Edition) -> Result<Self> {
        let mut o = Self::parse(|name| {
            edition
                .overrides_subdir()
                .and_then(|sub| file(&format!("{sub}/{name}")))
                .or_else(|| file(name))
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
        struct Travel {
            #[serde(default)]
            link: Vec<LinkDef>,
        }
        #[derive(Deserialize, Default)]
        struct Passes {
            #[serde(default)]
            pass: Vec<crate::world::PassDef>,
        }
        let class_quests: HashMap<String, ClassQuests> = parse_toml(&file, "class_quests.toml")?;
        Ok(Self {
            races: parse_toml(&file, "races.toml")?,
            class_powers: class_quests
                .iter()
                .map(|(k, v)| (k.clone(), v.power.clone().into_iter().collect()))
                .collect(),
            class_quests: class_quests.into_iter().map(|(k, v)| (k, v.required)).collect(),
            spells: parse_toml(&file, "spells.toml")?,
            dungeons: parse_toml::<Dungeons>(&file, "dungeons.toml")?.dungeon,
            links: parse_toml::<Travel>(&file, "travel.toml")?.link,
            passes: parse_toml::<Passes>(&file, "passes.toml")?.pass,
            params: parse_toml(&file, "routing.toml")?,
        })
    }
}
