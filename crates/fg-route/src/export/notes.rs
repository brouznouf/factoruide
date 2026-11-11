//! Quest notes and step icons (`notes/*.toml`): what a player is told about a quest beyond the
//! step's sentence (an event to wait for, a mob that wanders), and an icon saying how a quest is
//! taken or an objective done. Written into a guide when it is shown or installed (`render`),
//! never read by the planner.
//!
//! A note is for a quest, at one of its steps (`phase`: accept, objective, turnin; all three
//! when none), on one objective (`objective`, from 1 in the game's order) or all, in some game
//! versions (`editions`, all when none). Its text is in English (`en`) with translations by
//! language (`fr`, `de`...), and may name things by ID (`{npc:1234}`, `{item:..}`, `{object:..}`,
//! `{quest:..}`, `{zone:..}`): written in the guide's language like the step's sentence.
//!
//! An icon is found from the step (an item starting the quest, a mob to kill...), or given by
//! an `[[icon]]` of the files (a rare mob, an event at the giver).

use super::templates::Lang;
use super::types::{Step, StepKind, TargetKind};
use crate::model::Names;
use crate::xp::Edition;
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/notes.rs"));
}

/// Characters a note's text may have at most (in any language, tags counted as written).
pub const MAX_LEN: usize = 300;

/// How much a note matters, the least first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// Good to know.
    Info,
    /// Something to watch for.
    Warning,
    /// The step goes wrong without it.
    MustRead,
}

impl Level {
    /// The number the addon gets (`nt = {{ l = 1, ... }}`).
    pub fn number(self) -> u8 {
        match self {
            Self::Info => 1,
            Self::Warning => 2,
            Self::MustRead => 3,
        }
    }
}

/// The step of a quest a note or an icon is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Accept,
    Objective,
    TurnIn,
}

impl Phase {
    fn of(kind: StepKind) -> Option<Self> {
        match kind {
            StepKind::Accept => Some(Self::Accept),
            StepKind::Objective => Some(Self::Objective),
            StepKind::TurnIn => Some(Self::TurnIn),
            _ => None,
        }
    }
}

/// A note of a step, written in the guide's language.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepNote {
    pub level: Level,
    pub text: String,
}

/// The icons a step may have: how a quest is taken, then how an objective is done, then those
/// only given by hand.
pub const ICONS: &[&str] = &[
    // Taking a quest: from an NPC, an object of the world, an item in the bags, an item a mob drops.
    "npc",
    "object",
    "item",
    "item_drop",
    // Objectives: kill (an elite), loot, use an object, talk, buy, anything else (explore, an event).
    "kill",
    "elite",
    "get",
    "use",
    "talk",
    "buy",
    "text",
    // By hand: a rare or special mob, an event to wait for, an item to pick up from the ground.
    "rare",
    "event",
    "pickup",
];

/// A note as written in a file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct NoteDef {
    quest: i64,
    phase: Option<Phase>,
    objective: Option<u8>,
    level: Level,
    editions: Option<Vec<Edition>>,
    en: String,
    fr: Option<String>,
    de: Option<String>,
    es: Option<String>,
    pt: Option<String>,
    ru: Option<String>,
    ko: Option<String>,
    zh_cn: Option<String>,
    zh_tw: Option<String>,
}

impl NoteDef {
    /// Its text in `lang`, English when it has no translation.
    fn text(&self, lang: Lang) -> &str {
        let own = match lang {
            Lang::En => None,
            Lang::Fr => self.fr.as_deref(),
            Lang::De => self.de.as_deref(),
            Lang::Es => self.es.as_deref(),
            Lang::Pt => self.pt.as_deref(),
            Lang::Ru => self.ru.as_deref(),
            Lang::Ko => self.ko.as_deref(),
            Lang::ZhCn => self.zh_cn.as_deref(),
            Lang::ZhTw => self.zh_tw.as_deref(),
        };
        own.unwrap_or(&self.en)
    }

    fn texts(&self) -> impl Iterator<Item = &str> {
        [
            Some(self.en.as_str()),
            self.fr.as_deref(),
            self.de.as_deref(),
            self.es.as_deref(),
            self.pt.as_deref(),
            self.ru.as_deref(),
            self.ko.as_deref(),
            self.zh_cn.as_deref(),
            self.zh_tw.as_deref(),
        ]
        .into_iter()
        .flatten()
    }
}

