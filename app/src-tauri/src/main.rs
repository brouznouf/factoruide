// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![expect(
    clippy::needless_pass_by_value,
    reason = "tauri commands take their arguments by value"
)]

use fg_app::{CmdError, EditOpened, Host, Settings, SettingsStore, Studio, now};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The settings file, in the app's config folder.
const SETTINGS_FILE: &str = "settings.json";
use fg_contrib::contribution::{Contribution, SendState};
use fg_library::{Comparison, DiskFiles, GuideId, GuideSummary, GuideVersion, Library, Origin, VersionMeta};
use fg_route::Route;
use fg_route::diagnostics::Diagnostics;
use fg_route::job::{self, Options, Overrides, PlanOutcome, PlanRequest, RouteStep, StartState};
use fg_route::sync::LockExt;
use fg_route::xp::Edition;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager, State};

/// Game database, overrides and addon, embedded at build time (see build.rs).
mod game;

/// The data addon and the links.
mod addon;
/// The characters the addon recorded.
mod characters;
/// Contributions to the contribution server.
mod contributions;
use addon::{
    ADDON_VERSION, addon_synced, export_polestar, guide_text, install_addon, install_polestar, open_link,
    polestar_installed, polestar_installed_in, polestar_string, write_data,
};
use job::DATA_ADDON;
/// Guides, planning and the editor.
mod guides;
use characters::{forget_character, import_profile, list_characters};
use guides::{
    cancel_plan, compare_versions, delete_guide, delete_version, edit_close, edit_open, edit_save, edit_simulate,
    edit_suggest, export_version, get_guide, get_version, guide_timeline, import_guide, inferred_flights, list_guides,
    manual_route, migrate_results, plan_route, preview_starts, quest_choices, set_installed, stop_plan,
};
/// The game's maps.
mod maps;
use contributions::{
    auto_contribute_loop, contribution_preview, need_help, read_send_record, send_contribution, send_issue,
    send_record_file, sent_lua, setup_report,
};
use maps::{map_image, map_index, minimap_image, minimap_index};

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/embedded.rs"));
}

/// The settings the installed addon depends on (its options, the guides' language, what shows
/// the guides).
fn addon_snapshot(settings: &Settings) -> String {
    serde_json::to_string(&(
        &settings.addon,
        &settings.guide_locale,
        settings.guide_addon(settings.edition),
    ))
    .unwrap_or_default()
}

use game::{GUIDE_ADDON, PROFILE_ADDON, WowProblem, game_root};

/// Where the game usually is, when no folder was chosen.
fn default_wow_dir() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\Program Files (x86)\World of Warcraft")
    } else if cfg!(target_os = "macos") {
        PathBuf::from("/Applications/World of Warcraft")
    } else {
        // Under WSL, the Windows game; otherwise Battle.net in Lutris' default Wine prefix.
        let wsl = PathBuf::from("/mnt/c/Program Files (x86)/World of Warcraft");
        match std::env::var_os("HOME") {
            Some(home) if !wsl.is_dir() => {
                PathBuf::from(home).join("Games/battlenet/drive_c/Program Files (x86)/World of Warcraft")
            }
            _ => wsl,
        }
    }
}

struct AppState {
    settings: SettingsStore<DiskFiles>,
    /// The last install added files the game reads only when it starts.
    restart_needed: AtomicBool,
    /// Where the embedded databases are unpacked, and those unpacked so far.
    db_dir: PathBuf,
    dbs: Mutex<HashMap<Edition, PathBuf>>,
    /// Saved guides and debug exports.
    data_dir: PathBuf,
    /// The same folder, for the guide library.
    files: DiskFiles,
    /// What the app remembers between commands: the generation under way, the guide in the
    /// editor, the walking memory, the names read.
    studio: Studio,
    /// The window's handle, for the events (set once the app is built).
    app: OnceLock<AppHandle>,
    /// Held while a contribution is read, sent and recorded: one send at a time, manual or
    /// automatic.
    send_lock: Mutex<()>,
    /// Held while the addon's files are written: off the main thread, two installs could touch
    /// the same files at once.
    installing: Mutex<()>,
    /// Wakes the automatic contributions up (turned on in the settings).
    auto_wake: std::sync::mpsc::Sender<()>,
    /// What the `FG_*` variables the app started with ask the generations to tell or check.
    diagnostics: Diagnostics,
}

