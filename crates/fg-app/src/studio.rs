use crate::{CmdError, Host};
use fg_library::{Comparison, GuideId, GuideVersion, Library, Origin, SharedGuide, VersionMeta};
use fg_route::Route;
use fg_route::job::{self, EditCatalog, PlanRequest, RouteStep, StartState};
use fg_route::model::Names;
use fg_route::sync::LockExt;
use fg_route::terrain::Walks;
use fg_route::xp::Edition;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

type Result<T> = std::result::Result<T, CmdError>;

/// The guide open in the editor.
struct EditSession {
    edition: Edition,
    request: PlanRequest,
    notes: Vec<String>,
    editor: job::Editor,
}

/// A version opened in the editor.
#[derive(Serialize)]
pub struct EditOpened {
    pub steps: Vec<RouteStep>,
    /// Steps read back from the guide (computed before the editor): its time may differ a little.
    pub rebuilt: bool,
    /// The steps replayed.
    pub view: job::EditView,
    pub catalog: EditCatalog,
}

/// A guide version replayed (game version, guide, version, when it was computed, language).
type TimelineKey = (Edition, GuideId, u32, u64, String);

/// What the app remembers between its commands: the generation under way, the guide in the
/// editor, the walking memory and the names read. The files, the game data and the events are
/// the [`Host`]'s, given to each call.
#[derive(Default)]
pub struct Studio {
    interrupt: job::Interrupt,
    edit_requests: job::Requests,
    editor: Mutex<Option<EditSession>>,
    /// The last guide version replayed to be watched: opened again at once.
    timeline: Mutex<Option<(TimelineKey, job::Timeline)>>,
    /// Names of the game in each language read so far, to write guides in it.
    names: Mutex<HashMap<(Edition, String), Arc<Names>>>,
    /// Walking memory of each edition (paths around the terrain already searched), kept between
    /// generations: filled at the first one with the memories saved with the guides.
    walks: Mutex<HashMap<Edition, Arc<Walks>>>,
}

/// Version of the quest data imported in a database.
fn data_version(conn: &Connection) -> Option<String> {
    conn.query_row("SELECT detail FROM import_run WHERE source = 'questie'", [], |r| {
        r.get::<_, Option<String>>(0)
    })
    .ok()
    .flatten()
}

/// Unix seconds.
///
/// # Errors
///
/// [`CmdError`]: `Cancelled` and `Superseded` when the page gave up, otherwise the planner, the game data or the guide files failed.
pub fn now() -> Result<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|e| CmdError::App(e.to_string()))
}

/// The guides of an edition.
pub fn library<H: Host>(host: &H, edition: Edition) -> Library<'_, H::Files> {
    Library::new(host.files(), edition)
}

/// The edition a configuration was made for (the current one when it does not say).
pub fn request_edition<H: Host>(host: &H, request: &PlanRequest) -> Edition {
    request.params.as_ref().map_or_else(|| host.edition(), |p| p.edition)
}

impl Studio {
    // Planning ------------------------------------------------------------------------

    /// Plans a guide, streaming progress through the host, and saves it as a new version of the
    /// guide of the request's name. `stop_plan` ends it early with the best guide found so far;
    /// `cancel_plan` makes it fail with `Cancelled` (nothing saved).
    ///
    /// # Errors
    ///
    /// [`CmdError`]: `Cancelled` and `Superseded` when the page gave up, otherwise the planner, the game data or the guide files failed.
    pub fn plan_route<H: Host>(&self, host: &H, request: &PlanRequest) -> Result<VersionMeta> {
        self.interrupt.reset();
        let edition = request_edition(host, request);
        let conn = host.open_db(edition)?;
        let overrides = host.overrides(edition)?;
        let walks = self.walks(host, edition);
        let progress = |line: &str| host.progress(line);
        let outcome = job::plan_interruptible(&conn, &overrides, request, &progress, &self.interrupt, Some(&walks))?;
        Ok(library(host, edition).add_version(request, &outcome, data_version(&conn), Origin::COMPUTED, now()?)?)
    }

