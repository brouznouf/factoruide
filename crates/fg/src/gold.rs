//! `fg gold`: import the money the quests give and the prices of their items from a VMaNGOS
//! database (`db_latest`, https://github.com/vmangos/core/releases — a source NOTICE.md
//! credits) into the edition's database (data/<edition>.sqlite[.zst]).
//!
//!   fg gold --mangos mangos.sqlite [--no-pack]
//!
//! Adds `reward_money` (copper) to `m_quest` and `buy_price`/`sell_price` to `m_item_stats`,
//! and repacks `data/<edition>.sqlite.zst` to keep the packed copy in sync. Idempotent: run
//! again over a newer VMaNGOS database to refresh the values.

use anyhow::{Context, Result};
use clap::Args;
use rusqlite::Connection;
use std::path::{Path, PathBuf};

use crate::edition;

/// A column the import adds to the edition's database, and where it comes from: the VMaNGOS
/// table and column its rows are read in.
struct Column {
    /// The table of the edition's database it is added to.
    table: &'static str,
    /// The column to add.
    name: &'static str,
    /// The VMaNGOS table and column it comes from (`entry` matching the row's `id`).
    from: (&'static str, &'static str),
}

const COLUMNS: [Column; 3] = [
    Column {
        table: "m_quest",
        name: "reward_money",
        from: ("quest_template", "RewOrReqMoney"),
    },
    Column {
        table: "m_item_stats",
        name: "buy_price",
        from: ("item_template", "buy_price"),
    },
    Column {
        table: "m_item_stats",
        name: "sell_price",
        from: ("item_template", "sell_price"),
    },
];

#[derive(Args)]
pub struct Gold {
    /// The VMaNGOS database (mangos.sqlite of the db_latest release, or a copy of it)
    #[arg(long)]
    mangos: PathBuf,
    /// Do not repack data/<edition>.sqlite.zst (leave it older than the .sqlite)
    #[arg(long)]
    no_pack: bool,
}

impl Gold {
    pub fn run(&self, data: &Path) -> Result<()> {
        let key = edition().key();
        let sqlite = crate::database(edition())
            .with_context(|| format!("no data/{key}.sqlite: run at the repository's root"))?;
        let packed = data.join(format!("{key}.sqlite.zst"));
        anyhow::ensure!(
            self.mangos.exists(),
            "{} is missing (the db_latest release of vmangos/core)",
            self.mangos.display()
        );
        let conn = Connection::open(&sqlite).with_context(|| format!("opening {}", sqlite.display()))?;
        conn.execute("ATTACH DATABASE ? AS vmangos", [self.mangos.to_string_lossy().as_ref()])?;
        let mut imported = 0;
        for c in COLUMNS {
            let has: bool = conn.query_row(
                &format!(
                    "SELECT count(*) > 0 FROM pragma_table_info('{}') WHERE name = '{}'",
                    c.table, c.name
                ),
                [],
                |r| r.get(0),
            )?;
            if !has {
                conn.execute(&format!("ALTER TABLE {} ADD COLUMN {} INTEGER", c.table, c.name), [])?;
            }
            // Every row, not only the ones the source has one for: a value kept from a
            // previous import would pass for this one's.
            conn.execute(
                &format!(
                    "UPDATE {t} SET {c} =
                        (SELECT coalesce(s.{sc}, 0) FROM vmangos.{st} s WHERE s.entry = {t}.id)",
                    t = c.table,
                    c = c.name,
                    st = c.from.0,
                    sc = c.from.1
                ),
                [],
            )?;
            let set: i64 = conn.query_row(
                &format!("SELECT count(*) FROM {} WHERE {} > 0", c.table, c.name),
                [],
                |r| r.get(0),
            )?;
            println!(
                "{key}.sqlite: {}.{} filled for {set} rows (from {}.{})",
                c.table, c.name, c.from.0, c.from.1
            );
            imported += 1;
        }
        // The rows the import filled keep their source, like every other row of the database
        // (the quest database alone keeps one: `m_item_stats` has no `sources` column).
        conn.execute(
            "UPDATE m_quest SET sources = sources || ',vmangos-money'
             WHERE reward_money > 0 AND NOT instr(coalesce(sources, ''), 'vmangos-money')",
            [],
        )?;
        println!("{key}: {imported} columns imported, packing {}", packed.display());
        conn.execute("DETACH vmangos", [])?;
        drop(conn);
        if !self.no_pack {
            pack(&sqlite, &packed)?;
        }
        Ok(())
    }
}

/// Rewrite the packed copy (`data/<edition>.sqlite.zst`) from the unpacked database.
fn pack(sqlite: &Path, packed: &Path) -> Result<()> {
    let read = std::fs::File::open(sqlite)?;
    let mut write = std::fs::File::create(packed)?;
    zstd::stream::copy_encode(&read, &mut write, crate::PACK_LEVEL)?;
    write.sync_all()?;
    Ok(())
}
