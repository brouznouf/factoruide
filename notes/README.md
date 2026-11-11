# Quest notes

Notes tell a player more about a quest than the step's sentence: an event to wait for, a mob that
wanders, an item to pick up first. The app writes them into every guide when it is shown or
installed, in the guide's language; the addon shows a yellow, orange or red mark after the step and
the notes in its tooltip. The planner never reads them.

Every `.toml` file of this folder (subfolders too) is read, in name order. Group them as you like,
by zone for instance.

## Notes

```toml
# Why the note is right, for the reviewers (comments are not shown).
[[note]]
quest = 215                 # quest ID
phase = "accept"            # optional: accept, objective or turnin; none = every step of the quest
objective = 2               # optional, with phase = "objective": the quest's objective, from 1
level = "warning"           # info (good to know), warning (watch out) or must_read
editions = ["classic"]      # optional: forever, classic, tbc; none = all
en = "Talk to {npc:738} once he leaves the camp."
fr = "Parlez à {npc:738} une fois qu'il a quitté le camp."
```

- `en` is required; translations are optional: `fr`, `de`, `es`, `pt`, `ru`, `ko`, `zh_cn`,
  `zh_tw`. A guide in a language without its translation gets the English text.
- 300 characters at most, in any language.
- Tags name things by ID in the guide's language: `{npc:ID}`, `{item:ID}`, `{object:ID}`,
  `{quest:ID}`, `{zone:ID}` (AreaTable). A translation names the same things as the English text.
- Several notes may be on the same quest and step (one more for a single game version, say). The
  step's mark takes the color of the one that matters most; its tooltip lists them all, the one
  that matters most first.
- Write notes in your own words, as general knowledge of the game: no copied text, no source.

## Icons

A step's icon says how its quest is taken or its objective done. It is found from the data:

- taking a quest: `npc`, `object` (a sign, a chest...), `item` (an item in the bags starts it),
  `item_drop` (an item a mob drops starts it);
- objectives: `kill` (`elite` for elites), `get`, `use`, `talk`, `buy`, `text` (anything else:
  explore, an event).

An `[[icon]]` gives another one, those above or `rare` (a rare or special mob), `event` (an event
to wait for), `pickup` (an item to pick up from the ground):

```toml
[[icon]]
quest = 215
phase = "accept"            # accept or objective
objective = 1               # optional, with phase = "objective"
icon = "event"
editions = ["classic"]      # optional
```

## Checks

- `cargo test -p fg-route notes` checks the files: fields, levels, phases, tags, lengths, icons.
- `fg --edition <edition> audit --check notes` checks them against each game version's data:
  the quests, objectives and tagged things must exist there (or limit the note with `editions`).
