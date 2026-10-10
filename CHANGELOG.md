# Changelog

The notes of each version are the message of its tag (`scripts/release.sh`), shown in the app.

## Unreleased (0.2.0)

Personalized leveling guides for WoW Forever, Classic Era / Hardcore and TBC Anniversary.

### Character power

- Fights depend on power: level plus gear and class spells.
- Each level of power makes fights 10% faster; XP still follows the level.
- The guide wears quest rewards and dungeon loot, and favours real upgrades.
- The addon picks the reward the guide planned.
- Training happens when the new spells are worth it.
- The addon buys only the spells useful for leveling.
- Each guide step shows the character's power and what its gear brings.

### What the character takes on

- New progression profile: cautious (hardcore survival), normal or risky, on the main settings screen.
- Cautious never fights packs of mobs more than one level above you.
- Cautious still takes yellow quests and lone targets up to two levels above.
- Normal does yellow quests, never orange.
- Risky takes orange quests and borderline elites.
- Elite camps need one more level of power per extra elite pulled, at any level.
- Quests started by an item need the power to kill the mob dropping it (no level 24 rare at level 11).
- Exploring and escorting count as fighting; talking and delivering don't.

### Grinding

- New "Grind on the way" option (on by default): mobs met on foot are killed on the way, as most guides ask.
- Only mobs close to your level (3 below to 1 above); the guide step says how many: "(kill ~5 mobs on the way)".
- It replaces pure grinding and never wins over a good quest: flights and walks are chosen as before.
- Grinding is limited to a share of each level, 10% by default (adjustable), mobs on the way included.
- Beyond it the guide takes quests instead, even less rewarding ones.
- New "Grinding allowed" option: without it, the guide takes more quests instead, even if longer.
- Quest objects lying among the mobs of another quest are gathered between kills, while resting.

### Professions

- Professions no longer add time or steps to the guide.
- The addon shows whether your skill is ahead or behind the curve for your level.
- The curve starts when you take the profession (level 5 by default, adjustable); taken later, it follows later.
- New curves: slow start, catching up by level 20.
- Profession quests are suggested, marked optional; the addon skips them without the skill.

### Checkpoints

- Checkpoints from level 20, where many quests open: ahead of plan, a button skips to the next one.
- Skipping keeps chains going on after it, class quests, planned rewards and training.
- Behind the route, a checkpoint step has you grind before going on, so the guide stays in sync.
- Grinding happens where mobs of your level live, with an XP target: "be level 19 and 93% (19969 xp)".

### Quests

- Dungeon quests are never planned without dungeons.
- Battleground quests are left out unless the new PvP option is on.
- Marks, tokens and crafted items never get a made-up mob source.
- Quests that can be picked up before the target level are kept, whatever their level.
- Quests are taken and turned in at hand, at quest hubs.
- The hearthstone is used only when it saves at least 3 minutes.
- A zone is done in one go instead of several short visits.
- Walks go around mountains and cliffs, from the game's terrain heights.
- Old chains no longer worth it are left out instead of coming back for them.
- Mobs of quests in the log are killed on the way.
- Class quests giving a pet or a form are done early: they count as power, like gear.
- Pet quests stay early even after a long optimization (up to 10 min slower).
- Objects of quests in the log are picked up on the way, like their mobs.
- Quests started by a rare drop (under 10%) are no longer planned: take them if it drops.
- Gray quests are no longer taken, unless they open a chain worth doing.
- A quest gone gray in the log is dropped instead of turned in many levels later.
- Quests taken but never turned in are left out of the guide.
- Long follow-ups (sixty cloth to gather) no longer make a chain worth starting.
- Fewer trips for one or two quests: changing zone counts 4 minutes (new advanced setting).
- Fixed: Deeprun Tram quests, Frostmane Hold.
- Fixed: the paladin's Tome of Divinity chain.

### In the app

- Generation: the times of the best routes are shown while they are picked, then each one's curve as it improves, in its own color; the guide kept is in bold, with why it is not always the shortest (it is scored on comfort too).
- The calculation keeps to its time: 2 minutes by default (30 s to pick the best starts, then 1 min 30 to improve them), 6 minutes for a thorough one.
- My guides: class colors, a Horde or Alliance emblem with the race, and every column sorts the list.
- The guide page shows the character the same way.
- The addon accepts only the guide's quests by default (Addon page, "Only the guide's quests"); others can still be taken by hand.

### Addon look

- The addon takes the game's look by default.
- With ElvUI, EllesmereUI or SpartanUI installed, it takes their style.
- New "Look" option: automatic, game style or dark.
- Other skins can style it through `FactoruideAPI.RegisterSkin`.
- A big arrow points to the current step: green ahead, red behind, distance below.
- Single scrollbar: only the content area scrolls.

### Fixes

- TBC: mobs of the blood elf and draenei starting zones give Azeroth XP (they counted as Outland: five times too much). Guides to 20 there are longer and take more quests.
- TBC: grinding XP follows where you are (Outland mobs from Hellfire Peninsula on, not from level 60); the Caverns of Time dungeons count as Outland.
- A quest dropped from the log no longer opens its follow-up (the guide asked for quests you could not get).
- Dungeon mobs keep their level: a dungeon run late no longer gives XP as if they had yours.
- Flight prices are those of the flight taken (the price of a cheaper, slower path was counted).
- Only flight masters are flight paths: no more "Flight path: Quest Path 11517… Shaman… A" steps (the nodes of quest flights, Eastern Plaguelands towers, the other faction's nodes).
- Forever: quests whose objectives or turn-in are unknown are left out instead of being taken for free (a Horde guide went to Menethil Harbor for an Alliance quest).
- The optimizer judged about a third of the changes it tried wrongly (quests taken at hand look ahead in the guide); it now evaluates them exactly.
- A new character's hearthstone takes it back to its starting area until bound at an inn (orcs were sent to Ratchet).
- Hearthstone steps name the town: "Hearthstone to Crossroads (The Barrens)".
- Ratchet and the Crossroads flight paths were shown in Durotar.
- Hearthstone, flight and boat steps show their own time (the whole trip was on the arrival step).

- The computing time is the whole time: the race between starts takes a share of it.
- A search could hand back an invalid route (when cutting the stops after the target level).
- Walks on two continents no longer share cached answers.
- The time shown during the calculation is the guide's play time, as in the result (it showed the optimizer's score, hours higher).
- Guides can be computed again with the default settings.
- Mobs killed on the way to an objective are no longer counted twice (time and XP).
- The guide and tracker windows always grow downwards: their buttons stay under the mouse.
- Objectives in progress stay in the guide until done, not only right after their step.
- A turn-in shows its quest's unfinished objectives first.
- Quest names are in English when no translation exists.
- Updates are checked only in published builds.
