//! What the desktop app and the browser one both do when the page calls them: plan, keep and
//! edit guides. They differ in where the data is (a folder and database files, or the browser's
//! storage and downloads), which they give through [`Host`]; the rest is the [`Studio`].

#![warn(clippy::missing_errors_doc, clippy::missing_panics_doc)]

mod characters;
mod error;
mod host;
mod maps;
mod settings;
mod studio;

pub use characters::{EditionInfo, editions, older, profession_ranks, race_key, rested_now};
pub use error::CmdError;
pub use host::{Host, embedded_overrides};
pub use maps::map_index;
pub use settings::{GuideAddon, Settings, SettingsStore};
pub use studio::{EditOpened, IssueDraft, IssueGuide, Studio, library, now, request_edition};
