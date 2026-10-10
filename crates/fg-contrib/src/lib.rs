//! What Factoruide collects from players' game clients: the client's quest cache and the
//! addon's recordings, read locally and sent as a JSON contribution to consolidate the quest
//! database. Shared by the app (sends) and the data tools (import).

pub mod collector;
pub mod contribution;
#[cfg(feature = "local")]
pub mod lua;
#[cfg(feature = "local")]
pub mod profile;
pub mod receipt;
pub mod validate;
pub mod wdb;
