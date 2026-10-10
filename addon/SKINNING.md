# Skinning the Factoruide addon

The addon draws its frames in one place, `Factoruide/Skin.lua`, and picks their look with the
`uiTheme` setting (Addon page of the app):

- `auto` (default): the style of a loaded UI suite, else the game's own style;
- `blizzard`: tooltip-like frames and game fonts, so UIs restyling Blizzard's frames match it;
- `dark`: flat dark boxes.

## UI suites supported out of the box

| Suite | How |
|---|---|
| ElvUI | `Skins:AddCallbackForAddon("Factoruide", ...)`, then `SetTemplate("Transparent")`, `HandleButton`, `HandleItemButton`, `FontTemplate` |
| EllesmereUI | `EllesmereUI.RegisterSkin("Factoruide", ...)`: `Shell`, `Panel`, `Button`, `SquareIcon`, `Font` |
| SpartanUI | `SUI.Skins:Register("Factoruide", ...)`: `SetTemplate`, `SkinObj("Button", ...)` |

## Your own skin

```lua
if FactoruideAPI then
    FactoruideAPI.RegisterSkin("MySkin", function(frame, kind)
        -- kind: "window", "panel", "button" or "icon"
    end)
end
```

The function runs on every frame already made and on every frame made later. Frames:

| Frame | Kind | Fields |
|---|---|---|
| `FactoruideTodo` (guide) | window | `header` (title strip texture), `title`, `info`, `rows` |
| `FactoruideNext` (next steps) | panel | same |
| `FactoruideTracker` (quest tracker) | panel | `header`, `title`, `count` |
| `FactoruideTalents` (talent advice) | panel | `text`, `button` |
| `FGItemButton`, `FactoruideItemButton<n>` | icon | `icon`, `cooldown` |

`FactoruideAPI.GetFrames()` returns `{ { frame = ..., kind = ... }, ... }` for the frames made so
far.