/// An icon given by hand.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct IconDef {
    quest: i64,
    phase: Phase,
    objective: Option<u8>,
    icon: String,
    editions: Option<Vec<Edition>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct NotesFile {
    #[serde(default)]
    note: Vec<NoteDef>,
    #[serde(default)]
    icon: Vec<IconDef>,
}

/// Where a note or an icon applies.
#[derive(Debug, Clone, Copy)]
struct Scope {
    phase: Option<Phase>,
    objective: Option<u8>,
}

impl Scope {
    /// Whether it applies to `step` (a step of its quest).
    fn fits(self, step: &Step) -> bool {
        let Some(phase) = Phase::of(step.kind) else {
            return false;
        };
        self.phase.is_none_or(|p| p == phase)
            && (self.objective.is_none() || (phase == Phase::Objective && self.objective == step.slot))
    }
}

/// A thing a note's text names by ID (`{npc:1234}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Tag {
    Npc(i64),
    Item(i64),
    Object(i64),
    Quest(i64),
    Zone(i64),
}

impl Tag {
    fn kind(self) -> &'static str {
        match self {
            Self::Npc(_) => "npc",
            Self::Item(_) => "item",
            Self::Object(_) => "object",
            Self::Quest(_) => "quest",
            Self::Zone(_) => "zone",
        }
    }

    fn id(self) -> i64 {
        match self {
            Self::Npc(id) | Self::Item(id) | Self::Object(id) | Self::Quest(id) | Self::Zone(id) => id,
        }
    }

    /// Its English name in the database, None when the database does not have it.
    fn english(self, conn: &Connection) -> Result<Option<String>> {
        let query = match self {
            Self::Npc(_) => "SELECT name FROM m_npc WHERE id = ?1",
            Self::Item(_) => "SELECT name FROM m_item WHERE id = ?1",
            Self::Object(_) => "SELECT name FROM m_object WHERE id = ?1",
            Self::Quest(_) => "SELECT name FROM m_quest WHERE id = ?1",
            Self::Zone(_) => "SELECT AreaName_lang FROM client_areatable WHERE ID = ?1",
        };
        Ok(conn.query_row(query, [self.id()], |r| r.get(0)).optional()?)
    }
}

/// A text cut into its plain parts and its tags, or why it cannot be.
fn parse_text(text: &str) -> Result<Vec<Result<&str, Tag>>> {
    let mut parts = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find(['{', '}']) {
        if rest[open..].starts_with('}') {
            bail!("a '}}' with no '{{' before it");
        }
        parts.push(Ok(&rest[..open]));
        let Some(close) = rest[open..].find('}') else {
            bail!("a '{{' with no '}}' after it")
        };
        let inner = &rest[open + 1..open + close];
        let (kind, id) = inner
            .split_once(':')
            .with_context(|| format!("{{{inner}}}: a tag is {{kind:id}}"))?;
        let id: i64 = id.parse().with_context(|| format!("{{{inner}}}: the ID is a number"))?;
        parts.push(Err(match kind {
            "npc" => Tag::Npc(id),
            "item" => Tag::Item(id),
            "object" => Tag::Object(id),
            "quest" => Tag::Quest(id),
            "zone" => Tag::Zone(id),
            _ => bail!("{{{inner}}}: kinds are npc, item, object, quest, zone"),
        }));
        rest = &rest[open + close + 1..];
    }
    parts.push(Ok(rest));
    Ok(parts)
}

/// The notes and icons of every file, in the files' order.
fn files() -> Result<Vec<(&'static str, NotesFile)>> {
    embedded::NOTES
        .iter()
        .map(|(name, text)| Ok((*name, toml::from_str(text).with_context(|| format!("notes/{name}"))?)))
        .collect()
}

fn for_edition(editions: Option<&Vec<Edition>>, edition: Edition) -> bool {
    editions.is_none_or(|e| e.contains(&edition))
}

/// The notes and hand-given icons of a game version, written in a language.
#[derive(Debug, Clone, Default)]
pub struct Book {
    notes: HashMap<i64, Vec<(Scope, StepNote)>>,
    icons: HashMap<i64, Vec<(Scope, String)>>,
}

impl Book {
    /// The notes of `edition` written in the language of `names`, the things they name in it
    /// (English names from `conn`, the edition's database).
    pub fn load(conn: &Connection, edition: Edition, names: &Names) -> Result<Self> {
        Self::of_files(files()?, conn, edition, names)
    }

