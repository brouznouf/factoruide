//! The NPCs and objects that serve the player (trainers, flight masters, innkeepers, vendors,
//! bankers, mailboxes...) for the addon's compass: Services.lua.

use crate::model::Names;
use crate::world::World;
use crate::xp::Edition;
use anyhow::Result;
use rusqlite::Connection;
use std::fmt::Write as _;

/// What a place offers, as the addon names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Service {
    /// A class trainer (`Place::of`: the class ID).
    Class,
    /// A profession trainer (`Place::of`: the profession's skill line).
    Profession,
    Flight,
    Inn,
    Repair,
    Food,
    Bank,
    Auction,
    Stable,
    Mail,
}

impl Service {
    pub fn key(self) -> &'static str {
        match self {
            Self::Class => "class",
            Self::Profession => "profession",
            Self::Flight => "flight",
            Self::Inn => "inn",
            Self::Repair => "repair",
            Self::Food => "food",
            Self::Bank => "bank",
            Self::Auction => "auction",
            Self::Stable => "stable",
            Self::Mail => "mail",
        }
    }
}

/// A place of a service.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub service: Service,
    /// The class or the profession's skill line it trains (0 for the others).
    pub of: i64,
    /// Factions it serves: "A", "H" or "AH".
    pub factions: String,
    pub continent: i64,
    /// World position (yards).
    pub x: f64,
    pub y: f64,
    /// Its map and percent coordinates on it.
    pub ui_map: i64,
    pub map_x: f64,
    pub map_y: f64,
    pub name: String,
    /// The NPC (0: an object): the addon marks it above its head when its nameplate shows.
    pub npc: i64,
    /// Only seen from afar by players (where they stood, not where it stands).
    pub rough: bool,
}

const CLASSES: [(&str, i64); 9] = [
    ("Warrior", 1),
    ("Paladin", 2),
    ("Hunter", 3),
    ("Rogue", 4),
    ("Priest", 5),
    ("Shaman", 7),
    ("Mage", 8),
    ("Warlock", 9),
    ("Druid", 11),
];

/// Words of a profession trainer's title ("Journeyman Blacksmith", "Tailoring Trainer") and the
/// profession's skill line.
const PROFESSIONS: [(&str, i64); 15] = [
    ("Alchem", 171),
    ("Blacksmith", 164),
    ("Armorsmith", 164),
    ("Weaponsmith", 164),
    ("Cook", 185),
    ("Enchant", 333),
    ("Engineer", 202),
    ("First Aid", 129),
    ("Physician", 129),
    ("Fishing", 356),
    ("Herbalis", 182),
    ("Leather", 165),
    ("Mining", 186),
    ("Skinning", 393),
    ("Jewel", 755),
];

/// Tailors apart: "Tailor" is in no other title, but "Shadoweave Tailor" is one.
const TAILORING: i64 = 197;

const FOOD: [&str; 11] = [
    "Food",
    "Drink",
    "Baker",
    "Butcher",
    "Fruit",
    "Mushroom",
    "Cheese",
    "Bread",
    "Wine",
    "General Goods",
    "Bartender",
];

const FLIGHT: [&str; 7] = [
    "Gryphon Master",
    "Wind Rider Master",
    "Hippogryph Master",
    "Bat Handler",
    "Flight Master",
    "Dragonhawk Master",
    "Wind Rider Keeper",
];

/// The NPC flags that matter here, as the edition's data writes them (TBC moved them).
struct Flags {
    vendor: i64,
    trainer: i64,
    flight: i64,
    inn: i64,
    bank: i64,
    auction: i64,
    stable: i64,
    repair: i64,
}

impl Flags {
    fn of(edition: Edition) -> Self {
        match edition {
            Edition::Tbc => Self {
                vendor: 128,
                trainer: 16,
                flight: 8192,
                inn: 65536,
                bank: 131_072,
                auction: 2_097_152,
                stable: 4_194_304,
                repair: 4096,
            },
            Edition::Forever | Edition::Classic => Self {
                vendor: 4,
                trainer: 16,
                flight: 8,
                inn: 128,
                bank: 256,
                auction: 4096,
                stable: 8192,
                repair: 16384,
            },
        }
    }
}

/// The skill line of the profession a trainer teaches, from its title and flags.
pub(crate) fn profession_of(title: &str, flags: i64, edition: Edition) -> Option<i64> {
    if flags & Flags::of(edition).trainer == 0 {
        return None;
    }
    match PROFESSIONS.iter().find(|(word, _)| title.contains(word)) {
        Some(&(_, line)) => Some(line),
        None => (title.contains("Tailor") || title.contains("Shadoweave")).then_some(TAILORING),
    }
}

