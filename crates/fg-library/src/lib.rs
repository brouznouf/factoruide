//! The guides a player keeps: a name and its versions, every computation (the first one, or
//! another with the same name) adding one. The guides go to the game all together (the data
//! addon) or one by one (the text of `/fg import`), each in the version its page says to
//! follow; the player picks a character's guide in game.
//!
//! The library knows the layout and the rules, not where the files are (see [`Files`]):
//!
//! ```text
//! guides/<edition>/<slug>/guide.json          name, version to follow
//! guides/<edition>/<slug>/v<N>/meta.json      summary of the version
//! guides/<edition>/<slug>/v<N>/request.json   configuration
//! guides/<edition>/<slug>/v<N>/outcome.json   computed guide
//! ```

#![warn(clippy::missing_errors_doc, clippy::missing_panics_doc)]

mod error;
mod files;
mod id;
mod share;
mod types;

pub use error::LibraryError;
pub use files::{DiskFiles, Files};
pub use id::GuideId;
pub use share::SharedGuide;
pub use types::{Comparison, GuideInfo, GuideSummary, GuideVersion, Origin, VersionMeta};

use fg_route::Route;
use fg_route::export::diff;
use fg_route::job::{PlanOutcome, PlanRequest};
use fg_route::terrain::WalkMemory;
use fg_route::xp::Edition;
use serde::{Deserialize, Serialize};

type Result<T> = std::result::Result<T, LibraryError>;

/// The guides of one edition, over some files.
pub struct Library<'a, F: Files> {
    files: &'a F,
    dir: String,
}

impl<'a, F: Files> Library<'a, F> {
    pub fn new(files: &'a F, edition: Edition) -> Self {
        Self {
            files,
            dir: format!("guides/{}", edition.key()),
        }
    }

    fn read_json<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T> {
        let bytes = self.files.read(path).map_err(|e| LibraryError::io(path, e))?;
        serde_json::from_slice(&bytes).map_err(|e| LibraryError::corrupt(path, e))
    }

    fn write_json<T: Serialize>(&self, path: &str, value: &T, pretty: bool) -> Result<()> {
        let bytes = if pretty {
            serde_json::to_vec_pretty(value)
        } else {
            serde_json::to_vec(value)
        }
        .map_err(|e| LibraryError::corrupt(path, e))?;
        self.files.write(path, &bytes).map_err(|e| LibraryError::io(path, e))
    }

    /// The identifiers of the guides.
    pub fn ids(&self) -> Vec<GuideId> {
        self.files
            .list(&self.dir)
            .into_iter()
            .filter_map(|name| GuideId::try_from(name).ok())
            .collect()
    }

    /// The guide's name and the version its characters follow.
    ///
    /// # Errors
    ///
    /// [`LibraryError`] when a file cannot be read or written, or is not what the library saved.
    pub fn info(&self, id: &GuideId) -> Result<GuideInfo> {
        self.read_json(&format!("{}/{id}/guide.json", self.dir))
    }

    fn write_info(&self, id: &GuideId, info: &GuideInfo) -> Result<()> {
        self.write_json(&format!("{}/{id}/guide.json", self.dir), info, true)
    }

    /// The versions of a guide, newest first.
    pub fn versions(&self, id: &GuideId) -> Vec<VersionMeta> {
        let dir = format!("{}/{id}", self.dir);
        let mut out: Vec<VersionMeta> = self
            .files
            .list(&dir)
            .into_iter()
            .filter(|n| n.starts_with('v'))
            .filter_map(|v| self.read_json(&format!("{dir}/{v}/meta.json")).ok())
            .collect();
        out.sort_by_key(|v| std::cmp::Reverse(v.version));
        out
    }

