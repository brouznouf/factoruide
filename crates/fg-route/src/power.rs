//! Character power: how fast the character fights compared with its level alone.
//!
//! Power is counted in levels: `level + gear + spells`, each level of power making fights 10%
//! faster (0.9 of the time). It does not replace the level against the mobs: the level difference
//! keeps its weight in the kill time, and which mobs are too high to fight still follows the level
//! (gear removes neither misses and resists nor the danger of higher mobs). XP follows the level.
//!
//! - Gear: what quest rewards and dungeon loot bring over a floor of ordinary gear for the level
//!   (vendor items, drops: `gear_floor` times the typical uncommon item). Items are valued with a
//!   simple fight model of the class (damage, health, mana), relative to the character's power.
//! - Spells: class trainer spells (overrides/spells.toml) the character has not learned yet, or
//!   learned at a lower rank than the trainer sells, cost their share of power.
//!
//! A level is worth 10% of fight speed (the kill time table: a mob one level below dies in 0.9
//! of the time), so a 10% better character gains one level of power.

use rusqlite::Connection;
use std::collections::HashMap;

/// Fight speed a level of power is worth: ln(1 / 0.9).
pub const LEVEL_LN: f64 = 0.105_360_5;

// Stats of an item (`item_stats` columns), in this order.
const STR: usize = 0;
const AGI: usize = 1;
const STA: usize = 2;
const INT: usize = 3;
const SPI: usize = 4;
const ARMOR: usize = 5;
const AP: usize = 6;
const RAP: usize = 7;
const SP: usize = 8;
const CRIT: usize = 10;
const SPELL_CRIT: usize = 11;
const HIT: usize = 12;
const MP5: usize = 13;
/// Damage per second of a melee weapon, a bow/gun/crossbow, a wand.
const DPS: usize = 14;
const RDPS: usize = 15;
const WAND: usize = 16;
const STATS: usize = 17;

/// Equipment slots.
pub const SLOTS: usize = 18;
const FINGER: usize = 10;
const TRINKET: usize = 12;
const MAIN_HAND: usize = 14;
const OFF_HAND: usize = 15;
const TWO_HAND: usize = 16;
const RANGED: usize = 17;

/// Where an item goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fit {
    Slot(usize),
    /// Either of two slots (rings, trinkets).
    Pair(usize),
    /// Main hand, or off hand for a dual wielder.
    OneHand,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub id: i64,
    pub name: String,
    fit: Fit,
    stats: [f32; STATS],
    required_level: i64,
    /// Level from which the class can wear it (mail and plate at 40).
    wear_level: i64,
}

/// What the character wears: per slot, the item owned and possibly a better one owned with
/// some chance (dungeon loot), as (item index + 1, chance in %); 0 = nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Gear {
    slots: [[(u16, u8); 2]; SLOTS],
}

/// A class trainer spell rank.
#[derive(Debug, Clone)]
pub struct Rank {
    /// Spell ID (the first of the rank).
    pub id: i64,
    pub name: String,
    pub rank: i64,
    pub level: i64,
    pub cost: i64,
    value: f64,
    weapon: bool,
    cast_time: f64,
}

/// A spell family that counts (overrides/spells.toml), its ranks by level.
#[derive(Debug, Clone)]
struct Family {
    weight: f64,
    ranks: bool,
    list: Vec<Rank>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(untagged)]
pub enum SpellWeight {
    #[default]
    None,
    Weight(f64),
    Full {
        weight: f64,
        #[serde(default = "yes")]
        ranks: bool,
    },
}

fn yes() -> bool {
    true
}

impl SpellWeight {
    pub fn weight(&self) -> f64 {
        match self {
            SpellWeight::None => 0.0,
            SpellWeight::Weight(w) | SpellWeight::Full { weight: w, .. } => *w,
        }
    }

    pub fn ranks(&self) -> bool {
        match self {
            SpellWeight::Full { ranks, .. } => *ranks,
            _ => true,
        }
    }
}

