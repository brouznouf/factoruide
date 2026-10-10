// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use fg_route::Route;
use fg_route::job::{self, Options, Overrides, PlanOutcome, PlanRequest};
use fg_route::xp::Edition;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};

/// Game database, overrides and addon, embedded at build time (see build.rs).
mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded.rs"));
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Settings {
    wow_dir: PathBuf,
    /// Game version the app works on (forever, classic, tbc).
    #[serde(default)]
    edition: Edition,
    /// App language (also the default guide language); unset: the system language.
    #[serde(default)]
    language: Option<String>,
    /// Contribution server (its base address); unset: the one the app was built with.
    #[serde(default)]
    contribution_url: Option<String>,
    /// Random id of this install, sent with its contributions (tells how many different
    /// players back what they report; nothing else about the player).
    #[serde(default)]
    contributor: Option<String>,
    /// When this computer last sent one (unix seconds).
    #[serde(default)]
    last_contribution: Option<u64>,
    /// Addon behaviour, written to the addon's Config.lua on install.
    #[serde(default)]
    addon: serde_json::Map<String, serde_json::Value>,
}

impl Default for Settings {
    fn default() -> Self {
        let wow_dir = if cfg!(windows) {
            PathBuf::from(r"C:\Program Files (x86)\World of Warcraft")
        } else {
            PathBuf::from("/mnt/c/Program Files (x86)/World of Warcraft")
        };
        Self {
            wow_dir,
            edition: Edition::Forever,
            language: None,
            contribution_url: None,
            contributor: None,
            last_contribution: None,
            addon: serde_json::Map::new(),
        }
    }
}

impl Settings {
    /// The addon folder of the edition's game client.
    fn installed_addon(&self) -> PathBuf {
        self.wow_dir
            .join(self.edition.client_folder())
            .join("Interface/AddOns/Factoruide")
    }
}

struct AppState {
    settings: Mutex<Settings>,
    config_file: PathBuf,
    /// Where the embedded databases are unpacked, and those unpacked so far.
    db_dir: PathBuf,
    dbs: Mutex<std::collections::HashMap<Edition, PathBuf>>,
    /// Saved guides and debug exports.
    data_dir: PathBuf,
}

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    format!("{e:#}")
}

/// Embedded overrides of an edition.
fn overrides(edition: Edition) -> CmdResult<Overrides> {
    Overrides::parse_edition(
        |name| {
            let (_, bytes) = embedded::OVERRIDES.iter().find(|(n, _)| *n == name)?;
            Some(String::from_utf8_lossy(bytes).into_owned())
        },
        edition,
    )
    .map_err(err)
}

impl AppState {
    fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }
    fn edition(&self) -> Edition {
        self.settings.lock().unwrap().edition
    }
    /// The database of an edition, unpacked the first time.
    fn db_path(&self, edition: Edition) -> CmdResult<PathBuf> {
        if let Some(p) = self.dbs.lock().unwrap().get(&edition) {
            return Ok(p.clone());
        }
        let path = unpack_database(&self.db_dir, edition).map_err(err)?;
        self.dbs.lock().unwrap().insert(edition, path.clone());
        Ok(path)
    }
    fn open_db(&self) -> CmdResult<Connection> {
        open_db(&self.db_path(self.edition())?)
    }
}