    /// A version of a guide, as saved.
    ///
    /// # Errors
    ///
    /// [`LibraryError`] when a file cannot be read or written, or is not what the library saved.
    pub fn version(&self, id: &GuideId, version: u32) -> Result<GuideVersion> {
        let folder = format!("{}/{id}/v{version}", self.dir);
        let outcome = format!("{folder}/outcome.json");
        let bytes = self.files.read(&outcome).map_err(|e| LibraryError::io(&outcome, e))?;
        Ok(GuideVersion {
            meta: self.read_json(&format!("{folder}/meta.json"))?,
            request: self.read_json(&format!("{folder}/request.json"))?,
            outcome: PlanOutcome::from_json(&String::from_utf8_lossy(&bytes)).map_err(LibraryError::Outcome)?,
        })
    }

    /// A version for the page: without the walking memory and the plan, which stay in the files
    /// (the app shows neither).
    ///
    /// # Errors
    ///
    /// [`LibraryError`] when a file cannot be read or written, or is not what the library saved.
    pub fn version_to_show(&self, id: &GuideId, version: u32) -> Result<GuideVersion> {
        let mut v = self.version(id, version)?;
        v.outcome.walks = None;
        v.outcome.plan.clear();
        Ok(v)
    }

    /// The guide, with its versions.
    ///
    /// # Errors
    ///
    /// [`LibraryError`] when a file cannot be read or written, or is not what the library saved.
    pub fn summary(&self, id: &GuideId) -> Result<GuideSummary> {
        let info = self.info(id)?;
        Ok(GuideSummary {
            id: id.to_string(),
            name: info.name,
            installed: info.installed,
            versions: self.versions(id),
        })
    }

    /// Every guide, most recently computed first.
    pub fn list(&self) -> Vec<GuideSummary> {
        let mut out: Vec<GuideSummary> = self
            .ids()
            .iter()
            .filter_map(|id| self.summary(id).ok())
            .filter(|g| !g.versions.is_empty())
            .collect();
        out.sort_by(|a, b| b.versions[0].created.cmp(&a.versions[0].created));
        out
    }

    /// Adds a version to the guide of this name (created when new): the one its characters follow
    /// from now on, even when an older one was chosen.
    ///
    /// # Errors
    ///
    /// [`LibraryError`] when a file cannot be read or written, or is not what the library saved.
    pub fn add_version(
        &self,
        request: &PlanRequest,
        outcome: &PlanOutcome,
        data: Option<String>,
        origin: Origin,
        created: u64,
    ) -> Result<VersionMeta> {
        let route = &outcome.route;
        let id = GuideId::from_name(&route.name);
        // The player made this version: the game gets it, even over an older one chosen before.
        self.write_info(
            &id,
            &GuideInfo {
                name: route.name.clone(),
                installed: None,
            },
        )?;
        let version = self.versions(&id).first().map_or(1, |v| v.version + 1);
        let meta = VersionMeta {
            guide: id.to_string(),
            version,
            name: route.name.clone(),
            created,
            race: request.race.to_string(),
            class: request.class.to_string(),
            group: request.mates.iter().map(|m| m.class.to_lowercase()).collect(),
            from_level: route.from_level,
            to_level: route.to_level,
            total_time: route.total_time,
            total_time_text: route.total_time_text.clone(),
            quests: route.quests,
            data,
            imported: origin.imported,
            edited: origin.edited,
            character: request.start.as_ref().map(|s| s.name.clone()),
        };
        let folder = format!("{}/{id}/v{version}", self.dir);
        self.write_json(&format!("{folder}/meta.json"), &meta, true)?;
        self.write_json(&format!("{folder}/request.json"), request, true)?;
        self.write_json(&format!("{folder}/outcome.json"), outcome, false)?;
        Ok(meta)
    }

    /// The version of a guide its characters follow: the one chosen, else the latest.
    pub fn followed_version(&self, id: &GuideId) -> Option<u32> {
        let chosen = self.info(id).ok()?.installed;
        chosen.or_else(|| self.versions(id).first().map(|v| v.version))
    }

    /// Every guide in the version its page says to follow (the latest unless one is chosen),
    /// as saved, with when that version was computed and its number.
    pub fn followed(&self) -> Vec<(Route, u64, u32)> {
        self.ids()
            .iter()
            .filter_map(|id| {
                let n = self.followed_version(id)?;
                let v = self.version(id, n).ok()?;
                Some((v.outcome.route, v.meta.created, n))
            })
            .collect()
    }

