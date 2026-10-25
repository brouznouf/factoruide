# Changelog

Each version's tag message (`scripts/release.sh`), shown in the app's update prompt, is kept short: a link to this file.

Writing an entry: one short line per change, saying what the player sees or gets (not how it works inside), grouped under App, Guides, Addon and WoW data. Numbers and examples only when they tell the change better than words; details go to the commit.

An addon change also goes, in the same words, under Unreleased in that addon's own `addon/<addon>/CHANGELOG.md`: its changelog on CurseForge and Wago.

## Unreleased

### App

- Choose must-have quests by ID or exact name.
- In the guide editor, a step that only a zone too strong for the character makes possible (the flight path of Gadgetzan from Thousand Needles at level 34) is accepted, with a warning on the step, instead of being impossible. Computed guides still keep out of those zones.
- A new guide's character shows the flight paths it knows, by continent, and you can tick or untick them: the game only gives the addon those of the continent where a flight master's map is open, so the others are deduced from the quests done in their towns (marked as such), and what you change is kept for the character.
- Before a guide is computed for a character already on its way, the app can show a few ways to begin (a zone, a flight, the hearthstone) with their estimated time, steps and quests, and you choose; the fastest is chosen at first. It is only shown when there is really a choice, and can be turned off ("Compare the starts").

### Guides

