//! `fg talents`: a leveling talent plan per class, laid on the game's own talent trees (WoW
//! Forever changes some of them), written for the addon (`TalentBuilds.lua`).
//!
//! Each class has talents in priority order (English names, with ranks). Points go one per
//! level from 10, each to the first wanted talent the tree allows (points spent in the tab,
//! prerequisites); names missing from the trees are ignored and spare points fill the main tab.

use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

/// Class file name, main tab (0-based), talents in priority order ("Name:ranks").
const BUILDS: &[(&str, &str, usize, &[&str])] = &[
    (
        "WARRIOR",
        "Arms",
        0,
        &[
            "Improved Heroic Strike:3",
            "Tactical Mastery:5",
            "Improved Tactical Mastery:5",
            "Improved Rend:3",
            "Improved Overpower:2",
            "Anger Management:1",
            "Deep Wounds:3",
            "Two-Handed Weapon Specialization:5",
            "Impale:2",
            "Sweeping Strikes:1",
            "Weaponmaster:5",
            "Axe Specialization:5",
            "Mortal Strike:1",
            "Cruelty:5",
            "Unbridled Wrath:5",
            "Improved Battle Shout:5",
            "Booming Voice:5",
        ],
    ),
    (
        "PALADIN",
        "Retribution",
        2,
        &[
            "Benediction:5",
            "Improved Judgement:2",
            "Seal of Command:1",
            "Improved Seal of the Crusader:3",
            "Deflection:5",
            "Vindication:3",
            "Conviction:5",
            "Pursuit of Justice:2",
            "Two-Handed Weapon Specialization:3",
            "Sanctity Aura:1",
            "Vengeance:5",
            "Repentance:1",
            "Divine Strength:5",
            "Divine Intellect:5",
            "Improved Blessing of Might:5",
        ],
    ),
    (
        "HUNTER",
        "Beast Mastery",
        0,
        &[
            "Improved Aspect of the Hawk:5",
            "Endurance Training:5",
            "Thick Hide:3",
            "Unleashed Fury:5",
            "Ferocity:5",
            "Improved Revive Pet:2",
            "Spirit Bond:2",
            "Intimidation:1",
            "Bestial Discipline:2",
            "Frenzy:5",
            "Bestial Wrath:1",
            "Lethal Shots:5",
            "Improved Concussive Shot:5",
            "Aimed Shot:1",
            "Mortal Shots:5",
        ],
    ),
    (
        "ROGUE",
        "Combat",
        1,
        &[
            "Improved Sinister Strike:2",
            "Lightning Reflexes:5",
            "Precision:5",
            "Improved Slice and Dice:3",
            "Dual Wield Specialization:5",
            "Sword Specialization:5",
            "Blade Flurry:1",
            "Weapon Expertise:2",
            "Aggression:3",
            "Adrenaline Rush:1",
            "Malice:5",
            "Ruthlessness:3",
            "Relentless Strikes:1",
            "Lethality:5",
            "Improved Eviscerate:3",
        ],
    ),
    (
        "PRIEST",
        "Shadow",
        2,
        &[
            "Spirit Tap:5",
            "Improved Shadow Word: Pain:2",
            "Shadow Focus:5",
            "Improved Mind Blast:5",
            "Mind Flay:1",
            "Shadow Reach:3",
            "Shadow Weaving:5",
            "Vampiric Embrace:1",
            "Darkness:5",
            "Shadowform:1",
            "Blackout:5",
            "Wand Specialization:5",
            "Improved Power Word: Shield:3",
            "Meditation:3",
        ],
    ),
    (
        "SHAMAN",
        "Enhancement",
        1,
        &[
            "Ancestral Knowledge:5",
            "Thundering Strikes:5",
            "Improved Ghost Wolf:2",
            "Two-Handed Axes and Maces:1",
            "Flurry:5",
            "Enhancing Totems:2",
            "Elemental Weapons:3",
            "Parry:1",
            "Anticipation:5",
            "Stormstrike:1",
            "Weapon Mastery:5",
            "Shield Specialization:5",
            "Toughness:5",
            "Convection:5",
        ],
    ),
    (
        "MAGE",
        "Frost",
        2,
        &[
            "Improved Frostbolt:5",
            "Elemental Precision:3",
            "Ice Shards:5",
            "Improved Frost Nova:2",
            "Permafrost:3",
            "Piercing Ice:3",
            "Cold Snap:1",
            "Shatter:5",
            "Frost Channeling:3",
            "Ice Block:1",
            "Improved Cone of Cold:3",
            "Ice Barrier:1",
            "Winter's Chill:5",
            "Arcane Subtlety:2",
            "Arcane Focus:5",
            "Improved Arcane Missiles:5",
        ],
    ),
    (
        "WARLOCK",
        "Affliction",
        0,
        &[
            "Improved Corruption:5",
            "Suppression:5",
            "Improved Life Tap:2",
            "Fel Concentration:5",
            "Nightfall:2",
            "Improved Drain Life:5",
            "Grim Reach:2",
            "Siphon Life:1",
            "Amplify Curse:1",
            "Improved Curse of Agony:3",
            "Shadow Mastery:5",
            "Dark Pact:1",
            "Demonic Embrace:5",
            "Improved Voidwalker:3",
            "Fel Intellect:5",
            "Fel Domination:1",
        ],
    ),
    (
        "DRUID",
        "Feral Combat",
        1,
        &[
            "Ferocity:5",
            "Feral Instinct:5",
            "Thick Hide:5",
            "Sharpened Claws:3",
            "Feline Swiftness:2",
            "Predatory Strikes:3",
            "Blood Frenzy:2",
            "Primal Fury:2",
            "Feral Charge:1",
            "Savage Fury:2",
            "Faerie Fire (Feral):1",
            "Heart of the Wild:5",
            "Leader of the Pack:1",
            "Improved Shred:2",
            "Furor:5",
            "Improved Mark of the Wild:5",
        ],
    ),
];

