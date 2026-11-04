//! The game on this computer: its folder (found, or taken back from one chosen too precisely),
//! what keeps the addon from going into it, and what the game did with the addon (loaded it,
//! when, which version, disabled for some characters).

use fg_route::xp::Edition;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The addon's two parts (folders under Interface/AddOns): the profile and recordings, always
/// installed, and the guide, which depends on it.
pub const PROFILE_ADDON: &str = "Factoruide_Profile";
pub const GUIDE_ADDON: &str = "Factoruide_Guide";

/// Game version folders found in a folder (_retail_, _classic_era_...).
pub fn client_folders(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .map(|d| {
            d.flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.len() > 2 && n.starts_with('_') && n.ends_with('_'))
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

fn is_game_root(dir: &Path) -> bool {
    dir.join(".build.info").is_file() || !client_folders(dir).is_empty()
}

/// Whether a folder holds a game version the app has guides for.
fn has_supported_client(dir: &Path) -> bool {
    Edition::ALL.iter().any(|e| e.installed_in(dir))
}

/// The World of Warcraft folder from the one the player chose: the folder holding the game
/// versions' folders (_classic_era_...). A version's folder or one inside it (Interface\AddOns,
/// WTF...) gives the folder above, the folder holding "World of Warcraft" gives that one; any
/// other is kept as it is.
pub fn game_root(dir: &Path) -> PathBuf {
    if let Some(root) = dir.ancestors().find(|d| is_game_root(d)) {
        return root.to_path_buf();
    }
    let below = dir.join("World of Warcraft");
    if is_game_root(&below) { below } else { dir.to_path_buf() }
}

/// Where World of Warcraft usually is, under a drive or a Wine prefix's drive_c.
const USUAL_PLACES: &[&str] = &[
    "Program Files (x86)/World of Warcraft",
    "Program Files/World of Warcraft",
    "World of Warcraft",
    "Games/World of Warcraft",
    "Jeux/World of Warcraft",
    "Blizzard/World of Warcraft",
    "Battle.net/World of Warcraft",
    "Program Files (x86)/Battle.net/World of Warcraft",
];

/// The folders where Battle.net installed its games: written in its product.db (a protobuf file),
/// as plain text paths ("C:/Program Files (x86)/World of Warcraft").
fn battle_net_paths(product_db: &Path) -> Vec<PathBuf> {
    let Ok(bytes) = std::fs::read(product_db) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for run in bytes.split(|b| !(0x20..0x7f).contains(b)) {
        let Ok(text) = std::str::from_utf8(run) else { continue };
        if !text.contains("World of Warcraft") {
            continue;
        }
        // The run may start with the string's length (a printable byte): from the drive on.
        let start = text
            .as_bytes()
            .windows(3)
            .position(|w| w[0].is_ascii_alphabetic() && w[1] == b':' && (w[2] == b'/' || w[2] == b'\\'));
        if let Some(start) = start {
            // Battle.net writes "/": the system's separator, as the folders chosen by hand.
            let path = &text[start..];
            out.push(PathBuf::from(if cfg!(windows) {
                path.replace('/', "\\")
            } else {
                path.to_owned()
            }));
        }
    }
    out
}

/// World of Warcraft folders found on this computer, holding a game version the app has guides
/// for, the most likely first.
pub fn detect() -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        let data = std::env::var_os("ProgramData").map_or_else(|| PathBuf::from(r"C:\ProgramData"), PathBuf::from);
        candidates.extend(battle_net_paths(&data.join(r"Battle.net\Agent\product.db")));
        for drive in b'C'..=b'Z' {
            let root = PathBuf::from(format!("{}:\\", drive as char));
            if root.is_dir() {
                candidates.extend(USUAL_PLACES.iter().map(|p| root.join(p)));
            }
        }
    } else if cfg!(target_os = "macos") {
        candidates.push(PathBuf::from("/Applications/World of Warcraft"));
        if let Some(home) = std::env::var_os("HOME") {
            candidates.push(PathBuf::from(home).join("Applications/World of Warcraft"));
        }
    } else {
        // WSL: the Windows drives; otherwise Wine prefixes (Lutris' Battle.net, the default one).
        for drive in b'c'..=b'z' {
            let root = PathBuf::from(format!("/mnt/{}", drive as char));
            if root.join("Windows").is_dir()
                || root.join("Program Files").is_dir()
                || root.join("Program Files (x86)").is_dir()
            {
                candidates.extend(USUAL_PLACES.iter().map(|p| root.join(p)));
                candidates.extend(battle_net_paths(&root.join("ProgramData/Battle.net/Agent/product.db")));
            }
        }
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            for prefix in [
                "Games/battlenet",
                "Games/battle-net",
                "Games/world-of-warcraft",
                ".wine",
            ] {
                let drive = home.join(prefix).join("drive_c");
                candidates.extend(USUAL_PLACES.iter().map(|p| drive.join(p)));
            }
        }
    }
    let mut found: Vec<PathBuf> = Vec::new();
    for c in candidates {
        // Battle.net's paths under WSL are Windows ones: their drive under /mnt.
        let c = wsl_path(&c).unwrap_or(c);
        if !c.is_dir() {
            continue;
        }
        let root = game_root(&c);
        if has_supported_client(&root) && !found.contains(&root) {
            found.push(root);
        }
    }
    found
}

