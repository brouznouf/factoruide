//! What the desktop app and the browser one both do when the page calls them: plan, keep and
//! edit guides. They differ in where the data is (a folder and database files, or the browser's
//! storage and downloads), which they give through [`Host`]; the rest is the [`Studio`].

#![warn(clippy::missing_errors_doc, clippy::missing_panics_doc)]

mod error;
mod host;
mod studio;

pub use error::CmdError;
pub use host::Host;
pub use studio::{EditOpened, IssueDraft, IssueGuide, Studio, library, now, request_edition};
