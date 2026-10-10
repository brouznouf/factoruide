//! Language-neutral sentences of a guide: a template key and its arguments (names by ID with
//! their English name, numbers, texts kept as they are). They are written in a language when
//! the guide is shown or installed (`render`), so that a guide is not tied to one language.

use super::templates::{Lang, profession_name, template};
use crate::model::Names;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Phrase {
    #[serde(rename = "k")]
    pub key: String,
    #[serde(rename = "a", default, skip_serializing_if = "BTreeMap::is_empty")]
    pub args: BTreeMap<String, Arg>,
}

/// An argument of a sentence: a name translated by ID (its English name when no translation
/// exists), a profession, a text kept as it is, a number or a list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Arg {
    Quest(i64, String),
    Npc(i64, String),
    Object(i64, String),
    Item(i64, String),
    Zone(i64, String),
    Profession(String, String),
    Text(String),
    Num(i64),
    List(Vec<Arg>),
}

impl Phrase {
    pub fn new(key: &str) -> Self {
        Self {
            key: key.to_owned(),
            args: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn arg(mut self, name: &str, value: Arg) -> Self {
        self.args.insert(name.to_owned(), value);
        self
    }

    /// The sentence in the language of `names`.
    pub fn render(&self, names: &Names) -> String {
        let lang = Lang::of(&names.locale);
        // Objectives in English read as the game writes them (the database's text).
        if self.key.starts_with("objective.")
            && (lang == Lang::En || self.key == "objective.text")
            && let Some(Arg::Text(text)) = self.args.get("text")
        {
            return text.clone();
        }
        let mut out = String::new();
        let mut rest = template(&self.key, lang);
        while let Some(open) = rest.find('{') {
            out.push_str(&rest[..open]);
            let Some(close) = rest[open..].find('}') else { break };
            let name = &rest[open + 1..open + close];
            if let Some(value) = self.args.get(name) {
                out.push_str(&value.render(names, lang));
            }
            rest = &rest[open + close + 1..];
        }
        out.push_str(rest);
        // A count of things to kill, use or get.
        if let Some(Arg::Num(n)) = self.args.get("n")
            && *n > 1
            && self.key.starts_with("objective.")
        {
            let _ = write!(out, " x{n}");
        }
        out
    }
}

impl Arg {
    fn render(&self, names: &Names, lang: Lang) -> String {
        match self {
            Arg::Quest(id, en) => names.get("quest", *id, en).to_owned(),
            Arg::Npc(id, en) => names.get("npc", *id, en).to_owned(),
            Arg::Object(id, en) => names.get("object", *id, en).to_owned(),
            Arg::Item(id, en) => names.get("item", *id, en).to_owned(),
            Arg::Zone(id, en) => names.get("zone", *id, en).to_owned(),
            Arg::Profession(key, en) => profession_name(key, lang).unwrap_or(en).to_owned(),
            Arg::Text(text) => text.clone(),
            Arg::Num(n) => n.to_string(),
            Arg::List(items) => items
                .iter()
                .map(|a| a.render(names, lang))
                .collect::<Vec<_>>()
                .join(", "),
        }
    }
}

/// The sentences of a step, one after the other.
pub fn render_all(phrases: &[Phrase], names: &Names) -> String {
    phrases.iter().map(|p| p.render(names)).collect::<Vec<_>>().join(" ")
}
