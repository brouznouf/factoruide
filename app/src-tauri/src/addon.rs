//! The data addon and the Polestar pack the app writes into the game's folders, and what it opens for the player.

#![expect(
    clippy::needless_pass_by_value,
    reason = "tauri commands take their arguments by value"
)]

use super::{
    AppState, CmdError, CmdResult, DATA_ADDON, Edition, GUIDE_ADDON, GuideId, Host, Ordering, PROFILE_ADDON, Path,
    Settings, State, WowProblem, addon_snapshot, embedded, err, game, job, loaded_files, overrides, read_send_record,
    send_record_file, sent_lua, update_settings,
};

/// The oldest addon this app works with (its .toc version): older ones are to be updated.
pub(crate) const ADDON_VERSION: &str = embedded::ADDON_VERSION;

/// Write the guides assigned to a character (in the version they follow) and who follows which,
/// the places of the trainers, vendors... and what the contribution server has (the addon drops
/// those recordings) into the data addon; on WoW Forever with Polestar, its guide pack too. The
/// addon itself comes from an addon site and is set up in game. Returns the number of guides.
#[tauri::command(async)]
pub(crate) fn install_addon(state: State<'_, AppState>) -> CmdResult<usize> {
    write_data(&state)
}

/// What `install_addon` does.
pub(crate) fn write_data(state: &AppState) -> CmdResult<usize> {
    let _one = state
        .installing
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let settings = state.settings();
    match game::status(&settings.wow_dir, settings.edition).problem {
        None => {}
        Some(WowProblem::NotWritable) => {
            return Err(CmdError::App(format!(
                "the app may not write in {}: give your Windows user the right to modify the World of \
                 Warcraft folder (its Properties, Security tab), or start the app once as administrator",
                settings.addons_dir(settings.edition).display()
            )));
        }
        Some(_) => {
            return Err(CmdError::App(format!(
                "{} not found in {}: choose the World of Warcraft folder in Settings",
                settings.edition.rules().edition_name,
                settings.wow_dir.display()
            )));
        }
    }
    let addons = settings.addons_dir(settings.edition);
    let before = loaded_files(&addons);
    migrate_legacy_addon(&addons);
    let guides = state.followed_guides()?;
    let count = guides.len();
    let mut lua = job::guides_lua(
        &state.open_db()?,
        &*overrides(settings.edition)?,
        settings.edition,
        guides,
    )
    .map_err(err)?;
    lua.push('\n');
    let sent = read_send_record(&send_record_file(&state.data_dir, settings.edition)).unwrap_or_default();
    lua.push_str(&sent_lua(&sent.state));
    let guide_toc = addons.join(GUIDE_ADDON).join(format!("{GUIDE_ADDON}.toc"));
    let interface = std::fs::read_to_string(&guide_toc)
        .ok()
        .and_then(|toc| toc.lines().find(|l| l.starts_with("## Interface:")).map(str::to_owned))
        .unwrap_or_else(|| embedded::ADDON_INTERFACE.to_owned());
    job::write_data_addon(
        &addons.join(DATA_ADDON),
        &lua,
        &interface,
        env!("CARGO_PKG_VERSION"),
        state.now()?,
    )
    .map_err(err)?;
    // A folder or a file the game does not load yet: it reads the .toc files when it starts.
    let after = loaded_files(&addons);
    let restart = after.iter().any(|(toc, files)| before.get(toc) != Some(files));
    state.restart_needed.store(restart, Ordering::Relaxed);
    // Only what was installed: the other settings may have changed meanwhile.
    let installed = addon_snapshot(&settings);
    update_settings(state, |s| s.addon_installed = Some(installed))?;
    Ok(count)
}

/// WoW Forever: every guide (see `followed_guides`) as a guide pack for Polestar, written into
/// the game (Polestar picks each character's guide by its race, class and level). Returns the
/// number of guides.
#[tauri::command(async)]
pub(crate) fn install_polestar(state: State<'_, AppState>) -> CmdResult<usize> {
    let settings = state.settings();
    if settings.edition != Edition::Forever {
        return Err("Polestar is for WoW Forever only".into());
    }
    let guides = state
        .followed_guides()?
        .into_iter()
        .map(|(route, _, _)| (job::slug(&route.name), route))
        .collect::<Vec<_>>();
    let pack = settings
        .addons_dir(Edition::Forever)
        .join(fg_route::export::polestar::ADDON);
    let files = fg_route::export::polestar::pack(&guides, env!("CARGO_PKG_VERSION"));
    let _ = std::fs::remove_dir_all(&pack);
    std::fs::create_dir_all(&pack).map_err(err)?;
    for (name, content) in files {
        std::fs::write(pack.join(name), content).map_err(err)?;
    }
    state.restart_needed.store(true, Ordering::Relaxed);
    Ok(guides.len())
}

/// What the player pastes in game with `/fg import`: one guide version (the addon adds it to
/// the guides it has, or replaces the one of its name).
#[tauri::command(async)]
pub(crate) fn guide_text(state: State<'_, AppState>, guide: GuideId, version: u32) -> CmdResult<String> {
    state.studio.guide_text(&*state, &guide, version)
}