    /// The starts the character can choose from before the guide is computed. Fewer than two:
    /// nothing to choose. `cancel_plan` ends it with `Cancelled`.
    ///
    /// # Errors
    ///
    /// [`CmdError`]: `Cancelled` and `Superseded` when the page gave up, otherwise the planner, the game data or the guide files failed.
    pub fn preview_starts<H: Host>(&self, host: &H, request: &PlanRequest) -> Result<job::StartPreview> {
        self.interrupt.reset();
        let edition = request_edition(host, request);
        let conn = host.open_db(edition)?;
        let walks = self.walks(host, edition);
        let progress = |line: &str| host.progress(line);
        Ok(job::preview_starts(
            &conn,
            &host.overrides(edition)?,
            request,
            &progress,
            &self.interrupt,
            Some(&walks),
        )?)
    }

    /// A new guide of no steps from the request's character and settings, saved as a version
    /// edited by hand: its steps are then added in the editor.
    ///
    /// # Errors
    ///
    /// [`CmdError`]: `Cancelled` and `Superseded` when the page gave up, otherwise the planner, the game data or the guide files failed.
    pub fn manual_route<H: Host>(&self, host: &H, request: &PlanRequest) -> Result<VersionMeta> {
        let edition = request_edition(host, request);
        let conn = host.open_db(edition)?;
        let walks = self.walks(host, edition);
        let outcome = job::manual(&conn, &host.overrides(edition)?, request, Some(&walks))?;
        Ok(library(host, edition).add_version(request, &outcome, data_version(&conn), Origin::EDITED, now()?)?)
    }

    /// The flight paths a character probably knows, besides those its addon recorded: its race's
    /// from the start, and those of the towns it did quests in.
    ///
    /// # Errors
    ///
    /// [`CmdError`]: `Cancelled` and `Superseded` when the page gave up, otherwise the planner, the game data or the guide files failed.
    pub fn inferred_flights<H: Host>(&self, host: &H, start: &StartState) -> Result<Vec<i64>> {
        let conn = host.open_db(host.edition())?;
        let side = start.faction.ok_or("unknown faction")?;
        let mut found = job::inferred_flights(&conn, side, &start.completed)?;
        found.extend(host.overrides(host.edition())?.start_flights(start.race));
        found.sort_unstable();
        found.dedup();
        Ok(found)
    }

    /// Quests to choose as must-have ones for a race and class, by ID or name (`job::quest_choices`).
    ///
    /// # Errors
    ///
    /// [`CmdError`]: the game data or the configuration could not be read.
    pub fn quest_choices<H: Host>(
        &self,
        host: &H,
        race: &str,
        class: &str,
        query: &str,
        locale: Option<&str>,
    ) -> Result<Vec<job::QuestChoice>> {
        let conn = host.open_db(host.edition())?;
        Ok(job::quest_choices(
            &conn,
            &host.overrides(host.edition())?,
            race,
            class,
            query,
            locale,
        )?)
    }

    /// Ends the optimization under way now: `plan_route` saves and returns the best guide so far.
    pub fn stop_plan(&self) {
        self.interrupt.stop();
    }

    /// Gives the generation under way up: `plan_route` fails with `Cancelled` within a moment.
    pub fn cancel_plan(&self) {
        self.interrupt.cancel();
    }

    // Shared state --------------------------------------------------------------------

    /// The walking memory of an edition: the first time, the memories of all its guides (each
    /// version's outcome holds the paths it searched).
    pub fn walks<H: Host>(&self, host: &H, edition: Edition) -> Arc<Walks> {
        if let Some(w) = self.walks.locked().get(&edition) {
            return Arc::clone(w);
        }
        let walks = Arc::new(Walks::default());
        for memory in library(host, edition).walk_memories() {
            walks.add(memory);
        }
        Arc::clone(self.walks.locked().entry(edition).or_insert(walks))
    }

    /// Names of an edition in a game language, read once.
    ///
    /// # Errors
    ///
    /// [`CmdError`]: `Cancelled` and `Superseded` when the page gave up, otherwise the planner, the game data or the guide files failed.
    pub fn names<H: Host>(&self, host: &H, edition: Edition, locale: &str) -> Result<Arc<Names>> {
        let key = (edition, locale.to_owned());
        if let Some(n) = self.names.locked().get(&key) {
            return Ok(n.clone());
        }
        let names = Arc::new(Names::load(&host.open_db(edition)?, locale)?);
        self.names.locked().insert(key, names.clone());
        Ok(names)
    }