fn open_db(path: &Path) -> CmdResult<Connection> {
    Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Unpack an edition's embedded database into `dir` once per build (the file name carries its
/// hash), removing the ones of previous builds.
fn unpack_database(dir: &Path, edition: Edition) -> anyhow::Result<PathBuf> {
    use std::hash::{Hash, Hasher};
    let Some((_, data)) = embedded::DATABASES.iter().find(|(e, _)| *e == edition.key()) else {
        anyhow::bail!("no data for {} in this build", edition.rules().edition_name);
    };
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut hasher);
    let prefix = format!("{}-", edition.key());
    let name = format!("{prefix}{:016x}.sqlite", hasher.finish());
    let path = dir.join(&name);
    if !path.exists() {
        std::fs::create_dir_all(dir)?;
        for entry in std::fs::read_dir(dir)?.flatten() {
            let n = entry.file_name().to_string_lossy().into_owned();
            if (n.starts_with(&prefix) || n.starts_with("factoruide-")) && n.ends_with(".sqlite") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        let tmp = dir.join(format!("{name}.tmp"));
        std::fs::write(&tmp, zstd::decode_all(*data)?)?;
        std::fs::rename(&tmp, &path)?;
    }
    Ok(path)
}

/// An edition the app knows: whether its data is bundled and its game client installed.
#[derive(Serialize)]
struct EditionInfo {
    key: &'static str,
    name: &'static str,
    /// Data bundled in this build.
    available: bool,
    /// Game client folder found in the WoW folder.
    installed: bool,
    max_level: i64,
}

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "tauri commands take their arguments by value"
)]
fn list_editions(state: State<'_, AppState>) -> Vec<EditionInfo> {
    let settings = state.settings();
    Edition::ALL
        .iter()
        .map(|e| EditionInfo {
            key: e.key(),
            name: e.rules().edition_name,
            available: embedded::DATABASES.iter().any(|(k, _)| *k == e.key()) && e.rules().known(),
            installed: settings.wow_dir.join(e.client_folder()).is_dir(),
            max_level: e.rules().max_level,
        })
        .collect()
}

/// Routes and results used to live in the Factoruide checkout (`workspace` setting): copy them
/// into the app data folder the first time.
fn migrate_workspace(config: &serde_json::Value, data_dir: &Path) {
    let Some(workspace) = config.get("workspace").and_then(|w| w.as_str()).map(PathBuf::from) else {
        return;
    };
    {
        let folder = "results";
        let (from, to) = (workspace.join(folder), data_dir.join(folder));
        if from.is_dir() && !to.exists() {
            let _ = copy_dir(&from, &to);
        }
    }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            copy_dir(&path, &to.join(entry.file_name()))?;
        } else {
            std::fs::copy(&path, to.join(entry.file_name()))?;
        }
    }
    Ok(())
}

/// Whether this build checks for updates: only the published ones (`FG_RELEASE` set by the
/// release workflow), not development or local builds, whose version is not a release's.
#[tauri::command]
fn updates_enabled() -> bool {
    option_env!("FG_RELEASE").is_some()
}

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "tauri commands take their arguments by value"
)]
fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings()
}

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "tauri commands take their arguments by value"
)]
fn set_settings(state: State<'_, AppState>, mut settings: Settings) -> CmdResult<()> {
    // Not edited by the settings page: kept.
    if settings.contributor.is_none() {
        settings.contributor = state.settings().contributor;
    }
    write_settings(&state, settings)
}

fn write_settings(state: &AppState, settings: Settings) -> CmdResult<()> {
    if let Some(dir) = state.config_file.parent() {
        std::fs::create_dir_all(dir).map_err(err)?;
    }
    std::fs::write(
        &state.config_file,
        serde_json::to_string_pretty(&settings).map_err(err)?,
    )
    .map_err(err)?;
    *state.settings.lock().unwrap() = settings;
    Ok(())
}

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "tauri commands take their arguments by value"
)]
fn get_options(state: State<'_, AppState>) -> CmdResult<Options> {
    job::options(&state.open_db()?, &overrides(state.edition())?).map_err(err)
}

/// Save a planning request for debugging (replay with `fg route --request <file>`).
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "tauri commands take their arguments by value"
)]
fn export_request(state: State<'_, AppState>, request: PlanRequest) -> CmdResult<String> {
    let dir = state.data_dir.join("debug");
    std::fs::create_dir_all(&dir).map_err(err)?;
    let name = request
        .name
        .clone()
        .unwrap_or_else(|| format!("{}-{}", request.race, request.class));
    let path = dir.join(format!("request-{name}.json"));
    std::fs::write(&path, serde_json::to_string_pretty(&request).map_err(err)?).map_err(err)?;
    Ok(path.display().to_string())
}

/// Game version folders found in the WoW folder (_retail_, _classic_era_...).
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "tauri commands take their arguments by value"
)]
fn list_clients(wow_dir: PathBuf) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(&wow_dir)
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

