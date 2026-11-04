use crate::CmdError;
use fg_library::Files;
use fg_route::diagnostics::Diagnostics;
use fg_route::job::{JobEvent, Overrides};
use fg_route::xp::Edition;
use rusqlite::Connection;
use std::sync::{Arc, LazyLock};

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
    fn overrides(&self, edition: Edition) -> Result<Arc<Overrides>, CmdError>;
    /// What a generation tells (it may come from several threads).
    fn event(&self, event: &JobEvent<'_>);
    /// Unix seconds now: when a guide version is made, a report written... ([`crate::now`] for
    /// the system clock; tests give a fixed time).
    ///
    /// # Errors
    ///
    /// [`CmdError`] when the clock is before 1970.
    fn now(&self) -> Result<u64, CmdError>;
    /// What a developer asks the generations to tell or check (read once where the app starts).
    fn diagnostics(&self) -> &Diagnostics;
}

/// The corrections of an edition from the override files built into an app (`(name, content)`),
/// read once for each edition: they do not change while the app runs.
///
/// # Errors
///
/// [`CmdError`] when one of them does not read.
pub fn embedded_overrides(
    files: &'static [(&'static str, &'static [u8])],
    edition: Edition,
) -> Result<Arc<Overrides>, CmdError> {
    static KEPT: LazyLock<quick_cache::sync::Cache<(usize, Edition), Arc<Overrides>>> =
        LazyLock::new(|| quick_cache::sync::Cache::new(Edition::ALL.len()));
    // The files of one app: one table of them, told by its address.
    KEPT.get_or_insert_with(&(files.as_ptr() as usize, edition), || {
        Overrides::parse_edition(
            |name| {
                let (_, bytes) = files.iter().find(|(n, _)| *n == name)?;
                Some(String::from_utf8_lossy(bytes).into_owned())
            },
            edition,
        )
        .map(Arc::new)
        .map_err(|e| CmdError::App(format!("{e:#}")))
    })
}
