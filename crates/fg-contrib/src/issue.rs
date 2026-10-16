//! A problem a player reports from the app, sent to the contribution server (`/v1/issues`):
//! about a guide (its steps the player points at, and everything the guide was computed from,
//! so that it can be computed again exactly) or about anything else.
//!
//! Unlike contributions, a report holds text typed by the player (the message) and, if they
//! want to be thanked, a name. Nothing else about them: no character name (the guide's character is cleared), no time of day.

use crate::validate::{CLOCK_SKEW, EDITIONS, MIN_TIME, clean_text, is_hex};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub const FORMAT: &str = "factoruide-issue";
pub const VERSION: u32 = 1;
/// Folder of the reports in the server's storage, next to the editions' folders.
pub const DIR: &str = "issues";
/// Largest report accepted (a whole guide's request and plan fit in a few hundred KB).
pub const MAX_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_MESSAGE: usize = 4_000;
pub const MAX_STEPS: usize = 50;
pub const MAX_NAME: usize = 40;
const MAX_STEP_TEXT: usize = 500;
/// Free JSON (the guide's request and plan): nesting, values, and their texts.
const MAX_DEPTH: usize = 12;
const MAX_NODES: usize = 300_000;
const MAX_FREE_TEXT: usize = 1_000;
pub const KINDS: &[&str] = &["guide", "other"];
pub const SYSTEMS: &[&str] = &["windows", "linux", "macos"];
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Issue {
    pub format: String,
    pub version: u32,
    pub edition: String,
    pub app_version: String,
    /// The app install's random id (`Contribution::contributor`).
    pub contributor: String,
    /// Day the report was made (days since 1970-01-01).
    pub day: u32,
    /// Operating system the app runs on.
    pub os: String,
    /// The app's language (`fr`, `en`, `zhCN`...).
    pub language: String,
    /// `guide` or `other`.
    pub kind: String,
    pub message: String,
    /// Who to thank, as the player wants to be named.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guide: Option<GuideReport>,
}

/// The guide a report is about: what it was computed from and the steps pointed at.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuideReport {
    /// The planning request (`fg_route::job::PlanRequest`), every setting filled in, seed
    /// included: with the same app and data versions, `fg replay` computes the same guide.
    pub request: serde_json::Value,
    /// The route the guide was written from (`fg_route::job::RouteStep`s): the guide itself
    /// even when it was stopped early or edited by hand.
    pub plan: serde_json::Value,
    /// Version of the quest data it was computed with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    pub version: u32,
    pub edited: bool,
    pub imported: bool,
    pub total_time: f64,
    /// Steps of the guide the report is about (none: the whole guide).
    pub steps: Vec<ReportedStep>,
}

/// A step of the guide as the player saw it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportedStep {
    /// Its place in the guide (from 0).
    pub index: u32,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quest: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective: Option<u32>,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,
    pub level: u32,
    /// UiMap ID and position in it (0-100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map: Option<(u32, f64, f64)>,
}

/// Free JSON within bounds: nesting, number of values, plain keys and texts.
fn check_free(v: &serde_json::Value, depth: usize, nodes: &mut usize) -> Result<()> {
    *nodes += 1;
    ensure!(depth <= MAX_DEPTH && *nodes <= MAX_NODES, "guide too large");
    match v {
        serde_json::Value::String(s) => ensure!(clean_text(s, MAX_FREE_TEXT, false), "bad text in the guide"),
        serde_json::Value::Number(n) => ensure!(n.as_f64().is_some_and(f64::is_finite), "bad number in the guide"),
        serde_json::Value::Array(list) => {
            for v in list {
                check_free(v, depth + 1, nodes)?;
            }
        }
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                ensure!(
                    !k.is_empty()
                        && k.len() <= 64
                        && k.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
                    "bad key in the guide"
                );
                check_free(v, depth + 1, nodes)?;
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) => {}
    }
    Ok(())
}

fn check_guide(g: &GuideReport) -> Result<()> {
    ensure!(g.request.is_object(), "bad guide request");
    ensure!(g.plan.is_array(), "bad guide plan");
    let mut nodes = 0;
    check_free(&g.request, 0, &mut nodes)?;
    check_free(&g.plan, 0, &mut nodes)?;
    if let Some(d) = &g.data {
        ensure!(clean_text(d, 300, false), "bad data version");
    }
    ensure!((1..=100_000).contains(&g.version), "bad guide version");
    ensure!(
        g.total_time.is_finite() && (0.0..=1e8).contains(&g.total_time),
        "bad guide time"
    );
    ensure!(g.steps.len() <= MAX_STEPS, "too many steps");
    let mut indices = Vec::new();
    for s in &g.steps {
        ensure!(s.index <= 100_000 && !indices.contains(&s.index), "bad step index");
        indices.push(s.index);
        ensure!(
            !s.kind.is_empty() && s.kind.len() <= 32 && s.kind.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
            "bad step kind"
        );
        ensure!(s.quest.is_none_or(|q| (1..=1_000_000).contains(&q)), "bad step quest");
        ensure!(s.objective.is_none_or(|o| o < 32), "bad step objective");
        ensure!(clean_text(&s.text, MAX_STEP_TEXT, false), "bad step text");
        ensure!(
            s.zone.as_deref().is_none_or(|z| clean_text(z, 100, false)),
            "bad step zone"
        );
        ensure!(s.level <= 100, "bad step level");
        ensure!(
            s.map
                .is_none_or(|(m, x, y)| m < 100_000 && (0.0..=100.0).contains(&x) && (0.0..=100.0).contains(&y)),
            "bad step position"
        );
    }
    Ok(())
}