// Guides ---------------------------------------------------------------------------
//
// A guide is a name and its versions: every computation (first one, or computed again with the
// same name) adds a version. One version per guide can be installed in the addon.
//
//   guides/<slug>/guide.json          name, installed version
//   guides/<slug>/v<N>/meta.json      summary of the version
//   guides/<slug>/v<N>/request.json   configuration
//   guides/<slug>/v<N>/outcome.json   computed guide

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GuideInfo {
    name: String,
    /// Version installed in the addon.
    #[serde(default)]
    installed: Option<u32>,
}

/// Summary of one version.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct VersionMeta {
    /// Guide folder (slug of the name).
    guide: String,
    version: u32,
    name: String,
    /// Unix seconds.
    created: u64,
    race: String,
    class: String,
    #[serde(default)]
    group: Vec<String>,
    from_level: i64,
    to_level: i64,
    total_time: f64,
    total_time_text: String,
    quests: usize,
    /// Game data used (QuestieDB import).
    data: Option<String>,
    /// Received from someone else (imported from a .fgguide file).
    #[serde(default)]
    imported: bool,
}

/// A guide with its versions, newest first.
#[derive(Serialize)]
struct GuideSummary {
    id: String,
    name: String,
    installed: Option<u32>,
    versions: Vec<VersionMeta>,
}

#[derive(Serialize, Deserialize)]
struct GuideVersion {
    meta: VersionMeta,
    request: PlanRequest,
    outcome: PlanOutcome,
}

fn now() -> CmdResult<u64> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(err)?
        .as_secs())
}

impl AppState {
    /// Guides of the current edition (`guides/<edition>/`; the app switches edition only
    /// when no generation is running).
    fn guides(&self) -> PathBuf {
        self.data_dir.join("guides").join(self.edition().key())
    }

    fn guide_info(&self, id: &str) -> CmdResult<GuideInfo> {
        let path = self.guides().join(id).join("guide.json");
        serde_json::from_str(&std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?)
            .map_err(err)
    }

    fn write_guide_info(&self, id: &str, info: &GuideInfo) -> CmdResult<()> {
        let dir = self.guides().join(id);
        std::fs::create_dir_all(&dir).map_err(err)?;
        std::fs::write(dir.join("guide.json"), serde_json::to_string_pretty(info).map_err(err)?).map_err(err)
    }