/// What an NPC offers from its title and flags (several things for some: an innkeeper selling
/// food is only an innkeeper, an armorer repairing is a repairer).
fn services(title: &str, flags: i64, edition: Edition) -> Vec<(Service, i64)> {
    let f = Flags::of(edition);
    let mut out = Vec::new();
    if let Some(class) = title.strip_suffix(" Trainer")
        && let Some(&(_, id)) = CLASSES.iter().find(|(name, _)| *name == class)
    {
        out.push((Service::Class, id));
    } else if let Some(line) = profession_of(title, flags, edition) {
        out.push((Service::Profession, line));
    }
    if flags & f.flight != 0 || FLIGHT.contains(&title) {
        out.push((Service::Flight, 0));
    }
    let mut any = false;
    for (service, flag, name) in [
        (Service::Inn, f.inn, "Innkeeper"),
        (Service::Bank, f.bank, "Banker"),
        (Service::Auction, f.auction, "Auctioneer"),
        (Service::Stable, f.stable, "Stable Master"),
    ] {
        if flags & flag != 0 || title == name {
            out.push((service, 0));
            any = true;
        }
    }
    // An innkeeper sells food too, but is shown as one.
    if !any && flags & f.vendor != 0 && FOOD.iter().any(|w| title.contains(w)) {
        out.push((Service::Food, 0));
    }
    if flags & f.repair != 0 {
        out.push((Service::Repair, 0));
    }
    out
}

/// Every place of a service that some faction may use, names in the language of `names`.
pub fn places(conn: &Connection, world: &World, edition: Edition, names: &Names) -> Result<Vec<Place>> {
    let mut out = Vec::new();
    let mut place = |service: Service,
                     of: i64,
                     factions: &str,
                     (zone, x, y): (i64, f64, f64),
                     name: String,
                     npc: i64,
                     rough: bool| {
        let Some(pos) = world.to_world(zone, x, y) else {
            return;
        };
        let Some(map) = world.to_map(zone, &pos) else {
            return;
        };
        out.push(Place {
            service,
            of,
            factions: factions.to_owned(),
            continent: pos.continent,
            x: pos.x.round(),
            y: pos.y.round(),
            ui_map: map.ui_map,
            map_x: map.x,
            map_y: map.y,
            name,
            npc,
            rough,
        });
    };
    let mut stmt = conn.prepare(
        "SELECT n.id, n.name, coalesce(n.subname, ''), coalesce(n.npc_flags, 0), n.friendly_to, s.zone_id, s.x, s.y,
                coalesce(s.sources, '')
         FROM m_npc n JOIN m_spawn s ON s.entity_type = 'npc' AND s.entity_id = n.id
         WHERE coalesce(n.friendly_to, '') != '' AND s.x IS NOT NULL AND s.y IS NOT NULL
         ORDER BY n.id, s.zone_id, s.x, s.y",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, i64>(5)?,
            r.get::<_, f64>(6)?,
            r.get::<_, f64>(7)?,
            r.get::<_, String>(8)?,
        ))
    })?;
    for row in rows {
        let (id, name, title, flags, factions, zone, x, y, sources) = row?;
        // Left in the data by the game's makers: "[UNUSED] Smith Theo".
        if name.starts_with('[') || name.contains("UNUSED") {
            continue;
        }
        let rough = sources.split(',').all(|s| s == SEEN);
        for (service, of) in services(&title, flags, edition) {
            place(
                service,
                of,
                &factions,
                (zone, x, y),
                names.get("npc", id, &name).to_owned(),
                id,
                rough,
            );
        }
    }
    let mut stmt = conn.prepare(
        "SELECT o.id, o.name, s.zone_id, s.x, s.y
         FROM m_object o JOIN m_spawn s ON s.entity_type = 'object' AND s.entity_id = o.id
         WHERE o.name = 'Mailbox' AND s.x IS NOT NULL AND s.y IS NOT NULL
         ORDER BY o.id, s.zone_id, s.x, s.y",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, f64>(3)?,
            r.get::<_, f64>(4)?,
        ))
    })?;
    for row in rows {
        let (id, name, zone, x, y) = row?;
        place(
            Service::Mail,
            0,
            "AH",
            (zone, x, y),
            names.get("object", id, &name).to_owned(),
            0,
            false,
        );
    }
    Ok(dedupe(out))
}

/// The source of the places where players saw an NPC from (rough: they stood there, the NPC
/// stood somewhere around).
const SEEN: &str = "collector_seen";
/// Places closer than this (yards) to another of the same NPC and service are the same; a place
/// only seen from afar this close to another of the same NPC too.
const SAME: f64 = 15.0;
const SAME_SEEN: f64 = 150.0;

