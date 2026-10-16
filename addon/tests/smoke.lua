-- Smoke test of the addon outside the game: minimal WoW API stubs, load files in TOC
-- order, fire login and quest events, check the guide advances.
-- Run from the repository root: luajit addon/tests/smoke.lua
-- `luajit addon/tests/smoke.lua profile`: Factoruide_Profile alone (the guide is another addon's).
local profileOnly = arg and arg[1] == "profile"

local onQuest, completed = {}, {}
local playerLevel = 1
local waypoint
local frames = {}

-- Generic UI object: any unknown method is a no-op returning another stub.
local function stub()
    local o = { events = {} }
    function o:RegisterEvent(e) self.events[e] = true end
    function o:SetScript(name, fn) self[name] = fn end
    function o:SetText(t) self.text = t end
    function o:GetText() return self.text end
    -- The game refuses a missing font.
    function o:SetFontObject(font) assert(font ~= nil, "SetFontObject without a font") end
    -- Strict as the newer clients' API: a boolean, not nil.
    function o:SetShown(shown) assert(type(shown) == "boolean", "SetShown(" .. tostring(shown) .. ")") end
    function o:GetPoint() return "TOP", nil, "TOP", 0, 0 end
    function o:GetID() return 1 end
    setmetatable(o, {
        __index = function()
            return function() return stub() end
        end,
    })
    return o
end

CreateFrame = function()
    local f = stub()
    table.insert(frames, f)
    return f
end
CreateVector2D = function(x, y)
    return {
        GetXY = function() return x, y end,
    }
end
MinimalSliderWithSteppersMixin = { Label = {} }
local settingsRegistered = 0
Settings = {
    VarType = { Boolean = "boolean", Number = "number" },
    RegisterVerticalLayoutCategory = function()
        return {
            GetID = function() return 1 end,
        }
    end,
    RegisterProxySetting = function()
        settingsRegistered = settingsRegistered + 1
        return {}
    end,
    CreateCheckbox = function() end,
    CreateSliderOptions = function()
        return { SetLabelFormatter = function() end }
    end,
    CreateSlider = function() end,
    RegisterAddOnCategory = function() end,
    OpenToCategory = function() end,
}
local calls = {}
local function record(name)
    return function(...) calls[name] = { ... } end
end
AcceptQuest = record("AcceptQuest")
CompleteQuest = record("CompleteQuest")
GetQuestReward = record("GetQuestReward")
LootSlot = function(i) calls.loot = (calls.loot or 0) + 1 end
QuestGetAutoAccept = function() return false end
IsShiftKeyDown = function() return false end
IsQuestCompletable = function() return true end
GetNumQuestChoices = function() return 1 end
GetNumLootItems = function() return 3 end
GetCVarBool = function() return true end
IsModifiedClick = function() return false end
GetNumTrainerServices = function() return 0 end
IsTradeskillTrainer = function() return false end
GetMoney = function() return 0 end
IsInInstance = function() return false end
IsInGroup = function() return false end
IsInRaid = function() return false end
ObjectiveTrackerFrame = {
    hidden = false,
    Hide = function(self) self.hidden = true end,
    IsProtected = function() return false end,
    HookScript = function() end,
    Update = function() end,
}
GetQuestLogSpecialItemInfo = function() return nil end
GetDifficultyColor = function() return { r = 1, g = 0.8, b = 0 } end
C_PlayerInfo = {
    GetContentDifficultyQuestForPlayer = function() return 1 end,
}
GameTooltip = { SetOwner = function() end, SetHyperlink = function() end, Show = function() end, Hide = function() end }
-- Items: their class (12: quest items, the default) and how many are in the bags and the bank.
local itemClasses, bags, bank = {}, {}, {}
C_Item = {
    GetItemInfoInstant = function(id) return id, nil, nil, nil, 134400, itemClasses[id] or 12, 0 end,
    GetItemCount = function(id, includeBank) return (bags[id] or 0) + (includeBank and (bank[id] or 0) or 0) end,
}
UIParent = {}
print = print
strsplit = function(sep, s, n)
    local out, start = {}, 1
    while true do
        if n and #out == n - 1 then
            table.insert(out, s:sub(start))
            break
        end
        local i = s:find(sep, start, true)
        if not i then
            table.insert(out, s:sub(start))
            break
        end
        table.insert(out, s:sub(start, i - 1))
        start = i + 1
    end
    return unpack(out)
end
strtrim = function(s) return (s:gsub("^%s+", ""):gsub("%s+$", "")) end
wipe = function(t)
    for k in pairs(t) do
        t[k] = nil
    end
    return t
end
time = os.time
GetTime = function() return os.clock() + 1000 end
GetLocale = function() return "frFR" end
UnitName = function() return "Brouz" end
GetNormalizedRealmName = function() return "Realm" end
UnitRace = function() return "Gnome", "Gnome", 7 end
UnitClass = function() return "Mage", "MAGE", 8 end
UnitFactionGroup = function() return "Alliance" end
UnitLevel = function() return playerLevel end
UnitXP = function() return 0 end
UnitXPMax = function() return 400 end
GetXPExhaustion = function() return 600 end
IsResting = function() return true end
local bindLocation = "Coldridge Valley"
GetBindLocation = function() return bindLocation end
GetInventoryItemID = function(_, slot) return slot == 5 and 6125 or nil end
local reloaded = false
ReloadUI = function() reloaded = true end
UnitGUID = function(u)
    if u == "npc" then return "Creature-0-1-2-3-658-0000" end
    return "Player-1-2"
end
StaticPopupDialogs = {}
StaticPopup_Show = record("StaticPopup_Show")
YES, NO = "Yes", "No"
UnitOnTaxi = function() return false end
UnitIsPlayer = function() return false end
UnitExists = function() return false end
InCombatLockdown = function() return false end
local macros = {}
GetMacroIndexByName = function(name)
    for i, m in pairs(macros) do
        if m.name == name then return i end
    end
    return 0
end
GetNumMacros = function() return 0, #macros end
CreateMacro = function(name, icon, body)
    table.insert(macros, { name = name, body = body })
    return #macros
end
EditMacro = function(i, name, icon, body) macros[i].body = body end
PickupMacro = function() end
issecretvalue = function() return false end
local timers = {}
C_Timer = { NewTicker = function() end, After = function(_, fn) table.insert(timers, fn) end }
--- Run the C_Timer.After callbacks waiting.
local function runTimers()
    local due = timers
    timers = {}
    for _, fn in ipairs(due) do
        fn()
    end
end
C_Map = {
    GetBestMapForUnit = function() return 1426 end,
    GetMapInfo = function(m) return { mapID = m, mapType = m == 1415 and 2 or 3, parentMapID = 1415 } end,
    GetPlayerMapPosition = function()
        return {
            GetXY = function() return 0.2993, 0.712 end,
        }
    end,
    CanSetUserWaypointOnMap = function() return true end,
    SetUserWaypoint = function(p) waypoint = p end,
    ClearUserWaypoint = function() waypoint = nil end,
    GetWorldPosFromMapPos = function(m, v)
        local x, y = v:GetXY()
        return 0, CreateVector2D(x * 1000, y * 1000)
    end,
}
UiMapPoint = {
    CreateFromCoordinates = function(m, x, y) return { m = m, x = x, y = y } end,
}
C_SuperTrack = { SetSuperTrackedUserWaypoint = function() end }
C_QuestLog = {
    GetAllCompletedQuestIDs = function() return {} end,
    GetNumQuestLogEntries = function() return 0 end,
    GetInfo = function() return nil end,
    GetLogIndexForQuestID = function() return nil end,
    IsOnQuest = function(q) return onQuest[q] or false end,
    IsQuestFlaggedCompleted = function(q) return completed[q] or false end,
    IsComplete = function() return false end,
    GetQuestObjectives = function() return { { text = "Tough Wolf Meat: 0/8", finished = false } } end,
    GetTitleForQuestID = function() return nil end,
}
C_GossipInfo = {
    GetAvailableQuests = function() return { { questID = 233, questLevel = 2 } } end,
    GetActiveQuests = function() return {} end,
    SelectAvailableQuest = record("SelectAvailableQuest"),
    SelectActiveQuest = record("SelectActiveQuest"),
}
GetQuestID = function() return 179 end
SlashCmdList = {}

-- Each addon gets its own private table; the guide uses the profile's, the global `Factoruide`.
local function load(addon)
    local private = {}
    for line in io.lines("addon/" .. addon .. "/" .. addon .. ".toc") do
        -- Config.lua, Routes.lua and Sent.lua are written by the app and not versioned.
        if line == "Config.lua" then
            Factoruide.Config = {}
        elseif line == "Sent.lua" then
            FactoruideSent = nil
        elseif line == "Routes.lua" then
            assert(loadfile("addon/tests/fixtures/Routes.lua"))(addon, private)
        elseif line:match("%.lua$") then
            local chunk = assert(loadfile("addon/" .. addon .. "/" .. line:gsub("\\", "/")))
            chunk(addon, private)
        end
    end
end
load("Factoruide_Profile")
if not profileOnly then load("Factoruide_Guide") end
local FG = Factoruide

local function fire(event, ...)
    for _, f in ipairs(frames) do
        if f.OnEvent and f.events[event] then f.OnEvent(f, event, ...) end
    end
end

fire("ADDON_LOADED", "Factoruide_Profile")
if not profileOnly then fire("ADDON_LOADED", "Factoruide_Guide") end
fire("PLAYER_LOGIN")
local char = FactoruideDB.characters["Brouz-Realm"]

-- Another addon shows the guide: this one only records, and does nothing else.
if profileOnly then
    assert(FG.Guide == nil and FG.Routes == nil and FG.Get == nil, "no guide without Factoruide_Guide")
    onQuest[179] = true
    fire("QUEST_DETAIL", 0)
    fire("QUEST_ACCEPTED", 179)
    assert(#char.events >= 2, "collector recorded events")
    assert(char.guide == nil and waypoint == nil, "no guide state, no waypoint")
    assert(next(calls) == nil, "nothing done in game: " .. tostring(next(calls)))
    SlashCmdList.FACTORUIDE("")
    SlashCmdList.FACTORUIDE("help")
    FG.Profile:Snapshot()
    local profile = FactoruideDB.profiles["Brouz-Realm"]
    assert(profile and profile.level == 1 and profile.class == "MAGE", "profile saved")
    assert(next(FactoruideDB.errors or {}) == nil, "no error: " .. tostring(next(FactoruideDB.errors or {})))
    print("profile only ok: " .. #char.events .. " events")
    os.exit(0)
end
assert(char.guide.route == "gnome-mage", "assigned route followed: " .. tostring(char.guide.route))

--- Assign a route to the character, as the app does in Routes.lua, and follow it.
local function useRoute(name)
    FG.Assignments["Brouz-Realm"] = name
    FG.Guide:FollowAssignment()
end
assert(char.guide.step == 1)
assert(waypoint and waypoint.m == 1426, "waypoint set")

onQuest[179] = true
fire("QUEST_DETAIL", 0)
fire("QUEST_ACCEPTED", 179)
assert(char.guide.step == 2, "advanced after accept, step " .. char.guide.step)

completed[179] = true
onQuest[179] = nil
fire("QUEST_TURNED_IN", 179, 80, 0)
assert(char.guide.step == 4, "objective and turn-in skipped, step " .. char.guide.step)

-- Back one page: steps 3 to 6 (Sten Stoutarm, Balir Frosthammer) are one group, step 2 another.
SlashCmdList.FACTORUIDE("prev")
assert(char.guide.step == 2, "the page before, step " .. char.guide.step)
SlashCmdList.FACTORUIDE("next")
assert(char.guide.step == 3, "the page after it, step " .. char.guide.step)
SlashCmdList.FACTORUIDE("status")
assert(#char.events >= 2, "collector recorded events")
print("smoke test ok: step " .. char.guide.step .. ", " .. #char.events .. " events")

-- On a page with steps left, the current step is its first one not done; the page stays.
FG.Guide:Advance()
assert(char.guide.step == 4 and FG.Guide:CurrentSteps()[1] == 3, "same page, its step not done yet")
-- Going back to a page done already stays there (refresh and quest events), the resync goes forward.
SlashCmdList.FACTORUIDE("prev")
FG.Guide:Advance()
assert(char.guide.step == 2, "stays after manual navigation")
fire("QUEST_LOG_UPDATE")
assert(char.guide.step == 2, "quest events do not move a manual position")
FG.Guide:Resync()
assert(char.guide.step == 4, "resync goes to the first step not done")
print("navigation ok")

-- Automation
fire("GOSSIP_SHOW")
assert(calls.SelectAvailableQuest and calls.SelectAvailableQuest[1] == 233, "gossip quest selected")
local detailID = GetQuestID
GetQuestID = function() return 233 end
fire("QUEST_DETAIL", 0)
assert(calls.AcceptQuest, "quest accepted")
GetQuestID = detailID
fire("QUEST_PROGRESS")
assert(calls.CompleteQuest, "quest completed")
fire("QUEST_COMPLETE")
assert(calls.GetQuestReward and calls.GetQuestReward[1] == 1, "single reward taken")
fire("LOOT_READY", false)
assert(calls.loot == 3, "all loot slots taken")
-- The guide's mode: switched in game and kept, the minimal strip shown instead of the card.
SlashCmdList.FACTORUIDE("mode")
assert(FG:Get("guideMode") == "minimal" and FactoruideDB.ui.guideMode == "minimal", "minimal mode kept")
assert(FG.Guide:Box() == FG.Mini:Frame(), "the minimal strip is the guide")
SlashCmdList.FACTORUIDE("mode")
assert(FG:Get("guideMode") == "full" and FG.Guide:Box() == FG.Card:Frame(), "back to the full card")

-- Settings from the app (Config.lua) are respected.
FG.Config.autoAccept = false
calls.AcceptQuest = nil
fire("QUEST_DETAIL", 0)
assert(not calls.AcceptQuest, "auto accept disabled by the app")
print("automation ok")

-- Quest tracker: lists the log, hides Blizzard's tracker.
C_QuestLog.GetNumQuestLogEntries = function() return 1 end
C_QuestLog.GetInfo = function(i) return { questID = 233, title = "Coldridge Valley Mail Delivery", level = 2 } end
onQuest[233] = true
assert(ObjectiveTrackerFrame.hidden, "Blizzard tracker hidden")
FG.Tracker:Refresh()
local next = FG.Guide:NextStepByQuest()
assert(next[233], "quest 233 found in the route")
print("tracker ok")

-- Professions: practice steps are done once the skill reaches their target.
local cooking = 20
GetProfessions = function() return nil, nil, nil, nil, 5, nil end
GetProfessionInfo = function(i) return "Cuisine", 0, cooking, 75, 0, 0, 185 end
FG.Routes["prof-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    professions = {
        { key = "cooking", name = "Cooking", line = 185, target = 300, curve = { 0, 5, 10, 15, 20, 25, 30 } },
    },
    steps = {
        {
            k = "profession",
            t = "Learn Apprentice Cooking with Gremlock",
            e = "npc",
            id = 1699,
            n = "Gremlock",
            sk = 75,
            lvl = 5,
        },
        { k = "practice", t = "Raise Cooking to 25 (~7 min)", e = "skill", id = 185, n = "Cooking", sk = 25, lvl = 5 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
-- No guide assigned: none followed (never picked by race or class); /fg route does not switch.
useRoute(nil)
assert(char.guide.route == nil, "no assigned route, got " .. tostring(char.guide.route))
SlashCmdList.FACTORUIDE("route gnome-mage")
assert(char.guide.route == nil, "/fg route does not switch, got " .. tostring(char.guide.route))
useRoute("prof-test")
assert(char.guide.step == 2, "rank already learned, step " .. char.guide.step)
cooking = 25
fire("SKILL_LINES_CHANGED")
assert(char.guide.step == 3, "practice done, step " .. char.guide.step)
playerLevel = 5
FG.Tracker:Refresh()
print("professions ok")

-- Skipping: outdated steps, exclusive alternatives, by hand.
playerLevel = 21
FG.Routes["skip-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "accept", t = "Accept Old Quest", q = 5001, lvl = 1 },
        { k = "accept", t = "Accept Alt Quest", q = 5002, alt = { 5003 }, lvl = 21 },
        { k = "accept", t = "Accept Current Quest", q = 5004, lvl = 21 },
        { k = "turnin", t = "Turn in Current Quest", q = 5004, lvl = 21 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
completed[5003] = true
useRoute("skip-test")
assert(char.guide.step == 3, "outdated and alternative steps skipped, step " .. char.guide.step)
SlashCmdList.FACTORUIDE("skip")
assert(char.guide.step == 5, "current quest skipped by hand, step " .. char.guide.step)
SlashCmdList.FACTORUIDE("skip 5004")
assert(char.guide.step == 3, "undo brings the quest back, step " .. char.guide.step)
print("skip ok")

-- Progress belongs to its guide: a quest marked done by hand in one is still to do in another,
-- and coming back finds the marks again. A new version keeps the quests, not the step marks.
SlashCmdList.FACTORUIDE("skip")
FG.Routes["other-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 21,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "accept", t = "Accept Current Quest", q = 5004, lvl = 21 },
        { k = "turnin", t = "Turn in Current Quest", q = 5004, lvl = 21 },
    },
}
useRoute("other-test")
assert(char.guide.step == 1 and not char.guide.skippedQuests[5004], "marked in another guide only")
useRoute("skip-test")
assert(char.guide.step == 5 and char.guide.skippedQuests[5004], "back to the first guide")
char.guide.marks[1] = "skip"
FG.Routes["skip-test"].time = 2
useRoute("other-test")
useRoute("skip-test")
assert(not char.guide.marks[1] and char.guide.skippedQuests[5004], "new version")
print("progress per guide ok")

-- A quest of the log the guide does not take is abandoned first: the step is done once it has
-- left the log.
FG.Routes["abandon-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 21,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "abandon", t = "Abandon Unused Quest (not in this guide)", q = 7001, lvl = 21 },
        { k = "accept", t = "Accept Current Quest", q = 5004, lvl = 21 },
    },
}
onQuest[7001] = true
local untitled = C_QuestLog.GetTitleForQuestID
C_QuestLog.GetTitleForQuestID = function(q) return q == 7001 and "Quête inutile" or nil end
useRoute("abandon-test")
assert(char.guide.step == 1, "waits while the quest is in the log, step " .. char.guide.step)
-- The game's name of the quest, then the label and why, once each.
local abandonRow = FG.Guide:Box().rows[1]
local abandonLine = abandonRow.name:GetText() .. " / " .. abandonRow.sub:GetText()
assert(
    abandonLine:find("Abandon", 1, true)
        and abandonLine:find("Quête inutile", 1, true)
        and abandonLine:find("(not in this guide)", 1, true)
        and not abandonLine:find("Unused Quest", 1, true),
    "abandon line: " .. abandonLine
)
C_QuestLog.GetTitleForQuestID = untitled
onQuest[7001] = nil
fire("QUEST_REMOVED", 7001)
assert(char.guide.step == 2, "quest abandoned, step " .. char.guide.step)
print("abandon ok")

-- Checkpoints: reached in level before the planned step, the steps up to it are skipped but the
-- kept ones (a chain going on after it).
playerLevel = 22
FG.Routes["checkpoint-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    checkpoints = { { lvl = 22, step = 5 } },
    steps = {
        { k = "accept", t = "Accept Filler", q = 6001, lvl = 20 },
        { k = "accept", t = "Accept Chain", q = 6002, kp = true, lvl = 20 },
        { k = "turnin", t = "Turn in Filler", q = 6001, lvl = 21 },
        { k = "turnin", t = "Turn in Chain", q = 6002, kp = true, lvl = 21 },
        { k = "accept", t = "Accept New Hub Quest", q = 6003, lvl = 22 },
        { k = "accept", t = "Accept Chain Follow-up", q = 6004, lvl = 22 },
    },
}
useRoute("checkpoint-test")
assert(char.guide.step == 1, "checkpoint route starts at 1, step " .. char.guide.step)
FG.Guide:SkipAhead()
assert(char.guide.marks[1] == "skip" and char.guide.marks[3] == "skip", "filler skipped")
assert(not char.guide.marks[2] and not char.guide.marks[4], "chain kept")
assert(char.guide.step == 2, "on the kept chain, step " .. char.guide.step)
playerLevel = 21
onQuest[6002] = true
FG.Guide:Advance()
FG.Guide:SkipAhead()
assert(not char.guide.marks[4], "no skip below the checkpoint level")
onQuest[6002] = nil
print("checkpoints ok")

-- A quest whose accept was left behind: its turn-in is not asked.
playerLevel = 21
FG.Routes["turnin-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "accept", t = "Accept Keeper of the Flame", q = 6001, lvl = 10 },
        { k = "turnin", t = "Turn in Keeper of the Flame", q = 6001, lvl = 20 },
        { k = "accept", t = "Accept Current Quest", q = 6002, lvl = 21 },
    },
}
useRoute("turnin-test")
assert(char.guide.step == 3, "turn-in of a never accepted quest skipped, step " .. char.guide.step)
print("accept-skipped ok")

-- Background objectives do not block the guide; the tracker keeps them as in progress.
playerLevel = 5
onQuest[7001] = true
FG.Routes["bg-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "accept", t = "Accept Gold Dust Exchange", q = 7001, lvl = 5 },
        { k = "objective", t = "Gold Dust x10", q = 7001, o = 1, n = "Gold Dust", bg = true, lvl = 5 },
        { k = "accept", t = "Accept Next Quest", q = 7002, lvl = 5 },
        { k = "turnin", t = "Turn in Gold Dust Exchange", q = 7001, lvl = 6 },
    },
}
useRoute("bg-test")
assert(char.guide.step == 3, "background objective passed, step " .. char.guide.step)
assert(FG.Guide:NextStepByQuest()[7001].bg, "background quest in progress for the tracker")
print("background ok")

-- In progress: a background objective left behind stays listed, and comes before the turn-in of
-- its quest while not done.
do
    local listed
    local set = FG.TargetMacro.SetSteps
    FG.TargetMacro.SetSteps = function(self, steps)
        listed = steps
        return set(self, steps)
    end
    onQuest[7001], onQuest[7002] = true, true
    FG.Guide:Resync()
    assert(char.guide.step == 4, "on the turn-in, step " .. char.guide.step)
    assert(listed[1] and listed[1].k == "objective" and listed[1].q == 7001, "objective before its turn-in")
    assert(listed[2] and listed[2].k == "turnin", "then the turn-in")
    FG.TargetMacro.SetSteps = set
    onQuest[7001], onQuest[7002] = nil, nil
end
print("in progress ok")

-- Target macro: created on demand, then follows the current step's mobs.
playerLevel = 5
FG.Routes["macro-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        {
            k = "objective",
            t = "Kill Kobold Vermin x8",
            q = 7101,
            o = 1,
            e = "npc",
            id = 6,
            n = "Kobold Vermin",
            mobs = { "Kobold Vermin" },
            lvl = 5,
        },
        { k = "turnin", t = "Turn in Kobold Camp", q = 7101, e = "npc", id = 197, n = "Marshal McBride", lvl = 5 },
    },
}
onQuest[7101] = true
useRoute("macro-test")
SlashCmdList.FACTORUIDE("macro")
local index = GetMacroIndexByName("FG Cible")
assert(index > 0, "target macro created")
assert(
    macros[index].body:find("/targetexact Kobold Vermin", 1, true),
    "macro targets the step's mobs: " .. macros[index].body
)
-- The skull: WoW Forever blocks SetRaidTarget from a macro's /run (Classic Era and TBC do not).
assert(FG.Edition == "forever" and not macros[index].body:find("/run", 1, true), "no skull on Forever")
FG.Edition = "classic"
SlashCmdList.FACTORUIDE("next")
assert(
    macros[index].body:find("/targetexact Marshal McBride", 1, true),
    "macro follows the step: " .. macros[index].body
)
assert(macros[index].body:find("SetRaidTarget", 1, true), "skull on Classic")
FG.Edition = "forever"
print("target macro ok")

-- Talent plan: the next talent follows the class plan (mage: Improved Frostbolt x5, then
-- Elemental Precision).
local frostbolt = 2
GetNumTalentTabs = function() return 3 end
GetNumTalents = function(tab) return tab == 3 and 2 or 0 end
GetTalentInfo = function(tab, i)
    if i == 1 then return "Improved Frostbolt", nil, 1, 2, frostbolt, 5 end
    return "Elemental Precision", nil, 1, 3, 0, 3
end
assert(FG.Talents:Next().name == "Improved Frostbolt", "next talent: Improved Frostbolt")
frostbolt = 5
assert(FG.Talents:Next().name == "Elemental Precision", "next talent after 5/5: Elemental Precision")
print("talents ok")

-- Leveling time against the plan.
playerLevel = 3
FG.Routes["timing-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1000,
    levels = { [2] = 100, [3] = 300 },
    steps = {
        { k = "accept", t = "Accept", q = 7201, lvl = 3 },
    },
}
useRoute("timing-test")
FG.Guide:State().levelTimes = { [3] = 240 }
assert(FG.Timing:Short():find("1 min", 1, true), "timing ahead of plan: " .. FG.Timing:Short())
print("timing ok")

-- Flight master: the current fly step's destination is taken.
local taken
Enum = Enum or {}
Enum.FlightPathState = { Current = 0, Reachable = 1, Unreachable = 2 }
GetTaxiMapID = function() return 1415 end
C_TaxiMap = {
    GetAllTaxiNodes = function()
        return { { nodeID = 2, slotIndex = 7, state = 1 }, { nodeID = 6, slotIndex = 3, state = 1 } }
    end,
}
TakeTaxiNode = function(slot) taken = slot end
FG.Routes["fly-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "fly", t = "Fly to Ironforge", e = "taxi", id = 6, n = "Ironforge", lvl = 3 },
    },
}
useRoute("fly-test")
for _, f in ipairs(frames) do
    if f.events.TAXIMAP_OPENED and f.OnEvent then f:OnEvent("TAXIMAP_OPENED") end
end
assert(taken == 3, "flight to the step's node taken")
print("auto fly ok")

-- Teleport: done once the character is there (around the city's class trainer).
FG.Routes["teleport-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "teleport", t = "Teleport to Ironforge", m = 1426, x = 80, y = 20, lvl = 20 },
        { k = "teleport", t = "Teleport to Ironforge", m = 1426, x = 30.1, y = 71.3, lvl = 20 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
useRoute("teleport-test")
fire("QUEST_LOG_UPDATE")
assert(char.guide.step == 1, "far from it: not there yet, step " .. char.guide.step)
char.guide.step = 2
fire("QUEST_LOG_UPDATE")
assert(char.guide.step == 3, "arrived, step " .. char.guide.step)
print("teleport ok")

-- Group sync: a member's progress shows next to the objective.
Ambiguate = function(name) return (name:gsub("%-.*", "")) end
for _, f in ipairs(frames) do
    if f.events.CHAT_MSG_ADDON and f.OnEvent then
        f:OnEvent("CHAT_MSG_ADDON", "FGSYNC1", "R", "PARTY", "Pata-Realm")
        f:OnEvent("CHAT_MSG_ADDON", "FGSYNC1", "7101:3;33:x", "PARTY", "Pata-Realm")
    end
end
assert(FG.Party:Suffix(7101, 1, 8):find("Pata 3", 1, true), "party progress shown: " .. FG.Party:Suffix(7101, 1, 8))
print("party ok")

-- Merchant: grey items are sold.
local sold = 0
C_Container = {
    GetContainerNumSlots = function(bag) return bag == 0 and 2 or 0 end,
    GetContainerItemInfo = function(_, slot)
        return { itemID = 100 + slot, quality = slot == 1 and 0 or 2, stackCount = 1 }
    end,
    UseContainerItem = function() sold = sold + 1 end,
}
C_Item = C_Item or {}
C_Item.GetItemInfo = function() return "Item", nil, nil, nil, nil, nil, nil, nil, nil, nil, 5 end
CanMerchantRepair = function() return false end
for _, f in ipairs(frames) do
    if f.events.MERCHANT_SHOW and f.OnEvent then f:OnEvent("MERCHANT_SHOW") end
end
assert(sold == 1, "one grey item sold, got " .. sold)
print("merchant ok")

-- Skins: a skin registered through the public API styles every frame made so far.
local skinned = {}
FactoruideAPI.RegisterSkin("Test", function(frame, kind) skinned[kind] = (skinned[kind] or 0) + 1 end)
assert(skinned.window == 2, "guide windows skinned: the card and the minimal strip")
assert((skinned.panel or 0) >= 1, "panels skinned")
assert(#FactoruideAPI.GetFrames() >= 2)
assert(FG.Skin:Suite() == nil, "no UI suite in the tests")
print("skins ok")

-- Compass: straight ahead, to the left, behind (angles counter-clockwise, as the game's facing).
local close = function(a, b) return math.abs(a - b) < 1e-9 end
assert(close(FG.Compass.Angle(10, 0, 0), 0), "north, facing north: ahead")
assert(close(FG.Compass.Angle(0, 10, 0), math.pi / 2), "west, facing north: to the left")
assert(close(FG.Compass.Angle(0, -10, math.pi / 2), math.pi), "east, facing west: behind")
assert(close(FG.Compass.Angle(0, 10, math.pi / 2), 0), "west, facing west: ahead")
assert(close(FG.Compass.Left(FG.Compass.Angle(0, -10, 0)), -90), "east, facing north: 90° to the right")
print("compass angles ok")

-- A step whose mobs live in several places: the arrow moves on from a place where the character
-- waited out of combat (nothing there), and on /fg spot; around again once all are visited.
do
    local now, fighting = 100, false
    local realTime, realCombat = GetTime, UnitAffectingCombat
    GetTime = function() return now end
    UnitAffectingCombat = function() return fighting end
    FG.Routes["spots"] = {
        race = 7,
        class = 8,
        faction = "Alliance",
        fromLevel = 1,
        toLevel = 2,
        steps = {
            {
                k = "objective",
                t = "Kill Lost Stalker",
                q = 900,
                o = 1,
                m = 1426,
                x = 29.93,
                y = 71.2,
                lvl = 1,
                spots = { { m = 1426, x = 50, y = 50 }, { m = 1426, x = 10, y = 10 } },
            },
        },
    }
    onQuest[900] = true
    local before, beforeStep = char.guide.route, char.guide.step
    useRoute("spots")
    FG.Guide:Refresh()
    assert(waypoint.x == 0.2993, "its own place first")
    now, fighting = 130, true
    FG.Guide:Refresh()
    now, fighting = 145, false
    FG.Guide:Refresh()
    assert(waypoint.x == 0.2993, "fighting there: something to find, stays")
    now = 170
    FG.Guide:Refresh()
    assert(waypoint.x == 0.5 and waypoint.y == 0.5, "waited there out of combat: the closest other place")
    SlashCmdList.FACTORUIDE("spot")
    assert(waypoint.x == 0.1, "/fg spot: the next one")
    SlashCmdList.FACTORUIDE("spot")
    assert(waypoint.x == 0.2993, "all visited: around again from the closest")
    local places, at = FG.Guide:SpotPlaces()
    assert(#places == 3 and at == 1, "the places for the map pins")
    onQuest[900] = nil
    useRoute(before)
    FG.Guide:Go(beforeStep)
    GetTime, UnitAffectingCombat = realTime, realCombat
    print("spots ok")
end

-- Profile: what the app plans a guide from, saved at login, logout and on /fg profile.
local profile = FactoruideDB.profiles["Brouz-Realm"]
assert(profile and profile.level == 1 and profile.race == 7 and profile.class == "MAGE", "profile saved")
assert(profile.rested == 600 and profile.xp_max == 400 and profile.resting, "XP and rested XP")
assert(profile.gear[1] == 6125, "gear worn")
assert(profile.bind.name == "Coldridge Valley" and not profile.bind.position, "hearthstone, place unknown")
bindLocation = "Kharanos"
fire("CHAT_MSG_SYSTEM", "Kharanos is now your home.")
assert(profile.bind.name == "Kharanos" and profile.bind.position.map == 1426, "hearthstone bound here")
C_TaxiMap.GetAllTaxiNodes = function()
    return { { nodeID = 6, state = 0 }, { nodeID = 7, state = 1 }, { nodeID = 8, state = 2 } }
end
fire("TAXIMAP_OPENED")
assert(profile.flights[6] and profile.flights[7] and not profile.flights[8], "flight paths known")
C_QuestLog.GetQuestObjectives = function()
    return { { text = "Tough Wolf Meat: 3/8", numFulfilled = 3, numRequired = 8, finished = false } }
end
C_QuestLog.GetAllCompletedQuestIDs = function() return { 179, 233 } end
SlashCmdList.FACTORUIDE("profile")
assert(reloaded, "the interface reloads to write the profile")
assert(#profile.completed == 2, "quests done")
assert(profile.log[1].id == 233 and profile.log[1].objectives[1].done == 3, "log with its objectives")
print("profile ok")

-- Window size: Ctrl + mouse wheel over a window, or /fg scale; saved, nothing: the app's.
SlashCmdList.FACTORUIDE("scale 1.3")
assert(FG:Get("scale") == 1.3 and FactoruideDB.ui.scale == 1.3, "scale saved")
local wheel
for _, f in ipairs(frames) do
    if rawget(f, "OnMouseWheel") then wheel = f end
end
fire("MODIFIER_STATE_CHANGED", "LCTRL", 1)
wheel.OnMouseWheel(wheel, 1)
assert(FG:Get("scale") == 1.35, "zoomed in: " .. FG:Get("scale"))
SlashCmdList.FACTORUIDE("scale")
assert(FG:Get("scale") == 1 and FactoruideDB.ui.scale == nil, "back to the app's size")
print("scale ok")

-- A part of the profile the game fails to give keeps its previous value, the error is kept.
local inventory = GetInventoryItemID
GetInventoryItemID = function() error("gone") end
FG.Profile:Snapshot()
assert(profile.gear[1] == 6125 and profile.errors.gear, "gear kept")
assert(#profile.log == 1, "the other parts are saved")
GetInventoryItemID = inventory
FG.Profile:Snapshot()
assert(not profile.errors, "no error left")
print("profile parts ok")

-- Reputations: every faction of the panel, those under a collapsed header too (opened for the
-- reading, closed again), headers without points of their own left out.
local factionRows = {
    { name = "Alliance", header = true, id = 469 },
    { name = "Darnassus", id = 69, value = 4200, parent = "Alliance" },
    { name = "Other", header = true, collapsed = true, id = 169 },
    { name = "Booty Bay", id = 21, value = 2900, parent = "Other" },
}
local function visibleFactions()
    local out, hidden = {}, {}
    for _, f in ipairs(factionRows) do
        if f.header and f.collapsed then hidden[f.name] = true end
        if not (f.parent and hidden[f.parent]) then table.insert(out, f) end
    end
    return out
end
local function setCollapsed(i, collapsed) visibleFactions()[i].collapsed = collapsed end
C_Reputation = {
    GetNumFactions = function() return #visibleFactions() end,
    GetFactionDataByIndex = function(i)
        local f = visibleFactions()[i]
        return f
            and {
                factionID = f.id,
                name = f.name,
                currentStanding = f.value or 0,
                isHeader = f.header or false,
                isCollapsed = f.collapsed or false,
                isHeaderWithRep = false,
            }
    end,
    ExpandFactionHeader = function(i) setCollapsed(i, false) end,
    CollapseFactionHeader = function(i) setCollapsed(i, true) end,
}
FG.Profile:Snapshot()
local function points(list)
    local out = {}
    for _, r in ipairs(list or {}) do
        out[r.id] = r.value
    end
    return out
end
local seen = points(profile.reputation)
assert(seen[69] == 4200 and seen[21] == 2900 and not seen[469] and not seen[169], "reputations, collapsed ones too")
assert(factionRows[3].collapsed, "the collapsed header closed again")
C_Reputation = nil
-- Classic Era and TBC: the older globals.
GetNumFactions = function() return #visibleFactions() end
GetFactionInfo = function(i)
    local f = visibleFactions()[i]
    return f.name, "", 4, 0, 0, f.value or 0, false, false, f.header, f.collapsed, false, false, f.parent ~= nil, f.id
end
ExpandFactionHeader = function(i) setCollapsed(i, false) end
CollapseFactionHeader = function(i) setCollapsed(i, true) end
factionRows[2].value = 4300
FG.Profile:Snapshot()
seen = points(profile.reputation)
assert(seen[69] == 4300 and seen[21] == 2900 and factionRows[3].collapsed, "reputations on Classic")
GetNumFactions, GetFactionInfo, ExpandFactionHeader, CollapseFactionHeader = nil, nil, nil, nil
print("reputations ok")

-- Quests to pick up: the next accepts not done that can be taken now (quests needed done,
-- level reached), the hand-picked one pointed at until done.
playerLevel = 21
FG.Routes["pickup-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "accept", t = "Accept First", q = 6001, m = 1426, x = 30, y = 71, lvl = 21 },
        { k = "accept", t = "Accept Follow-up", q = 6002, pre = { 6001 }, m = 1426, x = 30, y = 72, lvl = 21 },
        { k = "accept", t = "Accept Later", q = 6003, min = 30, m = 1426, x = 31, y = 71, lvl = 21 },
        { k = "accept", t = "Accept Either", q = 6004, pa = { 6005, 6001 }, m = 1426, x = 31, y = 72, lvl = 21 },
        { k = "accept", t = "Accept Close", q = 6006, m = 1426, x = 32, y = 71, lvl = 21 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
FG:Set("pickupLines", 10)
useRoute("pickup-test")
local function pickups() return table.concat(FG.Guide:Pickups(FG:Get("pickupLines")), ",") end
assert(pickups() == "1,5", "only quests that can be taken now: " .. pickups())
completed[6001] = true
assert(pickups() == "2,4,5", "their prerequisites done: " .. pickups())
FG:Set("pickupLines", 1)
assert(pickups() == "2", "as many as configured: " .. pickups())
FG.Guide:PointToStep(5)
assert(waypoint and waypoint.x == 0.32, "hand-picked step pointed at")
FG.Guide:Advance()
assert(waypoint.x == 0.32, "still pointed at after a refresh")
onQuest[6006] = true
FG.Guide:Advance()
assert(waypoint.x ~= 0.32, "back to the route once taken")
print("pickups ok")

-- Grind steps count the XP of the quests complete in the log the route turns in before the
-- level they aim at, not of those it turns in later.
playerLevel = 3
FG.Routes["pending-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "grind", t = "Be level 3 and 300 xp", lvl = 3, xp = 300 },
        { k = "turnin", t = "Turn in Soon", q = 7001, rx = 400, lvl = 3 },
        { k = "turnin", t = "Turn in Reaching 4", q = 7003, rx = 900, lvl = 4 },
        { k = "turnin", t = "Turn in Later", q = 7002, rx = 500, lvl = 5 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
local complete = {}
onQuest[7001], onQuest[7002], onQuest[7003] = true, true, true
local log = { 7002, 7003, 7001 }
C_QuestLog.GetNumQuestLogEntries = function() return #log end
C_QuestLog.GetInfo = function(i) return { questID = log[i], title = "Q" .. log[i], level = 3 } end
local isComplete = C_QuestLog.IsComplete
C_QuestLog.IsComplete = function(q) return complete[q] or false end
useRoute("pending-test")
assert(char.guide.step == 1, "not enough XP yet, step " .. char.guide.step)
complete[7002] = true
FG.Guide:Advance()
assert(char.guide.step == 1, "a quest turned in after the level does not count")
complete[7003] = true
FG.Guide:Advance()
assert(char.guide.step == 2, "the turn-in reaching the level counts, step " .. char.guide.step)
FG.Guide:Go(1)
complete[7003] = nil
complete[7001] = true
FG.Guide:Resync()
assert(char.guide.step == 2, "a quest turned in before the level counts, step " .. char.guide.step)
C_QuestLog.IsComplete = isComplete
print("pending xp ok")

-- Just accepted, the game has no objectives yet and may say the quest complete: the kill step
-- is not skipped for it (Polestar #74, Kolkar Leaders went straight to the hand-in).
FG.Routes["loading-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "accept", t = "Accept Kolkar Leaders", q = 9301, id = 1, m = 1426, x = 29.93, y = 71.2, lvl = 3 },
        { k = "objective", t = "Kill Barak", q = 9301, o = 1, n = "Barak Kodobane", lvl = 3 },
        { k = "turnin", t = "Turn in Kolkar Leaders", q = 9301, rx = 500, id = 2, m = 1426, x = 50, y = 50, lvl = 3 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
local loading, loadingDone = nil, false
local objectivesBefore, numEntries, getInfo =
    C_QuestLog.GetQuestObjectives, C_QuestLog.GetNumQuestLogEntries, C_QuestLog.GetInfo
C_QuestLog.GetQuestObjectives = function(q)
    if q == 9301 then return loading end
    return objectivesBefore(q)
end
C_QuestLog.IsComplete = function(q) return q == 9301 and (loading == nil or loadingDone) end
C_QuestLog.GetNumQuestLogEntries = function() return 1 end
C_QuestLog.GetInfo = function() return { questID = 9301, title = "Kolkar Leaders", level = 3 } end
useRoute("loading-test")
assert(char.guide.step == 1, "accept first, step " .. char.guide.step)
onQuest[9301] = true
-- QUEST_ACCEPTED would add a collector row the checks further down do not expect.
fire("QUEST_LOG_UPDATE")
assert(char.guide.step == 2, "no objectives yet: not done, step " .. char.guide.step)
loading = { { text = "", finished = false } }
fire("QUEST_LOG_UPDATE")
assert(char.guide.step == 2, "blank objectives: not done, step " .. char.guide.step)
loading = { { text = "Barak Kodobane slain: 0/1", finished = false } }
fire("QUEST_LOG_UPDATE")
assert(char.guide.step == 2, "objective not finished, step " .. char.guide.step)
loading, loadingDone = { { text = "Barak Kodobane slain: 1/1", finished = true } }, true
fire("QUEST_LOG_UPDATE")
assert(char.guide.step == 3, "objective finished, step " .. char.guide.step)
-- The same while grinding: its XP is not counted before the objectives are there.
FG.Routes["loading-test"].steps[1] = { k = "grind", t = "Be level 3 and 300 xp", lvl = 3, xp = 300 }
loading, loadingDone = nil, false
useRoute("loading-test")
FG.Guide:Go(1)
FG.Guide:Resync()
assert(char.guide.step == 1, "loading quest XP not counted, step " .. char.guide.step)
loading, loadingDone = { { text = "Barak Kodobane slain: 1/1", finished = true } }, true
FG.Guide:Resync()
assert(char.guide.step > 1, "complete quest XP counted, step " .. char.guide.step)
C_QuestLog.GetQuestObjectives, C_QuestLog.GetNumQuestLogEntries, C_QuestLog.GetInfo =
    objectivesBefore, numEntries, getInfo
C_QuestLog.IsComplete = isComplete
onQuest[9301] = nil
print("objectives loading ok")

-- XP bar mark: where the guide expects the character before the step shown, and the gap.
FG.Routes["xp-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "accept", t = "Accept A", q = 8001, lvl = 3, px = 150 },
        { k = "accept", t = "Accept B", q = 8002, lvl = 3, px = 250 },
        { k = "grind", t = "Grind", lvl = 60, px = 0 },
    },
}
playerLevel = 3
useRoute("xp-test")
onQuest[8001] = true
FG.Guide:Advance()
assert(char.guide.step == 2, "step " .. char.guide.step)
local level, xp = FG.XpMarker:Planned()
assert(level == 3 and xp == 150, "planned before step 2: " .. tostring(level) .. " " .. tostring(xp))
local unitXP = UnitXP
UnitXP = function() return 100 end
assert(FG.XpMarker:Line():find("50"), "50 xp behind: " .. FG.XpMarker:Line())
UnitXP = function() return 350 end
assert(FG.XpMarker:Line():find("200"), "200 xp ahead: " .. FG.XpMarker:Line())
local span = FG.XpMarker:SpanLine()
assert(
    span and span:find("150", 1, true) and span:find("250", 1, true),
    "current tasks from 150 to 250: " .. tostring(span)
)
playerLevel = 1
assert(FG.XpMarker:Line():find("3"), "two levels behind: the planned level")
FG.XpMarker:Refresh()
UnitXP, playerLevel = unitXP, 3
print("xp marker ok")

-- A dungeon run followed by its quests is done on entering the dungeon, then each quest step as
-- the quest progresses; without them (guides made before), once back out of the dungeon.
local inside = false
IsInInstance = function() return inside end
local function dungeonRoute(name, withQuests)
    local steps = { { k = "dungeon", t = "Dungeon: The Deadmines with a group", lvl = 21 } }
    if withQuests then steps[2] = { k = "objective", t = "Red Silk Bandanas", q = 8001, di = true, lvl = 21 } end
    steps[#steps + 1] = { k = "grind", t = "Grind", lvl = 60 }
    FG.Routes[name] =
        { race = 7, class = 8, faction = "Alliance", fromLevel = 1, toLevel = 60, time = 1, steps = steps }
end
onQuest[8001] = true
dungeonRoute("dungeon-test", true)
useRoute("dungeon-test")
assert(char.guide.step == 1, "the run first, step " .. char.guide.step)
inside = true
fire("ZONE_CHANGED_NEW_AREA")
assert(char.guide.step == 2, "run done on entering, step " .. char.guide.step)
inside = false
dungeonRoute("dungeon-old", false)
useRoute("dungeon-old")
inside = true
fire("ZONE_CHANGED_NEW_AREA")
assert(char.guide.step == 1, "an old run is not done inside")
inside = false
fire("ZONE_CHANGED_NEW_AREA")
assert(char.guide.step == 2, "an old run is done back out, step " .. char.guide.step)
onQuest[8001] = nil
print("dungeon ok")

-- Only the guide's quests: those it still takes, not those it abandons, marked done by hand or
-- out of it, nor escorts shared by the group (the event does not tell which quest).
FG.Config.autoAccept = true
FG.Routes["accept-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "abandon", t = "Abandon Unwanted (not in this guide)", q = 9001, lvl = 21 },
        { k = "accept", t = "Accept Wanted", q = 9002, lvl = 21 },
        { k = "accept", t = "Accept Skipped", q = 9003, lvl = 21 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
useRoute("accept-test")
local questID = GetQuestID
local function taken(q)
    calls.AcceptQuest = nil
    GetQuestID = function() return q end
    fire("QUEST_DETAIL", 0)
    return calls.AcceptQuest ~= nil
end
assert(not taken(9001), "a quest the guide abandons is not taken")
assert(taken(9002), "a quest of the guide is taken")
SlashCmdList.FACTORUIDE("skip 9003")
assert(not taken(9003), "a quest marked done by hand is not taken")
assert(not taken(9999), "a quest out of the guide is not taken")
-- Classic and TBC have no quests taken on sight (no QuestGetAutoAccept).
local autoAccept = QuestGetAutoAccept
QuestGetAutoAccept = nil
assert(taken(9002), "a quest of the guide is taken on Classic")
assert(not taken(9999), "a quest out of the guide is not taken on Classic")
QuestGetAutoAccept = autoAccept
-- Quest greeting: a quest of the guide is opened; one without ID (older clients) is opened too,
-- its dialog decides.
local greeting = {}
GetNumAvailableQuests = function() return #greeting end
GetNumActiveQuests = function() return 0 end
GetAvailableQuestInfo = function(i) return false, 0, false, false, greeting[i] end
SelectAvailableQuest = record("SelectAvailableQuest")
local function opened(ids)
    greeting, calls.SelectAvailableQuest = ids, nil
    fire("QUEST_GREETING")
    return calls.SelectAvailableQuest and calls.SelectAvailableQuest[1]
end
assert(opened({ 9999, 9002 }) == 2, "the guide's quest opened")
assert(opened({ 9999 }) == nil, "a quest out of the guide is not opened")
assert(opened({ 0 }) == 1, "a quest without ID is opened")
calls.ConfirmAcceptQuest = nil
fire("QUEST_ACCEPT_CONFIRM", "Friend", "Shared Escort")
assert(not calls.ConfirmAcceptQuest, "a shared escort is not confirmed")
GetQuestID = questID
print("guide quests only ok")

-- Mobs on the way: the kills of the character (or its pet) counted for the current step.
FG.Routes["farm-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = { { k = "grind", t = "Grind", lvl = 60, fk = 3 } },
}
useRoute("farm-test")
local function kill(source, dest) fire("PARTY_KILL", source, dest) end
kill("Player-1-2", "Creature-0-1-2-3-299-0000")
kill("Player-1-2", "Creature-0-1-2-3-299-0001")
kill("Player-9-9", "Creature-0-1-2-3-299-0002") -- someone else's
kill("Player-1-2", "Player-3-4") -- not a mob
assert(char.guide.farm and char.guide.farm.step == 1 and char.guide.farm.kills == 2, "two mobs on the way counted")
local rows = FG.Guide:Box().rows
local farmLine, stepLine = rows[1].count:GetText(), rows[2].name:GetText()
assert(farmLine:find("2/3", 1, true), "a line of its own with the count: " .. farmLine)
assert(not stepLine:find("2/3", 1, true) and stepLine:find("Grind", 1, true), "the step's line apart: " .. stepLine)
print("farm on the way ok")

-- Start over: asks when steps or quests were marked by hand, then undoes them.
useRoute("skip-test")
char.guide.skippedQuests[5004] = true
char.guide.marks[1] = "skip"
calls.StaticPopup_Show = nil
FG.Guide:AskReset()
assert(calls.StaticPopup_Show and calls.StaticPopup_Show[1] == "FACTORUIDE_RESET", "asked first")
FG.Guide:Reset()
for q in pairs(char.guide.skippedQuests) do
    error("quest still marked by hand: " .. q)
end
for _, mark in pairs(char.guide.marks) do
    assert(mark ~= "skip", "no step marked by hand left")
end
calls.StaticPopup_Show = nil
FG.Guide:AskReset()
assert(not calls.StaticPopup_Show, "nothing to undo: no question")
print("start over ok")

-- The next steps' tab, kept.
FG:Save("nextTab", "pickup")
assert(FG:Get("nextTab") == "pickup" and FactoruideDB.ui.nextTab == "pickup", "tab kept")
FG.Guide:Refresh()
FG:Save("nextTab", nil)
assert(FG:Get("nextTab") == "next", "default back")
print("next steps tab ok")

-- The compass: the target, the other current task, the quest to pick up close by (not the far
-- one) and the next turn-in.
FG.Routes["compass-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "objective", t = "Kill A", n = "A", q = 8001, o = 1, id = 1, m = 1426, x = 29.93, y = 71.2, lvl = 1 },
        { k = "objective", t = "Kill B", n = "B", q = 8002, o = 1, id = 1, m = 1426, x = 29.95, y = 71.25, lvl = 1 },
        { k = "turnin", t = "Turn in A", q = 8001, id = 3, m = 1426, x = 40, y = 40, lvl = 1 },
        { k = "accept", t = "Accept Close", q = 8003, id = 4, m = 1426, x = 30, y = 71.3, lvl = 1 },
        { k = "accept", t = "Accept Far", q = 8004, id = 5, m = 1426, x = 50, y = 50, lvl = 1 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
onQuest[8001], onQuest[8002] = true, true
useRoute("compass-test")
local byIndex = {}
for _, p in ipairs(FG.Compass:Points()) do
    byIndex[p.index] = p
end
assert(byIndex[1] and byIndex[1].main, "the target first")
assert(byIndex[2] and not byIndex[2].main, "the other task")
assert(byIndex[3] and byIndex[3].kind == "turnin", "the next turn-in")
assert(byIndex[4] and byIndex[4].kind == "accept", "the close quest to pick up")
assert(not byIndex[5], "not the far quest to pick up")
-- Clicking the other task's point: the guide points at it.
FG.Guide:PointToStep(2)
assert(FG.Compass:Points()[1].index == 2 and FG.Compass:Points()[1].main, "the point clicked is the target")
onQuest[8001], onQuest[8002] = nil, nil
print("compass points ok")

-- The quest panel: one line a quest in both modes; folded, no line.
local logEntries, logInfo = C_QuestLog.GetNumQuestLogEntries, C_QuestLog.GetInfo
C_QuestLog.GetNumQuestLogEntries = function() return 6 end
C_QuestLog.GetInfo = function(i) return { questID = 8100 + i, title = "Quest " .. i, level = 1 } end
FG:Set("trackerAll", true)
FG.Tracker:Refresh()
assert(#FG.Tracker.shown == 6 and FG.Tracker.lines == 6, "a line a quest, got " .. FG.Tracker.lines)
FG:Save("guideMode", "minimal")
FG.Tracker:Refresh()
assert(FG.Tracker.lines == 6, "the same with the minimal guide")
FG:Save("guideMode", nil)
-- The guide's button hides the whole panel (kept), the bank shows it while open.
FG.Tracker:Toggle()
FG.Tracker:Refresh()
assert(not FG.Tracker.visible and FactoruideDB.ui.questPanel == false, "hidden by the guide's button")
FG.Tracker:ShowItems(true)
assert(FG.Tracker.visible, "shown at the bank")
FG.Tracker:ShowItems(false)
assert(not FG.Tracker.visible, "hidden again after the bank")
FG.Tracker:Toggle()
FG.Tracker:Refresh()
assert(FG.Tracker.visible, "shown again")
FG:Set("trackerAll", false)
C_QuestLog.GetNumQuestLogEntries, C_QuestLog.GetInfo = logEntries, logInfo
print("quest panel ok")

-- The windows' size: Ctrl + mouse wheel only, the wheel alone is the camera's.
local scaleBefore = FG:Get("scale")
IsControlKeyDown = function() return false end
FG.Card:Frame():OnMouseWheel(1)
assert(FG:Get("scale") == scaleBefore, "the wheel alone does not resize")
IsControlKeyDown = function() return true end
FG.Card:Frame():OnMouseWheel(1)
assert(FG:Get("scale") > scaleBefore, "Ctrl + wheel resizes")
FG:Save("scale", nil)
print("window size ok")

-- Items to keep: the common items of the quests not turned in yet (not quest items), in the bags
-- and the bank; the bank is read when opened, an item can be marked by hand as being there.
FG.Routes["keep-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "accept", t = "Accept Cloth", q = 9001, id = 1, m = 1426, x = 29.93, y = 71.2, lvl = 1 },
        {
            k = "objective",
            t = "Linen Cloth x10",
            q = 9001,
            o = 1,
            e = "item",
            id = 2589,
            n = "Linen Cloth",
            c = 10,
            lvl = 1,
        },
        { k = "objective", t = "Wolf Meat x8", q = 9001, o = 2, e = "item", id = 750, n = "Wolf Meat", lvl = 1 },
        { k = "turnin", t = "Turn in Cloth", q = 9001, id = 1, m = 1426, x = 29.93, y = 71.2, lvl = 1 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
itemClasses[2589] = 7
onQuest[9001] = true
useRoute("keep-test")
local keep = FG.Bank:Items()
assert(#keep == 1 and keep[1].id == 2589 and keep[1].need == 10, "the cloth, not the quest item")
bags[2589], bank[2589] = 4, 6
assert(FG.Bank:Items()[1].bags == 4 and FG.Bank:Items()[1].bank == nil, "the bank not seen yet")
FG.Bank:ToggleMark(2589)
assert(FG.Bank:Items()[1].marked, "marked by hand as in the bank")
fire("BANKFRAME_OPENED")
local atBank = FG.Bank:Items()[1]
assert(atBank.bank == 6 and not atBank.marked, "the bank read, the mark no longer needed")
assert(atBank.hint == "take", "needed soon: to take")
FG.Tracker:Refresh()
assert(FG.Tracker.tab == "items", "the bank shows the items tab")
fire("BANKFRAME_CLOSED")
FG.Tracker:Refresh()
assert(FG.Tracker.tab == "quests" and FG.Bank:Items()[1].hint == nil, "back to the quests")
-- An item needed much later shows once owned (or when all are asked for).
UnitLevel = function() return 1 end
FG.Routes["keep-test"].steps[3] =
    { k = "objective", t = "Silk x4", q = 9001, o = 2, e = "item", id = 4306, n = "Silk", c = 4, lvl = 20 }
itemClasses[4306] = 7
useRoute("keep-test")
local shownNow, later = FG.Bank:Items()
assert(#shownNow == 1 and later == 1, "the silk needed at 20 waits")
assert(#FG.Bank:Items(true) == 2, "all of them when asked")
bags[4306] = 2
assert(#FG.Bank:Items() == 2, "owned: shown")
bags[4306] = nil
UnitLevel = function() return playerLevel end
completed[9001], onQuest[9001] = true, nil
assert(#FG.Bank:Items(true) == 0, "turned in: nothing to keep")
completed[9001], bags[2589], bank[2589] = nil, nil, nil
print("items to keep ok")

-- Gone back to a step already done: the minimal strip still shows it, marked done.
onQuest[9001] = true
FG:Save("guideMode", "minimal")
useRoute("keep-test")
FG.Guide:Go(1)
local strip = FG.Mini:Frame()
assert(strip.name:GetText() == "Accept Cloth", "the step gone back to: " .. tostring(strip.name:GetText()))
assert(strip.verb:GetText():find("faite", 1, true), "marked done: " .. strip.verb:GetText())
FG:Save("guideMode", nil)
onQuest[9001] = nil
print("minimal strip on a done step ok")

-- The next steps show those done already (greyed), not hidden.
FG.Routes["next-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "accept", t = "Accept A", q = 9101, id = 1, m = 1426, x = 10, y = 10, lvl = 1 },
        { k = "accept", t = "Accept B", q = 9102, id = 2, m = 1426, x = 50, y = 50, lvl = 1 },
        { k = "accept", t = "Accept C", q = 9103, id = 3, m = 1426, x = 60, y = 60, lvl = 1 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
onQuest[9102] = true
useRoute("next-test")
local nextSteps = {}
for _, v in ipairs(FG.Guide.view.next) do
    if v.index then nextSteps[v.index] = v end
end
assert(nextSteps[2] and nextSteps[2].done, "B, done already, listed as done")
assert(nextSteps[3] and not nextSteps[3].done, "C to do")
onQuest[9102] = nil
print("done next steps ok")

-- Pages: two objectives close together are one page, kept until both are done; the turn-in of
-- their quest, elsewhere, is the next page and does not show them again (its progress instead).
FG.Routes["page-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        {
            k = "objective",
            t = "Kill Black Ravager",
            n = "Black Ravager",
            q = 9201,
            o = 1,
            id = 11,
            m = 1431,
            x = 66.8,
            y = 30.5,
            lvl = 1,
        },
        {
            k = "objective",
            t = "Kill Young Black Ravager",
            n = "Young Black Ravager",
            q = 9201,
            o = 2,
            id = 12,
            m = 1431,
            x = 68.3,
            y = 28.3,
            lvl = 1,
        },
        { k = "turnin", t = "Turn in Ira's Dagger", q = 9201, id = 13, m = 1431, x = 75, y = 45, lvl = 1 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
local ravagers = {
    { text = "Black Ravager slain: 0/10", numFulfilled = 0, numRequired = 10, finished = false },
    { text = "Young Black Ravager slain: 0/10", numFulfilled = 0, numRequired = 10, finished = false },
}
local pageObjectives = C_QuestLog.GetQuestObjectives
C_QuestLog.GetQuestObjectives = function(q) return q == 9201 and ravagers or pageObjectives(q) end
onQuest[9201] = true
useRoute("page-test")
local function page()
    local out = {}
    for _, v in ipairs(FG.Guide.view.tasks) do
        table.insert(out, v.index)
    end
    return table.concat(out, ",")
end
assert(page() == "1,2", "the two objectives together: " .. page())
ravagers[1].numFulfilled, ravagers[1].finished = 10, true
fire("QUEST_LOG_UPDATE")
assert(page() == "1,2" and char.guide.step == 2, "the page stays until both are done: " .. page())
SlashCmdList.FACTORUIDE("next")
assert(page() == "3", "the turn-in alone, not its objectives again: " .. page())
local turnin = FG.Guide.view.tasks[1]
assert(turnin.have == 10 and turnin.need == 20, "the turn-in shows how far the quest is")
SlashCmdList.FACTORUIDE("prev")
assert(page() == "1,2", "back to the objectives' page: " .. page())
C_QuestLog.GetQuestObjectives = pageObjectives
onQuest[9201] = nil
print("pages ok")

-- Texts: in the language chosen in the app (Config.lua), else the game's; English without a
-- translation; Mexican Spanish as Spain's.
assert(FG.T("Accept") == "Prendre", "the game's language: " .. FG.T("Accept"))
FG.Config.locale = "deDE"
assert(FG.T("Accept") == "Annehmen" and FG.T("No such text") == "No such text", "the app's language")
FG.Config.locale = "esMX"
assert(FG.T("Accept") == "Aceptar", "Mexican Spanish")
FG.Config.locale = "enUS"
assert(FG.T("Accept") == "Accept", "English")
FG.Config.locale = nil
print("languages ok")

-- The tracker lists the guide's quests in progress even when the quest log lists none
-- (headers collapsed in the quest log window).
FG.Routes["tracker-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "accept", t = "Accept Logged", q = 7101, lvl = 1 },
        { k = "turnin", t = "Turn in Logged", q = 7101, lvl = 1 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
useRoute("tracker-test")
onQuest[7101] = true
local entries = C_QuestLog.GetNumQuestLogEntries
C_QuestLog.GetNumQuestLogEntries = function() return 0 end
FG.Tracker:Refresh()
assert(FG.Tracker.shown[1] == 7101, "guide quest listed without the log")
C_QuestLog.GetNumQuestLogEntries = entries
onQuest[7101] = nil
print("tracker from the guide ok")

-- During combat, a box made protected by a secure button hanging on it (a quest item button)
-- keeps its size: the game would block the change. Resized after the fight.
local box = FG.Guide:Box()
local resized = false
box.SetHeight = function() resized = true end
box.IsProtected = function() return true end
InCombatLockdown = function() return true end
FG.Guide:Refresh()
assert(not resized, "a protected box resized during combat")
InCombatLockdown = function() return false end
fire("PLAYER_REGEN_ENABLED")
assert(resized, "resized after the fight")
print("combat layout ok")

-- Classic Era (1.15) has no C_QuestLog log functions: the quest log is read through the older
-- globals, and the guide still draws.
local modern = {}
for _, name in ipairs({
    "GetNumQuestLogEntries",
    "GetInfo",
    "GetLogIndexForQuestID",
    "GetTitleForQuestID",
    "IsComplete",
}) do
    modern[name] = C_QuestLog[name]
    C_QuestLog[name] = nil
end
local classicLog = {
    { "Northshire", nil, nil, true, false, nil, nil, 0 },
    { "A Threat Within", 1, nil, false, false, 1, nil, 783 },
    { "Wolves Across the Border", 2, nil, false, false, nil, nil, 33 },
    { "Lost Necklace", 3, nil, false, false, -1, nil, 85 },
}
GetNumQuestLogEntries = function() return #classicLog, 3 end
GetQuestLogTitle = function(i) return unpack(classicLog[i] or {}) end
GetQuestLogIndexByID = function(q)
    for i, e in ipairs(classicLog) do
        if e[8] == q then return i end
    end
    return 0
end
local log = FG.QuestLog
assert(log.GetNumQuestLogEntries() == 4, "entries")
assert(log.GetInfo(1).isHeader and log.GetInfo(2).questID == 783, "headers and quests")
assert(log.GetLogIndexForQuestID(33) == 3 and log.GetLogIndexForQuestID(99) == nil, "log index")
assert(log.GetTitleForQuestID(33) == "Wolves Across the Border", "title")
assert(log.IsComplete(783) and not log.IsComplete(33) and log.IsFailed(85), "complete and failed")
FG.Guide:Refresh()
FG.Tracker:Refresh()
for name, fn in pairs(modern) do
    C_QuestLog[name] = fn
end
print("classic era quest log ok")

-- Collector: rows as the contribution format expects them, on Forever and Classic payloads.
-- Rows are strings (seq|time|event|quest|entityType|entityID|map|x|y|level|data): decoded here
-- into the tables they replace, as the app reads them (keys typed as in its schemas).
local BOOL_KEYS = { complete = true, trivial = true, repeatable = true, elite = true, pet = true, died = true }
local PAIR_KEYS = { items = true, nodes = true }
local TEXT_KEYS = { title = true, name = true, mode = true }
local function decodeRow(row)
    if type(row) ~= "string" then return row end
    local f = {}
    for field in (row .. "|"):gmatch("([^|]*)|") do
        f[#f + 1] = field
    end
    assert(#f == 11, "11 fields: " .. row)
    local function num(v) return v ~= "" and assert(tonumber(v), row) or false end
    local data = false
    if f[11] ~= "" then
        data = {}
        for pair in f[11]:gmatch("[^;]+") do
            local k, v = pair:match("^([^=]+)=(.*)$")
            assert(k and data[k] == nil, "data pair: " .. pair)
            if BOOL_KEYS[k] then
                assert(v == "1", "booleans written as 1: " .. pair)
                data[k] = true
            elseif PAIR_KEYS[k] then
                data[k] = {}
                for a, b in v:gmatch("(%d+):(%d+)") do
                    table.insert(data[k], { tonumber(a), tonumber(b) })
                end
            elseif TEXT_KEYS[k] then
                data[k] = v:gsub("%%(%x%x)", function(h) return string.char(tonumber(h, 16)) end)
            else
                data[k] = assert(tonumber(v), "number: " .. pair)
            end
        end
    end
    local entityType = f[5] ~= "" and f[5] or false
    return {
        num(f[1]),
        num(f[2]),
        f[3],
        num(f[4]),
        entityType,
        num(f[6]),
        num(f[7]),
        num(f[8]),
        num(f[9]),
        num(f[10]),
        data,
    }
end
local function lastRow(event)
    for i = #char.events, 1, -1 do
        local row = decodeRow(char.events[i])
        if row[3] == event then return row, i end
    end
end
local function rowsSince(event, from)
    local out = {}
    for i = from + 1, #char.events do
        local row = decodeRow(char.events[i])
        if row[3] == event then table.insert(out, row) end
    end
    return out
end
local accepted = lastRow("accept")
assert(accepted and accepted[4] == 179, "Forever accept: (questID)")

-- Classic Era and TBC: QUEST_ACCEPTED gives (questLogIndex, questID), the log has no C_QuestLog.
for name in pairs(modern) do
    C_QuestLog[name] = nil
end
fire("QUEST_ACCEPTED", 3, 33)
accepted = lastRow("accept")
assert(accepted[4] == 33, "Classic accept: quest ID, not the log index: " .. tostring(accepted[4]))
assert(accepted[11].title == "Wolves Across the Border" and accepted[11].level == 2, "Classic accept title")

-- Quest greeting: Classic Era and TBC have no GetActiveQuestID, the title is found in the log.
local greetingActive = { "Wolves Across the Border", "Not In The Log" }
GetNumActiveQuests = function() return #greetingActive end
GetActiveTitle = function(i) return greetingActive[i], false end
GetActiveQuestID = nil
greeting = {}
fire("QUEST_GREETING")
local active = lastRow("active")
assert(active and active[4] == 33 and active[5] == "npc" and active[6] == 658, "Classic greeting active quest")
GetActiveQuestID = function(i) return i == 1 and 783 or 0 end
fire("QUEST_GREETING")
assert(lastRow("active")[4] == 783, "Forever greeting active quest")
GetNumActiveQuests = function() return 0 end

-- Classic Era: the completed quests come from GetQuestsCompleted.
local allCompleted = C_QuestLog.GetAllCompletedQuestIDs
C_QuestLog.GetAllCompletedQuestIDs = nil
GetQuestsCompleted = function() return { [33] = true, [12] = true } end
fire("PLAYER_LOGOUT")
assert(char.completed[1] == 12 and char.completed[2] == 33 and #char.completed == 2, "Classic completed quests")
C_QuestLog.GetAllCompletedQuestIDs = allCompleted
for name, fn in pairs(modern) do
    C_QuestLog[name] = fn
end
print("collector quests ok")

-- Turn-ins are not recorded as removed, in either order; an abandon is.
runTimers() -- those queued so far (tracker, party, talents...)
local mark = #char.events
fire("QUEST_TURNED_IN", 4001, 100, 0)
fire("QUEST_REMOVED", 4001)
fire("QUEST_REMOVED", 4002) -- Classic: removed before turned in
fire("QUEST_TURNED_IN", 4002, 100, 0)
fire("QUEST_REMOVED", 4003)
runTimers()
local removed = rowsSince("removed", mark)
assert(#removed == 1 and removed[1][4] == 4003 and removed[1][11] == false, "only the abandon is removed")
print("collector removed ok")

-- Loot: read before fast loot takes the slots, one row per corpse with item quantities (AoE
-- loot), money skipped, a window read once.
local slots = {
    { link = nil, sources = { "Creature-0-1-2-3-299-00A", 1 } }, -- money
    { link = "|Hitem:750::|h", sources = { "Creature-0-1-2-3-299-00A", 1, "Creature-0-1-2-3-300-00B", 2 } },
    { link = "|Hitem:2589::|h", sources = { "Creature-0-1-2-3-299-00A", 3 } },
}
GetNumLootItems = function() return #slots end
GetLootSlotLink = function(i) return slots[i] and slots[i].link end
GetLootSourceInfo = function(i) return unpack(slots[i] and slots[i].sources or {}) end
LootSlot = function(i) slots[i] = nil end
local clock = GetTime
GetTime = function() return clock() + 5000 end
mark = #char.events
fire("LOOT_READY", false)
assert(not slots[1] and not slots[2] and not slots[3], "fast loot took the slots")
local loot = rowsSince("loot", mark)
assert(#loot == 2, "one loot row per corpse, got " .. #loot)
local a, b = loot[1][11].items, loot[2][11].items
assert(
    loot[1][6] == 299 and #a == 2 and a[1][1] == 750 and a[1][2] == 1 and a[2][1] == 2589 and a[2][2] == 3,
    "corpse A"
)
assert(loot[2][6] == 300 and #b == 1 and b[1][1] == 750 and b[1][2] == 2, "corpse B: its own quantity")
fire("LOOT_READY", false)
fire("LOOT_OPENED", false)
assert(#rowsSince("loot", mark) == 2, "a loot window recorded once")
GetTime = clock
print("collector loot ok")

-- Flight master: node IDs and states only, bounded; Classic: the continent's map.
mark = #char.events
fire("TAXIMAP_OPENED")
local taxi = rowsSince("taxi", mark)[1][11]
assert(taxi.map == 1415 and taxi.nodes[1][1] == 6 and taxi.nodes[1][2] == 0 and taxi.nodes[3][2] == 2, "taxi nodes")
local manyNodes = {}
for i = 1, 200 do
    manyNodes[i] = { nodeID = i, name = "A long flight master name " .. i, slotIndex = i }
end
C_TaxiMap.GetAllTaxiNodes = function() return manyNodes end
TaxiNodeGetType = function(slot) return slot == 1 and "CURRENT" or slot == 2 and "REACHABLE" or "DISTANT" end
GetTaxiMapID = nil
fire("TAXIMAP_OPENED")
taxi = lastRow("taxi")[11]
assert(taxi.map == 1415, "Classic: continent map " .. tostring(taxi.map))
assert(#taxi.nodes == 128 and #taxi.nodes[1] == 2, "nodes bounded, ids and states only")
assert(taxi.nodes[1][2] == 0 and taxi.nodes[2][2] == 1 and taxi.nodes[3][2] == 2, "Classic node states")
print("collector taxi ok")

-- Transit: a boat ride is timed from the docked boat, a hearthstone is told apart.
local now, speed, here = 10000, 7, { 1437, 0.10, 0.10 }
local clockTime = time
time = function() return now end
GetUnitSpeed = function() return speed end
local continents = { [1415] = true, [1414] = true }
C_Map.GetMapInfo = function(m)
    if continents[m] then return { mapID = m, mapType = 2, parentMapID = 947 } end
    return { mapID = m, mapType = 3, parentMapID = m < 1415 and 1414 or 1415 }
end
C_Map.GetBestMapForUnit = function() return here[1] end
C_Map.GetPlayerMapPosition = function()
    return {
        GetXY = function() return here[2], here[3] end,
    }
end
local function sample(t, map, x, y, v)
    now, here, speed = t, { map, x, y }, v
    fire("ZONE_CHANGED_NEW_AREA")
end
mark = #char.events
sample(10000, 1437, 0.10, 0.10, 7) -- walking on the dock
sample(10005, 1437, 0.105, 0.10, 0) -- on the boat
sample(10010, 1437, 0.105, 0.10, 0) -- waiting on board
sample(10015, 1437, 0.12, 0.10, 0) -- the boat leaves
sample(10020, 1437, 0.14, 0.10, 0)
sample(10090, 1411, 0.50, 0.50, 0) -- landed on the other continent
local transit = rowsSince("transit", mark)
assert(#transit == 1, "one transit")
local ride = transit[1][11]
assert(ride.mode == "ride" and ride.seconds == 80 and ride.fromMap == 1437 and ride.fromX == 10.5, "boat ride")
sample(10100, 1411, 0.50, 0.50, 0)
fire("UNIT_SPELLCAST_SUCCEEDED", "player", "Cast-1", 8690)
sample(10130, 1426, 0.30, 0.70, 0)
local hearth = lastRow("transit")[11]
assert(hearth.mode == "hearth" and hearth.seconds == 30 and hearth.fromMap == 1411, "hearthstone")
sample(10200, 1426, 0.30, 0.71, 7) -- walking into a portal
sample(10203, 1411, 0.40, 0.40, 0)
local portal = lastRow("transit")[11]
assert(portal.mode == nil and portal.seconds == 3 and portal.fromY == 71, "walked in: no mode")
time = clockTime
print("collector transit ok")

-- Phase 2 recordings: fights, objective progress, guide steps, flights, spawn ages; Forever
-- secret values degrade to the fallbacks without errors.
local clockNow = 50000
local realGetTime = GetTime
GetTime = function() return clockNow end
GetServerTime = function() return 1000000 end
local function spawnGUID(npc, age) return string.format("Creature-0-1-2-3-%d-%010X", npc, (1000000 - age) % 8388608) end
local guids, levels, classes, exists = {}, {}, {}, {}
local unitGUID, unitLevel = UnitGUID, UnitLevel
UnitGUID = function(u)
    if guids[u] ~= nil then return guids[u] or nil end
    return unitGUID(u)
end
UnitLevel = function(u)
    if u == "player" then return playerLevel end
    return levels[u] or 1
end
UnitExists = function(u) return exists[u] or false end
UnitClassification = function(u) return classes[u] or "normal" end
UnitIsDead = function() return false end
UnitCanAttack = function() return true end
UnitName = function(u) return u == "player" and "Brouz" or "Kobold Vermin" end
UnitReaction = function() return 2 end
local secret = {}
issecretvalue = function(v) return v ~= nil and secret[v] == true end
local xpNow, xpMax = 100, 400
UnitXP = function() return xpNow end
UnitXPMax = function() return xpMax end
COMBATLOG_XPGAIN_FIRSTPERSON = "%s dies, you gain %d experience."
COMBATLOG_XPGAIN_EXHAUSTION1 = "%s dies, you gain %d experience. (%s exp %s bonus)"
playerLevel = 5

-- Target read out of combat: a seen row with its spawn age and elite.
local koboldGUID = spawnGUID(299, 90)
guids.target, levels.target, classes.target, exists.target = koboldGUID, 5, "elite", true
here = { 1426, 0.30, 0.70 }
mark = #char.events
fire("PLAYER_TARGET_CHANGED")
local seenRow = rowsSince("seen", mark)[1]
assert(seenRow and seenRow[6] == 299 and seenRow[11].age == 90 and seenRow[11].elite == true, "seen: age and elite")
assert(seenRow[11].name == "Kobold Vermin" and seenRow[11].level == 5, "seen: name and level")
-- The same NPC close by: one row per 5% of the map; elsewhere, a row without the name again.
guids.mouseover, exists.mouseover = spawnGUID(299, 30), true
here = { 1426, 0.301, 0.701 }
fire("UPDATE_MOUSEOVER_UNIT")
fire("UPDATE_MOUSEOVER_UNIT")
here = { 1426, 0.50, 0.50 }
fire("UPDATE_MOUSEOVER_UNIT")
local seenRows = rowsSince("seen", mark)
assert(#seenRows == 2 and seenRows[2][11].name == nil and seenRows[2][11].age == 30, "seen deduped, name once")
-- A secret GUID (Forever, restricted): nothing recorded, no error.
local hiddenGUID = spawnGUID(300, 5)
secret[hiddenGUID] = true
guids.mouseover = hiddenGUID
here = { 1426, 0.70, 0.70 }
fire("UPDATE_MOUSEOVER_UNIT")
assert(#rowsSince("seen", mark) == 2, "secret GUID: no seen row")
-- A static NPC up since the realm restart: seen without its age.
guids.mouseover = spawnGUID(302, 7200)
here = { 1426, 0.90, 0.90 }
fire("UPDATE_MOUSEOVER_UNIT")
local static = rowsSince("seen", mark)[3]
assert(static and static[6] == 302 and static[11].age == nil, "no age past an hour")
-- Safety alerts on a target whose level is secret (a table here: comparing it errors, as a
-- secret does): not compared, no error (checked below).
local secretLevel = {}
secret[secretLevel] = true
guids.target, levels.target = spawnGUID(301, 5), secretLevel
fire("PLAYER_TARGET_CHANGED")
guids.target, levels.target = koboldGUID, 5
fire("PLAYER_TARGET_CHANGED")

-- Fight 1: the target turns secret when the fight begins (the GUID read before is used), the
-- kill is told by the XP message only (secret attacker on PARTY_KILL).
guids.pet, exists.pet = "Pet-0-1-2-3-416-0000000001", true
here = { 1426, 0.40, 0.40 }
secret[koboldGUID] = true
mark = #char.events
fire("PLAYER_REGEN_DISABLED")
secret["Player-1-2"] = true
fire("PARTY_KILL", "Player-1-2", koboldGUID)
secret["Player-1-2"] = nil
fire("CHAT_MSG_COMBAT_XP_GAIN", "Kobold Vermin dies, you gain 45 experience. (45 exp Rested bonus)")
fire("CHAT_MSG_COMBAT_XP_GAIN", "You gain 120 experience.") -- not a kill
fire("PLAYER_TARGET_DIED")
xpNow, clockNow = 145, clockNow + 12
fire("PLAYER_REGEN_ENABLED")
local fight = rowsSince("combat", mark)[1]
assert(fight and fight[5] == "npc" and fight[6] == 299, "fight against the target read before")
assert(fight[7] == 1426 and fight[8] == 40 and fight[9] == 40, "where the fight began")
local f = fight[11]
assert(f.seconds == 12 and f.kills == 1 and f.xp == 45 and f.mobLevel == 5, "fight 1: " .. tostring(f.kills))
assert(f.elite == true and f.pet == true and f.died == nil, "fight 1 flags")
secret[koboldGUID] = nil

-- Fight 2: PARTY_KILL readable (player then pet), counted once each despite the XP messages;
-- XP across a level up.
local wolf1, wolf2 = spawnGUID(525, 600), spawnGUID(525, 50000) -- wolf2: up since the restart
guids.target, levels.target, classes.target = wolf1, 6, "normal"
xpNow, xpMax = 380, 400
mark = #char.events
fire("PLAYER_REGEN_DISABLED")
fire("PARTY_KILL", "Player-1-2", wolf1)
fire("CHAT_MSG_COMBAT_XP_GAIN", "Mangy Wolf dies, you gain 50 experience.")
here = { 1426, 0.42, 0.40 } -- kill rows: one per NPC and spot
fire("PARTY_KILL", "Pet-0-1-2-3-416-0000000001", wolf2)
fire("PARTY_KILL", "Player-9-9", spawnGUID(525, 800)) -- someone else's
playerLevel, xpNow, clockNow = 6, 30, clockNow + 20
fire("PLAYER_REGEN_ENABLED")
f = rowsSince("combat", mark)[1][11]
assert(f.kills == 2 and f.xp == 50 and f.mobLevel == 6 and f.elite == nil, "fight 2: kills " .. tostring(f.kills))
local kills = rowsSince("kill", mark)
assert(#kills == 2 and kills[1][11].age == 600, "kill rows with spawn age")
assert(kills[2][11] == false, "no age past an hour (it would tell the time of day)")

-- Fight 3: nothing killed, short: no row. Fight 4: target died, the player too.
exists.target, exists.pet = false, false
mark = #char.events
fire("PLAYER_REGEN_DISABLED")
clockNow = clockNow + 1
fire("PLAYER_REGEN_ENABLED")
assert(#rowsSince("combat", mark) == 0, "short fight without kill skipped")
fire("PLAYER_REGEN_DISABLED")
fire("PLAYER_TARGET_DIED")
fire("PLAYER_DEAD")
clockNow = clockNow + 8
fire("PLAYER_REGEN_ENABLED")
f = rowsSince("combat", mark)[1]
assert(f[6] == false and f[11].kills == 1 and f[11].died == true and f[11].pet == nil, "fight 4")
-- In an instance: nothing.
IsInInstance = function() return true end
mark = #char.events
fire("PLAYER_REGEN_DISABLED")
fire("PLAYER_TARGET_DIED")
clockNow = clockNow + 10
fire("PLAYER_REGEN_ENABLED")
IsInInstance = function() return false end
assert(#rowsSince("combat", mark) == 0, "no fight recorded in instances")
print("collector combat ok")

-- Objective progress: a row when a count goes up, the log read once per burst of events.
local objectives = { [179] = { { numFulfilled = 2, numRequired = 8 } } }
local questLog = { 179 }
C_QuestLog.GetNumQuestLogEntries = function() return #questLog end
C_QuestLog.GetInfo = function(i) return { questID = questLog[i], title = "Q" .. questLog[i], level = 2 } end
C_QuestLog.GetQuestObjectives = function(q) return objectives[q] end
fire("QUEST_LOG_UPDATE")
runTimers()
mark = #char.events
objectives[179][1].numFulfilled = 4
objectives[180] = { { numFulfilled = 5, numRequired = 8 } }
questLog[2] = 180 -- accepted with some done already: not progress
fire("QUEST_LOG_UPDATE")
fire("QUEST_WATCH_UPDATE")
fire("UNIT_QUEST_LOG_CHANGED", "player")
local queued = #timers
runTimers()
local progress = rowsSince("progress", mark)
assert(#progress == 1 and progress[1][4] == 179, "one progress row, got " .. #progress)
assert(progress[1][11].index == 1 and progress[1][11].value == 4, "objective 1 at 4")
objectives[180][1].numFulfilled = 6
fire("UNIT_QUEST_LOG_CHANGED", "target")
runTimers()
assert(#rowsSince("progress", mark) == 1, "another unit's quest log: not read")
fire("QUEST_LOG_UPDATE")
assert(queued > 0 and #rowsSince("progress", mark) == 1, "read a moment later")
runTimers()
assert(#rowsSince("progress", mark) == 2, "the new quest's progress")
print("collector progress ok")

-- Guide steps: done while playing recorded with their index, not those marked by hand.
FG.Routes["step-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "accept", t = "Accept One", q = 9101, lvl = 6 },
        { k = "objective", t = "Boar Meat x4", q = 9101, o = 1, n = "Boar Meat", lvl = 6, d = 240 },
        { k = "train", t = "Train", lvl = 6 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
objectives[9101] = { { text = "Boar Meat: 2/4", numFulfilled = 2, numRequired = 4, finished = false } }
useRoute("step-test")
mark = #char.events
onQuest[9101] = true
FG.Guide:Advance()
assert(char.guide.step == 2 and #rowsSince("step", mark) == 0, "an accept has its own row")
objectives[9101][1].finished = true
FG.Guide:Advance()
local steps = rowsSince("step", mark)
assert(#steps == 1 and steps[1][4] == 9101, "step done while playing")
assert(steps[1][11].step == 2 and steps[1][11].planned == 240, "its index and planned time")
SlashCmdList.FACTORUIDE("skip")
FG.Guide:Advance()
assert(char.guide.step == 4 and #rowsSince("step", mark) == 1, "a step marked by hand is not recorded")
onQuest[9101] = nil
print("collector steps ok")

-- Flights: from the flight master to the landing.
local onTaxi = false
UnitOnTaxi = function() return onTaxi end
here = { 1426, 0.30, 0.70 }
mark = #char.events
fire("PLAYER_CONTROL_LOST")
onTaxi = true
runTimers()
clockNow = clockNow + 125
onTaxi = false
here = { 1453, 0.60, 0.40 }
fire("PLAYER_CONTROL_GAINED")
local flightRow = rowsSince("flight", mark)[1]
assert(flightRow and flightRow[7] == 1453 and flightRow[8] == 60, "landing position")
assert(flightRow[11].fromMap == 1426 and flightRow[11].fromX == 30 and flightRow[11].seconds == 125, "flight")
fire("PLAYER_CONTROL_LOST") -- stunned, not a flight
runTimers()
fire("PLAYER_CONTROL_GAINED")
assert(#rowsSince("flight", mark) == 1, "control lost without a flight")
print("collector flight ok")

-- Quest details carry the XP reward when the client gives it.
GetRewardXP = function() return 450 end
GetQuestID = function() return 9201 end
fire("QUEST_DETAIL", 0)
GetQuestID = questID
assert(lastRow("detail")[4] == 9201 and lastRow("detail")[11].xp == 450, "detail XP")
print("collector detail ok")

-- Rows are strings; game text is percent-encoded where it would break the format.
C_QuestLog.GetLogIndexForQuestID = function() return 1 end
C_QuestLog.GetInfo = function() return { questID = 9301, title = "A|B;C=D%E\nF", level = 7 } end
fire("QUEST_ACCEPTED", 9301)
local raw = char.events[#char.events]
assert(type(raw) == "string" and raw:find("|accept|9301|", 1, true), "a string row: " .. tostring(raw))
assert(raw:find("level=7;title=A%7CB%3BC%3DD%25E%0AF", 1, true), "text encoded: " .. raw)
assert(lastRow("accept")[11].title == "A|B;C=D%E\nF", "and read back")
print("collector rows ok")

GetTime = realGetTime
issecretvalue = function() return false end

-- Handler errors are counted in the saved variables, a few short messages.
for msg in pairs(FactoruideDB.errors or {}) do
    error("handler error: " .. msg)
end
FG:On("FG_TEST", function() error("boom") end)
fire("FG_TEST")
fire("FG_TEST")
local errorKey, errorCount = pairs(FactoruideDB.errors)(FactoruideDB.errors)
assert(errorKey:find("FG_TEST", 1, true) and errorKey:find("boom", 1, true) and errorCount == 2, "error counted")
for i = 1, 30 do
    FG:Error("test", i .. string.rep("x", 300))
end
local kept = 0
for msg in pairs(FactoruideDB.errors) do
    kept = kept + 1
    assert(#msg <= 200, "message shortened")
end
assert(kept == 20, "errors capped, got " .. kept)
print("error counter ok")

-- Rows the app has (Sent.lua) are dropped at login, the seq keeps growing; at most 20000 rows.
FactoruideSent = { ["Brouz-Realm"] = decodeRow(char.events[3])[1] }
local before = #char.events
FG.Collector:Purge()
assert(#char.events == before - 3 and decodeRow(char.events[1])[1] > FactoruideSent["Brouz-Realm"], "sent rows dropped")
local output
local realPrint = print
print = function(s) output = s end
SlashCmdList.FACTORUIDE("status")
print = realPrint
assert(output:find(#char.events .. " not sent yet", 1, true), "status: " .. output)
for i = 1, 20010 do
    char.events[#char.events + 1] = { char.seq + i, 0, "kill", false, "npc", 6, false, false, false, 1, false }
end
char.seq = char.seq + 20010
FactoruideSent = { ["Brouz-Realm"] = char.seq + 50 } -- saved variables older than what the app has
FG.Collector:Purge()
assert(#char.events == 0 and char.seq == FactoruideSent["Brouz-Realm"], "all sent, the seq follows")
FactoruideSent = nil
FactoruideDB.errors = nil -- full after the error counter test
for i = 1, 20010 do
    char.events[i] = { char.seq + i, 0, "kill", false, "npc", 6, false, false, false, 1, false }
end
char.seq = char.seq + 20010
FG.Collector:Purge()
assert(#char.events == 20000 and char.events[20000][1] == char.seq, "capped, the newest kept")
local capLogged = false
for msg in pairs(FactoruideDB.errors) do
    if msg:find("oldest dropped", 1, true) then capLogged = true end
end
assert(capLogged, "the cap is logged")
print("collector purge ok")