    fn versions(&self, id: &str) -> Vec<VersionMeta> {
        let mut out: Vec<VersionMeta> = std::fs::read_dir(self.guides().join(id))
            .map(|d| {
                d.flatten()
                    .filter(|e| e.file_name().to_string_lossy().starts_with('v'))
                    .filter_map(|e| {
                        serde_json::from_str(&std::fs::read_to_string(e.path().join("meta.json")).ok()?).ok()
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.sort_by_key(|v| std::cmp::Reverse(v.version));
        out
    }

    fn version(&self, id: &str, version: u32) -> CmdResult<GuideVersion> {
        let folder = self.guides().join(id).join(format!("v{version}"));
        let read = |f: &str| std::fs::read_to_string(folder.join(f)).map_err(|e| format!("{f}: {e}"));
        Ok(GuideVersion {
            meta: serde_json::from_str(&read("meta.json")?).map_err(err)?,
            request: serde_json::from_str(&read("request.json")?).map_err(err)?,
            outcome: serde_json::from_str(&read("outcome.json")?).map_err(err)?,
        })
    }

    /// Add a version to the guide of this name (created when new).
    fn add_version(
        &self,
        request: &PlanRequest,
        outcome: &PlanOutcome,
        data: Option<String>,
        imported: bool,
        created: u64,
    ) -> CmdResult<VersionMeta> {
        let route = &outcome.route;
        let id = job::slug(&route.name);
        let info = self.guide_info(&id).unwrap_or(GuideInfo {
            name: route.name.clone(),
            installed: None,
        });
        self.write_guide_info(
            &id,
            &GuideInfo {
                name: route.name.clone(),
                ..info
            },
        )?;
        let version = self.versions(&id).first().map_or(1, |v| v.version + 1);
        let meta = VersionMeta {
            guide: id.clone(),
            version,
            name: route.name.clone(),
            created,
            race: request.race.clone(),
            class: request.class.clone(),
            group: request.group.clone(),
            from_level: route.from_level,
            to_level: route.to_level,
            total_time: route.total_time,
            total_time_text: route.total_time_text.clone(),
            quests: route.quests,
            data,
            imported,
        };
        let folder = self.guides().join(&id).join(format!("v{version}"));
        std::fs::create_dir_all(&folder).map_err(err)?;
        std::fs::write(
            folder.join("meta.json"),
            serde_json::to_string_pretty(&meta).map_err(err)?,
        )
        .map_err(err)?;
        std::fs::write(
            folder.join("request.json"),
            serde_json::to_string_pretty(request).map_err(err)?,
        )
        .map_err(err)?;
        std::fs::write(
            folder.join("outcome.json"),
            serde_json::to_string(outcome).map_err(err)?,
        )
        .map_err(err)?;
        Ok(meta)
    }

    /// The installed version of every guide.
    fn installed_routes(&self) -> Vec<Route> {
        let Ok(dir) = std::fs::read_dir(self.guides()) else {
            return Vec::new();
        };
        let mut routes: Vec<Route> = dir
            .flatten()
            .filter_map(|e| {
                let id = e.file_name().to_string_lossy().into_owned();
                let v = self.guide_info(&id).ok()?.installed?;
                Some(self.version(&id, v).ok()?.outcome.route)
            })
            .collect();
        routes.sort_by(|a, b| a.name.cmp(&b.name));
        routes
    }
}

/// Results of earlier versions of the app (`results/<id>/`): each becomes a version of the
/// guide of its name, oldest first.
fn migrate_results(state: &AppState) {
    #[derive(Deserialize)]
    struct OldMeta {
        created: u64,
        data: Option<String>,
        #[serde(default)]
        imported: bool,
    }
    let old = state.data_dir.join("results");
    let Ok(dir) = std::fs::read_dir(&old) else { return };
    let mut found: Vec<(OldMeta, PlanRequest, PlanOutcome)> = dir
        .flatten()
        .filter_map(|e| {
            let read = |f: &str| std::fs::read_to_string(e.path().join(f)).ok();
            Some((
                serde_json::from_str(&read("meta.json")?).ok()?,
                serde_json::from_str(&read("request.json")?).ok()?,
                serde_json::from_str(&read("outcome.json")?).ok()?,
            ))
        })
        .collect();
    found.sort_by_key(|(m, _, _)| m.created);
    for (meta, request, outcome) in &found {
        let _ = state.add_version(request, outcome, meta.data.clone(), meta.imported, meta.created);
    }
    let _ = std::fs::rename(&old, state.data_dir.join("results.old"));
}

/// Plan a guide on a worker thread, streaming progress as `plan-progress` events. The result
/// is saved as a new version of the guide of the request's name.
#[tauri::command]
async fn plan_route(app: AppHandle, request: PlanRequest) -> CmdResult<VersionMeta> {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app2.state::<AppState>();
        // The edition the configuration was made for (the current one when unset).
        let edition = request.params.as_ref().map_or_else(|| state.edition(), |p| p.edition);
        let conn = open_db(&state.db_path(edition)?)?;
        let overrides = overrides(edition)?;
        let progress = |m: &str| {
            let _ = app.emit("plan-progress", m.to_owned());
        };
        let outcome = job::plan(&conn, &overrides, &request, &progress).map_err(err)?;
        let data = conn
            .query_row("SELECT detail FROM import_run WHERE source = 'questie'", [], |r| {
                r.get::<_, Option<String>>(0)
            })
            .ok()
            .flatten();
        state.add_version(&request, &outcome, data, false, now()?)
    })
    .await
    .map_err(err)?
}

fn summary(state: &AppState, id: &str) -> CmdResult<GuideSummary> {
    let info = state.guide_info(id)?;
    Ok(GuideSummary {
        id: id.to_owned(),
        name: info.name,
        installed: info.installed,
        versions: state.versions(id),
    })
}

/// Every guide, most recently computed first.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "tauri commands take their arguments by value"
)]
fn list_guides(state: State<'_, AppState>) -> Vec<GuideSummary> {
    let Ok(dir) = std::fs::read_dir(state.guides()) else {
        return Vec::new();
    };
    let mut out: Vec<GuideSummary> = dir
        .flatten()
        .filter_map(|e| summary(&state, &e.file_name().to_string_lossy()).ok())
        .filter(|g| !g.versions.is_empty())
        .collect();
    out.sort_by(|a, b| b.versions[0].created.cmp(&a.versions[0].created));
    out
}

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "tauri commands take their arguments by value"
)]
fn get_guide(state: State<'_, AppState>, id: String) -> CmdResult<GuideSummary> {
    summary(&state, &id)
}

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "tauri commands take their arguments by value"
)]
fn get_version(state: State<'_, AppState>, guide: String, version: u32) -> CmdResult<GuideVersion> {
    state.version(&guide, version)
}