    /// The version of a guide its characters follow (`None`: the latest).
    ///
    /// # Errors
    ///
    /// [`LibraryError`] when a file cannot be read or written, or is not what the library saved.
    pub fn set_installed(&self, id: &GuideId, version: Option<u32>) -> Result<()> {
        let info = self.info(id)?;
        self.write_info(
            id,
            &GuideInfo {
                installed: version,
                ..info
            },
        )
    }

    /// Removes a version; the guide too when it was the last.
    ///
    /// # Errors
    ///
    /// [`LibraryError`] when a file cannot be read or written, or is not what the library saved.
    pub fn delete_version(&self, id: &GuideId, version: u32) -> Result<()> {
        let folder = format!("{}/{id}/v{version}", self.dir);
        self.files
            .remove_dir(&folder)
            .map_err(|e| LibraryError::io(&folder, e))?;
        let info = self.info(id)?;
        if self.versions(id).is_empty() {
            return self.delete_guide(id);
        }
        if info.installed == Some(version) {
            self.set_installed(id, None)?;
        }
        Ok(())
    }

    /// Removes a guide with all its versions.
    ///
    /// # Errors
    ///
    /// [`LibraryError`] when a file cannot be read or written, or is not what the library saved.
    pub fn delete_guide(&self, id: &GuideId) -> Result<()> {
        let folder = format!("{}/{id}", self.dir);
        self.files.remove_dir(&folder).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => LibraryError::NoGuide(id.to_string()),
            _ => LibraryError::io(&folder, e),
        })
    }

    /// The walking memories saved with the versions (each outcome holds the paths it searched).
    pub fn walk_memories(&self) -> Vec<WalkMemory> {
        /// Only the memory of a saved outcome.
        #[derive(Deserialize)]
        struct Saved {
            #[serde(default)]
            walks: Option<WalkMemory>,
        }
        let mut out = Vec::new();
        for guide in self.ids() {
            for v in self.files.list(&format!("{}/{guide}", self.dir)) {
                let path = format!("{}/{guide}/{v}/outcome.json", self.dir);
                let saved = self
                    .files
                    .read(&path)
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Saved>(&b).ok());
                out.extend(saved.and_then(|s| s.walks));
            }
        }
        out
    }

    /// Versions `a` and `b` of a guide compared as saved (zones by their English names).
    ///
    /// # Errors
    ///
    /// [`LibraryError`] when a file cannot be read or written, or is not what the library saved.
    pub fn compare(&self, id: &GuideId, a: u32, b: u32) -> Result<Comparison> {
        let (a, b) = (self.version_to_show(id, a)?, self.version_to_show(id, b)?);
        let settings = |v: &GuideVersion| -> Result<serde_json::Value> {
            let mut value = serde_json::to_value(&v.request).map_err(|e| LibraryError::corrupt("request", e))?;
            if let serde_json::Value::Object(map) = &mut value {
                map.insert("data".into(), v.meta.data.clone().into());
            }
            Ok(value)
        };
        let settings = diff::value_changes(&settings(&a)?, &settings(&b)?);
        let d = diff::diff(&a.outcome.route, &b.outcome.route);
        let labels = [format!("v{}", a.meta.version), format!("v{}", b.meta.version)];
        let text = diff::to_text(
            &d,
            &a.outcome.route,
            &b.outcome.route,
            [&labels[0], &labels[1]],
            &settings,
        );
        Ok(Comparison {
            a,
            b,
            diff: d,
            settings,
            text,
        })
    }

    /// A version to hand to someone else.
    ///
    /// # Errors
    ///
    /// [`LibraryError`] when a file cannot be read or written, or is not what the library saved.
    pub fn share(&self, id: &GuideId, version: u32) -> Result<SharedGuide> {
        SharedGuide::of(self.version(id, version)?)
    }
}

/// For the commands of the apps, whose errors are texts for the page.
impl From<LibraryError> for String {
    fn from(e: LibraryError) -> Self {
        e.to_string()
    }
}