struct Talent {
    id: i64,
    name: String,
    tab: usize,
    tier: usize,
    column: usize,
    ranks: usize,
    prereq: Option<(i64, usize)>,
}

pub fn run(conn: &Connection, out: &Path, edition: fg_route::xp::Edition) -> Result<()> {
    let max_level = edition.rules().max_level;
    let mut lua = String::from(
        "-- Generated by `fg talents` from the game's talent trees. Do not edit.\n\
         -- One talent per level from 10: { tab, tier, column } (1-based, as GetTalentInfo).\nlocal _, FG = ...\nFG.TalentBuilds = FG.TalentBuilds or {}\nFG.TalentBuilds.EDITION = {\n",
    )
    .replace("EDITION", edition.key());
    for (class, spec, main_tab, wanted) in BUILDS {
        let class_id: i64 = conn.query_row("SELECT ID FROM client_chrclasses WHERE Filename = ?1", [class], |r| {
            r.get(0)
        })?;
        let mut stmt = conn.prepare(
            "SELECT t.ID, coalesce(s.Name_lang, ''), tt.OrderIndex, t.TierID, t.ColumnIndex,
                    (t.SpellRank_0 > 0) + (t.SpellRank_1 > 0) + (t.SpellRank_2 > 0) + (t.SpellRank_3 > 0) + (t.SpellRank_4 > 0),
                    t.PrereqTalent_0, t.PrereqRank_0
             FROM client_talent t JOIN client_talenttab tt ON tt.ID = t.TabID
             LEFT JOIN client_spellname s ON s.ID = t.SpellRank_0
             WHERE tt.ClassMask & (1 << (?1 - 1)) != 0",
        )?;
        let talents: Vec<Talent> = stmt
            .query_map([class_id], |r| {
                let prereq: i64 = r.get(6)?;
                Ok(Talent {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    tab: r.get(2)?,
                    tier: r.get(3)?,
                    column: r.get(4)?,
                    ranks: r.get(5)?,
                    prereq: (prereq > 0).then(|| (prereq, r.get::<_, usize>(7).unwrap_or(0) + 1)),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut rank: HashMap<i64, usize> = HashMap::new();
        let mut spent = [0usize; 3];
        let allowed = |t: &Talent, rank: &HashMap<i64, usize>, spent: &[usize; 3]| {
            rank.get(&t.id).copied().unwrap_or(0) < t.ranks
                && spent[t.tab] >= t.tier * 5
                && t.prereq.is_none_or(|(p, r)| rank.get(&p).copied().unwrap_or(0) >= r)
        };
        let wanted: Vec<(&str, usize)> = wanted
            .iter()
            .filter_map(|w| w.rsplit_once(':').map(|(n, r)| (n, r.parse().unwrap_or(1))))
            .collect();
        let mut picks = Vec::new();
        let mut missing: Vec<&str> = wanted
            .iter()
            .map(|(n, _)| *n)
            .filter(|n| !talents.iter().any(|t| t.name == *n))
            .collect();
        missing.dedup();
        for _level in 10..=max_level {
            // First wanted talent the tree allows (up to its wanted ranks), else the lowest
            // available talent of the main tab, else of any tab.
            let pick = wanted
                .iter()
                .find_map(|(name, ranks)| {
                    talents.iter().find(|t| {
                        t.name == *name && rank.get(&t.id).copied().unwrap_or(0) < *ranks && allowed(t, &rank, &spent)
                    })
                })
                .or_else(|| {
                    let mut free: Vec<&Talent> = talents.iter().filter(|t| allowed(t, &rank, &spent)).collect();
                    free.sort_by_key(|t| (t.tab != *main_tab, t.tier, t.column));
                    free.first().copied()
                });
            let Some(t) = pick else { break };
            *rank.entry(t.id).or_default() += 1;
            spent[t.tab] += 1;
            picks.push(t);
        }
        let _ = writeln!(lua, "  {class} = {{ spec = {spec:?}, picks = {{");
        for t in &picks {
            let _ = writeln!(
                lua,
                "    {{ {}, {}, {} }}, -- {}",
                t.tab + 1,
                t.tier + 1,
                t.column + 1,
                t.name
            );
        }
        lua.push_str("  } },\n");
        println!(
            "{class}: {} points ({}/{}/{}){}",
            picks.len(),
            spent[0],
            spent[1],
            spent[2],
            if missing.is_empty() {
                String::new()
            } else {
                format!(", not in the trees: {}", missing.join(", "))
            }
        );
    }
    lua.push_str("}\n");
    std::fs::write(out, lua)?;
    println!("wrote {}", out.display());
    Ok(())
}
