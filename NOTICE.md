# Notice: data sources

The code of this repository is under the MIT license ([LICENSE](LICENSE)); the quest databases
are under GPL-3.0 ([data/LICENSE](data/LICENSE)), the parts below keeping their own
terms.

The quest databases in `data/<edition>.sqlite.zst` are compiled from the sources below. Each row
of the databases keeps the sources it comes from (`sources` columns, `import_run` table).

| Source | License | What the databases take from it |
| --- | --- | --- |
| World of Warcraft game client (Blizzard Entertainment) | Blizzard's | Maps and map images, zones, flight paths, races, classes, quest IDs and XP, names. World of Warcraft and Blizzard Entertainment are trademarks or registered trademarks of Blizzard Entertainment, Inc. This project is not affiliated with or endorsed by Blizzard. |
| [QuestieDB](https://github.com/Questie/QuestieDB) and [Questie](https://github.com/Questie/Questie) | None stated (see below) | Quests, givers and turn-ins, prerequisites, objectives, NPC/object/item data and positions, quest XP, drop rates, dungeon entrances, translated names, quest blacklist |
| [VMaNGOS](https://github.com/vmangos/core) | GPL-2.0 | Classic 1.12 world database: exclusive quest groups, loot tables and chances, NPC titles, positions, objective amounts |
| [CMaNGOS TBC database](https://github.com/cmangos/tbc-db) | GPL-3.0 | TBC 2.4.3 world database: the heights of creature and object spawns on the old continents and Outland, which tell the floor a place stands on |
| [AllTheThings](https://github.com/ATTWoWAddon/AllTheThings) | MIT | Forever quests: classes and races, givers and positions, prerequisites, exclusive quests |
| [pfQuest](https://github.com/shagu/pfQuest) | MIT (see below) | Its database, extracted from the VMaNGOS (Classic, Forever) and CMaNGOS (TBC) servers: quests, givers and turn-ins, prerequisites, objectives, NPC and object data and positions, drops and their chances; used only where no other source has the data (TBC: its quest item drop chances) |
| [Wowhead](https://www.wowhead.com) tooltips | Wowhead's | Whether quests exist in Forever, names, levels, objective texts, NPC titles and positions; Questie's drop rates also come from Wowhead |
| [EG Link output: World of Warcraft: Forever](https://github.com/JIVESCORP/eg-link-output-wowf), data from [Efficient Games](https://efficient.games) | CC BY 4.0 (see below) | Forever only: the quest data of players' game caches (XP rewards, objective amounts, races, levels, zones) |
| Factoruide players | — | Quests and positions observed in game, sent through the app and accepted only when several trusted players agree |

Used to build the databases but not included in them: [WoWDBDefs](https://github.com/wowdev/WoWDBDefs)
(client table definitions, CC BY-SA 4.0) and [TACTKeys](https://github.com/wowdev/TACTKeys).

## Questie

Questie and QuestieDB have no license file. Their maintainers explain in
[Questie#4447](https://github.com/Questie/Questie/issues/4447) that the contributors' consent to a
license could not be gathered, that the project should be considered "all rights reserved", and
that anyone accepting that risk may use it as they please. Factoruide credits Questie here and
will remove its data if its maintainers ask for it.

## VMaNGOS

The VMaNGOS data is distributed under the terms of the GNU General Public License, version 2
(<https://www.gnu.org/licenses/old-licenses/gpl-2.0.html>). It comes from the VMaNGOS
`db_latest` release (SQLite snapshot, <https://github.com/vmangos/core/releases>);
the build keeps the rows up to patch 1.12 and merges them with the other sources.

## CMaNGOS TBC database

The CMaNGOS TBC data (`tbc-db`) is distributed under the terms of the GNU General Public
License, version 3 (<https://www.gnu.org/licenses/gpl-3.0.html>). It comes from the `latest`
release of <https://github.com/cmangos/tbc-db> (SQLite snapshot); the build keeps only the
height of its creature and object spawns. Because of it, and since VMaNGOS's terms allow "any
later version", the databases as a whole are distributed under the GPL version 3
([data/LICENSE](data/LICENSE)).

## EG Link output

Data from Efficient Games (<https://efficient.games>), licensed under CC BY 4.0
(<https://creativecommons.org/licenses/by/4.0/>), from
<https://github.com/JIVESCORP/eg-link-output-wowf>. Changed: only the quest caches
(`quests.<locale>.json`) are read, the newest client build's record of each quest is kept, its
XP reward is computed with the game's XP table, and the values are merged with the other sources.

## pfQuest

```text
MIT License

Copyright (c) 2017-2021 Eric Mauser (Shagu)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## AllTheThings

```text
MIT License

Copyright (c) 2026 AllTheThings WoW Addon

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Addon libraries

The addon bundles, in `addon/Factoruide_Profile/Libs/` and `addon/Factoruide_Guide/Libs/`:

- [LibDeflate](https://github.com/SafeteeWoW/LibDeflate) 1.0.2, (C) 2018-2021 Haoqian He, under
  the zlib license (kept in `LibDeflate.lua` and `LICENSE.txt`): compresses the texts copied
  between the game and the web app.
- [LibStub](https://www.wowace.com/projects/libstub), in the public domain: lets the addons share
  one copy of LibDeflate.
