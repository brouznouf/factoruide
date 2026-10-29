# Factoruide Guide

What changes for players in each version of this addon, shown on its CurseForge and Wago pages.
Same rules as the app's CHANGELOG.md: one short line per change, what the player sees. New lines go
under Unreleased; `scripts/release.sh` gives them the release's version.

## Unreleased

- Death skip steps: done once back at the spirit healer.
- A quest that needs a number of other quests done (ten library books) is offered once enough of them are.
- Group guides: each player follows its own profession curve.
- Group guides made for characters the app had no profile of: the addon finds yours by race and class; when several match, choose yours in /fg, Guides ("Player: …").
- The wind guide (a gust of wind towards the step instead of the compass) is gone, with its settings and key binding; the compass stays.
- Objectives done in the same area (mobs next to each other) are shown together in Now, and a level checkpoint between them no longer cuts them apart: it comes after them (GitHub #41).
- Merchant, trainer, flight master and inn steps before the last quest you handed in are done on their own when a guide is loaded or reset, instead of each needing a right click.

## 0.14.0 (2026-10-20)

- The guide's XP bars show a hollow gold box over the XP the current steps give, from where the guide expects you before them to after them.
- In the game's addon list, the Factoruide addons are under Quests, with their own icon, Guide and Data grouped under Profile.
- The full and minimal guides each keep their own layout: moving or resizing the quest panel or the compass in one mode no longer changes the other.
- No mark on the game's XP bar any more, and the game's XP tooltip is back: the guide's own XP bar shows where you should be (GitHub #36).
- A step to sell your loot at a vendor on the way, ticked when the merchant's window opens.
- A kind of step this version does not know yet (a guide from a newer app) is shown and passed on its own.

## 0.13.0 (2026-10-19)

- First version on CurseForge and Wago: the guide made in the Factoruide app, its compass, quest automation and fast loot. Needs Factoruide Profile.
