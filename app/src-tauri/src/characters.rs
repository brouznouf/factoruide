//! The characters the addon recorded, to start a guide from.

#![expect(
    clippy::needless_pass_by_value,
    reason = "tauri commands take their arguments by value"
)]

use super::{
    AppHandle, AppState, BTreeMap, CmdResult, Edition, HashMap, Host, Manager, PathBuf, Serialize, StartState, State,
    err, overrides, update_settings,
};

// Characters -------------------------------------------------------------------------

/// A character the addon recorded (`/fg profile`, or at each logout), to start a guide from.
#[derive(Serialize)]
pub(crate) struct CharacterInfo {
    /// Full name when the game gives characters one (WoW Forever: first and last name), from the
    /// client's per-character folders (`WTF/Account/<account>/<realm>/<First>-<Last>/`).
    full_name: Option<String>,
    /// Race key of `races.toml` (None: a race this game version does not know).
    race: Option<String>,
    class: String,
    /// Professions it has, by key, with their skill.
    professions: Vec<(String, i64)>,
    /// Another player's, pasted from their export: for group guides, not played here.
    shared: bool,
    profile: StartState,
}

/// The characters of the game client of the current edition, most recently played first.
// Off the main thread: reading the SavedVariables can take a while.
#[tauri::command]
pub(crate) async fn list_characters(app: AppHandle) -> CmdResult<Vec<CharacterInfo>> {
    tauri::async_runtime::spawn_blocking(move || characters(&app.state::<AppState>()))
        .await
        .map_err(err)?
}

/// Full names of the characters by first name, from the per-character folders of the accounts
/// of these SavedVariables files (`<account>/<realm>/<First>-<Last>/`).
pub(crate) fn full_names(files: &[PathBuf]) -> HashMap<String, Vec<String>> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    for account in files.iter().filter_map(|f| f.parent()?.parent()) {
        for realm in std::fs::read_dir(account).into_iter().flatten().flatten() {
            for character in std::fs::read_dir(realm.path()).into_iter().flatten().flatten() {
                let folder = character.file_name().to_string_lossy().into_owned();
                if let Some((first, last)) = folder.split_once('-')
                    && character.path().is_dir()
                {
                    let full = format!("{first} {last}");
                    let names = out.entry(first.to_owned()).or_default();
                    if !names.contains(&full) {
                        names.push(full);
                    }
                }
            }
        }
    }
    out
}

/// Other players' characters pasted from their export (in game: /fg, "Export my character"), by
/// game version then "Name-Realm": for group guides, not played here.
type Imported = BTreeMap<String, BTreeMap<String, serde_json::Value>>;

fn imported_file(state: &AppState) -> PathBuf {
    state.data_dir.join("imported-characters.json")
}

fn read_imported(state: &AppState) -> Imported {
    std::fs::read(imported_file(state))
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

/// A character's export pasted: what it was read as.
#[derive(Serialize)]
pub(crate) struct ProfileImported {
    character: String,
    edition: &'static str,
    level: i64,
}

/// Keep another player's export (its text as the addon gives it), for group guides: pasted
/// again, its profile replaces the one kept.
#[tauri::command(async)]
pub(crate) fn import_profile(state: State<'_, AppState>, content: String) -> CmdResult<ProfileImported> {
    let export = fg_route::export::fgstring::read_profile(&content)?;
    let edition = match export.edition.as_deref() {
        Some(key) => (Edition::ALL.iter().copied())
            .find(|e| e.key() == key)
            .ok_or_else(|| format!("unknown game version {key}"))?,
        None => state.settings().edition,
    };
    let parsed: StartState =
        serde_json::from_value(export.profile.clone()).map_err(|e| format!("corrupt_profile: {e}"))?;
    let mut imported = read_imported(&state);
    (imported.entry(edition.key().to_owned()).or_default()).insert(export.character.clone(), export.profile);
    std::fs::create_dir_all(&state.data_dir).map_err(err)?;
    std::fs::write(imported_file(&state), serde_json::to_vec(&imported).map_err(err)?).map_err(err)?;
    // Pasted again after being taken out: back.
    if state.settings().is_out(edition, &export.character) {
        update_settings(&state, |s| s.remember(edition, &export.character))?;
    }
    Ok(ProfileImported {
        character: export.character,
        edition: edition.key(),
        level: parsed.level,
    })
}

/// Take a character out of the Characters page (deleted in game): it no longer follows a guide.
/// Played again, it is back.
#[tauri::command(async)]
pub(crate) fn forget_character(state: State<'_, AppState>, character: String) -> CmdResult<()> {
    let time = characters(&state)?
        .into_iter()
        .find(|c| c.profile.name == character)
        .map_or(i64::MAX, |c| c.profile.time);
    update_settings(&state, |s| {
        let edition = s.edition;
        s.forget(edition, &character, time);
    })
}

pub(crate) fn characters(state: &AppState) -> CmdResult<Vec<CharacterInfo>> {
    let settings = state.settings();
    let client_dir = settings.edition.client_dir(&settings.wow_dir);
    let files = fg_contrib::collector::find_saved_variables(&client_dir);
    let full_names = full_names(&files);
    let races = &overrides(settings.edition)?.races;
    let now = i64::try_from(state.now()?).map_err(err)?;
    let profiles: Vec<(bool, StartState)> = fg_contrib::profile::read(&files)
        .map_err(err)?
        .into_iter()
        .filter_map(|json| {
            let shared = json["shared"].as_bool().unwrap_or(false);
            Some((shared, serde_json::from_value::<StartState>(json).ok()?))
        })
        .chain(
            (read_imported(state)
                .remove(settings.edition.key())
                .unwrap_or_default()
                .into_values())
            .filter_map(|json| Some((true, serde_json::from_value::<StartState>(json).ok()?))),
        )
        .collect();
    // A character of this computer is not also listed as received from another player.
    let own: std::collections::HashSet<String> = (profiles.iter())
        .filter(|(shared, _)| !shared)
        .map(|(_, p)| p.name.clone())
        .collect();
    Ok(profiles
        .into_iter()
        .filter(|(shared, p)| !shared || !own.contains(&p.name))
        .filter(|(_, profile)| !settings.is_forgotten(settings.edition, profile))
        .map(|(shared, mut profile)| {
            profile.rested = fg_app::rested_now(&profile, now);
            // Only when the first name is not shared (two characters with the same one).
            let first = profile.name.split('-').next().unwrap_or_default();
            let full_name = match full_names.get(first).map(Vec::as_slice) {
                Some([one]) => Some(one.clone()),
                _ => None,
            };
            CharacterInfo {
                full_name,
                race: fg_app::race_key(races, &profile),
                class: profile.class.to_lowercase(),
                professions: fg_app::profession_ranks(&profile),
                shared,
                profile,
            }
        })
        .collect())
}