/// Install `version` of a guide in the addon (`None`: remove the guide from the addon).
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "tauri commands take their arguments by value"
)]
fn set_installed(state: State<'_, AppState>, guide: String, version: Option<u32>) -> CmdResult<usize> {
    let info = state.guide_info(&guide)?;
    state.write_guide_info(
        &guide,
        &GuideInfo {
            installed: version,
            ..info
        },
    )?;
    install_addon(state)
}

#[tauri::command]
fn delete_version(state: State<'_, AppState>, guide: String, version: u32) -> CmdResult<()> {
    std::fs::remove_dir_all(state.guides().join(&guide).join(format!("v{version}"))).map_err(err)?;
    let info = state.guide_info(&guide)?;
    if state.versions(&guide).is_empty() {
        delete_guide(state, guide)
    } else if info.installed == Some(version) {
        state.write_guide_info(
            &guide,
            &GuideInfo {
                installed: None,
                ..info
            },
        )?;
        install_addon(state).map(|_| ())
    } else {
        Ok(())
    }
}

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "tauri commands take their arguments by value"
)]
fn delete_guide(state: State<'_, AppState>, guide: String) -> CmdResult<()> {
    let installed = state.guide_info(&guide).ok().and_then(|i| i.installed).is_some();
    std::fs::remove_dir_all(state.guides().join(&guide)).map_err(err)?;
    if installed {
        install_addon(state)?;
    }
    Ok(())
}

/// Copy the addon into the game, with Routes.lua holding the installed version of each guide
/// and Config.lua the settings. Returns the number of guides installed.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "tauri commands take their arguments by value"
)]
fn install_addon(state: State<'_, AppState>) -> CmdResult<usize> {
    let settings = state.settings();
    let dest = settings.installed_addon();
    migrate_legacy_addon(&dest);
    std::fs::create_dir_all(&dest).map_err(err)?;
    for entry in std::fs::read_dir(&dest).map_err(err)?.flatten() {
        let p = entry.path();
        if p.extension().is_some_and(|e| e == "lua" || e == "toc" || e == "xml") {
            std::fs::remove_file(p).map_err(err)?;
        }
    }
    for (name, bytes) in embedded::ADDON {
        let path = dest.join(name);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(err)?;
        }
        std::fs::write(path, bytes).map_err(err)?;
    }
    let routes = state.installed_routes();
    std::fs::write(dest.join("Routes.lua"), fg_route::export::to_lua(&routes)).map_err(err)?;
    job::write_addon_config(&dest, &settings.addon).map_err(err)?;
    Ok(routes.len())
}