/// What the player pastes on Polestar's Import page (`/polestar import`): one guide version, in
/// the game's language. Polestar exists on WoW Forever only.
#[tauri::command(async)]
pub(crate) fn polestar_string(
    state: State<'_, AppState>,
    guide: GuideId,
    version: u32,
) -> CmdResult<fg_route::export::polestar::ImportString> {
    state.studio.polestar_string(&*state, &guide, version)
}

/// Whether Polestar is installed in WoW Forever (it shows guides there only).
#[tauri::command]
pub(crate) fn polestar_installed(state: State<'_, AppState>) -> bool {
    polestar_installed_in(&state.settings())
}

pub(crate) fn polestar_installed_in(settings: &Settings) -> bool {
    settings
        .addons_dir(Edition::Forever)
        .join("Polestar/Polestar.toc")
        .exists()
}

/// Export a guide version for Polestar's own tools (route planner, playground): its `.guide`
/// file (in English) added to the pack folder in `dir` (the folder chosen, or a pack folder made
/// in it), the pack's other guides kept. Returns the pack's folder. Polestar exists on WoW
/// Forever only.
#[tauri::command(async)]
pub(crate) fn export_polestar(
    state: State<'_, AppState>,
    guide: GuideId,
    version: u32,
    dir: String,
) -> CmdResult<String> {
    if state.edition() != Edition::Forever {
        return Err("Polestar is for WoW Forever only".into());
    }
    let route = state.render(&state.library().version(&guide, version)?.outcome.route, Some("enUS"))?;
    let pack = job::polestar_source_dir(Path::new(&dir));
    job::add_polestar_source(&pack, guide.as_str(), &route, env!("CARGO_PKG_VERSION")).map_err(err)?;
    Ok(pack.display().to_string())
}

/// Whether the data addon in the game has the current guides (else it is to be written again).
#[tauri::command]
pub(crate) fn addon_synced(state: State<'_, AppState>) -> bool {
    let settings = state.settings();
    let addons = settings.addons_dir(settings.edition);
    addons.join(DATA_ADDON).join("Data.lua").exists()
        && settings.addon_installed.as_deref() == Some(addon_snapshot(&settings).as_str())
}

/// Pages the app opens in the browser: the project's (repository, license, notice) and the
/// community tools the contribution page points to; nothing else.
pub(crate) const LINKS: &[&str] = &[
    "https://github.com/brouznouf/factoruide/",
    "https://www.curseforge.com/wow/",
    "https://github.com/Questie/QuestieTrace",
    "https://questie.dev/trace",
    "https://www.wowhead.com/",
];

/// Open one of the `LINKS` pages in the browser.
#[tauri::command]
pub(crate) fn open_link(url: String) -> CmdResult<()> {
    if !LINKS.iter().any(|l| url.starts_with(l)) {
        return Err(CmdError::App(format!("not a link the app opens: {url}")));
    }
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program).arg(&url).spawn().map_err(err)?;
    Ok(())
}

/// The addon was called BrouzQuest, then was a single addon, Factoruide, before its profile and
/// guide parts: drop their folders (the game would load them too) and carry their SavedVariables
/// over to the profile part. `addons`: the client's Interface/AddOns folder.
pub(crate) fn migrate_legacy_addon(addons: &Path) {
    let _ = std::fs::remove_dir_all(addons.join("BrouzQuest"));
    let _ = std::fs::remove_dir_all(addons.join("Factoruide"));
    if let Some(client) = addons.parent().and_then(Path::parent) {
        migrate_saved_variables(client);
    }
}

/// The SavedVariables of the former addons, carried over to the profile part, unless that one
/// has newer ones (a game still running the old addon writes its file until it restarts: carried
/// over again then). The old files are kept as `.migrated`.
pub(crate) fn migrate_saved_variables(client: &Path) {
    let Ok(accounts) = std::fs::read_dir(client.join("WTF/Account")) else {
        return;
    };
    let modified = |f: &Path| std::fs::metadata(f).and_then(|m| m.modified()).ok();
    for account in accounts.flatten() {
        let dir = account.path().join("SavedVariables");
        let (brouzquest, single, profile) = (
            dir.join("BrouzQuest.lua"),
            dir.join("Factoruide.lua"),
            dir.join(format!("{PROFILE_ADDON}.lua")),
        );
        if !single.exists()
            && !profile.exists()
            && let Ok(text) = std::fs::read_to_string(&brouzquest)
            && std::fs::write(&single, text.replace("BrouzQuestDB", "FactoruideDB")).is_ok()
        {
            let _ = std::fs::rename(&brouzquest, dir.join("BrouzQuest.lua.migrated"));
        }
        if single.exists()
            && (!profile.exists() || modified(&single) > modified(&profile))
            && std::fs::copy(&single, &profile).is_ok()
        {
            let _ = std::fs::rename(&single, dir.join("Factoruide.lua.migrated"));
        }
    }
}
