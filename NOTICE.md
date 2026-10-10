# Notice: data sources

The code of this repository is under the MIT license ([LICENSE](LICENSE)); the quest databases
are under GPL-2.0-or-later ([data/LICENSE](data/LICENSE)), the parts below keeping their own
terms.

The quest databases in `data/<edition>.sqlite.zst` are compiled from the sources below. Each row
of the databases keeps the sources it comes from (`sources` columns, `import_run` table).

| Source | License | What the databases take from it |
| --- | --- | --- |
| World of Warcraft game client (Blizzard Entertainment) | Blizzard's | Maps and map images, zones, flight paths, races, classes, quest IDs and XP, names. World of Warcraft and Blizzard Entertainment are trademarks or registered trademarks of Blizzard Entertainment, Inc. This project is not affiliated with or endorsed by Blizzard. |
| [QuestieDB](https://github.com/Questie/QuestieDB) and [Questie](https://github.com/Questie/Questie) | None stated (see below) | Quests, givers and turn-ins, prerequisites, objectives, NPC/object/item data and positions, quest XP, drop rates, dungeon entrances, translated names, quest blacklist |
| [VMaNGOS](https://github.com/vmangos/core) | GPL-2.0 | Classic 1.12 world database: exclusive quest groups, loot tables and chances, NPC titles, positions, objective amounts |
| [AllTheThings](https://github.com/ATTWoWAddon/AllTheThings) | MIT | Forever quests: classes and races, givers and positions, prerequisites, exclusive quests |
| [Wowhead](https://www.wowhead.com) tooltips | Wowhead's | Whether quests exist in Forever, names, levels, objective texts, NPC titles and positions; Questie's drop rates also come from Wowhead |
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
