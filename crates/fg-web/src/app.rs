//! The app's calls, as the desktop app's backend answers them (app/src-tauri/src/main.rs), on
//! the files of `store` instead of the app's folders and with the addon reached through texts:
//! the guide import the player pastes in game (`guide_text`), the character export pasted in
//! the app (`import_profile`).

use crate::{embedded, host, store};
use fg_app::{CmdError, Host, Settings, SettingsStore, Studio, now};
use fg_library::{GuideId, Library};
use fg_route::Route;
use fg_route::diagnostics::Diagnostics;
use fg_route::export::fgstring;
use fg_route::job::{self, Overrides, PlanRequest, RouteStep, StartState};
use fg_route::sync::LockExt;
use fg_route::xp::Edition;
use rusqlite::Connection;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

type CmdResult<T> = Result<T, CmdError>;

fn err(e: impl std::fmt::Display) -> CmdError {
    CmdError::App(format!("{e:#}"))
}

/// What the browser gives the studio.
struct WebHost;

impl Host for WebHost {
    type Files = store::Browser;

    fn files(&self) -> &store::Browser {
        &store::Browser
    }

    fn edition(&self) -> Edition {
        edition()
    }

    fn open_db(&self, edition: Edition) -> CmdResult<Connection> {
        open_db(edition)
    }

    fn overrides(&self, edition: Edition) -> CmdResult<Arc<Overrides>> {
        overrides(edition)
    }

    fn event(&self, event: &job::JobEvent<'_>) {
        host::event("plan-progress", &serde_json::Value::String(event.to_string()));
    }

    fn now(&self) -> CmdResult<u64> {
        now()
    }

    /// No `FG_*` variables in the browser: nothing to tell.
    fn diagnostics(&self) -> &Diagnostics {
        &fg_route::diagnostics::OFF
    }
}

static STUDIO: LazyLock<Studio> = LazyLock::new(Studio::default);

/// A guide written in `locale` (None: the language it was made in).
fn render(edition: Edition, route: &Route, locale: Option<&str>) -> CmdResult<Route> {
    STUDIO.render(&WebHost, edition, route, locale)
}

/// Every guide of an edition in the version its page says to follow.
fn followed_guides(edition: Edition) -> CmdResult<Vec<(Route, u64, u32)>> {
    STUDIO.followed_guides(&WebHost, edition)
}

/// The app's version: the release's (`FG_VERSION` at build time), else the crate's.
const APP_VERSION: &str = match option_env!("FG_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

// Settings -----------------------------------------------------------------------------

const SETTINGS_FILE: &str = "settings.json";

/// The settings, in the browser's storage (those of older versions read the same).
static SETTINGS: LazyLock<SettingsStore<store::Browser>> =
    LazyLock::new(|| SettingsStore::open(store::Browser, SETTINGS_FILE).0);

fn settings() -> Arc<Settings> {
    SETTINGS.get()
}

fn edition() -> Edition {
    SETTINGS.get().edition
}

// Game data ----------------------------------------------------------------------------

/// The databases fetched so far, unpacked (kept for good: each connection reads them in place).
static DATABASES: LazyLock<Mutex<HashMap<Edition, &'static [u8]>>> = LazyLock::new(Mutex::default);

/// Forget the databases fetched (tests serve different worlds of an edition one after the other).
#[cfg(test)]
pub(crate) fn forget_databases() {
    DATABASES.locked().clear();
}

/// An edition's database, fetched and unpacked the first time (`data/<edition>.sqlite.zst`).
fn database(edition: Edition) -> CmdResult<&'static [u8]> {
    let mut dbs = DATABASES.locked();
    if let Some(db) = dbs.get(&edition) {
        return Ok(db);
    }
    let url = format!("data/{}.sqlite.zst", edition.key());
    let answer = host::fetch("GET", &url, None, &[])?;
    if answer.status != 200 {
        return Err(CmdError::App(format!(
            "no data for {} ({url}: {})",
            edition.rules().edition_name,
            answer.status
        )));
    }
    let bytes = zstd::decode_all(answer.body.as_slice()).map_err(err)?;
    let bytes: &'static [u8] = Box::leak(bytes.into_boxed_slice());
    dbs.insert(edition, bytes);
    Ok(bytes)
}

