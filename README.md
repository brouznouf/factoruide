# Factoruide

**A leveling guide built for you**: your character, your professions, your zones, your dungeons
and the way you play. For **WoW Forever**, **Classic Era / Hardcore** and **TBC Anniversary**.

Leveling guides follow one route written for everyone. Factoruide computes yours: it simulates
the whole playthrough with your choices (travel, fights, drops, quest and mob XP, trainers,
professions, and how strong your character gets with its gear and spells) and searches for the
fastest route to your target level, killing the mobs met on the way and grinding only a small
share of each level. The in-game addon then takes you through it step by step.

![A guide's page: its route on the map with the level reached at each stage, its play time, quests, dungeons, flights and XP](docs/guide.png)

## An experiment

Factoruide is experimental. A guide comes from a simulation of the game, and the simulation is
not the game: it can produce inconsistencies (a quest you cannot get, a detour that makes no
sense, a time far from yours). When you meet one, please
[open an issue](https://github.com/brouznouf/factoruide/issues) with your guide attached: the
"Share" button of its page saves it as a `.fgguide` file.

It will never replace the guides written by hand, and that is not the goal: their authors play
their route again and again, and a good one will most likely be faster. Factoruide builds a
guide of your own that holds together as a whole and gives your leveling real directions: which
zones to do, the level to reach before moving on from each, and where the XP is.

## A guide that fits you

- **Your character**: race, class and level range: start a guide at level 1, or from your
  character as it is in game. The addon records it at each logout (or at once from its page of
  the game's options: **Save the profile now**): pick it in the list of the new guide and the guide goes on from where it
  stands: its quests done are left out, those in its log are taken again when they help (as if
  never taken) and abandoned as the first step otherwise, from its position, with its
  hearthstone, flight paths, rested XP, gear, spells and professions.
- **Your professions**: two primary professions and the secondary ones, with the skill to have
  at each level from the level you take them (curves from classic guides, editable). They add no
  time to the route: the addon shows whether you are ahead or behind, and their quests are
  suggested as optional steps.
- **Your zones**: click the map to prefer the zones you like and exclude the ones you don't want
  to see again.
- **Your dungeons**: force or exclude each dungeon, do the worthwhile ones in a group, take their
  quests a few levels ahead.
- **Your quests**: add a must-have quest by ID or exact name. The guide includes its
  prerequisites and finishes the chain, even past your target level. It cannot be saved with
  a required quest missing. For the Sleeping Bag in Forever, add **79976** (**This Must Be The
  Place**); its XP bonus is not simulated.
- **Your class spells**: quest rewards and trainer visits determine which spells are known.
  Form attacks need both the form and its learned attack rank. Imported characters use their
  recorded spells and completed quests. Without a character profile, earlier class unlocks
  are assumed known; selecting class quests does not change that starting assumption.
- **Your class quests**: the ones you want to do are kept, even when they are not the fastest.
- **Your play style**: a relaxed, normal or fast pace (time per fight, per loot, per stop), how far
  above your level you take quests and fight mobs, how much grinding you accept, flights or
  walking to save gold, finishing a zone before leaving, staying on one continent, elite quests
  solo or in a group, how crowded the server is (escorts, events and small spots fought over).
- **Your group**: level in a duo or a group with other classes: faster fights, shared XP with the
  group bonus, quest items for everyone, each class's quests and trainers.
- **Your language**: the app and the guide in 10 languages. A guide is computed once and shown in
  the app's language; the addon writes it in the language you choose in its options.

![A new guide: character, professions, play style, dungeons by level and the zones to prefer or exclude on the map](docs/new-guide.png)

## It adapts as you play

- The addon follows you: it advances by itself (quest accepted, objective done, quest turned in,
  flight, level reached), points the compass at the next step and compares your real time per level
  with the guide's (**Time per level** in its window).
- Ahead, behind, changed your mind about a profession or a zone? Generate the guide again from your
  character as it is now: it becomes a new version of the guide, with the difference from the previous
  one, and you choose the version its characters follow.
- The guides learn from the game: the XP and positions the addon records replace the estimates,
  and the "Contribute" page sends what your game sees to the Factoruide server (anonymous, only what
  is new, automatically if you turn it on) and lists the other tools that feed the community's quest
  databases.
- Rework a guide by hand: move, remove or add steps (a quest, the hearthstone, a flight, mobs to
  kill), the guide replayed after each change with its new time and the steps that are no longer
  possible; saved as a new version once every step holds. Or write one from scratch: "Manual
  creation" on the New guide page starts a guide with no steps from your character.
- Share a guide with a friend as a `.fgguide` file; they import it in their app.

![The characters the addon recorded, each with its level, professions and the guide it follows](docs/characters.png)

## Getting started

1. Download the app from the [latest release](https://github.com/brouznouf/factoruide/releases/latest)
   and install it. New versions are then offered by the app at startup.
   - **Windows**: run the `_x64-setup.exe` installer. Windows may warn about an unknown publisher
     the first time (the installer is not code-signed).
   - **macOS**: open the `_aarch64.dmg` (Apple Silicon) or `_x64.dmg` (Intel) and drag the app to
     Applications. The app is not notarized by Apple, so macOS refuses to open it the first time:
     go to System Settings > Privacy & Security and click "Open Anyway", or run
     `xattr -dr com.apple.quarantine /Applications/Factoruide.app`. Updates do not ask again.
   - **Linux** (x86_64): make the `_amd64.AppImage` executable (`chmod +x`) and run it; it updates
     itself. Or take the executable alone (`_amd64.tar.gz`), which needs WebKitGTK 4.1 installed
     (`libwebkit2gtk-4.1-0` on Debian/Ubuntu, `webkit2gtk4.1` on Fedora, `webkit2gtk-4.1` on Arch).
2. **Settings**: choose your WoW folder. Install the addon from
   [CurseForge](https://www.curseforge.com/wow/search?search=factoruide) (or your addon manager)
   and set it up in game: `/fg` opens the addon's page in the game's options (AddOns →
   Factoruide). The app warns when it is missing or too old.
3. **New guide**: pick your character, professions, zones, dungeons and play style, then start
   the generation (about 2 minutes, 6 for a thorough one, depending on the processor). The best
   starting routes are picked first, then improved in rounds where the better half goes on: their
   times go down live on the chart and the route is drawn on the map. The
   guide kept (in bold) is the best balance between play time and comfort (little grinding, few
   trips between zones), not always the very shortest.

   ![Generating a guide: the time of the guide kept going down live, and its route drawn on the map](docs/generation.png)
4. **Characters**: choose the guide each character follows (a guide started from a character
   follows it already), then `/reload` in game. A character follows only the guide chosen for it.
   The app writes the guides in a data addon of its own, `Factoruide_Data` (restart the game the
   first time, so that it finds the new folder).

### App updates

Published builds check for a new version at startup and offer it with its notes. To check
again, use **Check for updates** in Settings, under the version (on macOS also
**Factoruide → Check for Updates…**). Installing waits for a running generation to end.

### Building on macOS

Install Xcode Command Line Tools (`xcode-select --install`, or use a full Xcode
installation), Node.js 22 and npm, and Rust via [rustup](https://rustup.rs/).
The repository's `rust-toolchain.toml` selects the Rust version. Then, from a clone:

```sh
./scripts/build-macos.sh
open target/release/bundle/macos/Factoruide.app
```

The script installs the locked frontend dependencies, builds a native app for the
Mac running the build, and applies and verifies a local ad-hoc signature. No Apple
developer certificate or publisher updater key is needed. You can copy the app to
Applications. This is a local build, not an Apple-notarized distribution, and it
does not check for the publisher's automatic updates.

The app defaults to `/Applications/World of Warcraft`; choose another installation
in **Settings** if needed. Follow the addon and guide steps above.

Verified on Apple Silicon with WoW Forever: native app launch, character profile
import, guide generation and user-reported in-game use. Intel Macs have not been
verified.

### Building on Linux

Install WebKitGTK 4.1 and the build tools (on Debian/Ubuntu: `libwebkit2gtk-4.1-dev
libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev patchelf file build-essential`),
Node.js 22 and npm, and Rust via [rustup](https://rustup.rs/). Then, from a clone:

```sh
./scripts/build-linux.sh
./target/release/factoruide-app
```

The script builds the executable and an AppImage in `target/release/bundle/appimage`. Local
builds do not check for the publisher's automatic updates. Outside WSL, the app defaults to
Battle.net in Lutris' Wine prefix (`~/Games/battlenet/drive_c/Program Files (x86)/World of
Warcraft`); choose another folder in Settings if needed.

### In a browser

The app also builds as a website: a folder of static files where the guides are computed in
the browser (`scripts/build-web.sh`). The browser cannot reach the game's folder: characters
come from the game with **Export my character** in the addon's options (pasted in New guide), and
guides go to it with **Import a guide…** (the text a guide's Export gives). See
[web/README.md](web/README.md).

### Releases

`scripts/release.sh X.Y.Z` tags main and pushes the tag; `.github/workflows/release.yml` then
publishes everything under that one version: the desktop apps (GitHub release and updates), the
web app (Cloudflare Pages) and the addons. An addon is published (CurseForge, Wago and a zip in
the release) only when it changed since the last release: the script gives it the new version
first, and the apps built from the tag ask for that version at least. Each addon has its own
`CHANGELOG.md` (what changes for players, its changelog on CurseForge and Wago): new lines go under
Unreleased, which the script dates with the version, and it stops when a changed addon has none.
`scripts/publish-addons.sh X.Y.Z [addon] [beta|alpha|release]` publishes the addons of main
outside a release, to try them. A guide made by an older
app stays readable by a newer addon: `addon/tests/guides` keeps one guide per release, and the
addon's tests follow each of them.

## In game: the addon

The addon comes in two parts, installed from CurseForge: **Factoruide_Profile** records your
character and what the game shows (profile, collector below; **Export my character** gives it to
the web app), **Factoruide_Guide** shows the guide and everything around it. Each can go without
the other: another addon can show your guides. `/fg` (or `/factoruide`) opens the addon's page in
the game's options (AddOns → Factoruide): the character (**Export my character**, **Save the
profile now**) and the guides (choose the one each character follows, start over, import, delete),
then a page per group of settings and an Advanced page (the recordings). What you do while
playing is on the guide's buttons and in the game's key bindings (Factoruide section): next and
previous step, mark a step done, undo the last mark, next spot, full or minimal guide, show or
hide the guide or all the addon's windows, time per level, quest item, the options.
On WoW Forever with [Polestar](https://github.com/fitzforge/polestar) installed, the app also
writes your guides as a Polestar guide pack (Factoruide_Polestar; the web app downloads it). In Polestar, choose Factoruide
under "Suggest guides from" on the "Community guides" page: it offers each character the guide of
its race and class. **Export for Polestar…** on a guide writes it as Polestar's guide source (a
`.guide` file, in English) into a pack folder for Polestar's own tools, which its Planner and Play
open: `Factoruide_Polestar_Pack` in the folder you choose (or the folder itself when it is that
pack). Exporting another guide adds its `.guide` file to the pack and keeps the others; exporting
a guide again replaces its file. `fg polestar --source --out <dir>` writes such a pack from the
planned routes.

- Guide: follows the guide chosen for the character (in the options' guides; the only one of
  its race and class by itself) and advances by itself; previous and next step, start over on the
  guide's buttons, mark a step done with a right click or a key binding. Two modes: full (a card with the current tasks, the next steps and
  the talent to learn) or minimal (a strip at the top of the screen, the next steps on
  mouse-over); a button or a key binding switches. Mobs living in several places: the compass
  moves on to another place when nothing is found at one (**Next spot**, a key binding, at once), and the world
  map shows them all.
- **Show or hide the guide** (a key binding, or the setting) shows or hides the guide only;
  **Show or hide all the addon's windows** (a key binding) hides all of its windows and marks
  (guide, compass, quest panel, quest item button, map pins and waypoint, the
  upgrade line of item tooltips, safety alerts) and the same key brings them back as they were.
  For the session only: progress is kept and the automation goes on; pressed in combat, it
  applies when the fight ends.
- Windows move by dragging them anywhere; **Lock the windows** (a setting) locks them. Size:
  Ctrl + mouse wheel over a window, or the window size setting.
- The compass: drag it to move it, Ctrl + mouse wheel over it for its own size, drag its ends for
  its width (kept per character); a setting hides or shows it, **Compass back to the top** puts it
  back at the top of the screen at its first size. In minimal mode it sits on the strip and moves
  with it.
- Helpers: a compass bar at the top of the screen with the step to go to (it stands out, with its
  distance, even behind you), the other current tasks, the quests to pick up close by, the next
  turn-in and the places you choose in the app (your class trainer, your professions' trainers,
  flight masters, innkeepers, vendors, bankers, mailboxes...; mouse over: what and how far; click:
  go there). A green mark shows the way the guide planned on foot, around cliffs and water,
  orange when you stray from it; the same way is dotted on the world map and the minimap (an
  option), and a 3D arrow can follow it next to the compass (another option). When the NPC to go
  to shows its nameplate, a mark sits right above its head. On WoW Forever, [Shortest Path Forever](https://www.curseforge.com/wow/addons/shortest-path-forever)
  (when installed) gets the place to go to and guides you there with flights, boats and its own
  map lines; a click on the compass changes its destination. Targeting macro "FG Cible"
  (**Pick up the target macro**), quest item button (key binding in the game's
  key bindings, Factoruide section), the step's flight taken automatically, next steps numbered on
  the world map.
- Vendors: sells gray items, repairs, buys food, water and ammunition for your level (disabled by
  default; hold Shift when opening to skip everything).
- Gear and talents: the quest reward the guide planned (the route counts on wearing it), upgrades
  in tooltips and on quest rewards, the recommended talent in the guide. At the class trainer,
  only the spells that matter for leveling are bought.
- Quest panel: one line per quest in the guide's order, tabs for the guide's professions and
  reputation goals, and the items to keep for its quests (cloth, meat…) with how many are in the
  bags and the bank (read at each visit; at the bank it says what to take and what can stay).
- Group and safety: party members' progress in the quest panel, alerts on elite or high-level
  targets and low health, elite objectives flagged in the guide.
- Profile: what the app needs to start a guide from your character (level, XP, quests done and in
  the log, position, hearthstone, flight paths seen on flight maps, professions, gear, spells),
  kept up to date every minute and at logout; **Save the profile now** writes it to disk at once
  (the interface reloads).
- Collector: records what you see (givers, positions, XP received) for the next guides; its
  events, debug messages and clearing are in the window's Advanced section.

## How it works

How the optimized route is computed: [ARCHITECTURE.md](ARCHITECTURE.md).

## License

The code is under the MIT license ([LICENSE](LICENSE)). The quest databases (`data/`) are under
GPL-3.0, the license of the server data they include ([data/LICENSE](data/LICENSE));
their sources and the terms of each are listed in [NOTICE.md](NOTICE.md).