/// The addon was called BrouzQuest: drop its folder (the game would load both) and carry its
/// SavedVariables over to the new name, once.
fn migrate_legacy_addon(dest: &Path) {
    let Some(addons) = dest.parent() else { return };
    let _ = std::fs::remove_dir_all(addons.join("BrouzQuest"));
    let Some(client) = addons.parent().and_then(Path::parent) else {
        return;
    };
    let Ok(accounts) = std::fs::read_dir(client.join("WTF/Account")) else {
        return;
    };
    for account in accounts.flatten() {
        let dir = account.path().join("SavedVariables");
        let (old, new) = (dir.join("BrouzQuest.lua"), dir.join("Factoruide.lua"));
        if new.exists() {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&old)
            && std::fs::write(&new, text.replace("BrouzQuestDB", "FactoruideDB")).is_ok()
        {
            let _ = std::fs::rename(&old, dir.join("BrouzQuest.lua.migrated"));
        }
    }
}

/// What a .fgguide (formerly .bqroute) file holds: a guide version (configuration and computed
/// guide) to share.
#[derive(Serialize, Deserialize)]
struct SharedGuide {
    format: String,
    version: u32,
    meta: serde_json::Value,
    request: PlanRequest,
    outcome: PlanOutcome,
}

const SHARE_FORMAT: &str = "factoruide-guide";
/// Format of the files shared before the rename (BrouzQuest), still accepted.
const LEGACY_SHARE_FORMAT: &str = "brouzquest-result";

/// Write a version as a .fgguide file in the Downloads folder; returns its path.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "tauri commands take their arguments by value"
)]
fn export_version(app: AppHandle, state: State<'_, AppState>, guide: String, version: u32) -> CmdResult<String> {
    let v = state.version(&guide, version)?;
    let dir = app.path().download_dir().map_err(err)?;
    let path = dir.join(format!("{guide}-v{version}.fgguide"));
    let shared = SharedGuide {
        format: SHARE_FORMAT.into(),
        version: 2,
        meta: serde_json::to_value(&v.meta).map_err(err)?,
        request: v.request,
        outcome: v.outcome,
    };
    std::fs::write(&path, serde_json::to_string(&shared).map_err(err)?).map_err(err)?;
    Ok(path.display().to_string())
}

/// Add a guide received as a .fgguide file (its content): a new version of the guide of its name.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "tauri commands take their arguments by value"
)]
fn import_guide(state: State<'_, AppState>, content: String) -> CmdResult<VersionMeta> {
    let shared: SharedGuide =
        serde_json::from_str(&content).map_err(|e| format!("not a Factoruide guide file: {e}"))?;
    if shared.format != SHARE_FORMAT && shared.format != LEGACY_SHARE_FORMAT {
        return Err("not a Factoruide guide file".into());
    }
    let data = shared.meta.get("data").and_then(|d| d.as_str()).map(str::to_owned);
    state.add_version(&shared.request, &shared.outcome, data, true, now()?)
}

// Maps -----------------------------------------------------------------------------

/// Map index written by `fg maps` (ids, names, sizes, world bounds).
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "tauri commands take their arguments by value"
)]
fn map_index(state: State<'_, AppState>) -> CmdResult<serde_json::Value> {
    let db = state.open_db()?;
    let text: String = db
        .query_row("SELECT value FROM app_meta WHERE key = 'map_index'", [], |r| r.get(0))
        .map_err(err)?;
    let mut index: serde_json::Value = serde_json::from_str(&text).map_err(err)?;
    // Zone names in the game's languages (`names`: locale -> name), when the data has them.
    let mut names: std::collections::HashMap<i64, serde_json::Map<String, serde_json::Value>> =
        std::collections::HashMap::new();
    if let Ok(mut stmt) =
        db.prepare("SELECT id, locale, name FROM l10n WHERE entity_type = 'zone' AND name IS NOT NULL")
    {
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
            })
            .map_err(err)?;
        for (id, locale, name) in rows.flatten() {
            names.entry(id).or_default().insert(locale, name.into());
        }
    }
    for map in index.as_array_mut().into_iter().flatten() {
        if let Some(n) = map["area"].as_i64().and_then(|a| names.get(&a)) {
            map["names"] = serde_json::Value::Object(n.clone());
        }
    }
    Ok(index)
}

