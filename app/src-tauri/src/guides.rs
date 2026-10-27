//! The guides: their versions, the planning and the editor, as the page calls them.

#![expect(
    clippy::needless_pass_by_value,
    reason = "tauri commands take their arguments by value"
)]

use super::*;

// Guides ---------------------------------------------------------------------------
//
// A guide is a name and its versions: every computation (first one, or computed again with the
// same name) adds a version. The guides go to the game all together (the data addon) or one by
// one (the text of `/fg import`), each in the version its page says to follow; the player picks
// a character's guide in game.
//
//   guides/<slug>/guide.json          name, version to follow
//   guides/<slug>/v<N>/meta.json      summary of the version
//   guides/<slug>/v<N>/request.json   configuration
//   guides/<slug>/v<N>/outcome.json   computed guide

impl AppState {
    /// The guides of the current edition (the app switches edition only when no generation is
    /// running).
    pub(crate) fn library(&self) -> Library<'_, DiskFiles> {
        Library::new(&self.files, self.edition())
    }

    /// Every guide in the version its page says to follow (the latest unless one is chosen),
    /// written in the language it was made in, with when that version was computed and its
    /// number.
    pub(crate) fn followed_guides(&self) -> CmdResult<Vec<(Route, u64, u32)>> {
        self.studio.followed_guides(self, self.edition())
    }
}

/// Results of earlier versions of the app (`results/<id>/`): each becomes a version of the
/// guide of its name, oldest first.
pub(crate) fn migrate_results(state: &AppState) {
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
                PlanOutcome::from_json(&read("outcome.json")?).ok()?,
            ))
        })
        .collect();
    found.sort_by_key(|(m, _, _)| m.created);
    for (meta, request, outcome) in &found {
        let _ = state.library().add_version(
            request,
            outcome,
            meta.data.clone(),
            Origin {
                imported: meta.imported,
                edited: false,
            },
            meta.created,
        );
    }
    let _ = std::fs::rename(&old, state.data_dir.join("results.old"));
}

/// Runs a command on a worker thread (the planner takes seconds to minutes).
pub(crate) async fn blocking<T: Send + 'static>(
    app: AppHandle,
    work: impl FnOnce(&AppState) -> CmdResult<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(move || work(&app.state::<AppState>()))
        .await
        .map_err(err)?
}

/// Plan a guide on a worker thread, streaming progress as `plan-progress` events. The result
/// is saved as a new version of the guide of the request's name. `stop_plan` ends it early with
/// the best guide found so far; `cancel_plan` makes it fail with "cancelled" (nothing saved).
#[tauri::command]
pub(crate) async fn plan_route(app: AppHandle, request: PlanRequest) -> CmdResult<VersionMeta> {
    blocking(app, move |state| state.studio.plan_route(state, &request)).await
}

/// The flight paths a character probably knows from the quests it did in their towns
/// (`job::inferred_flights`), besides those its addon recorded.
#[tauri::command]
pub(crate) async fn inferred_flights(app: AppHandle, start: StartState) -> CmdResult<Vec<i64>> {
    blocking(app, move |state| state.studio.inferred_flights(state, &start)).await
}

/// Quests to choose as must-have ones (`job::quest_choices`).
#[tauri::command]
pub(crate) async fn quest_choices(
    app: AppHandle,
    race: String,
    class: String,
    query: String,
    locale: Option<String>,
) -> CmdResult<Vec<job::QuestChoice>> {
    blocking(app, move |state| {
        state
            .studio
            .quest_choices(state, &race, &class, &query, locale.as_deref())
    })
    .await
}

/// The starts the character can choose from before the guide is computed (`job::preview_starts`),
/// streaming `plan-progress` events (`@preview <done> <total>`). Fewer than two: nothing to choose.
/// `cancel_plan` ends it with "cancelled".
#[tauri::command]
pub(crate) async fn preview_starts(app: AppHandle, request: PlanRequest) -> CmdResult<job::StartPreview> {
    blocking(app, move |state| state.studio.preview_starts(state, &request)).await
}

/// A new guide of no steps from the request's character and settings, saved as a version
/// edited by hand: its steps are then added in the editor.
#[tauri::command]
pub(crate) async fn manual_route(app: AppHandle, request: PlanRequest) -> CmdResult<VersionMeta> {
    blocking(app, move |state| state.studio.manual_route(state, &request)).await
}

/// A version replayed to be watched: when each step starts and ends, the character's moves and
/// what it has after each step (`locale`: language of the names).
#[tauri::command]
pub(crate) async fn guide_timeline(
    app: AppHandle,
    guide: GuideId,
    version: u32,
    locale: String,
) -> CmdResult<job::Timeline> {
    blocking(app, move |state| {
        state.studio.guide_timeline(state, &guide, version, &locale)
    })
    .await
}

// Guide editor -----------------------------------------------------------------------

