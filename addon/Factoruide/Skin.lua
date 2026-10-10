-- Look of the addon's frames, decided in one place. By default the game's own style (tooltip
-- frame, game fonts), so UIs restyling Blizzard's frames match it; the UI suite's style when one
-- is loaded (ElvUI, EllesmereUI, SpartanUI); and any skin registered through FactoruideAPI.
--
-- Setting `uiTheme`: "auto" (a loaded UI suite, else Blizzard), "blizzard" or "dark" (flat dark
-- boxes). It is read when frames are created: changing it needs a /reload.
--
-- Frame kinds: "window" (the guide box), "panel" (secondary boxes: next steps, tracker,
-- talents), "button" (a text button), "icon" (a quest item button: `.icon`, maybe `.cooldown`).
-- Windows and panels have `.header` (title strip texture) and `.title`.

local _, FG = ...

local Skin = { frames = {} }
FG.Skin = Skin

local external = {}
-- What styled each frame already: frame -> { [style name] = true }.
local done = setmetatable({}, { __mode = "k" })

--- A field the addon set on a frame (not one of its methods).
local function field(frame, key) return rawget(frame, key) end

-- Base styles -------------------------------------------------------------------

local BLIZZARD_BACKDROP = {
    bgFile = "Interface\\Tooltips\\UI-Tooltip-Background",
    edgeFile = "Interface\\Tooltips\\UI-Tooltip-Border",
    tile = true,
    tileSize = 16,
    edgeSize = 14,
    insets = { left = 3, right = 3, top = 3, bottom = 3 },
}
local FLAT_BACKDROP = {
    bgFile = "Interface\\Buttons\\WHITE8x8",
    edgeFile = "Interface\\Buttons\\WHITE8x8",
    edgeSize = 1,
}

local BASE = {
    blizzard = function(frame, kind)
        if kind == "window" or kind == "panel" then
            frame:SetBackdrop(BLIZZARD_BACKDROP)
            frame:SetBackdropColor(0.06, 0.06, 0.1, kind == "window" and 0.88 or 0.8)
            frame:SetBackdropBorderColor(0.7, 0.7, 0.7, 1)
            if field(frame, "header") then frame.header:SetColorTexture(1, 1, 1, 0) end
        end
    end,
    dark = function(frame, kind)
        if kind == "window" or kind == "panel" then
            frame:SetBackdrop(FLAT_BACKDROP)
            frame:SetBackdropColor(0.05, 0.05, 0.08, kind == "window" and 0.82 or 0.75)
            if kind == "window" then
                frame:SetBackdropBorderColor(0.2, 0.6, 1, 0.6)
                if field(frame, "header") then frame.header:SetColorTexture(0.2, 0.6, 1, 0.18) end
            else
                frame:SetBackdropBorderColor(0.4, 0.4, 0.45, 0.5)
                if field(frame, "header") then frame.header:SetColorTexture(1, 1, 1, 0.06) end
            end
        end
    end,
}

-- UI suites ---------------------------------------------------------------------

-- Each suite: `ready` once it can style frames, `apply(frame, kind, fonts)`. Tried in order.
local suites = {}

local function Refresh()
    for _, entry in ipairs(Skin.frames) do
        Skin:Style(entry.frame, entry.kind, entry.fonts, true)
    end
end

