-- Smoke test of the addon outside the game: minimal WoW API stubs, load files in TOC
-- order, fire login and quest events, check the guide advances.
-- Run from the repository root: luajit addon/tests/smoke.lua

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
    return { GetXY = function() return x, y end }
end
MinimalSliderWithSteppersMixin = { Label = {} }
local settingsRegistered = 0
Settings = {
    VarType = { Boolean = "boolean", Number = "number" },
    RegisterVerticalLayoutCategory = function()
        return { GetID = function() return 1 end }
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
ObjectiveTrackerFrame = {
    hidden = false,
    Hide = function(self) self.hidden = true end,
    IsProtected = function() return false end,
    HookScript = function() end,
    Update = function() end,
}
GetQuestLogSpecialItemInfo = function() return nil end
GetDifficultyColor = function() return { r = 1, g = 0.8, b = 0 } end
C_PlayerInfo = { GetContentDifficultyQuestForPlayer = function() return 1 end }
GameTooltip = { SetOwner = function() end, SetHyperlink = function() end, Show = function() end, Hide = function() end }
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
C_Timer = { NewTicker = function() end }
C_Map = {
    GetBestMapForUnit = function() return 1426 end,
    GetMapInfo = function(m) return { mapID = m, mapType = m == 1415 and 2 or 3, parentMapID = 1415 } end,
    GetPlayerMapPosition = function()
        return { GetXY = function() return 0.2993, 0.712 end }
    end,
    CanSetUserWaypointOnMap = function() return true end,
    SetUserWaypoint = function(p) waypoint = p end,
    ClearUserWaypoint = function() waypoint = nil end,
    GetWorldPosFromMapPos = function(m, v)
        local x, y = v:GetXY()
        return 0, CreateVector2D(x * 1000, y * 1000)
    end,
}
UiMapPoint = { CreateFromCoordinates = function(m, x, y) return { m = m, x = x, y = y } end }
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

local FG = {}
for line in io.lines("addon/Factoruide/Factoruide.toc") do
    -- Config.lua and Routes.lua are written by the app and not versioned: use a fixture instead.
    if line == "Config.lua" then
        FG.Config = {}
    elseif line == "Routes.lua" then
        assert(loadfile("addon/tests/fixtures/Routes.lua"))("Factoruide", FG)
    elseif line:match("%.lua$") then
        local chunk = assert(loadfile("addon/Factoruide/" .. line))
        chunk("Factoruide", FG)
    end
end

local function fire(event, ...)
    for _, f in ipairs(frames) do
        if f.OnEvent and f.events[event] then f.OnEvent(f, event, ...) end
    end
end

fire("ADDON_LOADED", "Factoruide")
fire("PLAYER_LOGIN")
local char = FactoruideDB.characters["Brouz-Realm"]
assert(char.guide.route == "gnome-mage", "route picked: " .. tostring(char.guide.route))
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

SlashCmdList.FACTORUIDE("prev")
assert(char.guide.step == 3)
SlashCmdList.FACTORUIDE("status")
assert(#char.events >= 2, "collector recorded events")
print("smoke test ok: step " .. char.guide.step .. ", " .. #char.events .. " events")

-- Going back to a done step stays there (refresh and quest events), the resync goes forward.
FG.Guide:Advance()
assert(char.guide.step == 3, "stays after manual navigation")
fire("QUEST_LOG_UPDATE")
assert(char.guide.step == 3, "quest events do not move a manual position")
FG.Guide:Resync()
assert(char.guide.step == 4, "resync goes to the first step not done")
print("navigation ok")

-- Automation
fire("GOSSIP_SHOW")
assert(calls.SelectAvailableQuest and calls.SelectAvailableQuest[1] == 233, "gossip quest selected")
fire("QUEST_DETAIL", 0)
assert(calls.AcceptQuest, "quest accepted")
fire("QUEST_PROGRESS")
assert(calls.CompleteQuest, "quest completed")
fire("QUEST_COMPLETE")
assert(calls.GetQuestReward and calls.GetQuestReward[1] == 1, "single reward taken")
fire("LOOT_READY", false)
assert(calls.loot == 3, "all loot slots taken")
SlashCmdList.FACTORUIDE("lines 2")
assert(FG:Get("nextLines") == 2, "session tweak")

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
SlashCmdList.FACTORUIDE("route prof-test")
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
SlashCmdList.FACTORUIDE("route skip-test")
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
SlashCmdList.FACTORUIDE("route other-test")
assert(char.guide.step == 1 and not char.guide.skippedQuests[5004], "marked in another guide only")
SlashCmdList.FACTORUIDE("route skip-test")
assert(char.guide.step == 5 and char.guide.skippedQuests[5004], "back to the first guide")
char.guide.marks[1] = "skip"
FG.Routes["skip-test"].time = 2
SlashCmdList.FACTORUIDE("route other-test")
SlashCmdList.FACTORUIDE("route skip-test")
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
        { k = "abandon", t = "Abandon Unused Quest", q = 7001, lvl = 21 },
        { k = "accept", t = "Accept Current Quest", q = 5004, lvl = 21 },
    },
}
onQuest[7001] = true
SlashCmdList.FACTORUIDE("route abandon-test")
assert(char.guide.step == 1, "waits while the quest is in the log, step " .. char.guide.step)
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
SlashCmdList.FACTORUIDE("route checkpoint-test")
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
SlashCmdList.FACTORUIDE("route turnin-test")
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
SlashCmdList.FACTORUIDE("route bg-test")
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
SlashCmdList.FACTORUIDE("route macro-test")
SlashCmdList.FACTORUIDE("macro")
local index = GetMacroIndexByName("FG Cible")
assert(index > 0, "target macro created")
assert(
    macros[index].body:find("/targetexact Kobold Vermin", 1, true),
    "macro targets the step's mobs: " .. macros[index].body
)
SlashCmdList.FACTORUIDE("next")
assert(
    macros[index].body:find("/targetexact Marshal McBride", 1, true),
    "macro follows the step: " .. macros[index].body
)
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
SlashCmdList.FACTORUIDE("route timing-test")
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
SlashCmdList.FACTORUIDE("route fly-test")
for _, f in ipairs(frames) do
    if f.events.TAXIMAP_OPENED and f.OnEvent then f:OnEvent("TAXIMAP_OPENED") end
end
assert(taken == 3, "flight to the step's node taken")
print("auto fly ok")

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
assert(skinned.window == 1, "guide window skinned")
assert((skinned.panel or 0) >= 1, "panels skinned")
assert(#FactoruideAPI.GetFrames() >= 2)
assert(FG.Skin:Suite() == nil, "no UI suite in the tests")
print("skins ok")

-- Arrow: straight ahead, to the left, behind (angles counter-clockwise, as the game's facing).
local close = function(a, b) return math.abs(a - b) < 1e-9 end
assert(close(FG.Arrow.Angle(10, 0, 0), 0), "north, facing north: ahead")
assert(close(FG.Arrow.Angle(0, 10, 0), math.pi / 2), "west, facing north: to the left")
assert(close(FG.Arrow.Angle(0, -10, math.pi / 2), math.pi), "east, facing west: behind")
assert(close(FG.Arrow.Angle(0, 10, math.pi / 2), 0), "west, facing west: ahead")
print("arrow ok")

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