/// Whether SQLite was built for threads. The calls run on threads of their own, at the same time,
/// each with its own connection: SQLite's shared state (page cache, memory) must then be guarded,
/// which a single-threaded build does not do (two calls reading the game data at once corrupt
/// each other: "database disk image is malformed", crashes). libsqlite3-sys builds it
/// single-threaded on WASI unless told otherwise (scripts/build-web.sh does).
fn sqlite_threadsafe() -> bool {
    #[expect(unsafe_code, reason = "a constant of SQLite's build")]
    // SAFETY: takes nothing, reads a value fixed when SQLite was compiled.
    unsafe {
        rusqlite::ffi::sqlite3_threadsafe() != 0
    }
}

fn open_db(edition: Edition) -> CmdResult<Connection> {
    if !sqlite_threadsafe() {
        return Err("SQLite was built single-threaded: build the web app with scripts/build-web.sh".into());
    }
    let bytes = database(edition)?;
    let mut conn = Connection::open_in_memory().map_err(err)?;
    conn.deserialize_bytes("main", bytes).map_err(err)?;
    // No folder for temporary files in the browser: sorts and temporary tables in memory.
    conn.pragma_update(None, "temp_store", "MEMORY").map_err(err)?;
    // The same bytes at every call: what is worked out from them is kept.
    fg_route::world::name_database(&conn, bytes.as_ptr() as u64 ^ bytes.len() as u64).map_err(err)?;
    Ok(conn)
}

fn overrides(edition: Edition) -> CmdResult<Arc<Overrides>> {
    fg_app::embedded_overrides(embedded::OVERRIDES, edition)
}

fn list_editions() -> Vec<fg_app::EditionInfo> {
    // The browser cannot tell which games are installed: every one.
    fg_app::editions(|_| true, |_| true)
}

// Guides -------------------------------------------------------------------------------
//
// The guides are kept by `fg-library`, over the files of `store`; `characters.json`, next to
// them, tells which guide each character follows.
//
//   guides/<edition>/characters.json          "Name-Realm" -> guide (slug)

fn guides(edition: Edition) -> Library<'static, store::Browser> {
    Library::new(&store::Browser, edition)
}

// The guides for the game ---------------------------------------------------------------------

/// What the player pastes in game with `/fg import`: one guide version (the addon adds it to the
/// guides it has, or replaces the one of its name).
fn guide_text(guide: &GuideId, n: u32) -> CmdResult<serde_json::Value> {
    Ok(serde_json::json!({ "text": STUDIO.guide_text(&WebHost, guide, n)? }))
}

/// Every guide (see `followed_guides`) as the data addon the desktop app writes
/// (Factoruide_Data): a zip to unzip in the game's AddOns folder.
fn data_zip() -> CmdResult<serde_json::Value> {
    let edition = edition();
    let guides = followed_guides(edition)?;
    if guides.is_empty() {
        return Err("no guide yet".into());
    }
    let lua = job::guides_lua(&open_db(edition)?, &*overrides(edition)?, edition, guides).map_err(err)?;
    let files: Vec<(String, Vec<u8>)> =
        job::data_addon_files(&lua, embedded::ADDON_INTERFACE, APP_VERSION, WebHost.now()?)
            .into_iter()
            .map(|(name, content)| (format!("{}/{name}", job::DATA_ADDON), content.into_bytes()))
            .collect();
    Ok(serde_json::json!({
        "file": format!("{}.zip", job::DATA_ADDON),
        "base64": base64(&crate::zip::zip(&files)),
    }))
}