/// A Windows path ("C:/Games/...") as WSL sees it (/mnt/c/Games/...), when the app runs there.
fn wsl_path(path: &Path) -> Option<PathBuf> {
    if cfg!(windows) || cfg!(target_os = "macos") {
        return None;
    }
    let text = path.to_str()?;
    let bytes = text.as_bytes();
    if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' {
        return None;
    }
    let rest = text[3..].replace('\\', "/");
    Some(PathBuf::from(format!(
        "/mnt/{}/{rest}",
        (bytes[0] as char).to_ascii_lowercase()
    )))
}

/// What keeps the addon from going into the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WowProblem {
    /// The folder does not exist.
    Missing,
    /// No game version in it.
    NoGame,
    /// Not the game version the app works on.
    NoEdition,
    /// The app may not write in the game's addons folder.
    NotWritable,
}

impl WowProblem {
    pub fn key(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::NoGame => "no_game",
            Self::NoEdition => "no_edition",
            Self::NotWritable => "not_writable",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct WowStatus {
    /// The game's folder, as it is saved (see `game_root`).
    pub root: PathBuf,
    /// The game versions' folders in it.
    pub clients: Vec<String>,
    pub problem: Option<WowProblem>,
}

/// The addons folder of a game version in the game's folder.
pub fn addons_dir(root: &Path, edition: Edition) -> PathBuf {
    edition.client_dir(root).join("Interface/AddOns")
}

/// Whether the app may write in a game version's addons folder (made when missing: the game
/// makes it only once an addon is there).
fn writable(addons: &Path) -> bool {
    if std::fs::create_dir_all(addons).is_err() {
        return false;
    }
    let probe = addons.join(".factoruide-write-test");
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

pub fn status(dir: &Path, edition: Edition) -> WowStatus {
    let root = game_root(dir);
    let clients = client_folders(&root);
    let problem = if !root.is_dir() {
        Some(WowProblem::Missing)
    } else if clients.is_empty() {
        Some(WowProblem::NoGame)
    } else if !clients.iter().any(|c| edition.has_client_folder(c)) {
        Some(WowProblem::NoEdition)
    } else if !writable(&addons_dir(&root, edition)) {
        Some(WowProblem::NotWritable)
    } else {
        None
    };
    WowStatus { root, clients, problem }
}

/// The version written in an addon's .toc ("## Version: 0.12.0").
pub fn toc_version(toc: &str) -> Option<String> {
    toc.lines()
        .find_map(|l| l.strip_prefix("## Version:"))
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

/// The files an addon's .toc loads (the game reads that list when it starts, not on /reload).
pub fn toc_files(toc: &str) -> Vec<String> {
    toc.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.replace('\\', "/").to_lowercase())
        .collect()
}

/// When the addon last ran in game and in which version (`FactoruideDB.seen`, written when it
/// loads; the game saves it at a /reload, a logout or when it quits).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Seen {
    pub time: i64,
    pub version: String,
}

/// `FactoruideDB.seen` in a SavedVariables file, read from its text (the whole file need not run).
pub fn seen_in(saved_variables: &str) -> Option<Seen> {
    let start = saved_variables.find("[\"seen\"] = {")?;
    let body = &saved_variables[start..];
    let body = &body[..body.find('}')?];
    let field = |name: &str| {
        let at = body.find(&format!("[\"{name}\"] = "))? + name.len() + 6;
        let value = body[at..].split([',', '\n']).next()?.trim();
        Some(value.trim_matches('"').to_owned())
    };
    Some(Seen {
        time: field("time")?.parse().ok()?,
        version: field("version")?,
    })
}

fn modified_secs(path: &Path) -> Option<i64> {
    let time = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    i64::try_from(time.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs()).ok()
}

/// What the game did with the installed addon.
#[derive(Debug, Default, Serialize)]
pub struct AddonHealth {
    pub app_version: String,
    /// The version of the addon installed (none: not installed).
    pub installed_version: Option<String>,
    /// The oldest version the app works with.
    pub required_version: String,
    /// Older than that: to update.
    pub outdated: bool,
    /// When it was installed (seconds).
    pub installed_at: Option<i64>,
    /// Both parts are there (the guide part is optional for the app: another addon may show the
    /// guides).
    pub complete: bool,
    /// The last time the addon ran in game, on any account.
    pub seen: Option<Seen>,
    /// The game saved its addons' data after the install, but the addon did not run: the game
    /// did not load it (turned off in its AddOns list, or out of date).
    pub not_loaded: bool,
    /// Characters ("Name - Realm") the addon is turned off for in the game's AddOns list.
    pub disabled_for: Vec<String>,
    /// Number of game accounts with the addon's SavedVariables.
    pub saved_variables: u32,
}

/// What the game did with the addon of a game version (`required`: the oldest version the app
/// works with).
pub fn addon_health(root: &Path, edition: Edition, required: &str) -> AddonHealth {
    let client = edition.client_dir(root);
    let addons = addons_dir(root, edition);
    let toc = addons.join(PROFILE_ADDON).join(format!("{PROFILE_ADDON}.toc"));
    let installed_version = std::fs::read_to_string(&toc).ok().as_deref().and_then(toc_version);
    let mut health = AddonHealth {
        app_version: env!("CARGO_PKG_VERSION").into(),
        outdated: installed_version.as_deref().is_some_and(|v| fg_app::older(v, required)),
        installed_version,
        required_version: required.to_owned(),
        installed_at: modified_secs(&toc),
        complete: [PROFILE_ADDON, GUIDE_ADDON]
            .iter()
            .all(|a| addons.join(a).join(format!("{a}.toc")).is_file()),
        ..AddonHealth::default()
    };
    // The game writes every loaded addon's SavedVariables together: one newer than the install
    // while the addon's is older (or missing) is a game that ran without it.
    let mut newest_other: Option<i64> = None;
    let Ok(accounts) = std::fs::read_dir(client.join("WTF/Account")) else {
        return health;
    };
    for account in accounts.flatten() {
        let dir = account.path().join("SavedVariables");
        let own = dir.join(format!("{PROFILE_ADDON}.lua"));
        if let Some(seen) = std::fs::read_to_string(&own).ok().as_deref().and_then(seen_in) {
            health.saved_variables += 1;
            if health.seen.as_ref().is_none_or(|s| s.time < seen.time) {
                health.seen = Some(seen);
            }
        }
        for file in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = file.path();
            if path != own && path.extension().is_some_and(|e| e == "lua") {
                newest_other = newest_other.max(modified_secs(&path));
            }
        }
        // <account>/<realm>/<character>/AddOns.txt: "Factoruide_Profile: disabled".
        for realm in std::fs::read_dir(account.path()).into_iter().flatten().flatten() {
            for character in std::fs::read_dir(realm.path()).into_iter().flatten().flatten() {
                let Ok(list) = std::fs::read_to_string(character.path().join("AddOns.txt")) else {
                    continue;
                };
                let off = list.lines().any(|l| {
                    let l = l.trim();
                    (l.starts_with(PROFILE_ADDON) || l.starts_with(GUIDE_ADDON)) && l.ends_with("disabled")
                });
                if off {
                    health.disabled_for.push(format!(
                        "{} - {}",
                        character.file_name().to_string_lossy(),
                        realm.file_name().to_string_lossy()
                    ));
                }
            }
        }
    }
    health.disabled_for.sort();
    if let (Some(installed), Some(other)) = (health.installed_at, newest_other) {
        let seen_after = health.seen.as_ref().is_some_and(|s| s.time >= installed);
        // A minute's margin: the install and the game's last save may be that close.
        health.not_loaded = other > installed + 60 && !seen_after;
    }
    health
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fg-game-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A version's folder, a folder inside it or the folder holding the game's are taken back to
    /// the game's folder; the addon never goes where the game would not read it.
    #[test]
    fn the_game_folder_is_found_from_a_too_precise_or_too_wide_one() {
        let base = temp("root");
        let root = base.join("World of Warcraft");
        let addons = root.join("_classic_era_/Interface/AddOns");
        std::fs::create_dir_all(&addons).unwrap();
        std::fs::write(root.join(".build.info"), "").unwrap();
        assert_eq!(game_root(&root.join("_classic_era_")), root);
        assert_eq!(game_root(&addons), root);
        assert_eq!(game_root(&root), root);
        assert_eq!(game_root(&base), root);
        let elsewhere = std::env::temp_dir().join(format!("fg-not-wow-{}", std::process::id()));
        assert_eq!(game_root(&elsewhere), elsewhere);

        assert_eq!(status(&addons, Edition::Classic).problem, None);
        assert_eq!(status(&root, Edition::Tbc).problem, Some(WowProblem::NoEdition));
        assert_eq!(status(&elsewhere, Edition::Classic).problem, Some(WowProblem::Missing));
        std::fs::create_dir_all(&elsewhere).unwrap();
        assert_eq!(status(&elsewhere, Edition::Classic).problem, Some(WowProblem::NoGame));
        std::fs::remove_dir_all(&base).unwrap();
        std::fs::remove_dir_all(&elsewhere).unwrap();
    }

    #[test]
    fn battle_net_tells_where_it_installed_the_game() {
        let dir = temp("bnet");
        let mut db = vec![0x0a, 0x12, 0x0a];
        db.push(b'+');
        db.extend_from_slice(b"D:/Jeux/World of Warcraft");
        db.extend_from_slice(&[0x12, 0x05]);
        db.extend_from_slice(b"enUS");
        std::fs::write(dir.join("product.db"), &db).unwrap();
        assert_eq!(
            battle_net_paths(&dir.join("product.db")),
            [PathBuf::from("D:/Jeux/World of Warcraft")]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_last_run_is_read_from_the_saved_variables() {
        let text = "FactoruideDB = {\n[\"version\"] = 3,\n[\"seen\"] = {\n[\"version\"] = \"0.13.0\",\n[\"time\"] = 1760000000,\n},\n}\n";
        assert_eq!(
            seen_in(text),
            Some(Seen {
                time: 1_760_000_000,
                version: "0.13.0".into()
            })
        );
        assert_eq!(seen_in("FactoruideDB = {}"), None);
    }

    /// The game saved other addons' data after the install, not this one's: it did not load it;
    /// the characters it is turned off for are named.
    #[test]
    fn a_game_that_ran_without_the_addon_is_told() {
        let root = temp("health");
        let client = root.join("_classic_era_");
        let addon = addons_dir(&root, Edition::Classic).join(PROFILE_ADDON);
        std::fs::create_dir_all(&addon).unwrap();
        std::fs::write(addon.join(format!("{PROFILE_ADDON}.toc")), "## Version: 0.13.0\n").unwrap();
        let account = client.join("WTF/Account/ACC");
        std::fs::create_dir_all(account.join("SavedVariables")).unwrap();
        std::fs::create_dir_all(account.join("Realm/Thrall")).unwrap();
        std::fs::write(
            account.join("Realm/Thrall/AddOns.txt"),
            "Factoruide_Profile: disabled\n",
        )
        .unwrap();
        let health = addon_health(&root, Edition::Classic, "0.14.0");
        assert_eq!(health.installed_version.as_deref(), Some("0.13.0"));
        assert!(health.outdated, "older than the app needs");
        assert!(!addon_health(&root, Edition::Classic, "0.13.0").outdated);
        assert!(!health.complete, "the guide part is missing");
        assert!(!health.not_loaded && health.seen.is_none());
        assert_eq!(health.disabled_for, ["Thrall - Realm"]);

        // Another addon's data saved two minutes after the install.
        let other = account.join("SavedVariables/Other.lua");
        std::fs::write(&other, "").unwrap();
        let later = SystemTime::now() + std::time::Duration::from_secs(120);
        std::fs::File::options()
            .write(true)
            .open(&other)
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert!(addon_health(&root, Edition::Classic, "0.13.0").not_loaded);

        // The addon ran after the install.
        let now = modified_secs(&other).unwrap();
        std::fs::write(
            account.join(format!("SavedVariables/{PROFILE_ADDON}.lua")),
            format!("FactoruideDB = {{\n[\"seen\"] = {{\n[\"time\"] = {now},\n[\"version\"] = \"0.13.0\",\n}},\n}}\n"),
        )
        .unwrap();
        let health = addon_health(&root, Edition::Classic, "0.13.0");
        assert!(!health.not_loaded && health.saved_variables == 1);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
