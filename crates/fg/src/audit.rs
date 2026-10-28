//! `fg audit`: look for each known kind of data problem everywhere in an edition's data.

use anyhow::Result;
use fg_route::audit;
use fg_route::job::Overrides;
use rusqlite::Connection;
use std::path::Path;

/// Print the findings, the accepted ones apart; fails when errors are left (not accepted).
pub fn run(
    conn: &Connection,
    overrides_dir: &Path,
    checks: &[String],
    all: bool,
    proposals: Option<&Path>,
) -> Result<()> {
    let edition = crate::edition();
    for c in checks {
        anyhow::ensure!(
            audit::CHECKS.iter().any(|check| check.name == c),
            "unknown check {c} ({})",
            audit::CHECKS
                .iter()
                .map(|check| check.name)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    let overrides = Overrides::load_edition(overrides_dir, edition)?;
    let accepted = audit::accepted(overrides_dir, edition)?;
    let findings = audit::audit(conn, &overrides, checks, &|m| eprintln!("{m}"))?;
    if let Some(path) = proposals {
        write_proposals(path, &findings, &accepted)?;
    }
    let mut open = 0;
    for check in audit::CHECKS {
        let name = &check.name;
        if !checks.is_empty() && !checks.iter().any(|c| c == name) {
            continue;
        }
        let of: Vec<_> = findings.iter().filter(|f| f.check == *name).collect();
        let reason = |key: &str| {
            accepted
                .iter()
                .find(|a| a.check == *name && a.key == key)
                .map(|a| a.reason.as_str())
        };
        let left = of.iter().filter(|f| reason(&f.key).is_none()).count();
        let verdict = if check.error {
            open += left;
            "to fix"
        } else {
            "to review"
        };
        println!(
            "== {name}: {left} {verdict}, {} accepted\n   {}",
            of.len() - left,
            check.what
        );
        for f in &of {
            match reason(&f.key) {
                None => println!(
                    "  [{}] {}\n      {} (races: {})",
                    name,
                    f.key,
                    f.detail,
                    f.races.join(", ")
                ),
                Some(r) if all => println!("  accepted [{}] {}: {r}", name, f.key),
                Some(_) => {}
            }
        }
    }
    anyhow::ensure!(open == 0, "{open} findings to fix ({})", edition.key());
    Ok(())
}

/// The creatures the findings point at, as facts for the review (`fgd review propose`): one
/// line a creature, `npc:ID`, the side that probably likes it, the check and what it found. A
/// creature several findings name is proposed once, with the side of the first.
fn write_proposals(path: &Path, findings: &[audit::Finding], accepted: &[audit::Accepted]) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    let mut lines = String::new();
    // The cities' first: they know a side.
    let mut ordered: Vec<&audit::Finding> = findings.iter().collect();
    ordered.sort_by_key(|f| f.check != "hostile-in-cities");
    for f in ordered {
        if accepted.iter().any(|a| a.check == f.check && a.key == f.key) {
            continue;
        }
        for &(id, side) in &f.npcs {
            if seen.insert(id) {
                lines.push_str(&format!("npc:{id}\t{side}\t{}\t{}: {}\n", f.check, f.key, f.detail));
            }
        }
    }
    std::fs::write(path, lines)?;
    eprintln!("{} creatures proposed in {}", seen.len(), path.display());
    Ok(())
}