/// Check every field of a report (`now`: Unix seconds). Messages only quote our own words.
pub fn validate(i: &Issue, now: u64) -> Result<()> {
    ensure!(i.format == FORMAT, "not a Factoruide report");
    ensure!(i.version == VERSION, "unsupported report version {}", i.version);
    ensure!(EDITIONS.contains(&i.edition.as_str()), "unknown edition");
    ensure!(
        !i.app_version.is_empty()
            && i.app_version.len() <= 32
            && i.app_version
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b)),
        "bad app_version"
    );
    ensure!(is_hex(&i.contributor, 32), "bad contributor id");
    let (first_day, last_day) = ((MIN_TIME / 86_400) as u32, ((now + CLOCK_SKEW) / 86_400) as u32);
    ensure!((first_day..=last_day).contains(&i.day), "bad day");
    ensure!(SYSTEMS.contains(&i.os.as_str()), "unknown system");
    ensure!(
        (2..=5).contains(&i.language.len()) && i.language.bytes().all(|b| b.is_ascii_alphabetic()),
        "bad language"
    );
    ensure!(KINDS.contains(&i.kind.as_str()), "unknown kind");
    ensure!(
        !i.message.trim().is_empty() && clean_text(&i.message, MAX_MESSAGE, true),
        "bad message"
    );
    ensure!(
        i.name
            .as_deref()
            .is_none_or(|n| !n.trim().is_empty() && clean_text(n, MAX_NAME, false)),
        "bad name"
    );
    match (&i.guide, i.kind.as_str()) {
        (Some(g), "guide") => check_guide(g)?,
        (None, "other") => {}
        _ => anyhow::bail!("guide missing or unexpected"),
    }
    Ok(())
}

/// Read and check a report.
pub fn parse(json: &[u8]) -> Result<Issue> {
    ensure!(json.len() <= MAX_BYTES, "report too large");
    let i: Issue = serde_json::from_slice(json)
        // serde's messages may quote the document: only say where it went wrong.
        .map_err(|e| anyhow::anyhow!("invalid report document (line {}, column {})", e.line(), e.column()))?;
    validate(&i, crate::contribution::now()?)?;
    Ok(i)
}

/// What the server records of each report it keeps, one JSON line in `issues/index.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueReceipt {
    /// SHA-256 of the stored document (hex), its file name: `issues/<sha256>.json`.
    pub sha256: String,
    /// Day the server received it (days since 1970-01-01, UTC).
    pub received: u32,
    pub edition: String,
    pub app_version: String,
    pub contributor: String,
    pub kind: String,
    pub bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue() -> Issue {
        Issue {
            format: FORMAT.into(),
            version: VERSION,
            edition: "classic".into(),
            app_version: "0.11.0".into(),
            contributor: "0".repeat(32),
            day: 20_000,
            os: "windows".into(),
            language: "fr".into(),
            kind: "guide".into(),
            message: "Le PNJ n'est pas là.\nIl est plus au nord.".into(),
            name: Some("Brouz".into()),
            guide: Some(GuideReport {
                request: serde_json::json!({"race": "gnome", "class": "mage", "params": {"seed": 42}}),
                plan: serde_json::json!([{"kind": "accept", "quest": 179}]),
                data: Some("v11.0.1".into()),
                version: 3,
                edited: false,
                imported: false,
                total_time: 3600.0,
                steps: vec![ReportedStep {
                    index: 0,
                    kind: "accept".into(),
                    quest: Some(179),
                    objective: None,
                    text: "Dwarven Outfitters".into(),
                    zone: Some("Dun Morogh".into()),
                    level: 1,
                    map: Some((1426, 29.9, 71.2)),
                }],
            }),
        }
    }

    const NOW: u64 = 20_000 * 86_400;

    #[test]
    fn a_report_goes_through_and_back() {
        let i = issue();
        validate(&i, NOW).unwrap();
        let back: Issue = serde_json::from_slice(&serde_json::to_vec(&i).unwrap()).unwrap();
        validate(&back, NOW).unwrap();
    }

    #[test]
    fn what_does_not_fit_is_refused() {
        let check = |f: &dyn Fn(&mut Issue)| {
            let mut i = issue();
            f(&mut i);
            validate(&i, NOW).is_err()
        };
        assert!(check(&|i| i.message = "  \n ".into()));
        assert!(check(&|i| i.message = "x".repeat(MAX_MESSAGE + 1)));
        assert!(check(&|i| i.message = "bell \u{7}".into()));
        assert!(check(&|i| i.kind = "other".into()));
        assert!(check(&|i| i.guide = None));
        assert!(check(&|i| i.os = "amiga".into()));
        assert!(check(&|i| i.name = Some(" ".into())));
        assert!(check(&|i| i.name = Some("x".repeat(MAX_NAME + 1))));
        assert!(check(&|i| {
            i.guide.as_mut().unwrap().request = serde_json::json!({"a": {"b": {"c": {"d": {"e": {"f": {"g": {"h": {"i": {"j": {"k": {"l": {"m": 1}}}}}}}}}}}}})
        }));
        assert!(check(
            &|i| i.guide.as_mut().unwrap().request = serde_json::json!({"bad key": 1})
        ));
        assert!(check(&|i| {
            let s = i.guide.as_mut().unwrap().steps[0].clone();
            i.guide.as_mut().unwrap().steps.push(s);
        }));
        assert!(!check(&|i| {
            i.kind = "other".into();
            i.guide = None;
            i.name = None;
        }));
    }
}
