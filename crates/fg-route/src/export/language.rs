//! The guide's language: French sentences, else English ones, and names translated where a
//! translation exists (English otherwise).

use crate::model::Names;

pub(super) struct Language<'a> {
    names: &'a Names,
    french: bool,
}

impl<'a> Language<'a> {
    pub(super) fn new(names: &'a Names) -> Self {
        Self {
            names,
            french: names.locale == "frFR",
        }
    }

    pub(super) fn french(&self) -> bool {
        self.french
    }

    /// The objective texts of the database are English: kept as they are in English guides.
    pub(super) fn english(&self) -> bool {
        self.names.locale == "enUS"
    }

    pub(super) fn sentence(&self, english: String, french: String) -> String {
        if self.french { french } else { english }
    }

    /// Name of an NPC, object, item or quest in the guide's language.
    pub(super) fn name(&self, kind: &str, id: i64, english: &str) -> String {
        let kind = match kind {
            "npc" | "object" | "item" | "quest" => kind,
            _ => "",
        };
        self.names.get(kind, id, english).to_owned()
    }

    pub(super) fn profession(&self, key: &str, english: &str) -> String {
        if self.french {
            crate::profession::french_name(key).unwrap_or(english).to_owned()
        } else {
            english.to_owned()
        }
    }
}