- Guides complete must-have quests and their prerequisites, even past the target level.
- Quest prerequisites keep the eligible race and class branches; Sleeping Bag uses your faction's starter.
- Walks jump down higher cliffs the bolder the profile: 15 yards for "cautious" (no fall damage), 30 for "normal" (about a third of the health), 50 for "risky" (two thirds); the health lost is paid as time resting.
- Quests in your log with nothing done are no longer tied to the guide: it takes them like any other, or abandons them first. The ones you can turn in, and those you have begun, are still kept.
- The walks the addon follows are the ones the guide shows, whole: long walks follow the ground between the tiles instead of cutting across it, and the guide no longer simplifies a walk (only the addon gets a lighter one).
- Guides leave a zone with almost nothing left to do sooner, and a few of the routes compared at the start begin in another zone rich in quests.
- The inn your hearthstone is bound to is found whatever the language of your game (the guide no longer starts with a trip back to your starting place).
- A spell you know a higher rank of (Seal of the Crusader, Divine Protection, Hammer of Justice) no longer sends you back to the trainer: the spell book only shows the best rank.
- Walks no longer go into a zone whose monsters are far above the character (Stranglethorn Vale at level 17 on the way to Booty Bay's boat): the guide goes round, by Ashenvale for the Barrens, or takes a boat or a flight.
- Guides are computed about 40 % faster.
- Dungeons are run later: not before the weakest of the group is within 3 levels of the dungeon's monsters (the Deadmines at 18, not 15), at their level in the cautious profile, 5 levels below in the risky one.
- WoW Forever: City of Dalaran is entered from the Alliance's own quarter (the pipe in the Alterac Mountains) in Alliance guides, and by the tunnel at the west edge only in Horde guides.
- Profession quests turned in at a city trainer (Camping 101: Skinning, Leatherworking in Orgrimmar) are planned too, even once they are gray.
- Guides are computed about a third faster again (walks between zones had slowed them down in 0.14.0).
- TBC: places in Outland, the blood elf and draenei zones are walked to on the floor they stand on (towers, ramps, cave levels), not the nearest ground.
- Valley of Trials: walks leave by the valley's road instead of over the mountains, and Burning Blade Medallion is done in the valley.
- Walks follow the game's real ground: into caves and mines, through tunnels, over bridges and up city levels, around buildings, walls and rocks. The app's download is a little smaller.
- Walks up Mount Hyjal and out of Timbermaw Hold follow Darkwhisper Gorge and the tunnel all the way instead of cutting their last corners.
- The Affray: the grate is stepped on before Big Will is fought, and timed quests are only taken once the character is strong enough to finish them in time.
- Mobs, NPCs and objects stand on their real floor: the kobolds inside Fargodeep Mine rather than on the hill above it, Undercity's NPCs down in the city, the watchtower's NPCs on Zephras Isle at its top.

### Addon

- The wind guide (a gust of wind towards the step instead of the compass) is gone, with its settings and key binding; the compass stays.
- Objectives done in the same area are shown together under Now; a level checkpoint between them comes after them.
- Merchant, trainer, flight master and inn steps before the last quest you handed in are done on their own when a guide is loaded or reset, instead of each needing a right click.

### WoW data

- Quest, NPC and spawn data brought up to date, with 51 more contributions from players: WoW Forever gets quests it had wrongly left out or marked unavailable, and corrected XP, races, classes and zones; TBC gets 11 quests it lacked; Classic is unchanged.
- 206 contributed names, titles, levels and objectives checked against public pages and added: Spanish and French translations that were old or literal, minimum levels, a few objectives.

## 0.14.0 (2026-10-21)

> **Important: install the addons from CurseForge (or Wago) now.** The app no longer installs or updates them. Install [Factoruide Guide](https://www.curseforge.com/wow/addons/factoruide-guide) (it brings [Factoruide Profile](https://www.curseforge.com/wow/addons/factoruide-profile) with it) with the CurseForge app, then restart the game. The copy an older app installed stays at its old version: replace it, your settings and progress are kept.

Factoruide in your browser, the addon from CurseForge and set up in the game's options, and a home page.

### App

- The web app: the whole app in a browser, nothing to install; guides are computed on your computer and stay in the browser.
- A home page: what Factoruide does, how it works, and your latest guides.
- Guides shown as boxes, each with its Export: a text to import in game, a file to share, a Polestar guide.
- Export all: every guide in the game at once (the web app downloads them as a folder to unzip in your addons).
- New guide starts from a character saved by the addon; in the web app, paste the character exported in game.
- The Characters and Addon pages are gone: the addon is installed from CurseForge and set up in game.
- A bar says when the addon is missing or out of date, with a link to its page.
- WoW Forever is also found in its new `_forever_` game folder (GitHub #32).
- The steps list starts a new zone where the hearthstone, a flight or a boat takes you, even for a short stop (GitHub #37).
- WoW Forever: a guide's Export gives the text to paste on Polestar's Import page (`/polestar import`); for now Polestar takes only short guides (60 steps), and the app says when a guide is too long.
- Factoruide's own icon in the menu and on the home page.
- The home page shows screenshots of the app and of the addon in game, one after the other.
- WoW Forever: an example guide to look around (human paladin, levels 1 to 30), from the home page or the empty Guides page.
- Manual creation next to Generate: a new guide that starts from your character and the form's settings, built step by step in the editor.
- WoW Forever: the guide map shows Zephras Isle, where the Skyborne start, with the guide's route and steps on it and the game's minimap when zoomed in.
- Web app: opening a guide, its replay and the editor at the same time no longer stops the app or says "database disk image is malformed".

### Addon

- Two addons on CurseForge and Wago: Factoruide Profile (your character, for the app) and Factoruide Guide (which needs it).
- Everything is set in the game's options (AddOns → Factoruide), opened with `/fg`: your character, the guides, every setting.
- Imported guides are listed there: choose the one each character follows, start over, delete; one is picked by itself when it is the only one for your race and class.
- Importing a guide again replaces it, and asks first when the new one is older.
- Pasting a guide no longer freezes the game.
- New key bindings: next and previous step, mark a step done, undo, next spot, show or hide the guide or every window, time per level, the options.
- A guide made by a newer app asks you to update the addon instead of going wrong; a new kind of step it does not know yet is shown and passed on its own.
- The guide's mark on the game's XP bar is gone, and the game's XP tooltip with it back: the guide's own XP bar shows where you should be (GitHub #36).
- The full and minimal guides each keep their own layout: moving or resizing the quest panel or the compass in one mode no longer changes the other.
- In the game's addon list, the Factoruide addons are under Quests, with their own icon, Guide and Data grouped under Profile.
- The guide's XP bars show a hollow gold box over the XP the current steps give, from where the guide expects you before them to after them.

### Guides

- The Great Outdoors is done in the starting camp right after The Adventurer: sitting at the campfire waits for no level (GitHub #35).
- Camping 101 quests offered, optional, for the professions you level.
- Tauren shamans get their Call of Earth quests again (GitHub #33), and Stars of Elune is planned.
- Timed quests are done within their time limit (GitHub #34).
- Quests started by a rare drop are no longer planned as if it dropped often.
- Forever's dungeon quests give the XP the game gives.
- Library books: the book is looted before going to the librarian.
- The Warsong scouts' updates are planned, and Flight to Auberdine uses the free hippogryph.
- Polestar guides wait for a quest to be complete before its turn-in.
- Walks follow the roads, passes and tunnels instead of a straight line over mountains, so guides fly where walking is long (GitHub #37).
- A trip can fly to a port before a boat, or fly on from where it lands; flight paths are picked up the first time the route passes near them (GitHub #37).
- Gold spent on flights counts most up to level 30, when the trainers take most of it, and much less after (GitHub #37).
- Mount Hyjal's flight paths are no longer picked up: the mountain is reached only through Winterspring (GitHub #37).
- In Polestar guides, the steps done around the same place (a town's NPCs, a keep's mobs, within 200 yards, up to 10) make one step (GitHub #39).
- Guides keep room in your bags: they count the loot and quest items you carry, with more room as you level, and sell at a vendor on the way when the bags fill up (a step that ticks when the merchant opens).
- Polestar guides collect an item a quest only uses (Sunscale Feather) without counting it as one of its objectives, and count the others in the game's order.
- Hunters' Taming the Beast quests say to tame the beast, once, instead of killing eight.
- Quests started by an item a rare creature drops (The Collector) are no longer planned as killing that rare: its common droppers at their own chance, or the quest is left out.
- A dungeon is no longer run for none of the guide's quests once you are past its levels (Ragefire Chasm at level 27).

## 0.13.0 (2026-10-18)

An assistant to set up the addon, help finding the game and fixing install problems, light themes, guides that follow your class and gear, and items in limited supply bought where they can be found.

### App

- Two light themes (Settings > Theme): the dark theme's look by day, and an old parchment, a yellowed page scorched at the edges with amber instead of blue.
- Theme "Automatic": dark or light as your system is set, switching with it.
- Advanced settings, Character power: your class's power curve level by level and how much its gear counts, to adjust what the guide has it take on.
- Addon setup assistant: a few questions (how to be guided, the screen, how much automation, advice) set the addon in one go; asked the first time, run again from the Addon page.
- The Addon page in five groups (display, navigation, automation, advice, immersion); an option that depends on one switched off is greyed out.
- First launch: the app looks for World of Warcraft on your computer; when it is not found, a few questions ask where it is and which game version you play.
- A World of Warcraft folder chosen too precisely (a game version's folder, or AddOns inside it) is taken back to the right one: the addon no longer goes where the game does not read it.
- A red bar over every page while the addon cannot go into the game (folder not found, game version not installed, no right to write there) or the game did not load it; an orange one when the addon in the game is out of date, with a button to install it.
- The Addon page tells what the game did with the addon: installed by which version, last seen in game, turned off for some characters, WoW to restart after new files.
- No characters yet: the Characters page names the likely cause.
- Problem reports can join the state of the game and the addon (no account or character name), to help with install problems.
- Guide map: a step chosen in the list, with the arrow keys or the playback bar is shown on the map, even when the map no longer follows the character.
- Linux: an AppImage that updates itself, and the executable alone (#30).

### Guides

- WoW Forever's library books (The Dalaran Digest and the others): the guide sends you to find the book first, then to the librarian, who only offers the quest once you have it (#29).
- Forsaken: A Light in the Darkness for every class, once The Damned is handed in; Hilda's and Ephram's quests after A Righteous Cause.
- The Blackwood Corrupted: the bowl and the three samples first, then the bowl by the bonfire to bring out Xabraxxis and his Talisman.
- What a guide asks of your character follows your class and your gear: plain gear holds you back more the higher you are, good rewards let you go further, warriors wait longer for the hard quests in their twenties, hunters and warlocks take them sooner.
- A timed quest whose time is up is given up where you stand, with no trip to its giver.
- With nothing left to do, the guide grinds to the level a quest's mobs need, not only to the level to take it (#31).
- An item to buy names its vendor, says when it is sold in limited supply (the vendor may be out), and lists other vendors of your side with their zone; your target macro targets them.
- An item in limited supply no longer holds the guide at its vendor: it stays listed along the way until you have it (the turn-in waits for it). The other vendors and auction houses you pass by offer it too, from the quest's level on, even before you have the quest.
- The server's crowd decides where such an item is bought: from the vendor on a quiet server, at an auction house on a busy one (the vendor often sold out, many players selling it).
- At any merchant, the addon buys the items your guide still needs for its quests.
- The map no longer shows far-off places for an objective: The Great Outdoors points to your starting area's campfire, not the other races'.

### Addon

- The same parchment in game ("Addon colors" on the Addon page): the guide and the quest panel on a light page, the compass, the arrow and the marks in amber.
- A library book's step is ticked as soon as the book is in your bags, and the librarian's quest is taken and turned in together.

### WoW data

- Latest quest data: the Forsaken priests' A Lesson in Divinity taken in order, more prerequisites of WoW Forever's new quests, quest NPCs in the right place.
- Discipline (Executor Zygand) only once Vital Intelligence is handed in, as a player checked in game for the "Need help" page.

## 0.12.0 (2026-10-18)

Guides watched and edited on a big map, quests that need others planned whole, camps counted with their real number of mobs, walks round places too strong, a "Need help" page to check things in game, a compass that shows the way (and moves where you want), mobs on the way counted in XP, and guides for Polestar's latest version.

### App

- A guide's page shows it on a big map: play it back in real time or faster, with its steps beside it and your character's level, quests, items, flight paths and hearthstone at each moment.
- Guides are edited on that same page: drag the steps, add from the map the quests left out (in the game's level colors) and the objectives or turn-ins the guide forgets (in red); Ctrl+Z and Ctrl+Y undo and redo.
- The guide line by line is now in the Summary tab of its page.
- Zoomed in, the guide's map shows the game's minimap: roads, buildings and paths instead of the blurry world map.
- Each step has its own line in the list and its own marker on the map, in the guide's order; hover a line for its details. Long guides scroll and edit without slowing the window.
- The guide's map shows the continents side by side, with the boats and zeppelins between them; no continent to pick any more.
- "Need help" page: questions to check in game (does this NPC offer this quest yet?), answered in one click; each answer fixes the guides of every player.
- Guides are made two to three times faster, and come out a little shorter (about an hour on a 1-60).
- The window no longer freezes while a guide opens, two versions are compared or the addon is installed.
- WoW Forever: Polestar can be chosen even when the app does not find it in the game: your guides are installed for it all the same.
- WoW Forever: "Export for Polestar…" on a guide writes it in Polestar's own guide format into a pack folder, to open in Polestar's tools (Planner, Play).

### Guides

- Quests finished by handing in other quests on the way are planned whole: for Bitter Rivals, Jarven gets his Thunder Ale first; Keeping Pace has Zamek's distraction.
- NPCs of your own side are no longer to "kill": Sentinel Shaya is healed, the Lazy Peons woken with the Foreman's Blackjack.
- Walks go round places with mobs far above your level (no more crossing the Western Plaguelands at level 20 on the way to Hillsbrad), and only those really on the way.
- WoW Forever: camps are no longer counted with several times their mobs: guides go back to zones they avoided (Darkshore for the Alliance), and their times count the real waits for respawns (a little longer).
- Starting from your character: the quests already in your log are kept when the guide goes their way, a complete one is turned in on the way instead of abandoned, and none has to be taken again from its giver.

### Addon

- The compass shows the step to go to much more clearly, with its distance even when it is behind you.
- The way the guide planned on foot, around cliffs and water, shows on the compass, the world map and the minimap.
- Trainers of your class and professions, flight masters, innkeepers, vendors, bankers and mailboxes close by on the compass, each kind chosen in the app; a click takes you there.
- A mark above the head of the NPC to go to, once its nameplate shows.
- An optional 3D arrow next to the compass.
- The compass can be moved (drag it), resized (Ctrl + mouse wheel) and widened (drag its ends); /fg compass hides or shows it, /fg lock locks the windows.
- WoW Forever: with Shortest Path Forever installed, it takes you to the step, and a click on the compass changes where.
- Mobs on the way are counted in XP ("1,240/3,000 XP"), and the mobs of a quest objective count again once that objective is done.
- The guide and the quest panel take less of the game's time while you loot or sell.
- Polestar: guides made for its latest version (0.2.5): none refused any more, flight paths in every language, the planned camp first, elites marked "group advised", profession quests only for those who have it.

### WoW data

- WoW Forever: quests the guides took too early now wait for the one the game asks first: Bandarion Keep, Scout Support, Ivar the Foul, Holy Diver, Stop the Spread, The Cult of the Damned, Remnants of War, Farsen's Watch, Horrors in the Highland.
- WoW Forever: quests whose objectives were only described are back: Discipline, A Light in the Darkness, Unwelcome Guests, Gatehouse Goods, Hillsbrad's Hoard, several escorts and deliveries.
- WoW Forever: The Great Outdoors says to sit by the campfire, The Gift of Skysight and Reading the Ley Lines to cast the racial spell; Finding the Source to use Krakle's Thermometer (all versions).
- WoW Forever: mob places recorded on a quest giver where the game has none are gone (Scrawny Ursera in Thendal Village).
- WoW Forever: the undead paladin's letter, A Difficult Path, is offered after The Mindless Ones, as in the game (GitHub #26).

## 0.11.0 (2026-10-17)

A new look for the addon (one card, minimal mode, compass bar, quest panel, 10 languages), farming on the way and respawn waits closer to the game, patrolling NPCs followed along their path, class quests planned for their spell, and windows that move from anywhere.

### App

- Report a problem from the app: on a guide, with the steps concerned ticked in its step table, or about anything else (menu, under "What's new"); a name to be thanked by is optional.
- WoW Forever: the dungeon list shows the new dungeons with quests: Ruins of Lordaeron, Excavation Site: Wetlands, City of Dalaran (GitHub #18).
- Classic and TBC: the dungeon list no longer shows The Hall of Thanes, a WoW Forever dungeon.
- Check for updates again from Settings, under the version, or from the macOS app menu; an update no longer restarts the app over a generation or unsaved edits (GitHub #21). Thanks to @csselement.

### Guides

- WoW Forever: Skyborne guides do Zephras Isle before leaving it by its teleport, instead of leaving at the first step (GitHub #19).
- Class quests are planned for the spell they teach: an undead warlock gets the Imp in Tirisfal Glades instead of going to Durotar for the orc's Vile Familiars (GitHub #22).
- The start zone's letter to the class trainer is always taken (Encrypted Sigil, Consecrated Rune, Simple Parchment…).
- A quest started by a boss's drop is taken during the dungeon run, not before entering (Unending Torment, Ruins of Lordaeron).
- "Speak with" objectives are talks, no longer kills of the NPC.
- Mobs killed on the way no longer include the mobs of the quests in the log: their kills were counted twice, on the way and for the objective (GitHub #20).
- Guides count on killing 80% of the mobs met on the way, fewer on a crowded server; the share is a new setting next to the other farming on the way settings.
- A trip to a grinding spot is a step of the guide even when the mobs on the way there already give the level.
- Steps at an NPC who walks a patrol say so and give places along the path; the addon points at the closest first (Gordo in Tirisfal Glades, GitHub #20).
- Waiting for respawns counts each mob's own respawn time, and crowded servers bring mobs back faster: a lone mob like Meven Korgal no longer costs a quarter of an hour of waiting (GitHub #20).

### Addon

- New look in the app's colors: the guide is one card with the current tasks, progress bars, the next steps and the talent to learn.
- Minimal mode: a strip at the top of the screen, the next steps on mouse-over (`/fg mode`, a key binding or the guide's button).
- A compass bar replaces the arrow: the step to go to, the other current tasks, the quests to pick up close by and the next turn-in; click a point to go there.
- Steps done at the same place are one page, passed together once all are done: a step no longer shows twice; ‹ › and `/fg next` / `/fg prev` go page by page.
- Quest panel: one line per quest (objectives on mouse-over), shown or hidden from the guide, with tabs for the guide's professions and reputation goals (progress bars).
- Items to keep: the common items the guide's quests still need (cloth, meat…), in the bags and in the bank; at the bank, what to take and what can stay.
- The addon speaks the app's 10 languages, in the language chosen on the Addon page (else the game's).
- ElvUI, EllesmereUI and SpartanUI: their style only when "UI suite style" is on (Addon page).
- `/fg hide` hides all of the addon's windows and marks until `/fg show`, for the session (GitHub #16). Thanks to @jdille.
- Right after accepting a quest, its kill or loot step is no longer passed before the game shows the objectives (Kolkar Leaders).
- A kill step is no longer passed when another objective of its quest holding its name is done: Night Web Spider after Young Night Web Spider (GitHub #20).
- Mobs on the way no longer count the mobs the current steps are done on (GitHub #20).
- The windows move when dragged from anywhere on them, their lines and buttons included (GitHub #1). Thanks to @maenorhio.

### WoW data

- WoW Forever: quests of a chain are taken in the order the game offers them (Walton and Vrang in the Barrens, Mankrik's Olgra, the Scourge paladin's quests…).
- WoW Forever: Return to Quinn (Again) only after Wild Eyes, no longer at the same time as Return to Quinn.
- WoW Forever: the dwarf shaman's Call of Water, the night elf hunter's Taming the Beast and the Skyborne and tauren druids' To Thunder Bluff and To Darnassus are back in their guides.
- TBC: the human and dwarf paladin's Tome of Divinity and Tome of Valor, the draenei shaman's Call of Fire and the Horde rogue's Hinott's Assistance (Poisons) are back in their guides.
- WoW Forever: Beer Basted Boar Ribs is offered without Cooking, as in game.
- WoW Forever: Secure the Mountain (Dun Morogh) is back in Alliance guides: Mountaineer Cornelius is found in the Grizzled Den.
- Quests whose place was unknown are back with the rest of their chain: Overmaster Pyron, Glyph Chasing, Jail Break!, Elemental Bracers (Classic); WoW Forever: Testing the Wells, From the Ashes, Accessories of the Lost, Key to the City, Bloom of the Heavens, Blackrock Blockade, Vengeful Trail; TBC: Ar'tor's Charge, Nat's Bargain, Imbuing the Headpiece.
- Quest data refreshed, with the players' latest contributions.
- WoW Forever: game data read again from the latest client update (1.60.1.70425).

## 0.10.0 (2026-10-15)

Polestar integration on WoW Forever, reputation goals, forced zones, and anonymous contributions.

### App

- WoW Forever: Polestar can show your guides instead of the Factoruide addon (Addon page).
- Reputation goals: pick factions and the standing to reach by the end of the guide (GitHub #15).
- Forced zones: every quest of the zone is done, even past the guide's target level (click the zone on the map, or ⚑ in the list).
- Contribute page: send what your game saw to the Factoruide server, anonymously, by hand or once a day.
- Guide editor: grinding over several camps no longer fails.

### Guides

- A quest started by an item a mob drops names the mob to kill.
- Mobs living in several places: the guide knows up to five other places to find them.
- Required class quests still in the log at the guide's end are now turned in (the druid's Body and Heart).

### Addon

- The addon is now two addons, Factoruide_Profile and Factoruide_Guide. Installing from the app replaces the old one and keeps your data; restart WoW once.
- The arrow moves on to another place when a mob's place is empty (or at once with `/fg spot`).
- WoW Forever: the target macro no longer marks the target with a skull (the game blocked it with a popup).
- Records your reputations, and what helps guides match the game (fights, loot, time per step): about 30 KB per hour of play, dropped once sent.
- Classic Era and TBC: quests taken and turned in are recorded right.
- WoW Forever: no error when the game hides a target's level.

### WoW data

- WoW Forever: about 720 more quests have their XP, and 100 have WoW Forever's own value: less grinding.
- WoW Forever: mobs Forever made elite or rare are avoided like other elites.
- WoW Forever: class quests teaching a spell are in their class's guides (Holy Light for undead paladins, Taming the Beast, totems...) (GitHub #17).
- WoW Forever: the Skyborne get their side's race-restricted quests; the library books' quests are in the guides.
- TBC: quest items drop at the rate of a player on the quest: fewer kills planned.
- Creatures and objects seen by the game client fill names and ranks missing elsewhere.

## 0.9.0 (2026-10-14)

Guides closer to the game: shorter walks, real drop rates, grinding at camps, and a comparison of a guide's versions.

### App

- Compare two versions of a guide side by side ("Compare" in its versions).
- Characters page: a character deleted in game can be removed.
- Smaller app: about 38 MB lighter to download, 60 MB less on disk per game version.
- Windows: the What's new page displays correctly again.
- Guide editor: two fixes (off-road shortcuts, objectives in several places).

### Guides

- Walks were counted about 30% too long: 1-60 guides are 9 to 12 hours shorter.
- Walks follow the terrain: rivers swum, the open sea avoided, cliffs jumped down but never climbed.
- No more walking over the mountains between the south and north of the Eastern Kingdoms: tram, flight or boat instead.
- Off-road shortcuts are steps of their own ("Take the shortcut: ...").
- Grinding happens at a camp of mobs of your level, shown on the map.
- Preferred zones weigh on the whole guide; excluded zones are no longer used for turn-ins or the hearthstone.
- Class quests teaching a spell are in by default (Redemption, pets, totems, forms...): untick those you do not want.
- No more quests taken, half done and abandoned hours later.
- Cloth donations and quests asking for common loot are in the guides: what earlier kills dropped counts.
- Quests needing an item another quest hands are in the guides (The Green Hills of Stranglethorn).
- Mobs killed on the way are those along the path really walked.
- Quest XP is the game's own when your game has seen the quest.
- WoW Forever: mobs below your level give the XP the game gives.

### Addon

- Abandon steps show a red cross and the quest's name.

### WoW data

- Quest items drop at their real rate, not a placeholder 80%: guides count the kills quests really take.
- The Feathermoon ferry (Feralas) is in the guides.
- WoW Forever: the new boats are in the guides (Stormwind Harbor, Menethil Harbor's loop, Steamwheedle Port).
- WoW Forever: the Dalaran to Stormwind portals are for the Skyborne only.
- WoW Forever: class quests of the new race and class pairs stay with their class (a troll hunter no longer gets the orc mage's quests).
- WoW Forever: the undead paladin's Redemption quests are done in order; The Adventurer is taken after The Red Messenger.
- More class quests teaching a spell are required: priest, druid forms, warrior's Defensive Stance, and TBC paladins, hunters and warlocks.

## 0.8.0 (2026-10-14)

The app on Mac, a new look, a Characters page, and class quests planned again.

### App

- macOS: the app now runs on Mac (Apple Silicon and Intel), with automatic updates. Thanks to @csselement.
- New look: a night-blue theme, explanations in tooltips, readable at the keyboard (visible focus, stronger contrast). The light theme is gone.
- Characters page: each recorded character with its full name, level and professions, and the guide it follows. A character follows only the guide chosen for it in the app.
- No character shown? The Characters page tells why: wrong game version chosen, addon not installed, or character not recorded yet (log in, then /reload, log out or quit).
- New guide: character, professions, dungeons by level and zone map on one screen; profession skill set on a curve.
- Advanced settings as sliders with their default marked, a search, and folded expert sections.
- Guides: every action on each row, "Clone" to start a new guide from the same setup, gold spent on flights.
- Addon page: settings saved as you change them; "Install in game" appears when the game is not up to date.
- Contribute page: the tools that send what your game sees to the community quest databases.
- What's new page, and links, licenses and data notice under Settings.
- Guide editor: leaving it with changes not saved (another page, another game version) asks first.
- New guide: each game version keeps its configuration; switching back finds it as you left it.
- Faster start and game version switch (the dungeon list took seconds), the window stays responsive meanwhile.

### Guides

- Fixed: camps around a mob of unknown level are counted right.
- Quests needing an item first now send you to get it: Marla's Last Wish has you kill Samuel Fipps before burying his remains.
- Night elves: Rut'theran Village and the top of Teldrassil are linked by the portal, no longer walked between.
- TBC: portals and orbs no longer count a boat's wait.
- Druids: the bear form quests are planned again. Moonglade is reached with the teleport the quest "Moonglade" teaches, and Lunaclaw is met at the right Moonkin Stone (the Barrens for the Horde).
- Warlocks: The Binding (Voidwalker, Succubus) is planned again, at the summoning circle.
- WoW Forever: class quests filed under a zone count as class quests: the dwarf shaman's Call of Earth, and Archaic Rune before it, are always in the guide.
- Classic and WoW Forever: Blackrock Depths, Blackrock Spire and the Sunken Temple are in the dungeon list again.

### WoW data

- In Search of Thaelrid is no longer offered to the Horde.

## 0.7.1 (2026-10-13)

- Addon on WoW Classic and TBC: the guide now shows, and quests are accepted automatically.

## 0.7.0 (2026-10-13)

A security fix (updating is recommended), and better guides.

### Security

- The addon's saved data is now read as plain data: a tampered file can no longer run anything on your PC.
- A tampered file can no longer freeze or crash the app either.

### Guides

- Better guides: shorter on average, by tens of minutes on a 1-60.
- Short guides (a few levels) are now optimized as carefully as long ones.
- The same settings now give the same guide on any PC; only the computing time differs.
- The calculation presets (Preview, Normal, Thorough) set how hard the app searches.
- Faster calculations.
- Fixed: some starting routes ended with a full quest log.
- Fixed: guides asked for too many of some items or mobs (6 sacks instead of 1, Henze Faulk killed 8 times).

### WoW Forever data

- Tauren guides start in Mulgore again, instead of walking to Durotar at level 1.
- Human guides start in Northshire again, instead of going through Ironforge to the dwarf start.
- Both came from vendors and trainers taken for hostile monsters.

### App

- The calculation screen shows its progress, and the best route so far on the map.

## 0.6.0 (2026-10-12)

The guide editor, and a much faster optimizer.

### Guide editor

- "Edit the steps" on a guide's page: move, remove or add steps by hand.
- Steps you can add: quests, hearthstone, flights, flight paths, dungeons, mobs to kill.
- After each change the guide is replayed: its time, and each step's time, level and XP.
- Impossible steps are shown with the reason, and block saving.
- Saving makes a new version of the guide.
- Each step does one thing: flights, training and grinding are steps of their own.
- A quest step can also kill every mob met on the way to it.
- The zone map beside the steps follows the pointer.

### Guides

- Much faster optimization, using less memory.
- Every quest taken, done or turned in is its own step, so it can be edited.
- Objectives at the same place come one after the other.
- Fixed: a generation that hit an internal error never ended.

### App

- New guide in two steps: setup, then generation; the guide then opens in "My guides".
- Fixed: on Linux with an NVIDIA card under Wayland, the window closed at start.

## 0.5.0 (2026-10-11)

Better use of the hearthstone, mage teleports and Astral Recall, timed quests, and no more addon popups.

### Guides

- Objectives in several places: the guide picks the closest worthwhile one.
- The hearthstone is set again as you move on, at the best inn.
- The hearthstone is used where it saves the most time.
- Mage teleports and the shaman's Astral Recall are used.
- No more grinding for a quest you are not strong enough for yet.
- No more "mobs" to kill in capitals.
- Timed quests are turned in on time ("timed: turn it in within 60 min").
- Fixed: a grind step could land between a dungeon and its quests.

### Addon

- No more "blocked action" popups.
- The guide's buttons have their own bar, with tooltips.
- The XP bar shows where the current tasks should take you.
- Mobs to kill on the way have their own line, with a counter.
- The box under the guide has two tabs: "Next" and "To pick up".
- Start over also undoes steps marked done by hand (`/fg restart`).
- The quest tracker follows the guide, not the game's quest log.
- "Only the guide's quests" no longer accepts quests the guide gave up.

### WoW Forever data

- Fixed wrong positions for 179 quest NPCs.
- Added 59 missing prerequisites: no more guides sending you to a quest you cannot take yet.

## 0.4.0 (2026-10-11)

Stop or cancel a generation, a new guide page, dungeon quests step by step.

### Generation

- "Stop and keep this guide" ends the calculation and keeps the best guide so far.
- "Cancel" goes back to the setup without saving.
- The chosen computing time is now all spent on optimizing.
- Paths around mountains are remembered, so the next guides are faster to compute.

### Guide page

- All steps in one table by zone, with filters and colors.
- The zone map beside the table on wide windows.
- A cleaner guide map, colored from start to end.
- Easier-to-read figures.

### Setup and settings

- A clearer setup screen.
- Advanced settings changed from their default are marked, with a reset button.
- Addon tab: the save bar stays visible.
- Settings: a "Browse…" button for the game folder.
- Dates and numbers follow the app's language.

### Dungeons

- Quests done in a dungeon are steps of their own.

### Addon

- New "To pick up" box: the quests you can take now, with their distance.
- Left-click a step to point the arrow at it.
- The quest tracker shows the next 10 quests in the guide's order.
- A yellow mark on the XP bar shows where the guide expects you.
- Grind steps count the XP of quests ready to turn in.

### Guides

- A catch-up step when you fall more than 10% of a level behind.
- Groups of mobs are harder than lone ones; the wait for respawns is counted.
- "Crowded server" becomes a slider.
- Killing mobs on the way has its own limit per level.

### Fixes

- WoW Forever, Redridge: Blackrock Blockade is no longer abandoned too early.

## 0.3.0 (2026-10-11)

Guides in every language, and guides started from your character.

### Languages

- Guides follow the app's language; the addon page picks the language in game.
- Guide texts in 10 languages.
- Older guides keep their texts: generate them again to translate them.
- Older guide files are updated when opened; newer ones ask you to update the app.

### Starting from your character

- Quests already in your log no longer have to be finished: the guide keeps the useful ones.

### Fixes

- Objects among stronger mobs wait until you are strong enough.
- Group: a companion whose class your race cannot play still gets their class quests.
- App: the window no longer stays scrolled down after ticking a box.

## 0.2.0 (2026-10-10)

Personalized leveling guides for WoW Forever, Classic Era / Hardcore and TBC Anniversary.

### Character power

- Fights depend on your level, gear and class spells.
- The guide wears quest rewards and dungeon loot when they are upgrades.
- The addon picks the reward the guide planned.
- Training happens when the new spells are worth it, and only for useful spells.
- Each step shows your power and what your gear brings.

### What the character takes on

- New play styles: cautious (hardcore), normal or risky.
- Cautious avoids groups of mobs above your level.
- Normal does yellow quests, never orange; risky takes orange ones and some elites.
- Elite camps need more power the more elites you pull.
- No more quests started by a mob far above your level.
- Quest NPCs inside a camp wait until you can fight through it.
- New "Crowded server" option: the guide counts the wait for contested mobs and events.

### Grinding

- New "Grind on the way" option: kill mobs met on the way, near your level.
- Grinding is limited to a share of each level (10% by default).
- New "Grinding allowed" option: without it, the guide takes more quests.
- Quest objects among other quests' mobs are picked up between kills.

### Start from your character

- The addon keeps a profile of your character (`/fg profile` saves it at once).
- New guide: pick one of your characters; race, class and level follow.
- The guide goes on from where you are: quests done, log, hearthstone, flight paths.
- Rested XP is counted.
- Flight paths are known once you open a flight master's map in game.

### Professions

- Professions no longer add time or steps.
- The addon tells you if your skill is ahead or behind for your level.
- The profession curve starts when you take it.
- Profession quests are suggested as optional.

### Checkpoints

- Checkpoints from level 20: ahead of plan, skip to the next one.
- Behind plan, grind before going on: "be level 19 and 93%".

### Quests

- No dungeon quests without dungeons, no battleground quests unless asked.
- Quests are taken and turned in together at quest hubs.
- A zone is done in one go.
- Walks go around mountains and cliffs.
- Mobs and objects of quests in the log are done on the way.
- Pet and form class quests are done early.
- No more quests started by rare drops, nor gray quests.
- Fewer trips for just one or two quests.
- Fixed: Deeprun Tram quests, Frostmane Hold, the paladin's Tome of Divinity.

### In the app

- During generation, the best routes and their curves are shown live.
- The calculation keeps to the time chosen.
- My guides: class colors, faction emblems, sortable columns.
- The addon accepts only the guide's quests by default.

### Addon look

- The addon takes the game's look, or the style of ElvUI, EllesmereUI or SpartanUI.
- New "Look" option: automatic, game style or dark.
- A big arrow points to the current step.
- Ctrl + mouse wheel (or `/fg scale 1.3`) resizes the windows.

### Fixes

- Fixed quests taken or turned in on the way too early, or dropped at the end.
- Group: every class's required class quests are planned.
- Addon: quests marked done by hand belong to their guide only.
- Addon: opening the world map no longer raises a Lua error.
- TBC: correct XP in the blood elf and draenei starting zones, and in Outland.
- A dropped quest no longer opens its follow-up.
- Dungeon mobs give the right XP when you outlevel them.
- Correct flight prices; only real flight paths are shown.
- Forever: quests with unknown objectives are left out.
- The hearthstone takes a new character back to its starting area.
- Hearthstone, flight and boat steps name the town and show their own time.
- Ratchet and the Crossroads flight paths are in the Barrens.
- The time shown during the calculation is the guide's play time.
- Mobs on the way are no longer counted twice.
- The guide windows always grow downwards.
- Objectives in progress stay in the guide until done.
- Quest names fall back to English.
- Updates are checked only in published builds.
