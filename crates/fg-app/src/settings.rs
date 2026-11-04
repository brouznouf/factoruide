//! The app's settings, the same on the desktop and in the browser, and where they are kept.

use crate::CmdError;
use fg_library::Files;
use fg_route::sync::LockExt;
use fg_route::xp::Edition;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// What the player chose in the app. The browser has no game folder, no addon to install and no
/// contribution to send: it keeps those fields as they are.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// The game's folder (empty: not chosen).
    #[serde(default)]
    pub wow_dir: PathBuf,
    /// Game version the app works on (forever, classic, tbc).
    #[serde(default)]
    pub edition: Edition,
    /// App language (also the default guide language); unset: the system language.
    #[serde(default)]
    pub language: Option<String>,
    /// Contribution server (its base address); unset: the one the app was built with.
    #[serde(default)]
    pub contribution_url: Option<String>,
    /// Random id of this install, sent with its contributions (tells how many different
    /// players back what they report; nothing else about the player).
    #[serde(default)]
    pub contributor: Option<String>,
    /// Send what is new to the contribution server on its own, once a day at most (off unless
    /// the player turns it on).
    #[serde(default)]
    pub auto_contribute: bool,
    /// Addon behaviour, written to the addon's Config.lua on install.
    #[serde(default)]
    pub addon: serde_json::Map<String, serde_json::Value>,
    /// What shows the guides in game on WoW Forever (see `guide_addon`).
    #[serde(default)]
    pub guide_addon: GuideAddon,
    /// Language the guides are written in in the addon (game locale); unset: each guide in the
    /// language it was made in.
    #[serde(default)]
    pub guide_locale: Option<String>,
    /// Offer every guide to every character, not only those of its race and class.
    #[serde(default)]
    pub all_guides: bool,
    /// What the addon in the game was last installed with (`addon_snapshot`): the settings page
    /// saves at once, the game gets them when asked.
    #[serde(default)]
    pub addon_installed: Option<String>,
    /// Whether the addon's setup questions were answered (or skipped): asked once, then on demand.
    #[serde(default)]
    pub addon_setup_done: bool,
    /// Whether the first launch's questions (where the game is) were answered or skipped.
    #[serde(default)]
    pub welcome_done: bool,
    /// Characters taken out of the Characters page ("<edition>/<Name-Realm>" -> the time of the
    /// profile then): deleted in game, but the game keeps their files and the addon their
    /// profile. One played again (a newer profile) is back.
    #[serde(default)]
    pub forgotten_characters: BTreeMap<String, i64>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            wow_dir: PathBuf::new(),
            edition: Edition::Forever,
            language: None,
            contribution_url: None,
            contributor: None,
            auto_contribute: false,
            addon: serde_json::Map::new(),
            guide_addon: GuideAddon::Factoruide,
            guide_locale: None,
            all_guides: false,
            addon_installed: None,
            addon_setup_done: false,
            welcome_done: false,
            forgotten_characters: BTreeMap::new(),
        }
    }
}

/// What shows the guides in game: the addon's guide part, or Polestar (WoW Forever only) with a
/// guide pack of them; the addon's profile part records in both cases.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GuideAddon {
    #[default]
    Factoruide,
    Polestar,
}

impl Settings {
    /// The addons folder of a game version's client.
    pub fn addons_dir(&self, edition: Edition) -> PathBuf {
        edition.client_dir(&self.wow_dir).join("Interface/AddOns")
    }

    /// What shows the guides in a game version: Polestar exists on WoW Forever only.
    pub fn guide_addon(&self, edition: Edition) -> GuideAddon {
        if edition == Edition::Forever {
            self.guide_addon
        } else {
            GuideAddon::Factoruide
        }
    }
}

/// The settings, kept in one file of `files`. Each change is saved before anyone sees it, and
/// changes never overwrite each other: `update` changes the settings as they are then.
pub struct SettingsStore<F: Files> {
    files: F,
    path: String,
    current: Mutex<Arc<Settings>>,
}