pub struct PowerModel {
    pub items: Vec<Item>,
    item_index: HashMap<i64, u16>,
    /// Stat weights by level (share of power a point brings).
    weights: Vec<[f64; STATS]>,
    /// Value of the floor gear by level and slot.
    floor: Vec<[f64; SLOTS]>,
    dual_wield: bool,
    families: Vec<Family>,
    /// `spells[trained][level]`: power factor of the spells learned at level `trained`
    /// compared with all those the trainer sells at `level`.
    spells: Vec<Vec<f64>>,
    gear_floor: f64,
}

/// Base stats of the class at a level.
#[derive(Debug, Clone, Copy, Default)]
struct Base {
    str: f64,
    agi: f64,
    sta: f64,
    int: f64,
    hp: f64,
    mana: f64,
    agi_per_crit: f64,
}

/// How a class fights.
#[derive(Debug, Clone, Copy)]
enum Style {
    /// Melee: attack power per strength and agility, weight of weapon damage.
    Melee {
        ap_str: f64,
        ap_agi: f64,
        weapon: f64,
    },
    Ranged,
    Caster,
}

/// Fight model of a class: style, share of damage, health and mana in its power, armor per
/// level of its usual armor type.
fn class_model(class: i64) -> (Style, [f64; 3], f64) {
    let melee = |ap_str, ap_agi, weapon| Style::Melee { ap_str, ap_agi, weapon };
    match class {
        1 => (melee(2.0, 0.0, 1.0), [0.85, 0.15, 0.0], 14.0),
        2 => (melee(2.0, 0.0, 1.0), [0.7, 0.15, 0.15], 14.0),
        4 => (melee(1.0, 1.0, 1.0), [0.85, 0.15, 0.0], 6.0),
        7 => (melee(2.0, 0.0, 1.0), [0.6, 0.15, 0.25], 8.0),
        11 => (melee(2.0, 0.0, 0.2), [0.65, 0.15, 0.2], 6.0),
        3 => (Style::Ranged, [0.7, 0.15, 0.15], 8.0),
        _ => (Style::Caster, [0.55, 0.15, 0.3], 3.0),
    }
}

/// Weapon damage per second of the floor gear (two-handed, or a bow for hunters).
fn floor_weapon(level: f64, gear_floor: f64, ranged: bool) -> f64 {
    if ranged {
        gear_floor * (1.0 + 0.55 * level)
    } else {
        gear_floor * (1.5 + 0.75 * level)
    }
}

