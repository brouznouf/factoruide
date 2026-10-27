use super::{Cancelled, GROUP_MAX, GROUP_MIN_LEVEL};

/// A request the planner cannot plan from: what the player asked for does not hold together.
#[derive(Debug, thiserror::Error)]
pub enum RequestError {
    #[error("unknown race {0}")]
    UnknownRace(String),
    #[error("{race} cannot be {class} (playable: {})", .playable.join(", "))]
    ClassNotPlayable {
        race: String,
        class: String,
        playable: Vec<String>,
    },
    #[error("the character is level {from}: nothing to plan up to level {to}")]
    NothingToPlan { from: i64, to: i64 },
    #[error("a group guide starts from the characters as the addon recorded them")]
    GroupWithoutProfiles,
    #[error("a group has {GROUP_MAX} players at most")]
    GroupTooBig,
    #[error(
        "{name} is level {level}: below level {GROUP_MIN_LEVEL}, the players of a group guide must all start in the same zone"
    )]
    GroupMemberTooLow { name: String, level: i64 },
    #[error("{0} is twice in the group")]
    GroupDuplicate(String),
    #[error("the players of a group guide are all of the same faction")]
    GroupMixedFactions,
    #[error("the dungeons' levels (from {from:+} to {to:+} around each) leave none for {dungeon}")]
    NoDungeonLevels { dungeon: String, from: i64, to: i64 },
}

/// Why a planning job gave no guide.
#[derive(Debug, thiserror::Error)]
pub enum JobError {
    /// The player gave the job up (`Interrupt::cancel`).
    #[error("cancelled")]
    Cancelled,
    #[error(transparent)]
    Request(#[from] RequestError),
    /// The settings leave no route that works; the reason, when the planner found one.
    #[error("no valid route found ({0})")]
    NoRoute(String),
    /// The game data or the planner failed.
    #[error("{0:#}")]
    Other(anyhow::Error),
}

impl From<anyhow::Error> for JobError {
    fn from(e: anyhow::Error) -> Self {
        if e.is::<Cancelled>() {
            return Self::Cancelled;
        }
        match e.downcast::<RequestError>() {
            Ok(request) => Self::Request(request),
            Err(e) => match e.downcast::<Self>() {
                Ok(job) => job,
                Err(e) => Self::Other(e),
            },
        }
    }
}

impl From<rusqlite::Error> for JobError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Other(e.into())
    }
}

impl From<serde_json::Error> for JobError {
    fn from(e: serde_json::Error) -> Self {
        Self::Other(e.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What came up as a plain error is told apart again where the job ends.
    #[test]
    fn errors_that_went_through_anyhow_are_told_apart() {
        let cancelled: anyhow::Error = Cancelled.into();
        assert!(matches!(JobError::from(cancelled), JobError::Cancelled));
        let request: anyhow::Error = RequestError::UnknownRace("gnoll".into()).into();
        let JobError::Request(RequestError::UnknownRace(race)) = JobError::from(request) else {
            panic!("a request error");
        };
        assert_eq!(race, "gnoll");
        let nested: anyhow::Error = JobError::NoRoute("stop 3".into()).into();
        assert!(matches!(JobError::from(nested), JobError::NoRoute(why) if why == "stop 3"));
        let other = JobError::from(anyhow::anyhow!("the database is gone"));
        assert_eq!(other.to_string(), "the database is gone");
    }

    #[test]
    fn requests_the_planner_cannot_follow_say_why() {
        let e = RequestError::ClassNotPlayable {
            race: "gnome".into(),
            class: "druid".into(),
            playable: vec!["mage".into(), "warlock".into()],
        };
        assert_eq!(e.to_string(), "gnome cannot be druid (playable: mage, warlock)");
        assert_eq!(
            RequestError::NothingToPlan { from: 60, to: 60 }.to_string(),
            "the character is level 60: nothing to plan up to level 60"
        );
    }
}
