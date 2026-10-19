//! Service locations from the bundled, edition-specific world data.
use super::{EntityKind, Loc, Profile};
use crate::{params::Params, world::World, xp::Edition};
use anyhow::{Result, ensure};
use rusqlite::Connection;

pub(super) fn load(conn: &Connection, world: &World, profile: &Profile, params: &Params) -> Result<[Vec<Loc>; 2]> {
    if !params.vendor_visits && !params.mailbox_visits {
        return Ok([Vec::new(), Vec::new()]);
    }
    let vendor_flag = if params.edition == Edition::Tbc { 128 } else { 4 };
    let mut vendors: Vec<Loc> = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT n.id, n.name, s.zone_id, s.x, s.y FROM m_npc n JOIN m_spawn s
         ON s.entity_type = 'npc' AND s.entity_id = n.id
         WHERE (coalesce(n.npc_flags, 0) & ?1) != 0 AND instr(coalesce(n.friendly_to, ''), ?2) > 0
         AND s.x BETWEEN 0 AND 100 AND s.y BETWEEN 0 AND 100 ORDER BY n.id, s.zone_id, s.x, s.y",
    )?;
    let rows = stmt.query_map(rusqlite::params![vendor_flag, profile.faction.letter()], |r| {
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
        if let Some(pos) = world.to_world(zone, x, y)
            && !vendors.iter().any(|p| p.id == id && p.pos.dist(&pos) < 25.0)
        {
            vendors.push(Loc {
                pos,
                zone,
                kind: EntityKind::Npc,
                id,
                name,
            });
        }
    }
    let mut mailboxes: Vec<Loc> = Vec::new();
    if params.mailbox_visits {
        // The normalized bundle has no gameobject-type column. Restrict its recognized
        // mailbox records to friendly service hubs, excluding isolated quest/instance props.
        let mut stmt = conn.prepare(
            "SELECT o.id, o.name, s.zone_id, s.x, s.y FROM m_object o JOIN m_spawn s
             ON s.entity_type = 'object' AND s.entity_id = o.id
             WHERE lower(o.name) LIKE '%mailbox%' AND s.x BETWEEN 0 AND 100 AND s.y BETWEEN 0 AND 100
             ORDER BY o.id, s.zone_id, s.x, s.y",
        )?;
        for row in stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, f64>(3)?,
                r.get::<_, f64>(4)?,
            ))
        })? {
            let (id, name, zone, x, y) = row?;
            if let Some(pos) = world.to_world(zone, x, y)
                && vendors.iter().any(|v| v.pos.dist(&pos) <= 250.0)
                && !mailboxes.iter().any(|p| p.pos.dist(&pos) < 25.0)
            {
                mailboxes.push(Loc {
                    pos,
                    zone,
                    kind: EntityKind::Object,
                    id,
                    name,
                });
            }
        }
    }
    ensure!(
        !params.vendor_visits || !vendors.is_empty(),
        "No usable vendors are available for this faction and game version"
    );
    ensure!(
        !params.mailbox_visits || !mailboxes.is_empty(),
        "No validated mailbox service hubs are available for this faction and game version"
    );
    Ok([vendors, mailboxes])
}