/// Share of power one point of each stat brings at `level` (damage, health and mana, each
/// relative to the character's own at that level).
fn stat_weights(class: i64, base: &Base, level: i64, gear_floor: f64) -> [f64; STATS] {
    let l = level as f64;
    let (style, [a_dmg, a_hp, a_mana], armor_per_level) = class_model(class);
    let mut w = [0.0; STATS];
    let apc = base.agi_per_crit.max(1.0);
    match style {
        Style::Melee { ap_str, ap_agi, weapon } => {
            let ap = (3.0 * l + ap_str * base.str + ap_agi * base.agi - 20.0).max(10.0);
            let crit = 1.0 + (5.0 + base.agi / apc) / 100.0;
            let hit = floor_weapon(l, gear_floor, false) * weapon + ap / 14.0;
            let dmg = hit * crit;
            w[DPS] = a_dmg * crit * weapon / dmg;
            w[AP] = a_dmg * crit / 14.0 / dmg;
            w[STR] = ap_str * w[AP];
            w[AGI] = ap_agi * w[AP] + a_dmg * hit / (100.0 * apc) / dmg;
            w[CRIT] = a_dmg * hit / 100.0 / dmg;
            w[HIT] = w[CRIT];
            w[SP] = 0.1 * a_mana / (1.0 + l);
        }
        Style::Ranged => {
            let rap = (2.0 * l + 2.0 * base.agi - 10.0).max(10.0);
            let crit = 1.0 + (5.0 + base.agi / apc) / 100.0;
            let hit = floor_weapon(l, gear_floor, true) + rap / 14.0;
            let dmg = hit * crit;
            w[RDPS] = a_dmg * crit / dmg;
            w[RAP] = a_dmg * crit / 14.0 / dmg;
            w[AGI] = 2.0 * w[RAP] + a_dmg * hit / (100.0 * apc) / dmg;
            w[CRIT] = a_dmg * hit / 100.0 / dmg;
            w[HIT] = w[CRIT];
        }
        Style::Caster => {
            // Damage per second of the class's main spell at the level, without gear.
            let dmg = 3.0 + 0.6 * l + 0.045 * l * l;
            let int_per_crit = 10.0 + 0.83 * l;
            w[SP] = a_dmg / 3.5 / dmg;
            w[SPELL_CRIT] = a_dmg * 0.5 / 100.0;
            w[INT] = w[SPELL_CRIT] / int_per_crit;
            w[HIT] = a_dmg / 100.0;
            // Wands finish the mobs (a third of the time or so).
            w[WAND] = a_dmg * 0.3 / dmg;
        }
    }
    let hp = (base.hp + 10.0 * (base.sta - 20.0).max(0.0) + 20.0).max(20.0);
    w[STA] = a_hp * 10.0 / hp;
    w[ARMOR] = a_hp / (400.0 + 85.0 * l + armor_per_level * l + 2.0 * base.agi);
    if a_mana > 0.0 {
        let mana = (base.mana + 15.0 * (base.int - 20.0).max(0.0) + 20.0).max(20.0);
        w[INT] += a_mana * 15.0 / mana;
        w[SPI] = a_mana * 4.0 / mana;
        w[MP5] = a_mana * 12.0 / mana;
    }
    w
}

/// Whether a class wears an item, and from which level (mail, plate).
fn wears(class: i64, item_class: i64, subclass: i64) -> Option<i64> {
    let any = |list: &[i64]| list.contains(&subclass).then_some(1);
    match item_class {
        4 => match (subclass, class) {
            // Misc and cloth, leather, mail, shields.
            (0 | 1, _) | (2, 1 | 2 | 3 | 4 | 7 | 11) | (3, 1 | 2) | (6, 1 | 2 | 7) => Some(1),
            (3, 3 | 7) | (4, 1 | 2) => Some(40),
            _ => None,
        },
        2 => match class {
            1 => any(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 13, 15, 16, 18]),
            2 => any(&[0, 1, 4, 5, 6, 7, 8]),
            3 => any(&[0, 1, 2, 3, 6, 7, 8, 10, 13, 15, 16, 18]),
            4 => any(&[2, 3, 4, 7, 13, 15, 16, 18]),
            5 => any(&[4, 10, 15, 19]),
            7 => any(&[0, 1, 4, 5, 10, 13, 15]),
            8 | 9 => any(&[7, 10, 15, 19]),
            11 => any(&[4, 5, 10, 13, 15]),
            _ => None,
        },
        _ => None,
    }
}

/// Slot of an inventory type (shirts, tabards, ammo, relics: none).
fn fit_of(inventory_type: i64) -> Option<Fit> {
    Some(match inventory_type {
        1 => Fit::Slot(0),
        2 => Fit::Slot(1),
        3 => Fit::Slot(2),
        16 => Fit::Slot(3),
        5 | 20 => Fit::Slot(4),
        9 => Fit::Slot(5),
        10 => Fit::Slot(6),
        6 => Fit::Slot(7),
        7 => Fit::Slot(8),
        8 => Fit::Slot(9),
        11 => Fit::Pair(FINGER),
        12 => Fit::Pair(TRINKET),
        13 => Fit::OneHand,
        21 => Fit::Slot(MAIN_HAND),
        14 | 22 | 23 => Fit::Slot(OFF_HAND),
        17 => Fit::Slot(TWO_HAND),
        15 | 25 | 26 => Fit::Slot(RANGED),
        _ => return None,
    })
}