/// A map image as a data URL.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "tauri commands take their arguments by value"
)]
fn map_image(state: State<'_, AppState>, id: i64) -> CmdResult<String> {
    use base64::Engine;
    let bytes: Vec<u8> = state
        .open_db()?
        .query_row("SELECT jpeg FROM app_map_image WHERE id = ?1", [id], |r| r.get(0))
        .map_err(err)?;
    Ok(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// WebKitGTK renders a blank window under WSLg (no usable GPU for DMA-BUF/EGL):
/// fall back to its software paths when running inside WSL.
#[expect(
    unsafe_code,
    reason = "set_var is unsafe in edition 2024; this runs before any thread is spawned"
)]
fn wsl_rendering_workaround() {
    let in_wsl = std::fs::read_to_string("/proc/version").is_ok_and(|v| v.to_lowercase().contains("microsoft"));
    if in_wsl {
        for var in ["WEBKIT_DISABLE_DMABUF_RENDERER", "WEBKIT_DISABLE_COMPOSITING_MODE"] {
            if std::env::var_os(var).is_none() {
                // SAFETY: called at startup, before any other thread exists.
                unsafe { std::env::set_var(var, "1") };
            }
        }
    }
}

fn main() {
    if cfg!(target_os = "linux") {
        wsl_rendering_workaround();
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            migrate_app_dirs(app.handle());
            let config_file = app.path().app_config_dir()?.join("settings.json");
            let data_dir = app.path().app_data_dir()?;
            let config: Option<serde_json::Value> = std::fs::read_to_string(&config_file)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok());
            if let Some(config) = &config {
                migrate_workspace(config, &data_dir);
            }
            let settings = config.and_then(|c| serde_json::from_value(c).ok()).unwrap_or_default();
            let db_dir = app.path().app_local_data_dir()?;
            app.manage(AppState {
                settings: Mutex::new(settings),
                config_file,
                db_dir,
                dbs: Mutex::new(std::collections::HashMap::default()),
                data_dir,
            });
            migrate_guides(&app.state::<AppState>().data_dir);
            migrate_results(&app.state::<AppState>());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            updates_enabled,
            get_settings,
            set_settings,
            get_options,
            plan_route,
            export_request,
            install_addon,
            list_clients,
            list_editions,
            set_edition,
            contribution_preview,
            send_contribution,
            list_guides,
            get_guide,
            get_version,
            set_installed,
            delete_version,
            delete_guide,
            export_version,
            import_guide,
            map_index,
            map_image,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Factoruide");
}

/// Switch the edition the app works on (saved with the settings).
#[tauri::command]
fn set_edition(state: State<'_, AppState>, edition: Edition) -> CmdResult<()> {
    let mut settings = state.settings();
    settings.edition = edition;
    set_settings(state, settings)
}

/// Guides made before editions existed (`guides/<slug>/`) are WoW Forever guides.
/// The app was called BrouzQuest (identifier fr.brouz.brouzquest): move its settings, guides and
/// databases to the new identifier's folders, unless these already exist.
fn migrate_app_dirs(app: &AppHandle) {
    let path = app.path();
    for dir in [path.app_config_dir(), path.app_data_dir(), path.app_local_data_dir()]
        .into_iter()
        .flatten()
    {
        let Some(old) = dir.parent().map(|p| p.join("fr.brouz.brouzquest")) else {
            continue;
        };
        if old.is_dir() && !dir.exists() {
            let _ = std::fs::rename(&old, &dir);
        }
    }
}

fn migrate_guides(data_dir: &Path) {
    let root = data_dir.join("guides");
    let Ok(dir) = std::fs::read_dir(&root) else { return };
    for entry in dir.flatten() {
        if entry.path().join("guide.json").is_file() {
            let dest = root.join(Edition::Forever.key());
            let _ = std::fs::create_dir_all(&dest);
            let _ = std::fs::rename(entry.path(), dest.join(entry.file_name()));
        }
    }
}

// Contributions --------------------------------------------------------------------

/// What a contribution of this computer would hold, for the current game version.
#[derive(Serialize)]
struct ContributionPreview {
    edition: &'static str,
    /// The game client folder read.
    client_dir: String,
    found: bool,
    quests: usize,
    events: usize,
    characters: usize,
    locales: Vec<String>,
    /// Size of the JSON sent (bytes).
    size: usize,
    url: Option<String>,
    last_sent: Option<u64>,
}