    fn of_files(files: Vec<(&str, NotesFile)>, conn: &Connection, edition: Edition, names: &Names) -> Result<Self> {
        let lang = Lang::of(&names.locale);
        let mut book = Self::default();
        let mut english: HashMap<Tag, Option<String>> = HashMap::new();
        for (_, file) in files {
            for def in file.note.iter().filter(|d| for_edition(d.editions.as_ref(), edition)) {
                let mut text = String::new();
                for part in parse_text(def.text(lang))? {
                    match part {
                        Ok(plain) => text.push_str(plain),
                        Err(tag) => {
                            if let std::collections::hash_map::Entry::Vacant(e) = english.entry(tag) {
                                e.insert(tag.english(conn)?);
                            }
                            let name = english[&tag].as_deref().unwrap_or("?");
                            text.push_str(names.get(tag.kind(), tag.id(), name));
                        }
                    }
                }
                let scope = Scope {
                    phase: def.phase,
                    objective: def.objective,
                };
                (book.notes.entry(def.quest).or_default()).push((scope, StepNote { level: def.level, text }));
            }
            for def in file
                .icon
                .into_iter()
                .filter(|d| for_edition(d.editions.as_ref(), edition))
            {
                let scope = Scope {
                    phase: Some(def.phase),
                    objective: def.objective,
                };
                book.icons.entry(def.quest).or_default().push((scope, def.icon));
            }
        }
        Ok(book)
    }

    /// The notes of `step`, the one that matters most first (files' order between equals).
    pub fn notes_of(&self, step: &Step) -> Vec<StepNote> {
        let Some(quest) = step.quest else { return Vec::new() };
        let mut notes: Vec<StepNote> = (self.notes.get(&quest).into_iter().flatten())
            .filter(|(scope, _)| scope.fits(step))
            .map(|(_, note)| note.clone())
            .collect();
        notes.sort_by_key(|n| std::cmp::Reverse(n.level));
        notes
    }

    /// The icon given by hand to `step`, the last one fitting.
    pub fn icon_of(&self, step: &Step) -> Option<&str> {
        let quest = step.quest?;
        (self.icons.get(&quest).into_iter().flatten())
            .rfind(|(scope, _)| scope.fits(step))
            .map(|(_, icon)| icon.as_str())
    }
}

/// The icon of a step from what it asks: how its quest is taken, how its objective is done.
pub fn auto_icon(step: &Step) -> Option<&'static str> {
    let key = step.say.first().map(|p| p.key.as_str());
    match step.kind {
        StepKind::Accept => Some(match key {
            Some("accept.item.mob") => "item_drop",
            Some("accept.item" | "accept.held") => "item",
            _ if step.held => "item",
            _ => match step.target.as_ref().map(|t| t.kind) {
                Some(TargetKind::Object) => "object",
                Some(TargetKind::Item) => "item",
                _ => "npc",
            },
        }),
        // A guide of format 1 (texts only): from what the objective is on.
        StepKind::Objective if key.is_none() => match step.target.as_ref()?.kind {
            TargetKind::Npc if step.elite => Some("elite"),
            TargetKind::Npc => Some("kill"),
            TargetKind::Item => Some("get"),
            TargetKind::Object => Some("use"),
            TargetKind::Area => Some("text"),
            _ => None,
        },
        StepKind::Objective => match key? {
            "objective.kill" if step.elite => Some("elite"),
            "objective.kill" => Some("kill"),
            "objective.talk" => Some("talk"),
            "objective.use" => Some("use"),
            "objective.get" => Some("get"),
            "objective.text" => Some("text"),
            k if k.starts_with("objective.buy") => Some("buy"),
            _ => None,
        },
        _ => None,
    }
}

