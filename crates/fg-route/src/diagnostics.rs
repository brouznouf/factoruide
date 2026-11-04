//! What a developer asks a planning job to tell or check (the `FG_*` variables). The program
//! reads them once where it starts (`from_env`) and hands them to the job; by default nothing.

/// Traces and checks of a planning job, all off by default.
#[derive(Debug, Clone, Default)]
pub struct Diagnostics {
    /// `FG_VERIFY`: each incremental evaluation of the search checked against a full replay
    /// (slow: panics when they differ).
    pub verify: bool,
    /// `FG_DEBUG`: how the construction went (stalls, settling, unused quests).
    pub debug: bool,
    /// `FG_DEBUG_STALL`: why the construction waited for a level.
    pub stall: bool,
    /// `FG_DEBUG_PREREQ`: the prerequisites of the high quests left out.
    pub prereq: bool,
    /// `FG_DEBUG_ROOT`: the first quest not done up the chain of a quest left out.
    pub root: bool,
    /// `FG_DEBUG_QUESTS=<ids>`: why these quests are left out of a region's construction.
    pub quests: Vec<i64>,
    /// `FG_LIST_SKIPPED=<reason>`: the quests the model leaves out for that reason.
    pub skipped_for: Option<String>,
    /// `FG_DEBUG_SKIP=<ids>`: why the model leaves these quests out.
    pub skipped: Vec<i64>,
    /// `FG_RACE_LOG`: the race of the candidates, score by score.
    pub race: bool,
}

/// Nothing told, nothing checked.
pub static OFF: Diagnostics = Diagnostics {
    verify: false,
    debug: false,
    stall: false,
    prereq: false,
    root: false,
    quests: Vec::new(),
    skipped_for: None,
    skipped: Vec::new(),
    race: false,
};

impl Diagnostics {
    /// The `FG_*` variables of this process.
    pub fn from_env() -> Self {
        Self::from_vars(|name| std::env::var(name).ok())
    }

    /// The diagnostics `var` gives (a variable's value, None when not set).
    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Self {
        let on = |name| var(name).is_some();
        let ids = |name| {
            var(name)
                .map(|ids| ids.split(',').filter_map(|id| id.trim().parse().ok()).collect())
                .unwrap_or_default()
        };
        Self {
            verify: on("FG_VERIFY"),
            debug: on("FG_DEBUG"),
            stall: on("FG_DEBUG_STALL"),
            prereq: on("FG_DEBUG_PREREQ"),
            root: on("FG_DEBUG_ROOT"),
            quests: ids("FG_DEBUG_QUESTS"),
            skipped_for: var("FG_LIST_SKIPPED"),
            skipped: ids("FG_DEBUG_SKIP"),
            race: on("FG_RACE_LOG"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Diagnostics;

    #[test]
    fn the_variables_set_are_the_diagnostics_asked() {
        let d = Diagnostics::from_vars(|name| match name {
            "FG_DEBUG" => Some(String::new()),
            "FG_DEBUG_QUESTS" => Some("12, 34,x".to_owned()),
            "FG_LIST_SKIPPED" => Some("dungeon".to_owned()),
            _ => None,
        });
        assert!(d.debug && !d.verify && !d.race);
        assert_eq!(d.quests, [12, 34]);
        assert_eq!(d.skipped_for.as_deref(), Some("dungeon"));
        assert_eq!(d.skipped, Vec::<i64>::new());
    }
}
