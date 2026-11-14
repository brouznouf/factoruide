-- The character's profile, read by the app to plan a guide from where the character stands:
-- level and XP, quests done and in the log (with their objectives), position, hearthstone,
-- flight paths, professions, riding, gear, spells, reputations and money. Saved at each logout and reload;
-- "Save the profile now" (the addon's options) saves it at once (the interface reloads to write it to disk).
--
-- FactoruideDB.profiles["Name-Realm"] = { level, xp, completed = { questID... }, log = {...} ... }

local _, FG = ...
local QuestLog = FG.QuestLog
local Profile = {}
FG.Profile = Profile
local T = FG.T

-- Professions (skill lines) and riding.
local SKILL_LINES = { 171, 164, 333, 202, 182, 165, 186, 393, 197, 185, 356, 129 }
local RIDING = 762
-- C_TaxiMap node state of a flight path the character does not know.
local UNKNOWN_FLIGHT = Enum and Enum.FlightPathState and Enum.FlightPathState.Unreachable or 2

local profile -- this character's saved profile

local function CompletedQuests() return QuestLog.GetAllCompletedQuestIDs() or {} end

--- Quests in the log, with what their objectives have so far.
local function LogQuests()
    local log = {}
    for i = 1, QuestLog.GetNumQuestLogEntries() do
        local info = QuestLog.GetInfo(i)
        local id = info and not info.isHeader and info.questID
        if id and id > 0 then
            local objectives = {}
            for _, o in ipairs(QuestLog.GetQuestObjectives and QuestLog.GetQuestObjectives(id) or {}) do
                table.insert(objectives, {
                    text = o.text or "",
                    done = o.numFulfilled or 0,
                    need = o.numRequired or 0,
                    finished = o.finished and true or false,
                })
            end
            local complete = QuestLog.IsComplete and QuestLog.IsComplete(id)
            table.insert(log, { id = id, complete = complete and true or false, objectives = objectives })
        end
    end
    return log
end

local function Professions()
    local out = {}
    for _, line in ipairs(SKILL_LINES) do
        local rank, cap = FG.ProfessionSkill(line)
        if rank and rank > 0 then table.insert(out, { line = line, rank = rank, cap = cap }) end
    end
    return out
end

local function Gear()
    local out = {}
    for slot = 1, 19 do
        local id = GetInventoryItemID("player", slot)
        if id then table.insert(out, id) end
    end
    return out
end

--- Spells of the spell book (every rank the character learned).
local function Spells()
    local out = {}
    if not (GetNumSpellTabs and GetSpellTabInfo and GetSpellBookItemInfo) then return out end
    for tab = 1, GetNumSpellTabs() do
        local _, _, offset, count = GetSpellTabInfo(tab)
        for i = offset + 1, offset + count do
            local kind, id = GetSpellBookItemInfo(i, BOOKTYPE_SPELL or "spell")
            if kind == "SPELL" and id then table.insert(out, id) end
        end
    end
    return out
end

--- The AreaTable IDs whose name (in the game's language) is `name`: the app only knows English
--- names, an ID tells it which inn the hearthstone is bound to.
local function AreasNamed(name)
    local out = {}
    local info = C_Map and C_Map.GetAreaInfo
    if not (name and info) then return out end
    for id = 1, 6000 do
        local ok, found = pcall(info, id)
        if ok and found == name then table.insert(out, id) end
    end
    return out
end

--- Keep the hearthstone's place: its name and area, and where the character stands when `here`
--- (just bound: next to the innkeeper). Returns whether it changed.
local function UpdateBind(here)
    local name = GetBindLocation and GetBindLocation()
    if not name then return false end
    local bind = profile.bind
    if bind and bind.name == name then
        if bind.areas then return false end
        bind.areas = AreasNamed(name)
        return true
    end
    profile.bind = { name = name, areas = AreasNamed(name) }
    if here then
        local map, x, y = FG.PlayerPosition()
        if x then profile.bind.position = { map = map, x = x, y = y } end
    end
    return true
end

--- One part of the profile; when the game fails to give it (at logout some data may already be
--- gone), the previous value stays and the error is kept for debugging (`profile.errors`).
local function Part(name, fn)
    local ok, value = pcall(fn)
    if ok then return value end
    profile.errors = profile.errors or {}
    profile.errors[name] = tostring(value)
    return profile[name]
end

-- The parts that cost the most to read (thousands of quests, the reputation panel opened and
-- closed, the spell book), read again by the minute's snapshot only when the game said they
-- changed; always on a full one (login, reload, logout). Opening the reputation headers makes
-- the game tell every addon and its own panel, each minute, for nothing.
local changed = { completed = true, reputation = true, spells = true }
local CHANGED_BY = {
    QUEST_TURNED_IN = "completed",
    CHAT_MSG_COMBAT_FACTION_CHANGE = "reputation",
    SPELLS_CHANGED = "spells",
}
for event, part in pairs(CHANGED_BY) do
    FG:On(event, function() changed[part] = true end)
end

--- Read part `name` with `fn` when `full` or the game said it changed (else keep it as it is).
local function Changed(name, fn, full)
    if not (full or changed[name]) then return profile[name] end
    changed[name] = false
    return Part(name, fn)
end

--- Record everything the app needs about the character now; `full` false (the minute's
--- snapshot): the costly parts only when they changed.
function Profile:Snapshot(full)
    if not profile then return end
    full = full ~= false
    profile.errors = nil
    profile.name = FG:CharacterKey()
    Part("character", function()
        local _, _, raceID = UnitRace("player")
        local _, classFile = UnitClass("player")
        profile.race = raceID
        profile.class = classFile
        profile.faction = UnitFactionGroup("player")
        profile.edition = FG.Edition
        profile.level = UnitLevel("player")
        profile.xp = UnitXP("player")
        profile.xp_max = UnitXPMax("player")
        profile.rested = GetXPExhaustion() or 0
        profile.resting = IsResting() and true or false
        profile.money = GetMoney()
        profile.instance = IsInInstance() and true or false
    end)
    profile.position = Part("position", function()
        local map, x, y = FG.PlayerPosition()
        return x and { map = map, x = x, y = y } or nil
    end)
    profile.completed = Changed("completed", CompletedQuests, full)
    profile.log = Part("log", LogQuests)
    profile.professions = Part("professions", Professions)
    profile.riding = Part("riding", function() return FG.ProfessionSkill(RIDING) or 0 end)
    profile.gear = Part("gear", Gear)
    profile.spells = Changed("spells", Spells, full)
    profile.reputation = Changed("reputation", FG.Reputations, full)
    profile.flights = profile.flights or {}
    Part("bind", function() UpdateBind(false) end)
    profile.time = time()
end

--- Save the profile now and reload the interface, which writes it to disk for the app (the
--- window's "Save the profile now").
function Profile:Save()
    self:Snapshot()
    FG:Print(
        T("profile saved (level %d, %d quests done, %d in the log), reloading the interface"),
        profile.level,
        #profile.completed,
        #profile.log
    )
    ReloadUI()
end

FG:On("PLAYER_LOGIN", function()
    local key = FG:CharacterKey()
    FactoruideDB.profiles = FactoruideDB.profiles or {}
    FactoruideDB.profiles[key] = FactoruideDB.profiles[key] or {}
    profile = FactoruideDB.profiles[key]
    Profile:Snapshot()
    -- Kept up to date while playing: the profile on disk is the one of the last minute even
    -- when the logout one fails.
    C_Timer.NewTicker(60, function() FG:Try("profile", Profile.Snapshot, Profile, false) end)
end)

FG:On("PLAYER_LEAVING_WORLD", function() Profile:Snapshot() end)
FG:On("PLAYER_LOGOUT", function() Profile:Snapshot() end)

-- Binding the hearthstone says so in the chat: the character stands next to the innkeeper.
FG:On("CHAT_MSG_SYSTEM", function()
    if profile then UpdateBind(true) end
end)

-- Flight paths are only known from a flight master's map: remember those seen, for good.
FG:On("TAXIMAP_OPENED", function()
    if not profile or not C_TaxiMap or not C_TaxiMap.GetAllTaxiNodes then return end
    local mapID = GetTaxiMapID and GetTaxiMapID() or C_Map.GetBestMapForUnit("player")
    if not mapID then return end
    profile.flights = profile.flights or {}
    for _, node in ipairs(C_TaxiMap.GetAllTaxiNodes(mapID)) do
        if node.nodeID and node.state ~= UNKNOWN_FLIGHT then profile.flights[node.nodeID] = true end
    end
end)
