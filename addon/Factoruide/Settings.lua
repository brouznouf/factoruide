-- Settings come from the Factoruide app (generated Config.lua). In game, a few UI
-- tweaks (+/- on the "Next" box) only last for the session.

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

function FG:Get(key)
    local v = session[key]
    if v == nil and FG.Config then v = FG.Config[key] end
    if v == nil then v = self.defaults[key] end
    return v
end

function FG:Set(key, value)
    session[key] = value
    for _, fn in ipairs(listeners[key] or {}) do
        fn(value)
    end
end

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
