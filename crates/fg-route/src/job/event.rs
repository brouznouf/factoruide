//! What a planning job tells whoever watches it (it may tell it from several threads).

use super::starts::{StartCandidate, StartOption};
use std::fmt;

/// One thing a job tells.
#[derive(Debug, Clone, Copy)]
pub enum JobEvent<'a> {
    /// A line for a log: what it is doing, a warning, a figure.
    Status(&'a str),
    /// Where the generation is, for the app's progress bar: its phase and the figures of it.
    Progress(&'a serde_json::Value),
    /// The best route so far, as points (continent, x, y) to draw.
    Route(&'a [[f64; 3]]),
    /// The starts about to be tried (`preview_starts`).
    Starts(&'a [StartCandidate]),
    /// One start computed (`preview_starts`).
    Start(&'a StartOption),
}

/// Who watches a job.
pub type Watch<'a> = &'a (dyn Fn(&JobEvent<'_>) + Sync);

impl JobEvent<'_> {
    /// The line of a `Status`, None for what the app shows otherwise.
    pub fn status(&self) -> Option<&str> {
        match self {
            Self::Status(line) => Some(line),
            _ => None,
        }
    }
}

/// The line the app reads: a status as it is, the others `@<kind> <JSON>` (`@progress`, `@route`,
/// `@starts`, `@start`).
impl fmt::Display for JobEvent<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Status(line) => f.write_str(line),
            Self::Progress(value) => write!(f, "@progress {value}"),
            Self::Route(points) => write!(f, "@route {}", serde_json::json!({ "points": points })),
            Self::Starts(candidates) => write!(f, "@starts {}", json(candidates)?),
            Self::Start(option) => write!(f, "@start {}", json(option)?),
        }
    }
}

/// A value as JSON, in a line.
fn json<T: serde::Serialize + ?Sized>(value: &T) -> Result<String, fmt::Error> {
    serde_json::to_string(value).map_err(|_| fmt::Error)
}

#[cfg(test)]
mod tests {
    use super::JobEvent;
    use crate::job::StartCandidate;

    /// The lines the app and the CLI read are those of the text protocol before the events.
    #[test]
    fn each_event_is_the_line_the_app_reads() {
        assert_eq!(JobEvent::Status("Loading quests").to_string(), "Loading quests");
        let phase = serde_json::json!({ "phase": "prepare", "step": "world" });
        assert_eq!(
            JobEvent::Progress(&phase).to_string(),
            r#"@progress {"phase":"prepare","step":"world"}"#
        );
        assert_eq!(
            JobEvent::Route(&[[1.0, 2.5, 3.0]]).to_string(),
            r#"@route {"points":[[1.0,2.5,3.0]]}"#
        );
        let starts = [StartCandidate {
            start_zone: 14,
            ui_map: 1411,
            quests: 30,
        }];
        assert_eq!(
            JobEvent::Starts(&starts).to_string(),
            format!("@starts {}", serde_json::to_string(&starts).unwrap())
        );
        assert_eq!(JobEvent::Progress(&phase).status(), None);
        assert_eq!(JobEvent::Status("x").status(), Some("x"));
    }
}
