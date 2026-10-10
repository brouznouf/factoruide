-- Settings come from the Factoruide app (generated Config.lua). In game, a few UI
-- tweaks (+/- on the "Next" box) only last for the session; the window size is saved (it
-- follows the screen, the same for every character).

local _, FG = ...

FG.defaults = {
    guideShown = true,
    uiTheme = "auto",
    nextLines = 5,
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
    trackerMaxQuests = 25,
    trackerAll = false,
    trackerWindow = 15,
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
    mapPins = true,
    mapPinsCount = 15,
    autoFly = true,
}

local session = {}
local listeners = {}

-- Settings changed in game and kept (FactoruideDB.ui), over the app's.
local SAVED = { scale = true }

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

function FG:OpenSettings()
    if GetLocale() == "frFR" then
        FG:Print("les réglages se font dans l'application Factoruide (onglet Addon), puis /reload")
    else
        FG:Print("settings are configured in the Factoruide app (Addon tab), then /reload")
    end
end