/// What is wrong in the notes files whatever the game data: a file that does not read, a
/// field out of its values, an English text missing, a text too long or with a bad tag, an
/// unknown icon.
pub fn check_files() -> Vec<String> {
    let files = match files() {
        Ok(files) => files,
        Err(e) => return vec![format!("{e:#}")],
    };
    let mut problems = Vec::new();
    for (name, file) in &files {
        for def in &file.note {
            let at = format!("notes/{name}: note of quest {}", def.quest);
            if def.en.trim().is_empty() {
                problems.push(format!("{at}: no English text"));
            }
            if def.objective.is_some() && def.phase != Some(Phase::Objective) {
                problems.push(format!("{at}: `objective` goes with phase = \"objective\""));
            }
            if def.objective == Some(0) {
                problems.push(format!("{at}: objectives count from 1"));
            }
            if def.editions.as_ref().is_some_and(Vec::is_empty) {
                problems.push(format!("{at}: `editions` is empty (leave it out for all)"));
            }
            let english_tags = parse_text(&def.en).map(|parts| tags(&parts)).unwrap_or_default();
            for text in def.texts() {
                if text.chars().count() > MAX_LEN {
                    problems.push(format!("{at}: longer than {MAX_LEN} characters"));
                }
                match parse_text(text) {
                    Err(e) => problems.push(format!("{at}: {e:#}")),
                    Ok(parts) if tags(&parts) != english_tags => {
                        problems.push(format!("{at}: a translation names other things than the English text"));
                    }
                    Ok(_) => {}
                }
            }
        }
        for def in &file.icon {
            let at = format!("notes/{name}: icon of quest {}", def.quest);
            if !ICONS.contains(&def.icon.as_str()) {
                problems.push(format!(
                    "{at}: unknown icon {:?} (one of {})",
                    def.icon,
                    ICONS.join(", ")
                ));
            }
            if def.phase == Phase::TurnIn {
                problems.push(format!("{at}: icons are for accept and objective steps"));
            }
            if def.objective.is_some() && def.phase != Phase::Objective {
                problems.push(format!("{at}: `objective` goes with phase = \"objective\""));
            }
        }
    }
    problems
}

/// The tags of a parsed text, sorted.
fn tags(parts: &[Result<&str, Tag>]) -> Vec<(&'static str, i64)> {
    let mut tags: Vec<_> = parts
        .iter()
        .filter_map(|p| p.err())
        .map(|t| (t.kind(), t.id()))
        .collect();
    tags.sort_unstable();
    tags
}

