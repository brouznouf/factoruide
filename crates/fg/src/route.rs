//! `fg route`: plan a leveling route for a race/class and export it for the addon.

use anyhow::{Context, Result};
use fg_route::Route;
use fg_route::export::StepKind;
use fg_route::job::{self, Overrides, PlanRequest};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

pub struct RouteOptions {
    pub race: String,
    pub class: String,
    pub from_level: i64,
    pub to_level: i64,
    pub effort: Option<f64>,
    pub construction: Option<String>,
    pub out_name: Option<String>,
    pub allow_elite: bool,
    pub no_dungeons: bool,
    pub pvp_quests: bool,
    /// Progression profile (cautious, normal, risky).
    pub progression: Option<String>,
    /// Planner parameter overrides (key=value).
    pub params: Vec<String>,
    pub professions: Vec<fg_route::profession::ProfessionGoal>,
    pub required_quests: Vec<String>,
    /// Language of the guide texts.
    pub locale: Option<String>,
    /// Profiles (JSON) of the character and of the other players of a group guide, and where to
    /// write the character's at the end.
    pub start: Option<PathBuf>,
    pub mates: Vec<PathBuf>,
    pub profile_out: Option<PathBuf>,
    pub overrides: PathBuf,
    pub routes_dir: PathBuf,
    pub addon_dir: PathBuf,
}

pub fn run(conn: &Connection, opts: &RouteOptions) -> Result<()> {
    let overrides = Overrides::load_edition(&opts.overrides, crate::edition())?;
    let mut params = overrides.params.with_overrides(&opts.params)?;
    if let Some(e) = opts.effort {
        params.effort = e;
    }
    if let Some(c) = &opts.construction {
        params = params.with_overrides(&[format!("construction={c}")])?;
    }
    if opts.allow_elite {
        params.allow_elite = true;
    }
    if opts.no_dungeons {
        params.dungeons = false;
    }
    if opts.pvp_quests {
        params.pvp_quests = true;
    }
    if let Some(p) = &opts.progression {
        params = params.with_overrides(&[format!("progression={p}")])?;
    }
    let request = PlanRequest {
        race: opts.race.clone(),
        class: opts.class.clone(),
        from_level: opts.from_level,
        to_level: opts.to_level,
        name: opts.out_name.clone(),
        params: Some(params),
        required_class_quests: None,
        required_quests: opts
            .required_quests
            .iter()
            .map(|q| {
                q.parse::<i64>()
                    .map_or_else(|_| job::RequiredQuest::Name(q.clone()), job::RequiredQuest::Id)
            })
            .collect(),
        professions: opts.professions.clone(),
        locale: opts.locale.clone(),
        start: opts.start.as_deref().map(read_profile).transpose()?,
        mates: opts.mates.iter().map(|p| read_profile(p)).collect::<Result<_>>()?,
    };
    // FG_PREVIEW_STARTS: the starts the app offers to choose from, as JSON (not a guide).
    if std::env::var_os("FG_PREVIEW_STARTS").is_some() {
        let began = std::time::Instant::now();
        let starts = job::preview_starts(
            conn,
            &overrides,
            &request,
            &|m| eprintln!("{m}"),
            &job::Interrupt::default(),
            None,
        )?;
        eprintln!(
            "{} starts in {:.0} s",
            starts.options.len(),
            began.elapsed().as_secs_f64()
        );
        println!("{}", serde_json::to_string_pretty(&starts)?);
        return Ok(());
    }
    let outcome = job::plan(conn, &overrides, &request, &|m| {
        if !m.starts_with('@') {
            eprintln!("{m}");
        }
    })?;
    for note in &outcome.notes {
        eprintln!("  {note}");
    }
    let route = &outcome.route;
    let b = &route.breakdown;
    let m = fg_route::plan::fmt_time;
    eprintln!(
        "  travel {} | flights {} | fighting {} | grinding {} (over cap {}) | farming on the way {} | overhead {}",
        m(b.travel),
        m(b.flights),
        m(b.fighting),
        m(b.grinding),
        m(b.grind_over),
        m(b.farming),
        m(b.overhead)
    );
    eprintln!(
        "  xp: quests {} | quest mobs {} | grinding {} | farming on the way {} | exploration {}",
        b.quest_xp, b.mob_xp, b.grind_xp, b.farm_xp, b.explore_xp
    );
    eprintln!(
        "  training {} | hearthstones {} | class quests missing {} | zone quests missing {} | profession quests missing {}",
        m(b.training),
        b.hearths,
        b.missing_class_quests,
        b.missing_zone_quests,
        b.missing_profession_quests
    );
    eprintln!("  dungeons: {} runs, {}", b.dungeon_runs, m(b.dungeons));
    for r in &route.reputation {
        eprintln!(
            "  reputation {}: {} -> {} (goal {})",
            r.name, r.start, r.reached, r.target
        );
    }
    if b.sync_time > 0.0 || b.desync > 0.0 {
        eprintln!(
            "  group levels together after {} (levels apart beyond the window: {:.0} level-seconds)",
            m(b.sync_time),
            b.desync
        );
    }
    if let (Some(path), Some(end)) = (&opts.profile_out, &outcome.end) {
        std::fs::write(path, serde_json::to_string_pretty(end)?)?;
        eprintln!("wrote {}", path.display());
    }
    job::save_route(&opts.routes_dir, route)?;
    write_addon_routes(conn, &opts.overrides, &opts.routes_dir, &opts.addon_dir)
}