/// The contribution server the app was built with (`FG_CONTRIBUTION_SERVER` at build time).
const DEFAULT_CONTRIBUTION_SERVER: Option<&str> = option_env!("FG_CONTRIBUTION_SERVER");

/// Where contributions are posted, from the configured (or built-in) server address.
fn contribution_endpoint(settings: &Settings) -> Option<String> {
    let base = settings
        .contribution_url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .or(DEFAULT_CONTRIBUTION_SERVER)?;
    // Addresses configured for the first server included its path.
    let base = base
        .trim_end_matches('/')
        .trim_end_matches("/v1/contributions")
        .trim_end_matches("/contributions");
    Some(format!("{base}/v1/contributions"))
}

/// This install's contributor id, created the first time.
fn contributor_id(state: &AppState) -> CmdResult<String> {
    let mut settings = state.settings();
    if let Some(id) = settings
        .contributor
        .clone()
        .filter(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Ok(id);
    }
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(err)?;
    let id = bytes.iter().fold(String::with_capacity(32), |mut id, b| {
        let _ = write!(id, "{b:02x}");
        id
    });
    settings.contributor = Some(id.clone());
    write_settings(state, settings)?;
    Ok(id)
}

fn contribution(state: &AppState) -> CmdResult<(fg_contrib::contribution::Contribution, PathBuf)> {
    let contributor = contributor_id(state)?;
    let settings = state.settings();
    let client_dir = settings.wow_dir.join(settings.edition.client_folder());
    let c = fg_contrib::contribution::collect(
        &client_dir,
        settings.edition.key(),
        env!("CARGO_PKG_VERSION"),
        &contributor,
    )
    .map_err(err)?;
    Ok((c, client_dir))
}

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "tauri commands take their arguments by value"
)]
fn contribution_preview(state: State<'_, AppState>) -> CmdResult<ContributionPreview> {
    let (c, client_dir) = contribution(&state)?;
    let settings = state.settings();
    Ok(ContributionPreview {
        edition: settings.edition.key(),
        found: client_dir.is_dir(),
        client_dir: client_dir.display().to_string(),
        quests: c.quests(),
        events: c.events(),
        characters: c.collector.len(),
        locales: c.quest_cache.iter().map(|f| f.locale.clone()).collect(),
        size: serde_json::to_vec(&c).map_err(err)?.len(),
        url: contribution_endpoint(&settings),
        last_sent: settings.last_contribution,
    })
}

/// Send the contribution of this computer to the server: "accepted", or "duplicate" when the
/// server already has the same data. Errors: "rate_limited", or the server's message.
#[tauri::command]
async fn send_contribution(app: AppHandle) -> CmdResult<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let url = contribution_endpoint(&state.settings()).ok_or("no contribution server configured")?;
        let (c, _) = contribution(&state)?;
        if c.quests() == 0 && c.events() == 0 {
            return Err("nothing to send: no quest cache nor addon recordings found".to_owned());
        }
        let body = serde_json::to_vec(&c).map_err(err)?;
        let message = |r: ureq::Response| {
            let json: serde_json::Value = r.into_json().unwrap_or_default();
            json["error"]
                .as_str()
                .or(json["status"].as_str())
                .unwrap_or("")
                .to_owned()
        };
        let status = match ureq::post(&url)
            .set("Content-Type", "application/json")
            .timeout(std::time::Duration::from_secs(120))
            .send_bytes(&body)
        {
            Ok(r) if r.status() == 201 => "accepted",
            Ok(_) => "duplicate",
            Err(ureq::Error::Status(429, _)) => return Err("rate_limited".into()),
            Err(ureq::Error::Status(code, r)) => return Err(format!("{code}: {}", message(r))),
            Err(e) => return Err(err(e)),
        };
        let mut settings = state.settings();
        settings.last_contribution = Some(now()?);
        write_settings(&state, settings)?;
        Ok(status.to_owned())
    })
    .await
    .map_err(err)?
}
