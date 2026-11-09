//! `fg icons`: the icons of the items, read from the installed game client
//! (see `fg-casc`, NOTICE.md) into a pack the app serves with the guide.
//!
//!   fg icons --client "F:\World of Warcraft"
//!
//! Which icon each item has comes from the Classic Era game client's Item
//! table, committed as `data/icons/item-icons.csv` (its items are the
//! classic-era ones the editions' routes run on); the icon pictures
//! themselves (`Interface\Icons\...`, by their file data ID) are read from the
//! installed client, as PNGs: `data/icons/<edition>/icons.pack` (`FGIC`: an
//! index as JSON, then the pictures). The addon needs none: the game draws
//! its own.

use anyhow::{Context, Result};
use clap::Args;
use rusqlite::Connection;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::edition;
use fg_casc::Storage;
use fg_casc::blp;

/// The pack's magic, and its layout: `<magic> <u32 len of the index> <JSON
/// index of (item, offset, length)> <PNG pictures>`.
const MAGIC: &[u8; 4] = b"FGIC";

/// The clients to read the icons from, in order of preference: whichever the
/// install has holding the icons' files (any does, the icons are shared).
const PRODUCTS: [&str; 3] = ["wowf", "wow_classic", "wow_anniversary"];

#[derive(Args)]
pub struct Icons {
    /// The game's install (its `World of Warcraft` folder, where `.build.info` is)
    #[arg(long)]
    client: PathBuf,
}

impl Icons {
    pub fn run(&self, data: &Path) -> Result<()> {
        let key = edition().key();
        // The items that need an icon: everything the guide shows, the items
        // it values, every quest's rewards and the quest starters.
        let sqlite = crate::database(edition())
            .with_context(|| format!("no data/{key}.sqlite: run at the repository's root"))?;
        let conn = Connection::open(&sqlite).with_context(|| format!("opening {}", sqlite.display()))?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT item FROM (
                 SELECT s.id AS item FROM m_item_stats s
                 UNION SELECT r.item_id FROM m_quest_reward r
                 UNION SELECT i.id FROM m_item i WHERE i.start_quest > 0
             ) WHERE item > 0 ORDER BY item",
        )?;
        let items: Vec<i64> = stmt.query_map([], |row| row.get(0))?.collect::<rusqlite::Result<_>>()?;
        // Which icon each of them has, and the client to read the icons from.
        let icons = item_icons(&data.join("icons").join("item-icons.csv"))?;
        let mut open_errors = Vec::new();
        let mut storage = None;
        for product in PRODUCTS {
            match Storage::open(&self.client, product) {
                Ok(opened) => {
                    storage = Some(opened);
                    break;
                }
                Err(err) => open_errors.push(format!("{product}: {err}")),
            }
        }
        let Some(storage) = storage else {
            anyhow::bail!(
                "no client of {} holds the icons: {}",
                self.client.display(),
                open_errors.join("; ")
            );
        };
        println!("{key}: {} items, reading the icons", items.len());
        // The icons the items name, each picture read once, at its place in
        // the pack.
        let mut wanted: Vec<u32> = items.iter().filter_map(|item| icons.get(item).copied()).collect();
        wanted.sort_unstable();
        wanted.dedup();
        let mut blobs = Vec::new();
        let mut places: HashMap<u32, (usize, usize)> = HashMap::new();
        let mut unreadable = 0;
        for icon in wanted {
            match read_icon(&storage, icon) {
                Ok(picture) => {
                    places.insert(icon, (blobs.len(), picture.len()));
                    blobs.extend_from_slice(&picture);
                }
                // A picture the client does not carry or cannot decode: the
                // item is left without an icon.
                Err(_) => unreadable += 1,
            }
        }
        // The index: each item at its icon's picture.
        let mut index = Vec::new();
        let mut missing = 0;
        for item in items {
            match icons.get(&item).and_then(|&icon| places.get(&icon)) {
                Some(&(offset, len)) => index.push((item, offset, len)),
                None => missing += 1,
            }
        }
        let packed = index.len();
        let out = data.join("icons").join(key).join("icons.pack");
        let dir = out.parent().context("data/icons/<edition>")?;
        std::fs::create_dir_all(dir)?;
        let index = serde_json::to_vec(&index)?;
        let mut file = std::fs::File::create(&out).with_context(|| format!("writing {}", out.display()))?;
        file.write_all(MAGIC)?;
        file.write_all(&(index.len() as u32).to_le_bytes())?;
        file.write_all(&index)?;
        file.write_all(&blobs)?;
        file.sync_all()?;
        println!(
            "{key}: {packed} icons packed ({} pictures, {missing} without an icon, {unreadable} unreadable), {:.1} MB, {}",
            places.len(),
            blobs.len() as f64 / 1_048_576.0,
            out.display()
        );
        Ok(())
    }
}

/// The icon of each item, by its file data ID: the Classic Era client's Item
/// table as `data/icons/item-icons.csv` (`item,icon` a line).
fn item_icons(path: &Path) -> Result<HashMap<i64, u32>> {
    let mut out = HashMap::new();
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading {} (the Classic Era client's Item table)", path.display()))?;
    for line in text.lines().skip_while(|l| l.starts_with('#')) {
        let line = line.trim();
        if line.is_empty() || line.starts_with("item") {
            continue; // the file's comments and its header
        }
        let Some((item, icon)) = line.split_once(',') else {
            anyhow::bail!("{}: not an `item,icon` line: {line:?}", path.display());
        };
        let (item, icon): (i64, u32) = (item.parse()?, icon.parse()?);
        if item <= 0 || icon == 0 {
            continue;
        }
        out.insert(item, icon);
    }
    Ok(out)
}

/// Read one icon as a PNG.
fn read_icon(storage: &Storage, fdid: u32) -> Result<Vec<u8>> {
    let blp = storage
        .file_by_fdid(fdid)?
        .with_context(|| format!("the client has no icon {fdid}"))?;
    blp::png(&blp).with_context(|| format!("decoding the icon {fdid}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reading the pack's index back, as a test of its layout: `(item,
    /// offset, length)` a picture.
    #[test]
    fn pack_layout() {
        let index = serde_json::to_vec(&Vec::<(i64, usize, usize)>::new()).unwrap();
        let mut pack = Vec::new();
        pack.extend_from_slice(MAGIC);
        pack.extend_from_slice(&(index.len() as u32).to_le_bytes());
        pack.extend_from_slice(&index);
        let len = u32::from_le_bytes(pack[4..8].try_into().unwrap()) as usize;
        let entries: Vec<(i64, usize, usize)> = serde_json::from_slice(&pack[8..8 + len]).unwrap();
        assert_eq!(entries.len(), 0);
    }
}