type CmdResult<T> = Result<T, CmdError>;

fn err(e: impl std::fmt::Display) -> CmdError {
    CmdError::App(format!("{e:#}"))
}

/// Embedded overrides of an edition.
fn overrides(edition: Edition) -> CmdResult<Arc<Overrides>> {
    fg_app::embedded_overrides(embedded::OVERRIDES, edition)
}

impl AppState {
    fn settings(&self) -> Arc<Settings> {
        self.settings.get()
    }
    fn edition(&self) -> Edition {
        self.settings.get().edition
    }
    /// The database of an edition, unpacked the first time (locked meanwhile: an unpack removes
    /// the files of other builds, so two at once could remove the one just written).
    fn db_path(&self, edition: Edition) -> CmdResult<PathBuf> {
        let mut dbs = self.dbs.locked();
        if let Some(p) = dbs.get(&edition) {
            return Ok(p.clone());
        }
        let path = unpack_database(&self.db_dir, edition).map_err(err)?;
        dbs.insert(edition, path.clone());
        Ok(path)
    }
    fn open_db(&self) -> CmdResult<Connection> {
        open_db(&self.db_path(self.edition())?)
    }
    /// A guide written in `locale` (None: the language it was made in).
    fn render(&self, route: &Route, locale: Option<&str>) -> CmdResult<Route> {
        self.studio.render(self, self.edition(), route, locale)
    }
}

impl Host for AppState {
    type Files = DiskFiles;

    fn files(&self) -> &DiskFiles {
        &self.files
    }

    fn edition(&self) -> Edition {
        AppState::edition(self)
    }

    fn open_db(&self, edition: Edition) -> CmdResult<Connection> {
        open_db(&self.db_path(edition)?)
    }

    fn overrides(&self, edition: Edition) -> CmdResult<Arc<Overrides>> {
        overrides(edition)
    }

    fn event(&self, event: &job::JobEvent<'_>) {
        if let Some(app) = self.app.get() {
            let _ = app.emit("plan-progress", event.to_string());
        }
    }

    fn now(&self) -> CmdResult<u64> {
        now()
    }

    fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }
}

fn open_db(path: &Path) -> CmdResult<Connection> {
    Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| CmdError::App(format!("{}: {e}", path.display())))
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
#[tauri::command]
fn list_editions(state: State<'_, AppState>) -> Vec<fg_app::EditionInfo> {
    let settings = state.settings();
    fg_app::editions(
        |e| embedded::DATABASES.iter().any(|(k, _)| *k == e.key()),
        |e| e.installed_in(&settings.wow_dir),
    )
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
fn get_settings(state: State<'_, AppState>) -> Settings {
    Settings::clone(&state.settings())
}

#[tauri::command]
fn set_settings(state: State<'_, AppState>, settings: Settings) -> CmdResult<()> {
    let wake = update_settings(&state, |current| {
        let wake = settings.auto_contribute && !current.auto_contribute;
        let before = std::mem::replace(current, settings);
        // Not edited by the settings page: kept.
        if current.contributor.is_none() {
            current.contributor = before.contributor;
        }
        if current.addon_installed.is_none() {
            current.addon_installed = before.addon_installed;
        }
        wake
    })?;
    if wake {
        let _ = state.auto_wake.send(());
    }
    Ok(())
}

/// Change the settings and save them, the settings locked from the read to the write: a change
/// made meanwhile elsewhere (the settings page, an install, a contribution) is never lost to an
/// older copy.
fn update_settings<T>(state: &AppState, change: impl FnOnce(&mut Settings) -> T) -> CmdResult<T> {
    state.settings.update(|settings| {
        let out = change(settings);
        settings.wow_dir = game_root(&settings.wow_dir);
        out
    })
}

/// Turn the automatic contributions on or off (only that setting: the pages showing it hold
/// copies of the others).
#[tauri::command]
fn set_auto_contribute(state: State<'_, AppState>, on: bool) -> CmdResult<()> {
    let wake = update_settings(&state, |settings| {
        let wake = on && !settings.auto_contribute;
        settings.auto_contribute = on;
        wake
    })?;
    if wake {
        let _ = state.auto_wake.send(());
    }
    Ok(())
}

/// Off the main thread: the window stays responsive while the database is unpacked and read.
#[tauri::command]
async fn get_options(app: AppHandle) -> CmdResult<Options> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        job::options(&state.open_db()?, &*overrides(state.edition())?).map_err(err)
    })
    .await
    .map_err(err)?
}