impl PowerModel {
    /// Load the class's power data. `None` when the database has none (TBC).
    #[expect(clippy::too_many_lines, reason = "items, floor, then spells, in load order")]
    pub fn load(
        conn: &Connection,
        class: i64,
        race: i64,
        max_level: i64,
        spell_weights: &[(String, SpellWeight)],
        params: &crate::Params,
    ) -> rusqlite::Result<Option<Self>> {
        let has: i64 = conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name IN ('m_item_stats', 'm_class_spell', 'm_player_stats')",
            [],
            |r| r.get(0),
        )?;
        if has < 3 {
            return Ok(None);
        }
        let top = max_level.max(1) as usize + 1;

        // Base stats by level: the race's, else the class average (races the data lacks).
        let mut bases = vec![Base::default(); top];
        let mut stmt = conn.prepare(
            "SELECT level, avg(str), avg(agi), avg(sta), avg(int), avg(hp), avg(mana), avg(agi_per_crit),
                    max(race = ?2)
             FROM m_player_stats WHERE class = ?1 AND (race = ?2 OR NOT EXISTS
                 (SELECT 1 FROM m_player_stats WHERE class = ?1 AND race = ?2))
             GROUP BY level",
        )?;
        let mut rows = stmt.query([class, race])?;
        let mut found = false;
        while let Some(r) = rows.next()? {
            let level: i64 = r.get(0)?;
            if level < 1 || level as usize >= top {
                continue;
            }
            found = true;
            bases[level as usize] = Base {
                str: r.get(1)?,
                agi: r.get(2)?,
                sta: r.get(3)?,
                int: r.get(4)?,
                hp: r.get(5)?,
                mana: r.get(6)?,
                agi_per_crit: r.get(7)?,
            };
        }
        if !found {
            return Ok(None);
        }
        for l in 1..top {
            if bases[l].hp == 0.0 {
                bases[l] = bases[l - 1];
            }
        }
        bases[0] = bases[1];
        let gear_floor = params.gear_floor;
        let weights: Vec<[f64; STATS]> = (0..top)
            .map(|l| stat_weights(class, &bases[l], l as i64, gear_floor))
            .collect();

