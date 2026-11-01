//! What is read from the game data and the same at every plan: kept by database, so that the
//! next plan (the editor, a preview, the next guide) does not query it again.
//!
//! A database is told from another by `world::file_stamp`: its file (path, date, size), or the
//! name given to one in memory. A database without either (a test's) is read each time.

use anyhow::Result;
use rusqlite::Connection;
use std::any::Any;
use std::sync::{Arc, Mutex, PoisonError};

type Kept = Arc<dyn Any + Send + Sync>;

/// (database, what, its settings): the rows read.
type Key = (u64, &'static str, u64);

static KEPT: Mutex<Vec<(Key, Kept)>> = Mutex::new(Vec::new());
/// Entries kept at most: a few queries for each of the editions and factions in use.
const KEPT_MAX: usize = 400;

/// The rows `read` gives for `what` (a name of the query) with `settings` (what else it depends
/// on, hashed), read the first time only for this database. Do not use it for tables a test
/// replaces with a temporary one (`m_quest`, `m_quest_objective`).
///
/// # Errors
///
/// When the query fails.
pub(crate) fn rows<T: Any + Send + Sync>(
    conn: &Connection,
    what: &'static str,
    settings: u64,
    read: impl FnOnce() -> Result<T>,
) -> Result<Arc<T>> {
    let Some(stamp) = crate::world::file_stamp(conn) else {
        return read().map(Arc::new);
    };
    let key = (stamp, what, settings);
    let found = KEPT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| Arc::clone(v));
    if let Some(Ok(rows)) = found.map(Arc::downcast::<T>) {
        return Ok(rows);
    }
    let rows = Arc::new(read()?);
    let mut kept = KEPT.lock().unwrap_or_else(PoisonError::into_inner);
    if kept.len() >= KEPT_MAX {
        kept.remove(0);
    }
    kept.push((key, Arc::clone(&rows) as Kept));
    Ok(rows)
}

/// A setting as a number for `rows`.
pub(crate) fn setting(value: impl std::hash::Hash) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}