    /// A guide written in `locale` (None: the language it was made in).
    ///
    /// # Errors
    ///
    /// [`CmdError`]: `Cancelled` and `Superseded` when the page gave up, otherwise the planner, the game data or the guide files failed.
    pub fn render<H: Host>(&self, host: &H, edition: Edition, route: &Route, locale: Option<&str>) -> Result<Route> {
        let locale = locale.or(route.locale.as_deref()).unwrap_or("enUS");
        Ok(fg_route::export::render(route, &*self.names(host, edition, locale)?))
    }

    // Guides --------------------------------------------------------------------------

    /// Every guide of an edition in the version its page says to follow (the latest unless one is
    /// chosen), written in the language it was made in, with when that version was computed and
    /// its number; by name.
    ///
    /// # Errors
    ///
    /// [`CmdError`]: `Cancelled` and `Superseded` when the page gave up, otherwise the planner, the game data or the guide files failed.
    pub fn followed_guides<H: Host>(&self, host: &H, edition: Edition) -> Result<Vec<(Route, u64, u32)>> {
        let mut out = Vec::new();
        for (route, created, n) in library(host, edition).followed() {
            out.push((self.render(host, edition, &route, None)?, created, n));
        }
        out.sort_by(|a, b| a.0.name.cmp(&b.0.name));
        Ok(out)
    }

    /// A version of a guide, written in `locale`.
    ///
    /// # Errors
    ///
    /// [`CmdError`]: `Cancelled` and `Superseded` when the page gave up, otherwise the planner, the game data or the guide files failed.
    pub fn get_version<H: Host>(
        &self,
        host: &H,
        guide: &GuideId,
        n: u32,
        locale: Option<&str>,
    ) -> Result<GuideVersion> {
        let edition = host.edition();
        let mut v = library(host, edition).version_to_show(guide, n)?;
        if let Some(locale) = locale {
            v.outcome.route = self.render(host, edition, &v.outcome.route, Some(locale))?;
        }
        Ok(v)
    }

    /// Versions `a` and `b` of a guide compared, shown in `locale` (game locale).
    ///
    /// # Errors
    ///
    /// [`CmdError`]: `Cancelled` and `Superseded` when the page gave up, otherwise the planner, the game data or the guide files failed.
    pub fn compare_versions<H: Host>(
        &self,
        host: &H,
        guide: &GuideId,
        a: u32,
        b: u32,
        locale: Option<&str>,
    ) -> Result<Comparison> {
        let edition = host.edition();
        let mut c = library(host, edition).compare(guide, a, b)?;
        if let Some(locale) = locale {
            c.a.outcome.route = self.render(host, edition, &c.a.outcome.route, Some(locale))?;
            c.b.outcome.route = self.render(host, edition, &c.b.outcome.route, Some(locale))?;
        }
        Ok(c)
    }

    /// Adds a guide received as a .fgguide file (its content): a new version of the guide of its
    /// name.
    ///
    /// # Errors
    ///
    /// [`CmdError`]: `Cancelled` and `Superseded` when the page gave up, otherwise the planner, the game data or the guide files failed.
    pub fn import_guide<H: Host>(&self, host: &H, content: &str) -> Result<VersionMeta> {
        let shared = SharedGuide::parse(content)?;
        let edition = host.edition();
        // Its walking memory joins the app's (when made on the same terrain data).
        if let (Some(memory), Some(walks)) = (&shared.outcome.walks, self.walks.locked().get(&edition)) {
            walks.add(memory.clone());
        }
        Ok(library(host, edition).add_version(
            &shared.request,
            &shared.outcome,
            shared.data(),
            Origin::IMPORTED,
            now()?,
        )?)
    }

    // The guide editor ----------------------------------------------------------------
    //
    // A version opened in the editor is loaded once (character, settings, world); each change of
    // its steps is replayed (`edit_simulate`), nothing is written until `edit_save` adds the
    // edited guide as a new version.

    /// A version replayed to be watched: when each step starts and ends, the character's moves
    /// and what it has after each step (`locale`: language of the names).
    ///
    /// # Errors
    ///
    /// [`CmdError`]: `Cancelled` and `Superseded` when the page gave up, otherwise the planner, the game data or the guide files failed.
    pub fn guide_timeline<H: Host>(&self, host: &H, guide: &GuideId, n: u32, locale: &str) -> Result<job::Timeline> {
        let v = library(host, host.edition()).version(guide, n)?;
        let edition = request_edition(host, &v.request);
        let key: TimelineKey = (edition, guide.clone(), n, v.meta.created, locale.to_owned());
        if let Some((k, timeline)) = self.timeline.locked().as_ref()
            && *k == key
        {
            return Ok(timeline.clone());
        }
        let conn = host.open_db(edition)?;
        let walks = self.walks(host, edition);
        let editor = job::Editor::open(&conn, &host.overrides(edition)?, &v.request, Some(&walks))?;
        let timeline = editor.timeline(&v.outcome, &*self.names(host, edition, locale)?);
        *self.timeline.locked() = Some((key, timeline.clone()));
        Ok(timeline)
    }