/// What the notes and icons of `edition` say that its database does not have: a quest, an
/// objective of a quest, a thing named by a tag. (key, detail) each.
pub fn check_data(conn: &Connection, edition: Edition) -> Result<Vec<(String, String)>> {
    let mut problems = Vec::new();
    let quest_exists = |id: i64| -> Result<bool> {
        Ok(conn
            .query_row("SELECT 1 FROM m_quest WHERE id = ?1", [id], |_| Ok(()))
            .optional()?
            .is_some())
    };
    let objective_exists = |quest: i64, idx: u8| -> Result<bool> {
        Ok(conn
            .query_row(
                "SELECT 1 FROM m_quest_objective WHERE quest_id = ?1 AND idx = ?2",
                (quest, idx),
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    };
    for (name, file) in files()? {
        let scopes = (file.note.iter())
            .filter(|d| for_edition(d.editions.as_ref(), edition))
            .map(|d| (d.quest, d.objective, "note"))
            .chain(
                (file.icon.iter())
                    .filter(|d| for_edition(d.editions.as_ref(), edition))
                    .map(|d| (d.quest, d.objective, "icon")),
            );
        for (quest, objective, what) in scopes {
            if !quest_exists(quest)? {
                problems.push((
                    format!("{name}:{what}:{quest}"),
                    format!("notes/{name}: {what} of quest {quest}, which this game version does not have"),
                ));
            } else if let Some(idx) = objective
                && !objective_exists(quest, idx)?
            {
                problems.push((
                    format!("{name}:{what}:{quest}:{idx}"),
                    format!("notes/{name}: {what} of quest {quest} on objective {idx}, which it does not have"),
                ));
            }
        }
        for def in file.note.iter().filter(|d| for_edition(d.editions.as_ref(), edition)) {
            for text in def.texts() {
                for part in parse_text(text)? {
                    if let Err(tag) = part
                        && tag.english(conn)?.is_none()
                    {
                        problems.push((
                            format!("{name}:tag:{}:{}", tag.kind(), tag.id()),
                            format!(
                                "notes/{name}: note of quest {} names {} {}, which this game version does not have",
                                def.quest,
                                tag.kind(),
                                tag.id()
                            ),
                        ));
                    }
                }
            }
        }
    }
    problems.sort();
    problems.dedup();
    Ok(problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_files_are_well_formed() {
        let problems = check_files();
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    #[test]
    fn tags_are_cut_from_the_text() {
        let parts = parse_text("Ask {npc:12} for {item:5}.").unwrap();
        assert_eq!(
            parts,
            vec![Ok("Ask "), Err(Tag::Npc(12)), Ok(" for "), Err(Tag::Item(5)), Ok(".")]
        );
        assert!(parse_text("a {spell:3}").is_err());
        assert!(parse_text("a {npc:x}").is_err());
        assert!(parse_text("a {npc:3").is_err());
        assert!(parse_text("a } b").is_err());
    }

    fn step(kind: StepKind, slot: Option<u8>) -> Step {
        let mut s = Step::bare(kind, 1, 0.0);
        s.quest = Some(7);
        s.slot = slot;
        s
    }

    /// The book of a file's text, in English, on a database without names.
    fn book(defs: &str) -> Book {
        let conn = Connection::open_in_memory().unwrap();
        Book::of_files(
            vec![("t.toml", toml::from_str(defs).unwrap())],
            &conn,
            Edition::Classic,
            &Names::new("enUS"),
        )
        .unwrap()
    }

    #[test]
    fn notes_name_things_in_the_guide_language_and_keep_to_their_editions() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE m_npc (id INTEGER, name TEXT); INSERT INTO m_npc VALUES (738, 'Private Thorsen');
             CREATE TABLE l10n (entity_type TEXT, id INTEGER, locale TEXT, name TEXT, source TEXT);
             INSERT INTO l10n VALUES ('npc', 738, 'frFR', 'Soldat Thorsen', 'client');",
        )
        .unwrap();
        let file = r#"
            [[note]]
            quest = 7
            level = "info"
            en = "Ask {npc:738}."
            fr = "Demandez à {npc:738}."
            [[note]]
            quest = 7
            level = "info"
            editions = ["tbc"]
            en = "Only in TBC."
            "#;
        let texts = |locale: &str| {
            let names = Names::load(&conn, locale).unwrap();
            let b = Book::of_files(
                vec![("t.toml", toml::from_str(file).unwrap())],
                &conn,
                Edition::Classic,
                &names,
            )
            .unwrap();
            b.notes_of(&step(StepKind::TurnIn, None))
                .into_iter()
                .map(|n| n.text)
                .collect::<Vec<_>>()
        };
        assert_eq!(texts("enUS"), ["Ask Private Thorsen."]);
        assert_eq!(texts("frFR"), ["Demandez à Soldat Thorsen."]);
        assert_eq!(texts("deDE"), ["Ask Private Thorsen."]);
    }

    #[test]
    fn a_note_without_phase_is_on_every_step_of_its_quest_most_important_first() {
        let b = book(
            r#"
            [[note]]
            quest = 7
            level = "info"
            en = "all"
            [[note]]
            quest = 7
            phase = "accept"
            level = "must_read"
            en = "accept"
            [[note]]
            quest = 7
            phase = "objective"
            objective = 2
            level = "warning"
            en = "second"
            "#,
        );
        let texts = |s: &Step| b.notes_of(s).into_iter().map(|n| n.text).collect::<Vec<_>>();
        assert_eq!(texts(&step(StepKind::Accept, None)), ["accept", "all"]);
        assert_eq!(texts(&step(StepKind::Objective, Some(1))), ["all"]);
        assert_eq!(texts(&step(StepKind::Objective, Some(2))), ["second", "all"]);
        assert_eq!(texts(&step(StepKind::TurnIn, None)), ["all"]);
        let mut other = step(StepKind::Accept, None);
        other.quest = Some(8);
        assert_eq!(b.notes_of(&other), []);
        assert_eq!(b.notes_of(&Step::bare(StepKind::Hearth, 1, 0.0)), []);
    }

    #[test]
    fn an_icon_given_by_hand_wins_over_the_one_found() {
        let b = book(
            r#"
            [[icon]]
            quest = 7
            phase = "accept"
            icon = "event"
            "#,
        );
        assert_eq!(auto_icon(&step(StepKind::Accept, None)), Some("npc"));
        assert_eq!(b.icon_of(&step(StepKind::Accept, None)), Some("event"));
        assert_eq!(b.icon_of(&step(StepKind::Objective, Some(1))), None);
    }

    #[test]
    fn an_elite_to_kill_has_its_own_icon() {
        let mut s = step(StepKind::Objective, Some(1));
        s.say.push(super::super::Phrase::new("objective.kill"));
        assert_eq!(auto_icon(&s), Some("kill"));
        s.elite = true;
        assert_eq!(auto_icon(&s), Some("elite"));
    }

    #[test]
    fn notes_are_written_in_the_guide_language_english_otherwise() {
        let def: NoteDef = toml::from_str("quest = 1\nlevel = \"info\"\nen = \"hello\"\nfr = \"bonjour\"").unwrap();
        assert_eq!(def.text(Lang::Fr), "bonjour");
        assert_eq!(def.text(Lang::De), "hello");
    }
}