        // Items the class can wear.
        let mut items = Vec::new();
        let mut uncommon = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT s.id, quality, inventory_type, s.class, s.subclass, allowable_class, s.required_level, str, agi,
                    sta, int, spi, armor, ap, rap, sp, heal, crit, spell_crit, hit, mp5, dps, coalesce(n.name, '')
             FROM m_item_stats s LEFT JOIN m_item n ON n.id = s.id WHERE quality >= 1",
        )?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            let allowable: i64 = r.get(5)?;
            if allowable > 0 && allowable & (1 << (class - 1)) == 0 {
                continue;
            }
            let (item_class, subclass): (i64, i64) = (r.get(3)?, r.get(4)?);
            let Some(wear_level) = wears(class, item_class, subclass) else {
                continue;
            };
            let Some(fit) = fit_of(r.get(2)?) else { continue };
            let mut stats = [0.0f32; STATS];
            for (k, stat) in stats.iter_mut().enumerate().take(MP5 + 1) {
                *stat = r.get::<_, f64>(7 + k)? as f32;
            }
            let dps = r.get::<_, f64>(21)? as f32;
            match (item_class, subclass) {
                (2, 2 | 3 | 18) => stats[RDPS] = dps,
                (2, 19) => stats[WAND] = dps,
                (2, 16) => {}
                _ => stats[DPS] = dps,
            }
            let quality: i64 = r.get(1)?;
            let item = Item {
                id: r.get(0)?,
                name: r.get(22)?,
                fit,
                stats,
                required_level: r.get(6)?,
                wear_level,
            };
            if quality == 2 {
                uncommon.push(items.len());
            }
            items.push(item);
        }
        let dual_wield = matches!(class, 4);
        let item_index = items.iter().enumerate().map(|(i, it)| (it.id, i as u16)).collect();
        let mut model = Self {
            items,
            item_index,
            weights,
            floor: vec![[0.0; SLOTS]; top],
            dual_wield,
            families: Vec::new(),
            spells: Vec::new(),
            gear_floor,
        };

        // Floor: `gear_floor` times the median uncommon item of the last levels, per slot.
        for level in 1..top {
            let mut by_slot: [Vec<f64>; SLOTS] = Default::default();
            for &i in &uncommon {
                let item = &model.items[i];
                let r = item.required_level.max(1);
                if r > level as i64 || r + 6 < level as i64 || item.wear_level > level as i64 {
                    continue;
                }
                let slots: &[usize] = match item.fit {
                    Fit::Slot(s) | Fit::Pair(s) => SINGLE[s],
                    Fit::OneHand if dual_wield => &[MAIN_HAND, OFF_HAND],
                    Fit::OneHand => &[MAIN_HAND],
                };
                for &s in slots {
                    by_slot[s].push(model.value_in(i, level as i64, s));
                }
            }
            for (s, values) in by_slot.iter_mut().enumerate() {
                if values.is_empty() {
                    continue;
                }
                values.sort_by(f64::total_cmp);
                model.floor[level][s] = gear_floor * values[values.len() / 2];
            }
            for s in [FINGER, TRINKET] {
                model.floor[level][s + 1] = model.floor[level][s];
            }
        }

        // Spells.
        let mut families: HashMap<String, Family> = HashMap::new();
        let mut stmt = conn.prepare(
            "SELECT name, rank, min(level), min(cost), max(value), max(weapon), max(cast_time), min(spell_id)
             FROM m_class_spell
             WHERE class_id = ?1 GROUP BY name, rank ORDER BY min(level)",
        )?;
        let mut rows = stmt.query([class])?;
        while let Some(r) = rows.next()? {
            let name: String = r.get(0)?;
            let Some((_, w)) = spell_weights.iter().find(|(n, _)| *n == name) else {
                continue;
            };
            if w.weight() <= 0.0 {
                continue;
            }
            let family = families.entry(name.clone()).or_insert_with(|| Family {
                weight: w.weight(),
                ranks: w.ranks(),
                list: Vec::new(),
            });
            family.list.push(Rank {
                id: r.get(7)?,
                name,
                rank: r.get(1)?,
                level: r.get(2)?,
                cost: r.get(3)?,
                value: r.get(4)?,
                weapon: r.get::<_, i64>(5)? != 0,
                cast_time: r.get(6)?,
            });
        }
        let mut families: Vec<Family> = families.into_values().collect();
        families.sort_by(|a, b| a.list[0].name.cmp(&b.list[0].name));
        for f in &mut families {
            f.list.sort_by_key(|r| r.level);
            if !f.ranks {
                f.list.truncate(1);
            }
        }
        model.spells = (0..top)
            .map(|trained| {
                (0..top)
                    .map(|level| spell_factor(&families, trained.min(level) as i64, level as i64))
                    .collect()
            })
            .collect();
        model.families = families;
        Ok(Some(model))
    }

    pub fn item(&self, id: i64) -> Option<u16> {
        self.item_index.get(&id).copied()
    }

    /// English name of an item.
    pub fn item_name(&self, id: i64) -> &str {
        self.item(id).map_or("", |i| &self.items[i as usize].name)
    }

    /// Value of item `i` worn in `slot` at `level` (share of power).
    fn value_in(&self, i: usize, level: i64, slot: usize) -> f64 {
        let item = &self.items[i];
        if level < item.required_level || level < item.wear_level {
            return 0.0;
        }
        let w = &self.weights[(level.max(0) as usize).min(self.weights.len() - 1)];
        let mut v: f64 = item.stats.iter().zip(w).map(|(s, w)| f64::from(*s) * w).sum();
        if slot == OFF_HAND && item.stats[DPS] > 0.0 {
            // Off-hand weapons hit for half.
            v -= 0.5 * f64::from(item.stats[DPS]) * w[DPS];
        }
        v
    }

    /// Expected value of a slot.
    fn slot_value(&self, gear: &Gear, slot: usize, level: i64) -> f64 {
        let [(a, _), (b, pb)] = gear.slots[slot];
        let va = if a == 0 {
            0.0
        } else {
            self.value_in(a as usize - 1, level, slot)
        };
        if b == 0 {
            return va;
        }
        let vb = self.value_in(b as usize - 1, level, slot);
        let p = f64::from(pb) / 100.0;
        p * vb.max(va) + (1.0 - p) * va
    }

    /// Power the gear brings over the floor gear, in levels.
    pub fn gear_bonus(&self, gear: &Gear, level: i64) -> f64 {
        let floor = &self.floor[(level.max(0) as usize).min(self.floor.len() - 1)];
        let mut gain = 0.0;
        for (slot, f) in floor.iter().enumerate().take(MAIN_HAND) {
            gain += (self.slot_value(gear, slot, level) - f).max(0.0);
        }
        let weapons = self
            .slot_value(gear, TWO_HAND, level)
            .max(self.slot_value(gear, MAIN_HAND, level) + self.slot_value(gear, OFF_HAND, level));
        let floor_weapons = floor[TWO_HAND].max(floor[MAIN_HAND] + floor[OFF_HAND]);
        gain += (weapons - floor_weapons).max(0.0);
        gain += (self.slot_value(gear, RANGED, level) - floor[RANGED]).max(0.0);
        (1.0 + gain).ln() / LEVEL_LN
    }

    /// Slots item `i` can go to.
    fn slots_of(&self, i: usize) -> &'static [usize] {
        match self.items[i].fit {
            Fit::Slot(s) => SINGLE[s],
            Fit::Pair(FINGER) => &[FINGER, FINGER + 1],
            Fit::Pair(_) => &[TRINKET, TRINKET + 1],
            Fit::OneHand if self.dual_wield => &[MAIN_HAND, OFF_HAND],
            Fit::OneHand => &[MAIN_HAND],
        }
    }

    /// Gear gain of wearing item `i` (owned with `chance`), in levels; the slot it would take.
    fn gain_of(&self, gear: &Gear, i: usize, chance: f64, level: i64) -> (f64, usize) {
        let mut best = (0.0, usize::MAX);
        let level = level.max(self.items[i].required_level);
        let before = self.gear_bonus(gear, level);
        for &slot in self.slots_of(i) {
            let mut g = *gear;
            put(&mut g, slot, i, chance, |k| self.value_in(k, level, slot));
            let gain = self.gear_bonus(&g, level) - before;
            if gain > best.0 + 1e-9 {
                best = (gain, slot);
            }
        }
        best
    }

    /// Wear item `i` if it is an upgrade (owned with `chance`, 1 = for sure). Returns whether
    /// the gear changed.
    pub fn equip(&self, gear: &mut Gear, i: u16, chance: f64, level: i64) -> bool {
        let i = i as usize;
        let (gain, slot) = self.gain_of(gear, i, chance, level);
        if gain <= 0.0 || slot == usize::MAX {
            return false;
        }
        let level = level.max(self.items[i].required_level);
        put(gear, slot, i, chance, |k| self.value_in(k, level, slot));
        true
    }

    /// Gear gain of the best of a quest's reward choices, and its index in `choices`.
    pub fn best_choice(&self, gear: &Gear, choices: &[u16], level: i64) -> Option<(usize, f64)> {
        choices
            .iter()
            .enumerate()
            .map(|(k, &i)| (k, self.gain_of(gear, i as usize, 1.0, level).0))
            .filter(|(_, g)| *g > 0.0)
            .max_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Rough gear gain of wearing item `i` at `level`, in levels: its value over what its best
    /// slot holds (or the floor), without rebuilding the gear (for the route construction).
    pub fn quick_gain(&self, gear: &Gear, i: u16, level: i64) -> f64 {
        let i = i as usize;
        let level = level.max(self.items[i].required_level);
        let floor = &self.floor[(level.max(0) as usize).min(self.floor.len() - 1)];
        let held = |slot: usize| self.slot_value(gear, slot, level).max(floor[slot]);
        let best = match self.items[i].fit {
            Fit::Slot(TWO_HAND) => {
                let now = held(TWO_HAND).max(held(MAIN_HAND) + held(OFF_HAND));
                self.value_in(i, level, TWO_HAND) - now
            }
            _ => self
                .slots_of(i)
                .iter()
                .map(|&slot| {
                    let mut gain = self.value_in(i, level, slot) - held(slot);
                    if matches!(slot, MAIN_HAND | OFF_HAND) {
                        // Only an upgrade over a two-handed weapon as a pair.
                        let pair = held(MAIN_HAND) + held(OFF_HAND);
                        gain = gain.min(pair + gain - held(TWO_HAND));
                    }
                    gain
                })
                .fold(0.0, f64::max),
        };
        best.max(0.0) / LEVEL_LN
    }

    /// Gear gain of items (owned with their chance) on top of `gear`, in levels.
    pub fn gain(&self, gear: &Gear, items: &[(u16, f64)], level: i64) -> f64 {
        let mut g = *gear;
        let before = self.gear_bonus(&g, level);
        for &(i, chance) in items {
            self.equip(&mut g, i, chance, level);
        }
        self.gear_bonus(&g, level) - before
    }

    /// Power the class spells learned at level `trained` lack at `level`, in levels (≤ 0).
    pub fn spell_bonus(&self, trained: i64, level: i64) -> f64 {
        self.spell_factor(trained, level).ln() / LEVEL_LN
    }

    fn spell_factor(&self, trained: i64, level: i64) -> f64 {
        let top = self.spells.len() - 1;
        let level = (level.max(0) as usize).min(top);
        self.spells[(trained.max(0) as usize).min(level)][level]
    }

    /// Fight speed gained by training now (0.1 = 10% faster).
    pub fn train_gain(&self, trained: i64, level: i64) -> f64 {
        self.spell_factor(level, level) / self.spell_factor(trained, level) - 1.0
    }

    /// Whether the data has class spells (else training follows `train_every`).
    pub fn has_spells(&self) -> bool {
        !self.families.is_empty()
    }

    /// Spell ranks that count learned when training at `level` after training at `trained`.
    pub fn new_ranks(&self, trained: i64, level: i64) -> Vec<&Rank> {
        let mut out: Vec<&Rank> = self
            .families
            .iter()
            .flat_map(|f| f.list.iter())
            .filter(|r| r.level > trained && r.level <= level)
            .collect();
        out.sort_by(|a, b| a.level.cmp(&b.level).then(a.name.cmp(&b.name)));
        out
    }

    /// Floor gear factor (for reports).
    pub fn gear_floor(&self) -> f64 {
        self.gear_floor
    }

    /// Every rank of the spells that count.
    pub fn ranks(&self) -> impl Iterator<Item = &Rank> {
        self.families.iter().flat_map(|f| f.list.iter())
    }
}

