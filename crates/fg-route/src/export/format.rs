//! A guide file carries the format it was written in. A file of an older format is migrated
//! when read, one format after the other, so that the guides made by an older app still open.
//! A migration that cannot carry a guide over refuses it, asking to compute it again; a guide
//! of a newer format is refused, asking to update the app.

use super::types::Route;
use anyhow::{Result, bail};
use serde_json::Value;

/// Format the app writes.
pub const FORMAT: u32 = 2;

/// Format of the files that do not say (before formats existed).
pub(super) fn first() -> u32 {
    1
}

/// Migration from each format to the next (index: format - 1).
type Migration = fn(&mut Value) -> Result<()>;
const MIGRATIONS: [Migration; (FORMAT - 1) as usize] = [v1_to_v2];

/// Bring a route (as JSON) to the current format.
pub fn migrate(route: &mut Value) -> Result<()> {
    let format = route
        .get("format")
        .and_then(Value::as_u64)
        .map_or(first(), |f| f as u32);
    if format > FORMAT {
        bail!("this guide was made by a newer version of Factoruide (format {format}): update the app");
    }
    for from in format.max(1)..FORMAT {
        MIGRATIONS[(from - 1) as usize](route)?;
    }
    if let Some(r) = route.as_object_mut() {
        r.insert("format".to_owned(), FORMAT.into());
    }
    Ok(())
}

/// A route read from its JSON, migrated to the current format.
pub fn read_route(mut route: Value) -> Result<Route> {
    migrate(&mut route)?;
    Ok(serde_json::from_value(route)?)
}

/// Format 2 writes the sentences of the steps from language-neutral phrases (`Step::say`), in
/// the language asked. The steps of format 1 only have their text, in the language the guide
/// was made in: it is kept as it is (no phrase, the step is shown as it was).
#[expect(clippy::unnecessary_wraps, reason = "every migration may refuse a guide")]
fn v1_to_v2(_route: &mut Value) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A guide made before formats opens: its texts are kept, its format is the current one.
    #[test]
    fn a_first_format_guide_opens() {
        let old = serde_json::json!({
            "name": "old", "race_id": 2, "class_id": 9, "class": "Warlock", "faction": "Horde",
            "from_level": 1, "to_level": 10, "total_time": 3600.0, "total_time_text": "1h00",
            "quests": 1, "breakdown": serde_json::to_value(crate::plan::Breakdown::default()).unwrap(),
            "locale": "frFR",
            "steps": [{ "kind": "accept", "quest": 788, "text": "Prendre Couper les dents auprès de Gornek", "level": 1, "time": 0.0 }],
        });
        let route = read_route(old).unwrap();
        assert_eq!(route.format, FORMAT);
        assert_eq!(route.steps[0].text, "Prendre Couper les dents auprès de Gornek");
        assert_eq!(route.steps[0].say, Vec::new());
        let newer = serde_json::json!({ "format": FORMAT + 1 });
        assert!(read_route(newer).is_err());
    }
}