impl<F: Files> SettingsStore<F> {
    /// The settings saved at `path` in `files`; the default ones when there are none (or they
    /// cannot be read), and whether there were.
    pub fn open(files: F, path: &str) -> (Self, bool) {
        let saved: Option<Settings> = files.read(path).ok().and_then(|b| serde_json::from_slice(&b).ok());
        let found = saved.is_some();
        let store = Self {
            files,
            path: path.to_owned(),
            current: Mutex::new(Arc::new(saved.unwrap_or_default())),
        };
        (store, found)
    }

    /// `settings`, kept at `path` in `files` from now on (saved at their first change).
    pub fn with(files: F, path: &str, settings: Settings) -> Self {
        Self {
            files,
            path: path.to_owned(),
            current: Mutex::new(Arc::new(settings)),
        }
    }

    /// The settings now (they do not change under the caller).
    pub fn get(&self) -> Arc<Settings> {
        Arc::clone(&self.current.locked())
    }

    /// Change the settings and save them; nothing else changes them meanwhile.
    ///
    /// # Errors
    ///
    /// [`CmdError`] when they cannot be saved (they stay as they were).
    pub fn update<T>(&self, change: impl FnOnce(&mut Settings) -> T) -> Result<T, CmdError> {
        let mut current = self.current.locked();
        let mut settings = Settings::clone(&current);
        let out = change(&mut settings);
        let json = serde_json::to_vec_pretty(&settings).map_err(|e| CmdError::App(e.to_string()))?;
        (self.files.write(&self.path, &json)).map_err(|e| CmdError::App(format!("{}: {e}", self.path)))?;
        *current = Arc::new(settings);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::{Settings, SettingsStore};
    use fg_library::Files;
    use std::collections::BTreeMap;
    use std::io;
    use std::sync::Mutex;

    /// Files in memory.
    #[derive(Default)]
    struct Memory(Mutex<BTreeMap<String, Vec<u8>>>);

    impl Files for Memory {
        fn read(&self, path: &str) -> io::Result<Vec<u8>> {
            (self.0.lock().unwrap().get(path).cloned()).ok_or_else(|| io::ErrorKind::NotFound.into())
        }
        fn write(&self, path: &str, data: &[u8]) -> io::Result<()> {
            self.0.lock().unwrap().insert(path.to_owned(), data.to_vec());
            Ok(())
        }
        fn list(&self, _: &str) -> Vec<String> {
            Vec::new()
        }
        fn remove_dir(&self, _: &str) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn settings_are_the_default_ones_until_saved_then_read_back() {
        let (store, found) = SettingsStore::open(Memory::default(), "settings.json");
        assert!(!found);
        assert!(!store.get().all_guides);
        store.update(|s| s.all_guides = true).unwrap();
        let saved = store.files.read("settings.json").unwrap();
        let (again, found) = SettingsStore::open(
            Memory(Mutex::new([("settings.json".to_owned(), saved)].into())),
            "settings.json",
        );
        assert!(found && again.get().all_guides);
    }

    /// A copy taken before a change keeps its values; each change starts from the last one.
    #[test]
    fn a_change_starts_from_the_settings_as_they_are_then() {
        let (store, _) = SettingsStore::open(Memory::default(), "settings.json");
        let before = store.get();
        store.update(|s| s.welcome_done = true).unwrap();
        store.update(|s| s.all_guides = true).unwrap();
        let now = store.get();
        assert!(!before.welcome_done && now.welcome_done && now.all_guides);
    }

    /// The browser's settings of older versions (a few fields, the game folder as text) still read.
    #[test]
    fn the_browser_settings_of_older_versions_are_read() {
        let old = br#"{"wow_dir":"","edition":"classic","all_guides":true,"forgotten_characters":{"classic/A-B":3}}"#;
        let s: Settings = serde_json::from_slice(old).unwrap();
        assert!(s.all_guides && s.forgotten_characters["classic/A-B"] == 3);
        assert_eq!(s.edition, fg_route::xp::Edition::Classic);
    }
}