/// One place per NPC standing somewhere: the data may hold its spawn several times, a few yards
/// apart (from several sources), and where players saw it from. Those seen from afar only count
/// where no other place of the same NPC is close.
fn dedupe(places: Vec<Place>) -> Vec<Place> {
    let mut kept: Vec<Place> = Vec::with_capacity(places.len());
    let mut same: std::collections::HashMap<(String, i64, &'static str, i64), Vec<usize>> =
        std::collections::HashMap::new();
    let (exact, rough): (Vec<Place>, Vec<Place>) = places.into_iter().partition(|p| !p.rough);
    for p in exact.into_iter().chain(rough) {
        let radius = if p.rough { SAME_SEEN } else { SAME };
        let others = same.entry((p.name.clone(), p.npc, p.service.key(), p.of)).or_default();
        let near = |q: &Place| q.continent == p.continent && (q.x - p.x).hypot(q.y - p.y) < radius;
        if !others.iter().any(|&i| near(&kept[i])) {
            others.push(kept.len());
            kept.push(p);
        }
    }
    kept
}

/// The addon's Services.lua: every place as `{ kind, factions, class or skill line, continent,
/// x, y, map, map x, map y, name, NPC }`.
pub fn services_lua(places: &[Place]) -> String {
    let mut out = String::from(
        "-- Generated by the Factoruide app. Do not edit.\nlocal FG = Factoruide\n\
         -- Places for the compass: kind, factions, class ID or profession skill line, continent,\n\
         -- world x, y (yards), map, x, y (percent), name, NPC ID (0: an object).\nFG.Services = {\n",
    );
    for p in places {
        let _ = writeln!(
            out,
            "  {{ {:?}, {:?}, {}, {}, {}, {}, {}, {}, {}, {:?}, {} }},",
            p.service.key(),
            p.factions,
            p.of,
            p.continent,
            p.x,
            p.y,
            p.ui_map,
            p.map_x,
            p.map_y,
            p.name,
            p.npc
        );
    }
    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_tell_the_service() {
        let classic = Edition::Classic;
        assert_eq!(services("Mage Trainer", 19, classic), [(Service::Class, 8)]);
        assert_eq!(services("Portal Trainer", 19, classic), []);
        assert_eq!(
            services("Journeyman Blacksmith", 19, classic),
            [(Service::Profession, 164)]
        );
        assert_eq!(
            services("Master Shadoweave Tailor", 19, classic),
            [(Service::Profession, 197)]
        );
        // A trainer's title without the trainer flag: a supplies vendor.
        assert_eq!(services("Leatherworking Supplies", 4, classic), []);
        assert_eq!(services("Gryphon Master", 11, classic), [(Service::Flight, 0)]);
        assert_eq!(services("Innkeeper", 135, classic), [(Service::Inn, 0)]);
        // Auctioneers have their title in their name.
        assert_eq!(services("", 4096, classic), [(Service::Auction, 0)]);
        assert_eq!(services("Weapon Merchant", 16388, classic), [(Service::Repair, 0)]);
        assert_eq!(services("Food & Drink", 4, classic), [(Service::Food, 0)]);
    }

    fn at(x: f64, rough: bool) -> Place {
        Place {
            service: Service::Flight,
            of: 0,
            factions: "A".into(),
            continent: 0,
            x,
            y: 0.0,
            ui_map: 1436,
            map_x: 0.0,
            map_y: 0.0,
            name: "Thor".into(),
            npc: 523,
            rough,
        }
    }

    /// Where players saw an NPC from is dropped near its spawn, kept where nothing else is.
    #[test]
    fn places_seen_from_afar_give_way_to_the_spawn() {
        let kept = dedupe(vec![
            at(50.0, true),
            at(0.0, false),
            at(5.0, false),
            at(900.0, true),
            at(950.0, true),
        ]);
        let xs: Vec<f64> = kept.iter().map(|p| p.x).collect();
        assert_eq!(xs, [0.0, 900.0]);
    }

    #[test]
    fn tbc_flags_are_read_as_tbc_writes_them() {
        let tbc = Edition::Tbc;
        assert_eq!(services("", 8195, tbc), [(Service::Flight, 0)]);
        assert_eq!(services("Armorer", 4096 + 128, tbc), [(Service::Repair, 0)]);
        assert_eq!(services("General Goods", 384, tbc), [(Service::Food, 0)]);
        // 8192 is a stable master's flag on Classic, a flight master's on TBC.
        assert_eq!(services("", 8192, Edition::Classic), [(Service::Stable, 0)]);
        assert_eq!(services("Innkeeper", 66179, tbc), [(Service::Inn, 0)]);
    }
}