const SINGLE: [&[usize]; SLOTS] = [
    &[0],
    &[1],
    &[2],
    &[3],
    &[4],
    &[5],
    &[6],
    &[7],
    &[8],
    &[9],
    &[10],
    &[11],
    &[12],
    &[13],
    &[14],
    &[15],
    &[16],
    &[17],
];

/// Put item `i` in `slot`: owned for sure replaces what is owned (keeping a better chance
/// item), a chance item is kept when it is the best chance upgrade.
fn put(gear: &mut Gear, slot: usize, i: usize, chance: f64, value: impl Fn(usize) -> f64) {
    let entry = (i as u16 + 1, (chance * 100.0).round().clamp(1.0, 100.0) as u8);
    let [owned, maybe] = gear.slots[slot];
    let v = |e: (u16, u8)| if e.0 == 0 { 0.0 } else { value(e.0 as usize - 1) };
    if entry.1 >= 100 {
        let keep = if maybe.0 != 0 && v(maybe) > value(i) {
            maybe
        } else {
            (0, 0)
        };
        gear.slots[slot] = [entry, keep];
    } else {
        let base = v(owned);
        let worth = |e: (u16, u8)| f64::from(e.1) / 100.0 * (v(e) - base).max(0.0);
        if worth(entry) > worth(maybe) {
            gear.slots[slot] = [owned, entry];
        }
    }
}