/// Open a version in the editor (`locale`: language of the lines and names shown).
#[tauri::command]
pub(crate) async fn edit_open(app: AppHandle, guide: GuideId, version: u32, locale: String) -> CmdResult<EditOpened> {
    blocking(app, move |state| {
        state.studio.edit_open(state, &guide, version, &locale)
    })
    .await
}

/// Replay the steps of the guide open in the editor: its report, its lines and their timeline.
/// The replay under way gives up for this one (it then fails with "superseded").
#[tauri::command]
pub(crate) async fn edit_simulate(app: AppHandle, steps: Vec<RouteStep>, locale: String) -> CmdResult<job::EditView> {
    blocking(app, move |state| state.studio.edit_simulate(state, &steps, &locale)).await
}

/// What could be added to the steps of the guide open in the editor after its step `after`
/// (none: before the first): quests not taken, objectives and turn-ins the steps forget. Gives
/// up for a newer replay or suggestion (failing with "superseded").
#[tauri::command]
pub(crate) async fn edit_suggest(
    app: AppHandle,
    steps: Vec<RouteStep>,
    after: Option<usize>,
    locale: String,
) -> CmdResult<job::Suggestions> {
    blocking(app, move |state| {
        state.studio.edit_suggest(state, &steps, after, &locale)
    })
    .await
}

/// Save the edited steps as a new version of the guide (refused while a step is not possible)
/// and close the editor.
#[tauri::command]
pub(crate) async fn edit_save(app: AppHandle, steps: Vec<RouteStep>) -> CmdResult<VersionMeta> {
    blocking(app, move |state| state.studio.edit_save(state, &steps)).await
}

/// Leave the editor without saving.
#[tauri::command]
pub(crate) fn edit_close(state: State<'_, AppState>) {
    state.studio.edit_close();
}

/// End the optimization under way now: `plan_route` saves and returns the best guide so far.
#[tauri::command]
pub(crate) fn stop_plan(state: State<'_, AppState>) {
    state.studio.stop_plan();
}

/// Give the generation under way up: `plan_route` fails with "cancelled" within a moment.
#[tauri::command]
pub(crate) fn cancel_plan(state: State<'_, AppState>) {
    state.studio.cancel_plan();
}

/// Every guide, most recently computed first.
#[tauri::command]
pub(crate) fn list_guides(state: State<'_, AppState>) -> Vec<GuideSummary> {
    state.library().list()
}

#[tauri::command]
pub(crate) fn get_guide(state: State<'_, AppState>, id: GuideId) -> CmdResult<GuideSummary> {
    Ok(state.library().summary(&id)?)
}

/// Versions `a` and `b` of a guide compared, written in `locale` (game locale).
#[tauri::command(async)]
pub(crate) fn compare_versions(
    state: State<'_, AppState>,
    guide: GuideId,
    a: u32,
    b: u32,
    locale: Option<String>,
) -> CmdResult<Comparison> {
    // Compared as saved (zones by their English names), shown in `locale`.
    state.studio.compare_versions(&*state, &guide, a, b, locale.as_deref())
}

#[tauri::command(async)]
pub(crate) fn get_version(
    state: State<'_, AppState>,
    guide: GuideId,
    version: u32,
    locale: Option<String>,
) -> CmdResult<GuideVersion> {
    state.studio.get_version(&*state, &guide, version, locale.as_deref())
}

/// The version of a guide its characters follow (`None`: the latest), written to the addon.
#[tauri::command(async)]
pub(crate) fn set_installed(state: State<'_, AppState>, guide: GuideId, version: Option<u32>) -> CmdResult<()> {
    Ok(state.library().set_installed(&guide, version)?)
}

#[tauri::command(async)]
pub(crate) fn delete_version(state: State<'_, AppState>, guide: GuideId, version: u32) -> CmdResult<()> {
    Ok(state.library().delete_version(&guide, version)?)
}

#[tauri::command(async)]
pub(crate) fn delete_guide(state: State<'_, AppState>, guide: GuideId) -> CmdResult<()> {
    Ok(state.library().delete_guide(&guide)?)
}

/// Write a version as a .fgguide file in the Downloads folder; returns its path.
#[tauri::command(async)]
pub(crate) fn export_version(
    app: AppHandle,
    state: State<'_, AppState>,
    guide: GuideId,
    version: u32,
) -> CmdResult<String> {
    let shared = state.library().share(&guide, version)?;
    let dir = app.path().download_dir().map_err(err)?;
    let path = dir.join(format!("{guide}-v{version}.fgguide"));
    std::fs::write(&path, shared.to_json()?).map_err(err)?;
    Ok(path.display().to_string())
}

/// Add a guide received as a .fgguide file (its content): a new version of the guide of its name.
#[tauri::command(async)]
pub(crate) fn import_guide(state: State<'_, AppState>, content: String) -> CmdResult<VersionMeta> {
    state.studio.import_guide(&*state, &content)
}
