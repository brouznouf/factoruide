//! An item's details, as the game's tooltip shows them, out of the game's own data.

use rusqlite::Connection;
use serde::Serialize;

/// An item's details, as the game's tooltip shows them (the app draws its own).
#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemInfo {
    /// The game's name ("" when the data has none).
    pub name: String,
    /// 0 (poor) ..= 6 (artifact): the game colors the name by it.
    pub quality: i64,
    /// Where it goes (`InventoryType`; 0: nowhere).
    pub inventory_type: i64,
    /// Level the character must have (0: none).
    pub required_level: i64,
    /// Armor it gives.
    pub armor: f64,
    /// A weapon's damage per second and its speed (0 when it is not one).
    pub dps: f64,
    pub speed: f64,
    /// What it gives, as (the stat's code, points) in the game's order, the zeros left out.
    pub stats: Vec<(String, f64)>,
    /// What a vendor sells and buys it for (copper; 0 in data built before the prices).
    pub buy_price: i64,
    pub sell_price: i64,
}

/// What an item gives, in the game's tooltip's order: the code the page knows the stat by, and
/// the column holding it (`m_item_stats`).
const STATS: [(&str, &str); 13] = [
    ("str", "str"),
    ("agi", "agi"),
    ("sta", "sta"),
    ("int", "int"),
    ("spi", "spi"),
    ("ap", "ap"),
    ("rap", "rap"),
    ("sp", "sp"),
    ("heal", "heal"),
    ("crit", "crit"),
    ("spellCrit", "spell_crit"),
    ("hit", "hit"),
    ("mp5", "mp5"),
];

/// An item's details (None: the game's data has none of its ID, or its table or a column is
/// missing — an unknown item, not a failure). The guide shows the items of `m_item_stats`: what
/// its quests give and the vendors sell.
pub fn item_info(conn: &Connection, item: i64) -> Option<ItemInfo> {
    // Data built before `fg gold` has no prices: read them as none.
    let has_prices: bool = conn
        .query_row(
            "SELECT count(*) = 2 FROM pragma_table_info('m_item_stats') WHERE name IN ('buy_price', 'sell_price')",
            [],
            |r| r.get(0),
        )
        .unwrap_or(false);
    let prices = if has_prices {
        "coalesce(s.buy_price, 0), coalesce(s.sell_price, 0)"
    } else {
        "0, 0"
    };
    let stats = STATS
        .iter()
        .map(|(_, c)| format!("s.{c}"))
        .collect::<Vec<_>>()
        .join(", ");
    let mut stmt = conn
        .prepare(&format!(
            "SELECT coalesce(n.name, ''), s.quality, s.inventory_type, s.required_level, s.armor, s.dps, s.speed,
                    {prices}, {stats}
             FROM m_item_stats s LEFT JOIN m_item n ON n.id = s.id WHERE s.id = ?1"
        ))
        .ok()?;
    let row = stmt
        .query_row([item], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, f64>(4)?,
                r.get::<_, f64>(5)?,
                r.get::<_, f64>(6)?,
                (r.get::<_, i64>(7)?, r.get::<_, i64>(8)?),
                // What it gives: the 13 stats, after the name, its place and its prices.
                (9..9 + STATS.len())
                    .map(|k| r.get::<_, f64>(k))
                    .collect::<rusqlite::Result<Vec<f64>>>()?,
            ))
        })
        .ok()?;
    let (name, quality, inventory_type, required_level, armor, dps, speed, (buy_price, sell_price), values) = row;
    let stats = STATS
        .iter()
        .zip(values)
        .filter(|(_, v)| *v != 0.0)
        .map(|((code, _), v)| ((*code).to_owned(), v))
        .collect();
    Some(ItemInfo {
        name,
        quality,
        inventory_type,
        required_level,
        armor,
        dps,
        speed,
        stats,
        buy_price,
        sell_price,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A database with the items the guide shows, one in it.
    fn conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE m_item(id INT, name TEXT);
             CREATE TABLE m_item_stats(
               id INT, quality INT, inventory_type INT, class INT, subclass INT, allowable_class INT,
               required_level INT, str REAL, agi REAL, sta REAL, int REAL, spi REAL, armor REAL, ap REAL,
               rap REAL, sp REAL, heal REAL, crit REAL, spell_crit REAL, hit REAL, mp5 REAL, dps REAL,
               speed REAL, buy_price INT, sell_price INT);
             INSERT INTO m_item VALUES (12284, 'Bronze Sword');
             INSERT INTO m_item_stats VALUES
               (12284, 2, 17, 2, 8, 0, 17, 6, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 32.4, 2.8, 500, 125);
             INSERT INTO m_item VALUES (19019, 'No stats');
             INSERT INTO m_item_stats VALUES
               (19019, 1, 0, 4, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 400, 100)",
        )
        .unwrap();
        conn
    }

    #[test]
    fn an_item_reads_as_the_tooltip_shows_it() {
        let info = item_info(&conn(), 12284).unwrap();
        assert_eq!(info.name, "Bronze Sword");
        assert_eq!(info.quality, 2);
        assert_eq!(info.inventory_type, 17);
        assert_eq!(info.required_level, 17);
        assert_eq!(info.armor, 0.0);
        assert_eq!(info.dps, 32.4);
        assert_eq!(info.speed, 2.8);
        // What it gives, in the game's order, the zeros left out.
        assert_eq!(info.stats, [("str".to_owned(), 6.0), ("sta".to_owned(), 5.0)]);
        assert_eq!((info.buy_price, info.sell_price), (500, 125));
    }

    #[test]
    fn an_item_without_a_name_is_read_without_one() {
        // A row without m_item (the data has stats of items it has no name of).
        let conn = conn();
        conn.execute("DELETE FROM m_item WHERE id = 19019", []).unwrap();
        let info = item_info(&conn, 19019).unwrap();
        assert_eq!(info.name, "");
        assert_eq!(info.stats, Vec::<(String, f64)>::new());
        assert_eq!(info.dps, 0.0);
    }

    #[test]
    fn an_unknown_item_is_none() {
        assert!(item_info(&conn(), 9999).is_none());
    }

    #[test]
    fn data_without_prices_reads_them_as_none() {
        let conn = conn();
        conn.execute_batch(
            "ALTER TABLE m_item_stats DROP COLUMN buy_price;
             ALTER TABLE m_item_stats DROP COLUMN sell_price",
        )
        .unwrap();
        let info = item_info(&conn, 12284).unwrap();
        assert_eq!((info.buy_price, info.sell_price), (0, 0));
        assert_eq!(info.stats, [("str".to_owned(), 6.0), ("sta".to_owned(), 5.0)]);
    }
}
