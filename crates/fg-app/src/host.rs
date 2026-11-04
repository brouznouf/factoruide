use crate::CmdError;
use fg_library::Files;
use fg_route::job::Overrides;
use fg_route::xp::Edition;
use rusqlite::Connection;

/// What the app around gives the [`Studio`](crate::Studio): the desktop app's folders and
/// database files, or the browser's storage and downloads.
pub trait Host: Sync {
    type Files: Files;

    /// Where the guides are kept.
    fn files(&self) -> &Self::Files;
    /// The game version the app is set to.
    fn edition(&self) -> Edition;
    /// A read-only connection to the game data of an edition.
    ///
    /// # Errors
    ///
    /// [`CmdError`] when the data is not there or cannot be read.
    fn open_db(&self, edition: Edition) -> Result<Connection, CmdError>;
    /// The corrections to the game data of an edition.
    ///
    /// # Errors
    ///
    /// [`CmdError`] when the data is not there or cannot be read.
    fn overrides(&self, edition: Edition) -> Result<Overrides, CmdError>;
    /// A line of what a generation is doing (it may come from several threads).
    fn progress(&self, line: &str);
    /// Unix seconds now: when a guide version is made, a report written... ([`crate::now`] for
    /// the system clock; tests give a fixed time).
    ///
    /// # Errors
    ///
    /// [`CmdError`] when the clock is before 1970.
    fn now(&self) -> Result<u64, CmdError>;
}