-- ElvUI: its Skins module, once it has loaded us.
if ElvUI then
    local elv = { name = "ElvUI" }
    local E = unpack(ElvUI)
    local S = E and E.GetModule and E:GetModule("Skins", true)
    if S and S.AddCallbackForAddon then
        S:AddCallbackForAddon("Factoruide", "Factoruide", function()
            elv.ready = true
            Refresh()
        end)
        elv.apply = function(frame, kind, fonts)
            if kind == "window" or kind == "panel" then
                frame:SetTemplate("Transparent")
                if field(frame, "header") then
                    frame.header:SetColorTexture(1, 1, 1, kind == "window" and 0.08 or 0.04)
                end
            elseif kind == "button" and field(frame, "Left") then
                S:HandleButton(frame)
            elseif kind == "icon" then
                S:HandleItemButton(frame, true)
            end
            for _, fs in ipairs(fonts or {}) do
                if fs.FontTemplate then
                    local _, size = fs:GetFont()
                    fs:FontTemplate(nil, size)
                end
            end
        end
        elv.font = function(fs)
            if fs.FontTemplate then
                local _, size = fs:GetFont()
                fs:FontTemplate(nil, size)
            end
        end
        suites[#suites + 1] = elv
    end
end

-- EllesmereUI: its third-party skinning API (Retail and Forever).
if EllesmereUI and EllesmereUI.RegisterSkin then
    local eui = { name = "EllesmereUI" }
    EllesmereUI.RegisterSkin("Factoruide", function(S)
        eui.S = S
        eui.ready = true
        Refresh()
    end)
    eui.apply = function(frame, kind, fonts)
        local S = eui.S
        if kind == "window" then
            S.Shell(frame, { noTopBar = true })
        elseif kind == "panel" then
            S.Panel(frame)
        elseif kind == "button" then
            S.Button(frame)
        elseif kind == "icon" and field(frame, "icon") then
            S.SquareIcon(frame.icon, frame)
        end
        for _, fs in ipairs(fonts or {}) do
            S.Font(fs)
        end
    end
    eui.font = function(fs) eui.S.Font(fs) end
    suites[#suites + 1] = eui
end

-- SpartanUI: its Skins handler.
if SUI and SUI.Skins and SUI.Skins.Register then
    local sui = { name = "SpartanUI" }
    SUI.Skins:Register("Factoruide", function()
        sui.ready = true
        Refresh()
    end)
    sui.apply = function(frame, kind)
        if kind == "window" or kind == "panel" then
            SUI.Skins.SetTemplate(frame)
            if field(frame, "header") then frame.header:SetColorTexture(1, 1, 1, 0.05) end
        elseif kind == "button" and field(frame, "Left") then
            SUI.Skins.SkinObj("Button", frame, nil, "Factoruide")
        end
    end
    suites[#suites + 1] = sui
end

local function ActiveSuite()
    if FG:Get("uiTheme") ~= "auto" then return nil end
    for _, suite in ipairs(suites) do
        if suite.ready then return suite end
    end
    return nil
end

-- Styling -------------------------------------------------------------------------

--- Style `frame` (see the kinds above), with `fonts` its font strings, and remember it for the
--- skins registered later. Safe to call again.
function Skin:Style(frame, kind, fonts, again)
    if not again then self.frames[#self.frames + 1] = { frame = frame, kind = kind, fonts = fonts } end
    local suite = ActiveSuite()
    local styled = done[frame] or {}
    done[frame] = styled
    if suite then
        if not styled[suite.name] then
            styled[suite.name] = true
            local ok, err = pcall(suite.apply, frame, kind, fonts)
            if not ok then FG:Debug("%s skin: %s", suite.name, tostring(err)) end
        end
    elseif not styled.base then
        styled.base = true
        local base = BASE[FG:Get("uiTheme")] or BASE.blizzard
        base(frame, kind)
    end
    for _, skin in ipairs(external) do
        if not styled[skin.name] then
            styled[skin.name] = true
            local ok, err = pcall(skin.fn, frame, kind)
            if not ok then FG:Debug("%s skin: %s", skin.name, tostring(err)) end
        end
    end
end

--- Give a font string the UI suite's font (rows created after the frame), same size.
function Skin:Font(fs)
    local suite = ActiveSuite()
    if suite and suite.font then pcall(suite.font, fs) end
end

--- Name of the UI suite styling the addon, nil when none.
function Skin:Suite()
    local suite = ActiveSuite()
    return suite and suite.name
end

-- Public API for other skins (AddOnSkins, NDui, a skin of your own) ---------------

FactoruideAPI = FactoruideAPI or {}
FactoruideAPI.version = 1

--- Call `fn(frame, kind)` on every Factoruide frame, now and when created later. Kinds:
--- "window", "panel", "button", "icon" (see Skin.lua). Frames are named FactoruideTodo,
--- FactoruideNext, FactoruideTracker, FactoruideTalents, FGItemButton, FactoruideItemButton<n>.
function FactoruideAPI.RegisterSkin(name, fn)
    for _, skin in ipairs(external) do
        if skin.name == name then return end
    end
    external[#external + 1] = { name = name, fn = fn }
    Refresh()
end

--- The frames styled so far: { { frame = frame, kind = kind }, ... }.
function FactoruideAPI.GetFrames()
    local out = {}
    for _, entry in ipairs(Skin.frames) do
        out[#out + 1] = { frame = entry.frame, kind = entry.kind }
    end
    return out
end