/// Save a planning request for debugging (replay with `fg route --request <file>`).
#[tauri::command]
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

/// The game's folder (`wow_dir`, else the saved one) and what is wrong with it for the game
/// version the app works on.
#[tauri::command]
fn wow_status(state: State<'_, AppState>, wow_dir: Option<PathBuf>) -> game::WowStatus {
    let settings = state.settings();
    game::status(wow_dir.as_deref().unwrap_or(&settings.wow_dir), settings.edition)
}

/// A World of Warcraft folder found on this computer and its game versions' folders.
#[derive(Debug, Serialize)]
struct FoundGame {
    root: PathBuf,
    clients: Vec<String>,
}

/// World of Warcraft folders found on this computer (Battle.net's, the usual places).
#[tauri::command(async)]
fn detect_wow() -> Vec<FoundGame> {
    game::detect()
        .into_iter()
        .map(|root| FoundGame {
            clients: game::client_folders(&root),
            root,
        })
        .collect()
}

/// What the game did with the addon of the game version the app works on: installed by which
/// app version, when it last ran, turned off for some characters, not loaded at all...
#[derive(Debug, Serialize)]
struct AddonState {
    #[serde(flatten)]
    health: game::AddonHealth,
    /// The last install added files the game reads only when it starts: a /reload is not enough.
    restart_needed: bool,
}

#[tauri::command(async)]
fn addon_health(state: State<'_, AppState>) -> AddonState {
    let settings = state.settings();
    AddonState {
        health: game::addon_health(&settings.wow_dir, settings.edition, ADDON_VERSION),
        restart_needed: state.restart_needed.load(Ordering::Relaxed),
    }
}

/// The .toc files of the addons the app installs and the files each loads ("<folder>" -> list).
fn loaded_files(addons: &Path) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    for folder in [
        PROFILE_ADDON,
        GUIDE_ADDON,
        DATA_ADDON,
        fg_route::export::polestar::ADDON,
    ] {
        for entry in std::fs::read_dir(addons.join(folder)).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "toc")
                && let Ok(toc) = std::fs::read_to_string(&path)
            {
                out.insert(
                    format!("{folder}/{}", entry.file_name().to_string_lossy()),
                    game::toc_files(&toc),
                );
            }
        }
    }
    out
}

/// WebKitGTK renders a blank window under WSLg (no usable GPU for DMA-BUF/EGL),
/// and its DMA-BUF renderer dies with "Error 71 (Protocol error)" on Wayland with
/// the NVIDIA driver: fall back to its software paths in those cases.
#[expect(
    unsafe_code,
    reason = "set_var is unsafe in edition 2024; this runs before any thread is spawned"
)]
fn linux_rendering_workaround() {
    let in_wsl = std::fs::read_to_string("/proc/version").is_ok_and(|v| v.to_lowercase().contains("microsoft"));
    let vars: &[&str] = if in_wsl {
        &["WEBKIT_DISABLE_DMABUF_RENDERER", "WEBKIT_DISABLE_COMPOSITING_MODE"]
    } else if Path::new("/proc/driver/nvidia").exists() {
        &["WEBKIT_DISABLE_DMABUF_RENDERER"]
    } else {
        &[]
    };
    for var in vars {
        if std::env::var_os(var).is_none() {
            // SAFETY: called at startup, before any other thread exists.
            unsafe { std::env::set_var(var, "1") };
        }
    }
}