    /// Opens a version in the editor (`locale`: language of the lines and names shown).
    ///
    /// # Errors
    ///
    /// [`CmdError`]: `Cancelled` and `Superseded` when the page gave up, otherwise the planner, the game data or the guide files failed.
    pub fn edit_open<H: Host>(&self, host: &H, guide: &GuideId, n: u32, locale: &str) -> Result<EditOpened> {
        let v = library(host, host.edition()).version(guide, n)?;
        let edition = request_edition(host, &v.request);
        let conn = host.open_db(edition)?;
        let walks = self.walks(host, edition);
        let editor = job::Editor::open(&conn, &host.overrides(edition)?, &v.request, Some(&walks))?;
        let names = self.names(host, edition, locale)?;
        let (steps, rebuilt) = editor.steps(&v.outcome);
        let request = self.edit_requests.next();
        let opened = EditOpened {
            view: editor.view(&steps, &names, &request).ok_or(CmdError::Superseded)?,
            catalog: editor.catalog(&names),
            steps,
            rebuilt,
        };
        *self.editor.locked() = Some(EditSession {
            edition,
            request: v.request,
            notes: v.outcome.notes,
            editor,
        });
        Ok(opened)
    }

    /// Replays the steps of the guide open in the editor: its report, its lines and their
    /// timeline. The replay under way gives up for this one (it then fails with `Superseded`).
    ///
    /// # Errors
    ///
    /// [`CmdError`]: `Cancelled` and `Superseded` when the page gave up, otherwise the planner, the game data or the guide files failed.
    pub fn edit_simulate<H: Host>(&self, host: &H, steps: &[RouteStep], locale: &str) -> Result<job::EditView> {
        let request = self.edit_requests.next();
        let session = self.editor.locked();
        let session = session.as_ref().ok_or(CmdError::NoEditor)?;
        let names = self.names(host, session.edition, locale)?;
        session.editor.view(steps, &names, &request).ok_or(CmdError::Superseded)
    }

    /// What could be added to the steps of the guide open in the editor after its step `after`
    /// (none: before the first): quests not taken, objectives and turn-ins the steps forget. Gives
    /// up for a newer replay or suggestion (failing with `Superseded`).
    ///
    /// # Errors
    ///
    /// [`CmdError`]: `Cancelled` and `Superseded` when the page gave up, otherwise the planner, the game data or the guide files failed.
    pub fn edit_suggest<H: Host>(
        &self,
        host: &H,
        steps: &[RouteStep],
        after: Option<usize>,
        locale: &str,
    ) -> Result<job::Suggestions> {
        let request = self.edit_requests.next();
        let session = self.editor.locked();
        let session = session.as_ref().ok_or(CmdError::NoEditor)?;
        let names = self.names(host, session.edition, locale)?;
        session
            .editor
            .suggest(steps, after, &names, &request)
            .ok_or(CmdError::Superseded)
    }

    /// Saves the edited steps as a new version of the guide (refused while a step is not
    /// possible) and closes the editor.
    ///
    /// # Errors
    ///
    /// [`CmdError`]: `Cancelled` and `Superseded` when the page gave up, otherwise the planner, the game data or the guide files failed.
    pub fn edit_save<H: Host>(&self, host: &H, steps: &[RouteStep]) -> Result<VersionMeta> {
        let mut session = self.editor.locked();
        let open = session.as_ref().ok_or(CmdError::NoEditor)?;
        let outcome = open.editor.save(steps, open.notes.clone())?;
        let data = data_version(&host.open_db(open.edition)?);
        let meta = library(host, open.edition).add_version(&open.request, &outcome, data, Origin::EDITED, now()?)?;
        *session = None;
        Ok(meta)
    }

    /// Leaves the editor without saving.
    pub fn edit_close(&self) {
        *self.editor.locked() = None;
    }
}