/// A character's profile (JSON, as `--profile-out` writes it).
fn read_profile(path: &Path) -> Result<job::StartState> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("{} is not a profile", path.display()))
}

/// Write the data addon (`addon_dir`, Factoruide_Data) from every route JSON (each in its own
/// language): copy it into the game next to the addon.
pub fn write_addon_routes(conn: &Connection, overrides_dir: &Path, routes_dir: &Path, addon_dir: &Path) -> Result<()> {
    let overrides = Overrides::load_edition(overrides_dir, crate::edition())?;
    // The game versions the guide addon of this checkout declares.
    let guide_toc = addon_dir
        .parent()
        .unwrap_or(Path::new("."))
        .join("Factoruide_Guide/Factoruide_Guide.toc");
    let interface = std::fs::read_to_string(&guide_toc)
        .ok()
        .and_then(|toc| toc.lines().find(|l| l.starts_with("## Interface:")).map(str::to_owned))
        .unwrap_or_else(|| "## Interface: 11508".to_owned());
    let n = job::write_addon_routes(conn, &overrides, crate::edition(), routes_dir, addon_dir, &interface)?;
    eprintln!("wrote {} ({n} routes)", addon_dir.join("Data.lua").display());
    Ok(())
}

/// Write the Lua of every route JSON as the apps export it (`job::guides_lua`) into `out`, each
/// guide stamped computed at 0, version 1.
pub fn write_guides_lua(conn: &Connection, overrides_dir: &Path, routes_dir: &Path, out: &Path) -> Result<()> {
    let overrides = Overrides::load_edition(overrides_dir, crate::edition())?;
    let routes = job::render_routes(conn, &job::load_routes(routes_dir)?, None)?;
    let n = routes.len();
    let lua = job::guides_lua(
        conn,
        &overrides,
        crate::edition(),
        routes.into_iter().map(|r| (r, 0, 1)).collect(),
    )?;
    std::fs::write(out, lua).with_context(|| format!("writing {}", out.display()))?;
    eprintln!("wrote {} ({n} routes)", out.display());
    Ok(())
}

/// Print a route as zone segments, with time and level at each change.
pub fn show(routes_dir: &Path, name: &str, full: bool) -> Result<()> {
    let path = routes_dir.join(format!("{}.json", job::slug(name)));
    let route: Route = fg_route::export::read_route(serde_json::from_str(
        &std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?,
    )?)?;
    let t = fg_route::plan::fmt_time;
    println!(
        "{}: level {} -> {} in {}, {} quests, {} steps",
        route.name,
        route.from_level,
        route.to_level,
        route.total_time_text,
        route.quests,
        route.steps.len()
    );
    let b = &route.breakdown;
    println!(
        "travel {} | flights {} | fighting {} | grinding {} | farming on the way {} | overhead {}",
        t(b.travel),
        t(b.flights),
        t(b.fighting),
        t(b.grinding),
        t(b.farming),
        t(b.overhead)
    );
    let mut zone = String::new();
    for (i, s) in route.steps.iter().enumerate() {
        let z = s.zone.clone().unwrap_or_default();
        if full || z != zone || matches!(s.kind, StepKind::Fly | StepKind::Grind) {
            let along = if s.bg { "  (along the way)" } else { "" };
            println!(
                "{:>4} {:>6} L{:<2} {:<20} {}{along}",
                i + 1,
                t(s.time),
                s.level,
                z,
                s.text
            );
        }
        zone = z;
    }
    Ok(())
}

/// What `fg share` plans: a character from a level to another, the app's settings otherwise.
pub struct ShareOptions {
    pub race: String,
    pub class: String,
    pub from_level: i64,
    pub to_level: i64,
    pub effort: Option<f64>,
    pub name: String,
    /// Planner parameter overrides (key=value).
    pub params: Vec<String>,
    pub locale: String,
}