/// Power factor of the spells learned at level `trained` compared with those sold at `level`.
fn spell_factor(families: &[Family], trained: i64, level: i64) -> f64 {
    let hit = 3.0 * (1.5 + 0.75 * level as f64);
    let strength = |r: &Rank, ranks: bool| {
        if !ranks {
            1.0
        } else if r.value <= 0.0 {
            (r.level as f64 + 5.0).powf(1.5)
        } else if r.weapon {
            r.value + hit
        } else {
            r.value / r.cast_time.max(1.5)
        }
    };
    let mut factor = 1.0;
    for f in families {
        let Some(best) = f.list.iter().rev().find(|r| r.level <= level) else {
            continue;
        };
        let known = f.list.iter().rev().find(|r| r.level <= trained);
        let loss = match known {
            Some(k) => 1.0 - (strength(k, f.ranks) / strength(best, f.ranks)).min(1.0),
            None => 1.0,
        };
        factor *= 1.0 - f.weight * loss;
    }
    factor
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rank(level: i64, value: f64) -> Rank {
        Rank {
            id: level,
            name: "Bolt".into(),
            rank: level,
            level,
            cost: 0,
            value,
            weapon: false,
            cast_time: 2.0,
        }
    }

    #[test]
    fn spells_behind_lose_their_share() {
        let families = vec![Family {
            weight: 0.4,
            ranks: true,
            list: vec![rank(1, 10.0), rank(10, 40.0)],
        }];
        assert!((spell_factor(&families, 10, 12) - 1.0).abs() < 1e-9);
        // Rank 1 is a quarter of rank 2: 3/4 of the 40% share lost.
        assert!((spell_factor(&families, 5, 12) - 0.7).abs() < 1e-9);
        // Nothing learned: the whole share.
        assert!((spell_factor(&families, 0, 12) - 0.6).abs() < 1e-9);
        // Not sold yet: no loss.
        assert!((spell_factor(&families, 0, 0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn chance_items_count_for_their_chance() {
        let mut gear = Gear::default();
        let value = |i: usize| [1.0, 3.0, 2.0][i];
        put(&mut gear, 0, 0, 1.0, value);
        put(&mut gear, 0, 1, 0.25, value);
        assert_eq!(gear.slots[0], [(1, 100), (2, 25)]);
        // An owned item below the chance item keeps it.
        put(&mut gear, 0, 2, 1.0, value);
        assert_eq!(gear.slots[0], [(3, 100), (2, 25)]);
    }

    #[test]
    fn kill_factor_matches_the_table_and_interpolates() {
        use crate::params::kill_factor;
        assert!((kill_factor(0.0) - 1.0).abs() < 1e-9);
        assert!((kill_factor(-1.0) - 0.9).abs() < 1e-9);
        assert!((kill_factor(2.0) - 1.5).abs() < 1e-9);
        assert!((kill_factor(9.0) - 3.5).abs() < 1e-9);
        assert!((kill_factor(-9.0) - 0.65).abs() < 1e-9);
        assert!((kill_factor(0.5) - 1.1).abs() < 1e-9);
    }

    #[test]
    fn a_better_weapon_adds_power() {
        let base = Base {
            str: 40.0,
            agi: 30.0,
            sta: 40.0,
            int: 20.0,
            hp: 300.0,
            mana: 0.0,
            agi_per_crit: 8.0,
        };
        let w = stat_weights(1, &base, 20, 0.75);
        // A 20 dps two-hander over the floor one (about 12 dps) is worth a couple of levels.
        let gain = (20.0 - floor_weapon(20.0, 0.75, false)) * w[DPS];
        let levels = (1.0 + gain).ln() / LEVEL_LN;
        assert!(levels > 1.0 && levels < 4.0, "{levels}");
    }
}
