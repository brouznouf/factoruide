-- Settings come from the Factoruide app (generated Config.lua). In game, a few UI
-- tweaks (+/- on the box under the guide) only last for the session; the window size, the tab of
-- that box and whether it is shown are saved (the same for every character).

local _, FG = ...

FG.defaults = {
    uiShown = true,
    guideShown = true,
    uiTheme = "auto",
    pickupLines = 10,
    -- Box under the guide: its tab ("next" steps or quests to "pickup") and whether it is shown.
    nextTab = "next",
    nextShown = true,
    lockFrames = false,
    scale = 1.0,
    waypoints = true,
    arrowShown = true,
    autoAccept = true,
    autoTurnIn = true,
    routeQuestsOnly = true,
    autoReward = false,
    fastLoot = true,
    autoTrain = true,
    trainUseful = true,
    trackerShown = true,
    replaceTracker = true,
    trackerQuests = 8,
    trackerAll = false,
    professionsShown = true,
    targetMacro = true,
    targetMark = true,
    questItemButton = true,
    autoSellJunk = true,
    autoRepair = true,
    autoSupplies = false,
    suppliesFood = 20,
    suppliesWater = 20,
    suppliesAmmo = 1000,
    gearAdvisor = true,
    talentAdvisor = true,
    autoLearnTalents = false,
    safetyAlerts = true,
    partySync = true,
    levelTiming = true,
    xpMarker = true,
    mapPins = true,
    mapPinsCount = 15,
    autoFly = true,
}

local session = {}
local listeners = {}

-- Settings changed in game and kept (FactoruideDB.ui), over the app's.
local SAVED = { scale = true, nextTab = true, nextShown = true }

function FG:Get(key)
    local v = session[key]
    if v == nil and SAVED[key] and FactoruideDB and FactoruideDB.ui then v = FactoruideDB.ui[key] end
    if v == nil and FG.Config then v = FG.Config[key] end
    if v == nil then v = self.defaults[key] end
    return v
end

local function Changed(key)
    local value = FG:Get(key)
    for _, fn in ipairs(listeners[key] or {}) do
        fn(value)
    end
end

function FG:Set(key, value)
    session[key] = value
    Changed(key)
end

-- Secure quest-item buttons protect the shared UI parent. Defer the whole change,
-- including overlays on Blizzard frames, so all surfaces switch together after combat.
local pendingUI
function FG:SetUIShown(shown)
    if InCombatLockdown() then
        pendingUI = shown
        self:Print("UI visibility will change when combat ends")
        return
    end
    pendingUI = nil
    self:Set("uiShown", shown)
end

FG:On("PLAYER_REGEN_ENABLED", function()
    if pendingUI ~= nil then FG:SetUIShown(pendingUI) end
end)

--- Keep a setting changed in game (nil: back to the app's).
function FG:Save(key, value)
    FactoruideDB.ui = FactoruideDB.ui or {}
    FactoruideDB.ui[key] = value
    session[key] = nil
    Changed(key)
end

local SCALE_MIN, SCALE_MAX = 0.6, 2

--- `/fg scale [size]`: window size (0.6 to 2; nothing: the app's).
function FG:ScaleCommand(arg)
    local v = tonumber(arg)
    self:Save("scale", v and math.max(SCALE_MIN, math.min(SCALE_MAX, v)) or nil)
    self:Print("scale %.2f", self:Get("scale"))
end

-- Ctrl + mouse wheel over a window changes the size of the windows. The wheel is only taken
-- while Ctrl is held, so that it still zooms the camera otherwise.
local zoomable = {}

function FG:Zoomable(frame)
    table.insert(zoomable, frame)
    frame:SetScript("OnMouseWheel", function(_, delta)
        local v = math.floor((FG:Get("scale") + 0.05 * delta) * 100 + 0.5) / 100
        FG:Save("scale", math.max(SCALE_MIN, math.min(SCALE_MAX, v)))
    end)
end

FG:On("MODIFIER_STATE_CHANGED", function(key, down)
    if key ~= "LCTRL" and key ~= "RCTRL" then return end
    for _, frame in ipairs(zoomable) do
        frame:EnableMouseWheel(down == 1)
    end
end)

--- Call `fn(value)` whenever setting `key` changes.
function FG:OnSetting(key, fn)
    listeners[key] = listeners[key] or {}
    table.insert(listeners[key], fn)
end

FG:OnSetting("uiShown", function(shown)
    FG.UIRoot:SetShown(shown)
    if not shown and StaticPopup_Hide then StaticPopup_Hide("FACTORUIDE_RESET") end
end)

function FG:OpenSettings()
    if GetLocale() == "frFR" then
        FG:Print("les réglages se font dans l'application Factoruide (onglet Addon), puis /reload")
    else
        FG:Print("settings are configured in the Factoruide app (Addon tab), then /reload")
    end
end
