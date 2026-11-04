//! What is read from the game data and the same at every plan: kept by database, so that the
//! next plan (the editor, a preview, the next guide) does not query it again.
//!
//! A database is told from another by `world::file_stamp`: its file (path, date, size), or the
//! name given to one in memory. A database without either (a test's) is read each time.

use anyhow::Result;
use rusqlite::Connection;
use std::any::Any;
use std::sync::{Arc, LazyLock};

type Kept = Arc<dyn Any + Send + Sync>;

/// (database, what, its settings): the rows read.
type Key = (u64, &'static str, u64);

/// Entries kept at most: a few queries for each of the editions and factions in use.
const KEPT_MAX: usize = 400;

static KEPT: LazyLock<quick_cache::sync::Cache<Key, Kept>> = LazyLock::new(|| quick_cache::sync::Cache::new(KEPT_MAX));

/// The rows `read` gives for `what` (a name of the query) with `settings` (what else it depends
/// on, hashed), read the first time only for this database (plans asking at once read it once).
/// Do not use it for tables a test replaces with a temporary one (`m_quest`,
/// `m_quest_objective`).
///
/// # Errors
///
/// When the query fails (nothing is kept then).
pub(crate) fn rows<T: Any + Send + Sync>(
    conn: &Connection,
    what: &'static str,
    settings: u64,
    read: impl FnOnce() -> Result<T>,
) -> Result<Arc<T>> {
    let Some(stamp) = crate::world::file_stamp(conn) else {
        return read().map(Arc::new);
    };
    let kept = KEPT.get_or_insert_with(&(stamp, what, settings), || read().map(|rows| Arc::new(rows) as Kept))?;
    Ok(Arc::downcast::<T>(kept).expect("one type of rows by query name"))
}
/// A setting as a number for `rows`.
pub(crate) fn setting(value: impl std::hash::Hash) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}