fn main() {
    if cfg!(target_os = "linux") {
        linux_rendering_workaround();
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            {
                let menu = tauri::menu::Menu::default(app.handle())?;
                // Tauri's default macOS menu begins with the application submenu.
                if let Some(app_menu) = menu.items()?.first().and_then(tauri::menu::MenuItemKind::as_submenu) {
                    let check = tauri::menu::MenuItem::with_id(
                        app,
                        "check-for-updates",
                        "Check for Updates…",
                        true,
                        None::<&str>,
                    )?;
                    app_menu.insert(&check, 1)?;
                }
                app.set_menu(menu)?;
                app.on_menu_event(|app, event| {
                    if event.id().as_ref() == "check-for-updates"
                        && let Some(window) = app.get_webview_window("main")
                    {
                        let _ = window.show();
                        let _ = window.unminimize();
                        let _ = window.set_focus();
                        let _ = window.emit("check-for-updates", ());
                    }
                });
            }
            migrate_app_dirs(app.handle());
            let config_dir = app.path().app_config_dir()?;
            let config_file = config_dir.join(SETTINGS_FILE);
            let data_dir = app.path().app_data_dir()?;
            let config: Option<serde_json::Value> = std::fs::read_to_string(&config_file)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok());
            if let Some(config) = &config {
                migrate_workspace(config, &data_dir);
            }
            let first_launch = config.is_none();
            let mut settings: Settings = config.and_then(|c| serde_json::from_value(c).ok()).unwrap_or_default();
            if settings.wow_dir.as_os_str().is_empty() {
                settings.wow_dir = default_wow_dir();
            }
            // A version's folder chosen by older versions of the app: the game's folder above.
            settings.wow_dir = game_root(&settings.wow_dir);
            let settings = SettingsStore::with(DiskFiles::new(&config_dir), SETTINGS_FILE, settings);
            if first_launch {
                // Found once: not looked for again at every launch.
                let _ = settings.update(find_game);
            }
            let db_dir = app.path().app_local_data_dir()?;
            let (auto_wake, wake) = std::sync::mpsc::channel();
            app.manage(AppState {
                settings,
                restart_needed: AtomicBool::new(false),
                installing: Mutex::new(()),
                db_dir,
                dbs: Mutex::new(HashMap::default()),
                files: DiskFiles::new(data_dir.clone()),
                data_dir,
                studio: Studio::default(),
                app: OnceLock::new(),
                send_lock: Mutex::new(()),
                auto_wake,
                diagnostics: Diagnostics::from_env(),
            });
            // The events of a generation go to this window.
            let _ = app.state::<AppState>().app.set(app.handle().clone());
            migrate_guides(&app.state::<AppState>().data_dir);
            migrate_results(&app.state::<AppState>());
            let handle = app.handle().clone();
            std::thread::spawn(move || auto_contribute_loop(&handle, &wake));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            updates_enabled,
            get_settings,
            set_settings,
            get_options,
            plan_route,
            inferred_flights,
            quest_choices,
            preview_starts,
            manual_route,
            stop_plan,
            cancel_plan,
            export_request,
            install_addon,
            addon_synced,
            polestar_installed,
            export_polestar,
            polestar_string,
            open_link,
            wow_status,
            detect_wow,
            setup_report,
            addon_health,
            list_editions,
            set_edition,
            contribution_preview,
            send_contribution,
            send_issue,
            need_help,
            set_auto_contribute,
            list_guides,
            get_guide,
            get_version,
            compare_versions,
            set_installed,
            delete_version,
            delete_guide,
            guide_text,
            install_polestar,
            forget_character,
            export_version,
            import_guide,
            map_index,
            map_image,
            minimap_index,
            minimap_image,
            list_characters,
            import_profile,
            guide_timeline,
            edit_open,
            edit_simulate,
            edit_suggest,
            edit_save,
            edit_close,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Factoruide");
}

/// At the first launch: the game where it was found, and a game version installed there (WoW
/// Forever first) when the default one is not.
fn find_game(settings: &mut Settings) {
    if matches!(
        game::status(&settings.wow_dir, settings.edition).problem,
        Some(WowProblem::Missing | WowProblem::NoGame)
    ) && let Some(found) = game::detect().into_iter().next()
    {
        settings.wow_dir = found;
    }
    let installed = |e: Edition| e.installed_in(&settings.wow_dir);
    if !installed(settings.edition)
        && let Some(e) = Edition::ALL.iter().copied().find(|e| installed(*e))
    {
        settings.edition = e;
    }
}

/// Switch the edition the app works on (saved with the settings).
#[tauri::command]
fn set_edition(state: State<'_, AppState>, edition: Edition) -> CmdResult<()> {
    update_settings(&state, |s| s.edition = edition)
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