/// WoW Forever: every guide as a guide pack for Polestar, a zip of its addon folder to unzip in
/// the game's AddOns folder.
fn polestar_pack() -> CmdResult<serde_json::Value> {
    if edition() != Edition::Forever {
        return Err("Polestar is for WoW Forever only".into());
    }
    let guides = followed_guides(Edition::Forever)?
        .into_iter()
        .map(|(route, _, _)| (job::slug(&route.name), route))
        .collect::<Vec<_>>();
    let folder = fg_route::export::polestar::ADDON;
    let files: Vec<(String, Vec<u8>)> = fg_route::export::polestar::pack(&guides, APP_VERSION)
        .into_iter()
        .map(|(name, content)| (format!("{folder}/{name}"), content.into_bytes()))
        .collect();
    Ok(serde_json::json!({ "file": format!("{folder}.zip"), "base64": base64(&crate::zip::zip(&files)) }))
}

fn base64(bytes: &[u8]) -> String {
    const CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let v = u32::from(chunk[0]) << 16
            | u32::from(*chunk.get(1).unwrap_or(&0)) << 8
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(CHARS[(v >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// What the player pastes on Polestar's Import page (`/polestar import`): one guide version, in
/// the game's language. Polestar exists on WoW Forever only.
fn polestar_string(guide: &GuideId, v: u32) -> CmdResult<serde_json::Value> {
    serde_json::to_value(STUDIO.polestar_string(&WebHost, guide, v)?).map_err(err)
}

/// A guide version written for Polestar's own tools: a zip of a pack folder holding its
/// `.guide` file (in English). Polestar exists on WoW Forever only.
fn export_polestar(guide: &GuideId, v: u32) -> CmdResult<serde_json::Value> {
    if edition() != Edition::Forever {
        return Err("Polestar is for WoW Forever only".into());
    }
    let route = render(
        Edition::Forever,
        &guides(Edition::Forever).version(guide, v)?.outcome.route,
        Some("enUS"),
    )?;
    let folder = job::POLESTAR_SOURCE_FOLDER;
    let files = vec![
        (
            format!("{folder}/pack.json"),
            fg_route::export::polestar::pack_json(APP_VERSION).into_bytes(),
        ),
        (
            format!(
                "{folder}/{}.guide",
                fg_route::export::polestar::guide_name(guide.as_str())
            ),
            fg_route::export::polestar::source(guide.as_str(), &route).into_bytes(),
        ),
    ];
    Ok(serde_json::json!({ "file": format!("{folder}.zip"), "base64": base64(&crate::zip::zip(&files)) }))
}

// Shared guides --------------------------------------------------------------------------

/// A version as a .fgguide file to download.
fn export_version(guide: &GuideId, v: u32) -> CmdResult<serde_json::Value> {
    let shared = guides(edition()).share(guide, v)?;
    Ok(serde_json::json!({
        "file": format!("{guide}-v{v}.fgguide"),
        "text": shared.to_json()?,
    }))
}

// Planning and the editor ----------------------------------------------------------------

// Characters ---------------------------------------------------------------------------
//
//   characters/<edition>/<Name-Realm>.json   the profile of the last export pasted

#[derive(Serialize)]
struct CharacterInfo {
    full_name: Option<String>,
    race: Option<String>,
    class: String,
    professions: Vec<(String, i64)>,
    profile: StartState,
}

/// A file name for a character's key ("Name-Realm" holds no slash, but a realm could).
fn character_file(dir: &str, edition: Edition, key: &str) -> String {
    format!("{dir}/{}/{}.json", edition.key(), key.replace('/', "_"))
}

fn profiles(edition: Edition) -> Vec<serde_json::Value> {
    let dir = format!("characters/{}", edition.key());
    let mut out: Vec<serde_json::Value> = store::list(&dir)
        .into_iter()
        .filter_map(|f| serde_json::from_slice(&store::read(&format!("{dir}/{f}"))?).ok())
        .collect();
    out.sort_by_key(|p: &serde_json::Value| std::cmp::Reverse(p["time"].as_i64().unwrap_or(0)));
    out
}

fn characters() -> CmdResult<Vec<CharacterInfo>> {
    let settings = settings();
    let edition = settings.edition;
    let races = &overrides(edition)?.races;
    let now = i64::try_from(WebHost.now()?).map_err(err)?;
    Ok(profiles(edition)
        .into_iter()
        .filter_map(|json| serde_json::from_value::<StartState>(json).ok())
        .filter(|profile| !settings.is_forgotten(edition, profile))
        .map(|mut profile| {
            profile.rested = fg_app::rested_now(&profile, now);
            CharacterInfo {
                full_name: None,
                race: fg_app::race_key(races, &profile),
                class: profile.class.to_lowercase(),
                professions: fg_app::profession_ranks(&profile),
                profile,
            }
        })
        .collect())
}

fn forget_character(character: &str) -> CmdResult<()> {
    let time = characters()?
        .into_iter()
        .find(|c| c.profile.name == character)
        .map_or(i64::MAX, |c| c.profile.time);
    SETTINGS.update(|s| {
        let edition = s.edition;
        s.forget(edition, character, time);
    })
}

/// A character's export pasted: what it was read as.
#[derive(Serialize)]
struct ProfileImported {
    character: String,
    edition: &'static str,
    level: i64,
}

/// When the addon was last seen (the newest export pasted), for the setup pages.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct LastExport {
    time: i64,
    version: String,
}

fn last_export_file(edition: Edition) -> String {
    format!("last-export-{}.json", edition.key())
}

/// Keep a character's export (its text as the addon gives it): its profile replaces the one
/// kept.
fn import_profile(content: &str) -> CmdResult<ProfileImported> {
    let export = fgstring::read_profile(content)?;
    let edition = match export.edition.as_deref() {
        Some(key) => Edition::ALL
            .iter()
            .copied()
            .find(|e| e.key() == key)
            .ok_or_else(|| format!("unknown game version {key}"))?,
        None => edition(),
    };
    let (key, profile) = (export.character, export.profile);
    let parsed: StartState = serde_json::from_value(profile.clone()).map_err(|e| format!("corrupt_profile: {e}"))?;
    store::write(
        &character_file("characters", edition, &key),
        serde_json::to_vec(&profile).map_err(err)?,
    );
    let last = LastExport {
        time: i64::try_from(WebHost.now()?).map_err(err)?,
        version: export.addon.unwrap_or_default(),
    };
    store::write(&last_export_file(edition), serde_json::to_vec(&last).map_err(err)?);
    // Pasted again after being taken out: back.
    if settings().is_out(edition, &key) {
        SETTINGS.update(|s| s.remember(edition, &key))?;
    }
    Ok(ProfileImported {
        character: key,
        edition: edition.key(),
        level: parsed.level,
    })
}

// The addon, as the exports show it -----------------------------------------------------------

#[derive(Serialize)]
struct AddonState {
    app_version: &'static str,
    /// The version of the addon of the last export pasted.
    installed_version: Option<String>,
    /// The oldest addon version this app works with, and whether the last export's is older.
    required_version: &'static str,
    outdated: bool,
    seen: Option<LastExport>,
}

fn last_export(edition: Edition) -> Option<LastExport> {
    serde_json::from_slice(&store::read(&last_export_file(edition))?).ok()
}

fn addon_health() -> AddonState {
    let seen = last_export(edition());
    AddonState {
        app_version: APP_VERSION,
        installed_version: seen.as_ref().map(|s| s.version.clone()),
        required_version: embedded::ADDON_VERSION,
        outdated: seen
            .as_ref()
            .is_some_and(|s| fg_app::older(&s.version, embedded::ADDON_VERSION)),
        seen,
    }
}

// Maps ------------------------------------------------------------------------------------

fn map_index() -> CmdResult<serde_json::Value> {
    fg_app::map_index(&open_db(edition())?)
}

// Calls -----------------------------------------------------------------------------------

#[derive(Deserialize)]
struct Call {
    cmd: String,
    #[serde(default)]
    args: serde_json::Value,
}

/// The arguments of a call, named as the page names them (camelCase, as Tauri takes them).
fn args<T: DeserializeOwned>(args: serde_json::Value) -> CmdResult<T> {
    serde_json::from_value(if args.is_null() { serde_json::json!({}) } else { args }).map_err(err)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GuideArgs {
    #[serde(alias = "id")]
    guide: GuideId,
    #[serde(default)]
    version: Option<u32>,
    #[serde(default)]
    locale: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StepsArgs {
    steps: Vec<RouteStep>,
    #[serde(default)]
    after: Option<usize>,
    #[serde(default)]
    locale: String,
}

fn to_json<T: Serialize>(result: CmdResult<T>) -> CmdResult<serde_json::Value> {
    result.and_then(|v| serde_json::to_value(v).map_err(err))
}

fn version_of(a: &GuideArgs) -> CmdResult<u32> {
    a.version.ok_or_else(|| CmdError::from("version missing"))
}

/// Answer a call from the page: `{"ok": value}` or `{"err": message}`.
pub fn dispatch(request: &[u8]) -> String {
    let result = serde_json::from_slice::<Call>(request)
        .map_err(err)
        .and_then(|call| run(&call.cmd, call.args));
    match result {
        Ok(value) => serde_json::json!({ "ok": value }).to_string(),
        Err(e) => serde_json::json!({ "err": e }).to_string(),
    }
}

fn run(cmd: &str, a: serde_json::Value) -> CmdResult<serde_json::Value> {
    use serde_json::{Value, json};
    match cmd {
        "updates_enabled" => Ok(Value::Bool(false)),
        "get_settings" => to_json(Ok(Settings::clone(&settings()))),
        "set_settings" => {
            #[derive(Deserialize)]
            struct A {
                settings: Settings,
            }
            let s = args::<A>(a)?.settings;
            to_json(SETTINGS.update(|current| *current = s))
        }
        "set_edition" => {
            #[derive(Deserialize)]
            struct A {
                edition: Edition,
            }
            let edition = args::<A>(a)?.edition;
            to_json(SETTINGS.update(|s| s.edition = edition))
        }
        "list_editions" => to_json(Ok(list_editions())),
        "get_options" => {
            let edition = edition();
            to_json(job::options(&open_db(edition)?, &*overrides(edition)?).map_err(err))
        }
        "plan_route" => {
            #[derive(Deserialize)]
            struct A {
                request: PlanRequest,
            }
            to_json(STUDIO.plan_route(&WebHost, &args::<A>(a)?.request))
        }
        "inferred_flights" => {
            #[derive(Deserialize)]
            struct A {
                start: StartState,
            }
            let start = args::<A>(a)?.start;
            to_json(STUDIO.inferred_flights(&WebHost, &start))
        }
        "quest_choices" => {
            #[derive(Deserialize)]
            struct A {
                race: String,
                class: String,
                query: String,
                locale: Option<String>,
            }
            let a = args::<A>(a)?;
            to_json(STUDIO.quest_choices(&WebHost, &a.race, &a.class, &a.query, a.locale.as_deref()))
        }
        "preview_starts" => {
            #[derive(Deserialize)]
            struct A {
                request: PlanRequest,
            }
            to_json(STUDIO.preview_starts(&WebHost, &args::<A>(a)?.request))
        }
        "manual_route" => {
            #[derive(Deserialize)]
            struct A {
                request: PlanRequest,
            }
            to_json(STUDIO.manual_route(&WebHost, &args::<A>(a)?.request))
        }
        "stop_plan" => {
            STUDIO.stop_plan();
            Ok(Value::Null)
        }
        "cancel_plan" => {
            STUDIO.cancel_plan();
            Ok(Value::Null)
        }
        "export_request" => {
            #[derive(Deserialize)]
            struct A {
                request: PlanRequest,
            }
            let request = args::<A>(a)?.request;
            let name = request
                .name
                .clone()
                .unwrap_or_else(|| format!("{}-{}", request.race, request.class));
            Ok(json!({
                "file": format!("request-{name}.json"),
                "text": serde_json::to_string_pretty(&request).map_err(err)?,
            }))
        }
        "list_guides" => to_json(Ok(guides(edition()).list())),
        "get_guide" => to_json(
            guides(edition())
                .summary(&args::<GuideArgs>(a)?.guide)
                .map_err(Into::into),
        ),
        "get_version" => {
            let g = args::<GuideArgs>(a)?;
            to_json(STUDIO.get_version(&WebHost, &g.guide, version_of(&g)?, g.locale.as_deref()))
        }
        "compare_versions" => {
            #[derive(Deserialize)]
            struct Versions {
                guide: GuideId,
                a: u32,
                b: u32,
                #[serde(default)]
                locale: Option<String>,
            }
            let x = args::<Versions>(a)?;
            to_json(STUDIO.compare_versions(&WebHost, &x.guide, x.a, x.b, x.locale.as_deref()))
        }
        // The report document the page posts to /v1/issues (the browser has no id of its own: the
        // page keeps one).
        "issue_document" => {
            #[derive(Deserialize)]
            struct Report {
                draft: fg_app::IssueDraft,
                contributor: String,
                os: String,
            }
            let r = args::<Report>(a)?;
            to_json(STUDIO.issue(&WebHost, r.draft, (APP_VERSION, &r.os, r.contributor), None))
        }
        "set_installed" => {
            let g = args::<GuideArgs>(a)?;
            to_json(guides(edition()).set_installed(&g.guide, g.version).map_err(Into::into))
        }
        "delete_version" => {
            let g = args::<GuideArgs>(a)?;
            to_json(
                guides(edition())
                    .delete_version(&g.guide, version_of(&g)?)
                    .map_err(Into::into),
            )
        }
        "delete_guide" => to_json(
            guides(edition())
                .delete_guide(&args::<GuideArgs>(a)?.guide)
                .map_err(Into::into),
        ),
        "forget_character" => {
            #[derive(Deserialize)]
            struct A {
                character: String,
            }
            to_json(forget_character(&args::<A>(a)?.character))
        }
        "export_version" => {
            let g = args::<GuideArgs>(a)?;
            export_version(&g.guide, version_of(&g)?)
        }
        "import_guide" => {
            #[derive(Deserialize)]
            struct A {
                content: String,
            }
            to_json(STUDIO.import_guide(&WebHost, &args::<A>(a)?.content))
        }
        "import_profile" => {
            #[derive(Deserialize)]
            struct A {
                content: String,
            }
            to_json(import_profile(&args::<A>(a)?.content))
        }
        "list_characters" => to_json(characters()),
        "guide_text" => {
            let g = args::<GuideArgs>(a)?;
            guide_text(&g.guide, version_of(&g)?)
        }
        "polestar_pack" => polestar_pack(),
        "data_zip" => data_zip(),
        "addon_health" => to_json(Ok(addon_health())),
        // Polestar is installed in the game: the browser cannot tell, the player says.
        "polestar_installed" => Ok(Value::Bool(true)),
        "polestar_string" => {
            let g = args::<GuideArgs>(a)?;
            polestar_string(&g.guide, version_of(&g)?)
        }
        "export_polestar" => {
            let g = args::<GuideArgs>(a)?;
            export_polestar(&g.guide, version_of(&g)?)
        }
        "wow_status" => Ok(json!({ "root": "", "clients": [], "problem": null })),
        "detect_wow" => Ok(json!([])),
        "map_index" => map_index(),
        "guide_timeline" => {
            let g = args::<GuideArgs>(a)?;
            to_json(STUDIO.guide_timeline(
                &WebHost,
                &g.guide,
                version_of(&g)?,
                g.locale.as_deref().unwrap_or("enUS"),
            ))
        }
        "edit_open" => {
            let g = args::<GuideArgs>(a)?;
            to_json(STUDIO.edit_open(
                &WebHost,
                &g.guide,
                version_of(&g)?,
                g.locale.as_deref().unwrap_or("enUS"),
            ))
        }
        "edit_simulate" => {
            let s = args::<StepsArgs>(a)?;
            to_json(STUDIO.edit_simulate(&WebHost, &s.steps, &s.locale))
        }
        "edit_suggest" => {
            let s = args::<StepsArgs>(a)?;
            to_json(STUDIO.edit_suggest(&WebHost, &s.steps, s.after, &s.locale))
        }
        "edit_save" => to_json(STUDIO.edit_save(&WebHost, &args::<StepsArgs>(a)?.steps)),
        "edit_close" => {
            STUDIO.edit_close();
            Ok(Value::Null)
        }
        _ => Err(CmdError::App(format!("unknown call {cmd}"))),
    }
}

#[cfg(test)]
mod tests;
