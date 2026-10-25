//! The planner's integration tests, one binary (one link, one unpacking of the databases).
//! `cargo test --profile fast -p fg-route --test it <name>`.

#![expect(clippy::unwrap_used, reason = "fixtures that cannot be read fail the test")]
mod addon;
mod common;
mod draft;
mod edit;
mod prerequisites;
mod required;
mod routes;
mod timeline;
