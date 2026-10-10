//! Sentences of the guide: French ones, else English ones, with translated names.

use crate::model::{EntityKind, Objective};

/// Objective text with a translated target name: French sentences, else English ones.
pub(super) fn objective_text(o: &Objective, name: &str, fr: bool) -> String {
    let n = o.count.round().max(1.0) as i64;
    let times = |s: String| if n > 1 { format!("{s} x{n}") } else { s };
    match o.loc.kind {
        EntityKind::Npc if o.kills > 0.0 => times(if fr {
            format!("Tuer {name}")
        } else {
            format!("Kill {name}")
        }),
        EntityKind::Npc => {
            if fr {
                format!("Parler à {name}")
            } else {
                format!("Talk to {name}")
            }
        }
        EntityKind::Object => times(if fr {
            format!("Utiliser {name}")
        } else {
            format!("Use {name}")
        }),
        EntityKind::Item if o.text.starts_with("Buy ") => times(if fr {
            format!("Acheter {name}")
        } else {
            format!("Buy {name}")
        }),
        _ => times(name.to_owned()),
    }
}
