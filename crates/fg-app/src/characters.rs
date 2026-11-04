//! What both apps work out of a character's profile and of the game versions.

use crate::settings::Settings;
use fg_route::job::{RaceDef, StartState};
use fg_route::xp::Edition;
use serde::Serialize;
use std::collections::BTreeMap;

/// Rested XP gained while logged out since the profile was saved: 5% of a level every 8 hours
/// in an inn or a city, a quarter of it elsewhere, up to a level and a half.
pub fn rested_now(p: &StartState, now: i64) -> i64 {
    if p.xp_max <= 0 || p.time <= 0 {
        return p.rested;
    }
    let hours = (now - p.time).max(0) as f64 / 3600.0;
    let per_hour = 0.05 / 8.0 * p.xp_max as f64 * if p.resting { 1.0 } else { 0.25 };
    let cap = p.xp_max * 3 / 2;
    (p.rested + (hours * per_hour) as i64).min(cap.max(p.rested))
}

/// The key of a profile's race among the races of the corrections (`Overrides::races`).
pub fn race_key(races: &BTreeMap<String, RaceDef>, profile: &StartState) -> Option<String> {
    races
        .iter()
        .find(|(_, r)| r.id == profile.race)
        .map(|(key, _)| key.clone())
}

/// A profile's professions the planner knows, by key, with their rank.
pub fn profession_ranks(profile: &StartState) -> Vec<(String, i64)> {
    (profile.professions.iter())
        .filter_map(|s| {
            let def = fg_route::profession::PROFESSIONS
                .iter()
                .find(|d| d.skill_id == s.line)?;
            Some((def.key.to_owned(), s.rank))
        })
        .collect()
}

/// Key of a character in `Settings::forgotten_characters`.
fn forgotten_key(edition: Edition, character: &str) -> String {
    format!("{}/{character}", edition.key())
}

impl Settings {
    /// Take a character of `edition` out of the Characters page, as its profile was at `time`.
    pub fn forget(&mut self, edition: Edition, character: &str, time: i64) {
        self.forgotten_characters
            .insert(forgotten_key(edition, character), time);
    }

    /// Put a character back on the Characters page; whether it was out.
    pub fn remember(&mut self, edition: Edition, character: &str) -> bool {
        self.forgotten_characters
            .remove(&forgotten_key(edition, character))
            .is_some()
    }

    /// Whether a character's profile was taken out of the list (and not played since).
    pub fn is_forgotten(&self, edition: Edition, profile: &StartState) -> bool {
        self.forgotten_characters
            .get(&forgotten_key(edition, &profile.name))
            .is_some_and(|&time| profile.time <= time)
    }

    /// Whether a character is out of the list, whatever its profile.
    pub fn is_out(&self, edition: Edition, character: &str) -> bool {
        self.forgotten_characters
            .contains_key(&forgotten_key(edition, character))
    }
}

/// Whether version `a` ("0.13.0") is older than `b`; versions that do not read as numbers are
/// never older (an addon built by hand).
pub fn older(a: &str, b: &str) -> bool {
    let parse = |v: &str| {
        v.split('.')
            .map(|p| p.trim().parse::<u32>().ok())
            .collect::<Option<Vec<_>>>()
    };
    match (parse(a), parse(b)) {
        (Some(a), Some(b)) => a < b,
        _ => false,
    }
}

/// A game version as the page lists it.
#[derive(Debug, Serialize)]
pub struct EditionInfo {
    pub key: &'static str,
    pub name: &'static str,
    /// Its data is in this build.
    pub available: bool,
    /// Its game client is installed (the browser cannot tell: every one).
    pub installed: bool,
    pub max_level: i64,
}

/// Every game version, `has_data` telling whether its data is in this build and `installed`
/// whether its game is there.
pub fn editions(has_data: impl Fn(Edition) -> bool, installed: impl Fn(Edition) -> bool) -> Vec<EditionInfo> {
    (Edition::ALL.iter().copied())
        .map(|e| EditionInfo {
            key: e.key(),
            name: e.rules().edition_name,
            available: has_data(e) && e.rules().known(),
            installed: installed(e),
            max_level: e.rules().max_level,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Settings, older};
    use fg_route::job::StartState;
    use fg_route::xp::Edition;

    #[test]
    fn a_character_taken_out_is_back_once_played_again_or_put_back() {
        let profile = |time| StartState {
            name: "Finnagnix-ClassicBetaPvP".into(),
            time,
            ..StartState::default()
        };
        let mut s = Settings::default();
        s.forget(Edition::Forever, "Finnagnix-ClassicBetaPvP", 100);
        assert!(s.is_forgotten(Edition::Forever, &profile(100)));
        assert!(!s.is_forgotten(Edition::Forever, &profile(101)), "played since");
        assert!(!s.is_forgotten(Edition::Classic, &profile(100)), "another game version");
        assert!(s.remember(Edition::Forever, "Finnagnix-ClassicBetaPvP"));
        assert!(!s.is_forgotten(Edition::Forever, &profile(100)));
    }

    #[test]
    fn versions_compare_by_their_numbers() {
        assert!(older("0.9.1", "0.13.0") && older("0.13.0", "0.14.0") && older("1.2", "1.2.1"));
        assert!(!older("0.14.0", "0.14.0") && !older("0.15.0", "0.14.0") && !older("dev", "0.14.0"));
    }
}