/// Plan a guide the way the app's "New guide" page does (the settings it starts from, the class
/// quests it lists for the class, no professions, no character, a walking memory saved with the
/// guide) and write it as the .fgguide file the apps share: one version with its configuration
/// and computed guide (its plan too, for the guide editor), which either app imports.
pub fn share(conn: &Connection, overrides_dir: &Path, opts: &ShareOptions, out: &Path) -> Result<()> {
    let overrides = Overrides::load_edition(overrides_dir, crate::edition())?;
    let mut params = overrides.params.with_overrides(&opts.params)?;
    if let Some(e) = opts.effort {
        params.effort = e;
    }
    // The class quests the page lists for the class (by its name in lower case).
    let options = job::options(conn, &overrides)?;
    let class_name = options
        .classes
        .iter()
        .find(|c| c.key == opts.class)
        .map_or_else(|| opts.class.to_lowercase(), |c| c.name.to_lowercase());
    let request = PlanRequest {
        race: opts.race.clone(),
        class: opts.class.clone(),
        from_level: opts.from_level,
        to_level: opts.to_level,
        name: Some(opts.name.clone()),
        params: Some(params),
        required_class_quests: Some(options.class_quests.get(&class_name).cloned().unwrap_or_default()),
        required_quests: vec![],
        professions: vec![],
        locale: Some(opts.locale.clone()),
        mates: vec![],
        start: None,
    };
    let walks = std::sync::Arc::new(fg_route::terrain::Walks::default());
    let outcome = job::plan_interruptible(
        conn,
        &overrides,
        &request,
        &|m| {
            if !m.starts_with('@') {
                eprintln!("{m}");
            }
        },
        &job::Interrupt::default(),
        Some(&walks),
    )?;
    for note in &outcome.notes {
        eprintln!("  {note}");
    }
    let route = &outcome.route;
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    // The version's summary as the apps write it (they keep its data version).
    let data: Option<String> = conn
        .query_row("SELECT detail FROM import_run WHERE source = 'questie'", [], |r| {
            r.get(0)
        })
        .ok()
        .flatten();
    let meta = serde_json::json!({
        "guide": job::slug(&route.name),
        "version": 1,
        "name": route.name,
        "created": created,
        "race": request.race,
        "class": request.class,
        "group": request.mates.iter().map(|m| m.class.to_lowercase()).collect::<Vec<_>>(),
        "from_level": route.from_level,
        "to_level": route.to_level,
        "total_time": route.total_time,
        "total_time_text": route.total_time_text,
        "quests": route.quests,
        "data": data,
        "imported": false,
        "edited": false,
        "character": null,
    });
    // The apps' share format (`SharedGuide` in app/src-tauri and crates/fg-web).
    let shared = serde_json::json!({
        "format": "factoruide-guide",
        "version": 2,
        "meta": meta,
        "request": request,
        "outcome": outcome,
    });
    std::fs::write(out, serde_json::to_string(&shared)?).with_context(|| format!("writing {}", out.display()))?;
    eprintln!(
        "wrote {} ({}: level {} -> {} in {}, {} quests, {} bytes)",
        out.display(),
        route.name,
        route.from_level,
        route.to_level,
        route.total_time_text,
        route.quests,
        std::fs::metadata(out)?.len()
    );
    Ok(())
}

/// Replay a request exported by the app (debug/request-*.json).
pub fn replay(
    conn: &Connection,
    overrides_dir: &Path,
    file: &Path,
    effort: Option<f64>,
    routes_dir: &Path,
) -> Result<()> {
    let mut request: PlanRequest =
        serde_json::from_str(&std::fs::read_to_string(file).with_context(|| format!("reading {}", file.display()))?)?;
    let overrides = Overrides::load_edition(overrides_dir, crate::edition())?;
    if let Some(e) = effort {
        request.params.get_or_insert_with(|| overrides.params.clone()).effort = e;
    }
    request.name = Some(format!(
        "{}-replay",
        request.name.clone().unwrap_or_else(|| "request".into())
    ));
    if std::env::var_os("FG_PREVIEW_STARTS").is_some() {
        let preview = job::preview_starts(
            conn,
            &overrides,
            &request,
            &|m| eprintln!("{m}"),
            &job::Interrupt::default(),
            None,
        )?;
        eprintln!("{} starts of {} tried", preview.options.len(), preview.tried);
        return Ok(());
    }
    let outcome = job::plan(conn, &overrides, &request, &|m| {
        if !m.starts_with('@') {
            eprintln!("{m}");
        }
    })?;
    for note in &outcome.notes {
        eprintln!("  {note}");
    }
    job::save_route(routes_dir, &outcome.route)?;
    eprintln!("saved {} (fg show {} --full)", outcome.route.name, outcome.route.name);
    Ok(())
}
