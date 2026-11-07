-- Records quest data observed in game: who gives and takes quests, where, at which level,
-- how long quests take, kills, loot sources and flight masters. `fg collector` imports it.
--
-- Events are stored as one string each (small saved variables, quick to load):
--   seq|time|event|questID|entityType|entityID|uiMapID|x|y|level|data
-- empty for none; data = key=value pairs joined by `;` (booleans 1, pairs as a:b,a:b, text with
-- %, |, ; = and control characters percent-encoded). Rows saved before are tables:
--   { seq, time, event, questID, entityType, entityID, uiMapID, x, y, level, data }
-- The rows the app has (`FactoruideSent[character] = last seq sent`) come with the guides written
-- by the desktop app into Factoruide_Data (FG.DataSent, Data.lua), the Sent.lua older versions of
-- the app wrote, or a guide imported in game (`FactoruideDB.sent`): they are dropped at
-- login.
-- Everything here is event driven (no OnUpdate), throttled and bounded.

local _, FG = ...
local QuestLog = FG.QuestLog
local Collector = {}
FG.Collector = Collector

local char -- this character's saved table
local interaction -- entity the player is currently talking to
local seen = {} -- per-session dedupe for noisy events
local MAX_EVENTS = 20000 -- per character, the oldest dropped beyond
local capped = false -- the cap was logged this session

local function Escape(text)
    return (text:gsub("[%%|;=%c]", function(c) return string.format("%%%02X", c:byte()) end))
end

local function EncodeValue(v)
    if v == true then
        return "1"
    elseif type(v) == "string" then
        return Escape(v)
    elseif type(v) == "table" then
        local pairsText = {}
        for i, p in ipairs(v) do
            pairsText[i] = p[1] .. ":" .. p[2]
        end
        return table.concat(pairsText, ",")
    end
    return tostring(v)
end

--- data as key=value;key=value, keys sorted, nil, false and 0 left out.
local function EncodeData(data)
    if not data then return "" end
    local keys = {}
    for k, v in pairs(data) do
        if v ~= false and v ~= 0 then keys[#keys + 1] = k end
    end
    table.sort(keys)
    for i, k in ipairs(keys) do
        keys[i] = k .. "=" .. EncodeValue(data[k])
    end
    return table.concat(keys, ";")
end

--- Seq and event of a row, a string or a table (saved before).
local function RowSeq(row)
    if type(row) == "string" then return tonumber(row:match("^(%d+)|")) or 0 end
    return row[1]
end

local function RowEvent(row)
    if type(row) == "string" then return row:match("^%d+|%d*|([^|]*)") end
    return row[3]
end

local function Record(event, questID, entityType, entityID, data, mapID, x, y)
    if not char then return end
    if not mapID then
        mapID, x, y = FG.PlayerPosition()
    end
    char.seq = char.seq + 1
    -- Not sent for a long time: keep the newest (checked every 500 rows, not on each one).
    if #char.events >= MAX_EVENTS + 500 then
        local kept = {}
        for i = #char.events - MAX_EVENTS + 1, #char.events do
            kept[#kept + 1] = char.events[i]
        end
        char.events = kept
        if not capped then FG:Error("collector", "over " .. MAX_EVENTS .. " events, the oldest dropped") end
        capped = true
    end
    table.insert(
        char.events,
        table.concat({
            char.seq,
            time(),
            event,
            questID or "",
            entityType or "",
            entityID or "",
            mapID or "",
            x or "",
            y or "",
            UnitLevel("player"),
            EncodeData(data),
        }, "|")
    )
    FG:Debug("%s q=%s %s=%s", event, tostring(questID), tostring(entityType), tostring(entityID))
end

--- Record once per session for a given key (positions rounded to ~1% of the map).
local function RecordOnce(key, ...)
    if seen[key] then return end
    seen[key] = true
    Record(...)
end

local function CurrentInteraction()
    local entityType, entityID = FG.ParseGUID(UnitGUID("npc"))
    if entityType == "npc" or entityType == "object" then return entityType, entityID end
    return nil
end

local function Remember()
    local entityType, entityID = CurrentInteraction()
    if entityType then interaction = { type = entityType, id = entityID, at = GetTime() } end
end

--- Entity the player interacted with in the last few seconds.
local function LastInteraction()
    if interaction and GetTime() - interaction.at < 10 then return interaction.type, interaction.id end
    return CurrentInteraction()
end

--- Race and class, sent with the rows. The quests done and in the log are the profile's
--- (Profile.lua): the copies older versions kept here are let go.
local function SnapshotCharacter()
    local _, raceFile = UnitRace("player")
    local _, classFile = UnitClass("player")
    char.race = raceFile
    char.class = classFile
    char.raceID, char.faction, char.level, char.completed, char.questLog = nil, nil, nil, nil, nil
end

--- Last seq the app has received for this character, 0 when none: from Factoruide_Data, Sent.lua
--- or the last guide imported, the highest (the seq only grows, so it is the most recent).
local function SentSeq()
    local key = FG:CharacterKey()
    local file = type(FactoruideSent) == "table" and tonumber(FactoruideSent[key]) or 0
    local imported = type(FactoruideDB.sent) == "table" and tonumber(FactoruideDB.sent[key]) or 0
    local data = FG.DataSent and FG.DataSent[key] or 0
    return math.max(file, imported, data)
end

--- Drop the rows the app already has, and the oldest beyond the cap. The seq keeps growing,
--- also past what the app saw (saved variables lost or reset).
function Collector:Purge()
    local events, sent = char.events, SentSeq()
    char.seq = math.max(char.seq, sent)
    local first = 1
    while events[first] and RowSeq(events[first]) <= sent do
        first = first + 1
    end
    if #events - first + 1 > MAX_EVENTS then
        first = #events - MAX_EVENTS + 1
        FG:Error("collector", "over " .. MAX_EVENTS .. " events, the oldest dropped")
        capped = true
    end
    if first > 1 then
        local kept = {}
        for i = first, #events do
            kept[#kept + 1] = events[i]
        end
        char.events = kept
    end
end

FG:On("PLAYER_LOGIN", function()
    local key = FG:CharacterKey()
    FactoruideDB.characters[key] = FactoruideDB.characters[key] or { seq = 0, events = {} }
    char = FactoruideDB.characters[key]
    Collector:Purge()
    SnapshotCharacter()
end)

-- Quest offers: NPCs with a gossip window or a quest greeting list their available quests.
local function RecordOffers(entityType, entityID, quests)
    for _, q in ipairs(quests) do
        local mapID, x, y = FG.PlayerPosition()
        RecordOnce("offer:" .. entityID .. ":" .. q.questID, "offer", q.questID, entityType, entityID, {
            level = q.questLevel,
            trivial = q.isTrivial or nil,
            repeatable = q.repeatable or nil,
        }, mapID, x, y)
    end
end

FG:On("GOSSIP_SHOW", function()
    Remember()
    local entityType, entityID = CurrentInteraction()
    if entityType then
        RecordOffers(entityType, entityID, C_GossipInfo.GetAvailableQuests())
        for _, q in ipairs(C_GossipInfo.GetActiveQuests()) do
            RecordOnce(
                "active:" .. entityID .. ":" .. q.questID,
                "active",
                q.questID,
                entityType,
                entityID,
                { complete = q.isComplete or nil }
            )
        end
    end
end)

--- Quest of the log with this title, when no other quest there has the same.
local function QuestByTitle(title)
    if not title or title == "" then return nil end
    local found
    for i = 1, QuestLog.GetNumQuestLogEntries() do
        local info = QuestLog.GetInfo(i)
        if info and not info.isHeader and info.title == title and info.questID and info.questID > 0 then
            if found then return nil end
            found = info.questID
        end
    end
    return found
end

--- Quest ID of an active quest of a quest greeting: Classic Era and TBC have no GetActiveQuestID,
--- only the title, found in the quest log.
local function ActiveGreetingQuest(index)
    local questID = GetActiveQuestID and GetActiveQuestID(index)
    if questID and questID > 0 then return questID end
    return QuestByTitle(GetActiveTitle and (GetActiveTitle(index)))
end

FG:On("QUEST_GREETING", function()
    Remember()
    local entityType, entityID = CurrentInteraction()
    if not entityType then return end
    local quests = {}
    for i = 1, GetNumAvailableQuests() do
        -- The quest ID (5th value) is missing on older clients: no offer recorded then.
        local isTrivial, _, isRepeatable, _, questID = GetAvailableQuestInfo(i)
        if questID and questID > 0 then
            table.insert(quests, { questID = questID, isTrivial = isTrivial, repeatable = isRepeatable })
        end
    end
    RecordOffers(entityType, entityID, quests)
    for i = 1, GetNumActiveQuests() do
        local questID = ActiveGreetingQuest(i)
        if questID then RecordOnce("active:" .. entityID .. ":" .. questID, "active", questID, entityType, entityID) end
    end
end)

FG:On("QUEST_DETAIL", function(questStartItemID)
    Remember()
    local questID = GetQuestID()
    if not questID or questID == 0 then return end
    -- The XP reward: Classic does not show it but has the function.
    local xp = GetRewardXP and FG.Readable(GetRewardXP())
    local data = xp and xp > 0 and { xp = math.min(xp, 10000000) } or nil
    if questStartItemID and questStartItemID ~= 0 then
        RecordOnce("detail:item:" .. questID, "detail", questID, "item", questStartItemID, data)
    else
        local entityType, entityID = LastInteraction()
        RecordOnce("detail:" .. tostring(entityID) .. ":" .. questID, "detail", questID, entityType, entityID, data)
    end
end)

-- Classic Era and TBC pass (questLogIndex, questID), Forever only (questID).
FG:On("QUEST_ACCEPTED", function(arg1, arg2)
    local questID = arg2 or arg1
    local entityType, entityID = LastInteraction()
    local info = QuestLog.GetInfo(QuestLog.GetLogIndexForQuestID(questID) or 0)
    Record("accept", questID, entityType, entityID, {
        title = info and info.title,
        level = info and info.level,
    })
    interaction = nil
end)

FG:On("QUEST_PROGRESS", Remember)
FG:On("QUEST_COMPLETE", Remember)

local turnedIn = {} -- questID -> GetTime() of its turn-in

FG:On("QUEST_TURNED_IN", function(questID, xpReward, moneyReward)
    turnedIn[questID] = GetTime()
    local entityType, entityID = LastInteraction()
    Record("turnin", questID, entityType, entityID, { xp = xpReward, money = moneyReward })
end)

local function TurnedIn(questID) return turnedIn[questID] ~= nil and GetTime() - turnedIn[questID] < 3 end

-- QUEST_REMOVED also fires on turn-in: recorded only when no turn-in comes with it (abandoned,
-- removed by the game). Classic can send it before QUEST_TURNED_IN, hence the short wait.
FG:On("QUEST_REMOVED", function(questID)
    if TurnedIn(questID) then return end
    local mapID, x, y = FG.PlayerPosition()
    local function RecordRemoved()
        if not TurnedIn(questID) then Record("removed", questID, nil, nil, nil, mapID, x, y) end
    end
    if C_Timer and C_Timer.After then
        C_Timer.After(1, function() FG:Try("QUEST_REMOVED", RecordRemoved) end)
    else
        RecordRemoved()
    end
end)

FG:On("PLAYER_LEVEL_UP", function(level) Record("level", nil, nil, nil, { level = level }) end)

-- Creature positions: kills and NPCs the player targets or mouses over. On Forever a GUID, a
-- name or a level can be secret (instances, sometimes combat): FG.Readable before any use.
local ELITE = { elite = true, rareelite = true, worldboss = true }
local SPAWN_TIME = 8388608 -- 2^23: the low 23 bits of a creature's spawn UID are its spawn time
-- Ages beyond an hour are not kept: a static NPC spawns at the realm restart, its age would tell
-- the time of day the player was there.
local MAX_AGE = 3600
local named = {} -- npc IDs whose name was recorded this session

--- Seconds since a creature spawned, from its GUID (nil when unreadable, not a creature or
--- older than MAX_AGE).
local function SpawnAge(guid)
    guid = FG.Readable(guid)
    if not guid or not GetServerTime then return nil end
    local kind, _, _, _, _, _, spawn = strsplit("-", guid)
    local uid = kind == "Creature" and spawn and tonumber(spawn, 16)
    if not uid then return nil end
    local age = (GetServerTime() - uid % SPAWN_TIME) % SPAWN_TIME
    return age <= MAX_AGE and age or nil
end

--- An NPC unit as { id, level, elite, guid } (nil for players, objects or unreadable GUIDs).
local function ReadNpc(unit)
    if not UnitExists(unit) or FG.Readable(UnitIsPlayer(unit)) then return nil end
    local guid = FG.Readable(UnitGUID(unit))
    local entityType, entityID = FG.ParseGUID(guid)
    if entityType ~= "npc" or not entityID then return nil end
    local level = FG.Readable(UnitLevel(unit))
    return {
        id = entityID,
        guid = guid,
        level = level and level >= -1 and level <= 90 and level or nil,
        elite = ELITE[FG.Readable(UnitClassification(unit)) or ""] or nil,
    }
end

-- One row per NPC and 5% of the map per session (a camp, not each spawn): `seen` was most
-- of the rows. The name only once per NPC and session.
local function RecordUnit(unit, event)
    local npc = ReadNpc(unit)
    if not npc then return end
    local mapID, x, y = FG.PlayerPosition()
    if not x then return end
    local key = string.format("%s:%d:%d:%d:%d", event, npc.id, mapID, math.floor(x / 5), math.floor(y / 5))
    if seen[key] then return end
    local name = not named[npc.id] and FG.Readable(UnitName(unit)) or nil
    if name then named[npc.id] = true end
    RecordOnce(key, event, nil, "npc", npc.id, {
        level = npc.level,
        name = name,
        reaction = FG.Readable(UnitReaction(unit, "player")),
        elite = npc.elite,
        age = SpawnAge(npc.guid),
    }, mapID, x, y)
end

-- Fights: one `combat` row each, written when it ends, where it started.
local combat -- the fight in progress
local target -- the target read out of combat (its GUID may turn secret during the fight)

FG:On("PLAYER_TARGET_CHANGED", function()
    target = ReadNpc("target")
    if combat and not combat.npc then combat.npc = target end
    if UnitExists("target") then RecordUnit("target", "seen") end
end)

FG:On("UPDATE_MOUSEOVER_UNIT", function()
    if not InCombatLockdown() then RecordUnit("mouseover", "seen") end
end)

--- Kill XP messages, from the client's own strings (any language), most specific first.
local killPatterns
local function KillPatterns()
    if killPatterns then return killPatterns end
    killPatterns = {}
    local names = {}
    for _, n in ipairs({ "1", "2", "4", "5" }) do
        table.insert(names, "COMBATLOG_XPGAIN_EXHAUSTION" .. n .. "_GROUP")
        table.insert(names, "COMBATLOG_XPGAIN_EXHAUSTION" .. n .. "_RAID")
    end
    for _, n in ipairs({ "1", "2", "4", "5" }) do
        table.insert(names, "COMBATLOG_XPGAIN_EXHAUSTION" .. n)
    end
    for _, name in ipairs({ "_GROUP", "_RAID", "" }) do
        table.insert(names, "COMBATLOG_XPGAIN_FIRSTPERSON" .. name)
    end
    for _, name in ipairs(names) do
        local text = _G[name]
        if type(text) == "string" and text ~= "" then
            -- Format specifiers (%s, %d, %1$s...) become captures, the rest is literal.
            local pattern = text:gsub("%%%d*%$?[%d%.]*[sdfg]", "\001")
                :gsub("[%^%$%(%)%%%.%[%]%*%+%-%?]", "%%%0")
                :gsub("\001", "(.-)")
            table.insert(killPatterns, "^" .. pattern .. "$")
        end
    end
    return killPatterns
end

--- Whether a CHAT_MSG_COMBAT_XP_GAIN message is a kill's XP (false when unreadable).
local function IsKillXP(msg)
    msg = FG.Readable(msg)
    if type(msg) ~= "string" then return false end
    for _, pattern in ipairs(KillPatterns()) do
        if msg:match(pattern) then return true end
    end
    return false
end
FG.IsKillXP = IsKillXP

FG:On("PARTY_KILL", function(attackerGUID, targetGUID)
    local attacker = FG.Readable(attackerGUID)
    if not attacker or (attacker ~= UnitGUID("player") and attacker ~= FG.Readable(UnitGUID("pet"))) then return end
    if combat then combat.kills = combat.kills + 1 end
    local entityType, entityID = FG.ParseGUID(targetGUID)
    if entityType == "npc" then
        local mapID, x, y = FG.PlayerPosition()
        if x then
            local key = string.format("kill:%d:%d:%d:%d", entityID, mapID, x, y)
            RecordOnce(key, "kill", nil, "npc", entityID, { age = SpawnAge(targetGUID) }, mapID, x, y)
        end
    end
end)

FG:On("CHAT_MSG_COMBAT_XP_GAIN", function(msg)
    if combat and IsKillXP(msg) then combat.xpKills = combat.xpKills + 1 end
end)

FG:On("PLAYER_TARGET_DIED", function()
    if combat then combat.targetDied = combat.targetDied + 1 end
end)

FG:On("PLAYER_DEAD", function()
    if combat then combat.died = true end
end)

FG:On("PLAYER_REGEN_DISABLED", function()
    combat = nil
    if not char or IsInInstance() then return end
    local mapID, x, y = FG.PlayerPosition()
    combat = {
        start = GetTime(),
        map = mapID,
        x = x,
        y = y,
        -- Read now, else the target as read before (its GUID turned secret).
        npc = ReadNpc("target") or (UnitExists("target") and target) or nil,
        pet = UnitExists("pet") and true or nil,
        level = UnitLevel("player"),
        xp = UnitXP("player"),
        max = UnitXPMax("player"),
        kills = 0, -- PARTY_KILL by the player or the pet
        xpKills = 0, -- kill XP messages
        targetDied = 0,
    }
end)

--- XP earned since the fight began, across a level up.
local function XPGained(c)
    local level, xp = UnitLevel("player"), UnitXP("player")
    local gained = level > c.level and (c.max - c.xp + xp) or (level == c.level and xp - c.xp) or 0
    return math.max(0, math.min(gained, 10000000))
end

FG:On("PLAYER_REGEN_ENABLED", function()
    local c = combat
    combat = nil
    if not c then return end
    local seconds = math.min(3600, math.floor(GetTime() - c.start + 0.5))
    -- One source of kills per fight, the surest first (the others may be secret or missing).
    local kills = c.kills > 0 and c.kills or (c.xpKills > 0 and c.xpKills) or c.targetDied
    kills = math.min(100, kills)
    if kills == 0 and seconds < 3 then return end
    local xp = XPGained(c)
    local npc = c.npc
    Record("combat", nil, npc and "npc", npc and npc.id, {
        seconds = seconds > 0 and seconds or nil,
        kills = kills > 0 and kills or nil,
        xp = xp > 0 and xp or nil,
        mobLevel = npc and npc.level,
        elite = npc and npc.elite,
        pet = c.pet,
        died = c.died,
    }, c.map, c.x, c.y)
end)

-- Loot: one row per source (corpse, object) with the items and quantities it gave. AoE loot
-- merges several corpses in one window: GetLootSourceInfo(slot) gives guid1, qty1, guid2, qty2...
-- Read on LOOT_READY, before fast loot (Automation.lua calls it first) empties the slots.
local MAX_LOOT_ITEMS = 16
local lootedSources, lootedCount = {}, 0 -- source GUIDs recorded (LOOT_READY twice, a corpse reopened)

function Collector:ReadLoot()
    if not char or not GetLootSourceInfo then return end
    local bySource, order = {}, {}
    for slot = 1, GetNumLootItems() do
        local link = GetLootSlotLink(slot) -- nil for money
        local itemID = link and tonumber(link:match("item:(%d+)"))
        if itemID then
            local sources = { GetLootSourceInfo(slot) }
            for i = 1, #sources, 2 do
                local guid = FG.Readable(sources[i])
                if guid and not lootedSources[guid] then
                    local source = bySource[guid]
                    if not source then
                        source = { items = {}, index = {} }
                        bySource[guid] = source
                        table.insert(order, guid)
                    end
                    local qty = math.max(1, math.min(1000, tonumber(FG.Readable(sources[i + 1])) or 1))
                    local at = source.index[itemID]
                    if at then
                        source.items[at][2] = math.min(1000, source.items[at][2] + qty)
                    elseif #source.items < MAX_LOOT_ITEMS then
                        table.insert(source.items, { itemID, qty })
                        source.index[itemID] = #source.items
                    end
                end
            end
        end
    end
    if lootedCount > 1000 then
        wipe(lootedSources)
        lootedCount = 0
    end
    for _, guid in ipairs(order) do
        lootedSources[guid] = true
        lootedCount = lootedCount + 1
        local entityType, entityID = FG.ParseGUID(guid)
        if entityType == "npc" or entityType == "object" then
            Record("loot", nil, entityType, entityID, { items = bySource[guid].items })
        end
    end
end

FG:On("LOOT_READY", function() Collector:ReadLoot() end)
FG:On("LOOT_OPENED", function() Collector:ReadLoot() end)

-- Continent of a map (uiMapID of type 2, or the highest parent below the world).
local function Continent(mapID)
    local info = mapID and C_Map.GetMapInfo(mapID)
    while info and info.mapType and info.mapType > 2 and info.parentMapID and info.parentMapID ~= 0 do
        info = C_Map.GetMapInfo(info.parentMapID)
    end
    return info and info.mapID
end

-- Flight masters: node IDs and states only (names come from the client data), bounded so the
-- row stays small.
local MAX_TAXI_NODES = 128
local TAXI_STATES = { CURRENT = 0, REACHABLE = 1 } -- TaxiNodeGetType; DISTANT, NONE: unreachable

FG:On("TAXIMAP_OPENED", function()
    if not C_TaxiMap or not C_TaxiMap.GetAllTaxiNodes then return end
    -- Classic Era and TBC have no GetTaxiMapID: the continent's map holds all its nodes.
    local mapID = GetTaxiMapID and GetTaxiMapID()
    if not mapID then
        local here = C_Map.GetBestMapForUnit("player")
        mapID = Continent(here) or here
    end
    if not mapID then return end
    local nodes = {}
    for _, node in ipairs(C_TaxiMap.GetAllTaxiNodes(mapID) or {}) do
        if #nodes >= MAX_TAXI_NODES then break end
        local state = node.state
        if type(state) ~= "number" then
            local kind = TaxiNodeGetType and node.slotIndex and TaxiNodeGetType(node.slotIndex)
            state = TAXI_STATES[kind] or 2
        end
        if node.nodeID then table.insert(nodes, { node.nodeID, state }) end
    end
    if #nodes > 0 then Record("taxi", nil, nil, nil, { map = mapID, nodes = nodes }) end
end)

-- Flights: from where, how long. UnitOnTaxi turns true a moment after control is lost.
local flight -- departure of the flight in progress: { map, x, y, time }

FG:On("PLAYER_CONTROL_LOST", function()
    local mapID, x, y = FG.PlayerPosition()
    if not x then return end
    local start = { map = mapID, x = x, y = y, time = GetTime() }
    local function Check()
        if UnitOnTaxi("player") then flight = start end
    end
    if UnitOnTaxi("player") or not (C_Timer and C_Timer.After) then
        Check()
    else
        C_Timer.After(1, function() FG:Try("PLAYER_CONTROL_LOST", Check) end)
    end
end)

FG:On("PLAYER_CONTROL_GAINED", function()
    if not flight or UnitOnTaxi("player") then return end
    local f = flight
    flight = nil
    Record("flight", nil, nil, nil, {
        fromMap = f.map,
        fromX = f.x,
        fromY = f.y,
        seconds = math.min(7200, math.floor(GetTime() - f.time + 0.5)),
    })
end)

-- Objective progress: a row when a count goes up. The quest log events come in bursts: read
-- at most every 0.5 s, compared with the counts of the quests in the log only.
local MAX_OBJECTIVES = 12
local counts -- questID -> { numFulfilled by objective }, nil until the first read
local scanning = false

local function ScanObjectives(record)
    if not char or not QuestLog.GetQuestObjectives then return end
    local now = {}
    for i = 1, QuestLog.GetNumQuestLogEntries() do
        local info = QuestLog.GetInfo(i)
        local questID = info and not info.isHeader and info.questID
        if questID and questID > 0 then
            local values, before = {}, counts and counts[questID]
            for index, o in ipairs(QuestLog.GetQuestObjectives(questID) or {}) do
                if index > MAX_OBJECTIVES then break end
                local value = tonumber(FG.Readable(o.numFulfilled)) or 0
                values[index] = value
                if record and before and before[index] and value > before[index] then
                    Record("progress", questID, nil, nil, { index = index, value = math.min(value, 9999) })
                end
            end
            now[questID] = values
        end
    end
    counts = now
end

local function ScheduleScan()
    if scanning or not counts then return end
    scanning = true
    local function Scan()
        scanning = false
        FG:Try("progress", ScanObjectives, true)
    end
    if C_Timer and C_Timer.After then
        C_Timer.After(0.5, Scan)
    else
        Scan()
    end
end

FG:On("PLAYER_LOGIN", function() ScanObjectives(false) end)
FG:On("QUEST_LOG_UPDATE", ScheduleScan)
FG:On("QUEST_WATCH_UPDATE", ScheduleScan)
FG:On("UNIT_QUEST_LOG_CHANGED", function(unit)
    if unit == "player" then ScheduleScan() end
end)

--- A guide step done while playing (Guide.lua; not those marked by hand or passed).
--- Accepts and turn-ins are not: they have rows of their own. `d`: the step's planned seconds.
function Collector:RecordStep(index, step)
    if step.k == "accept" or step.k == "turnin" then return end
    Record("step", step.q, nil, nil, { step = index, planned = step.d })
end

-- Transport links: when the continent changes (boat, zeppelin, portal, hearthstone), remember
-- where we left from, where we arrived and how long it took. `fg` turns these into travel links.
--
-- Positions are sampled every 5 s. A boat or zeppelin carries the player before the loading
-- screen: samples taken on board move while the player stands (speed 0). The ride is timed from
-- the last sample before that (on the docked boat), mode "ride". Without a readable
-- GetUnitSpeed, or if a client gives the transport's speed for the player, no ride is seen: the
-- transit is then timed from the last sample only (end of the crossing and loading screen).
local last, departure -- samples: { map, x, y, continent, time }
local cast -- last teleport spell of the player: { mode, time }

-- Hearthstone, Astral Recall; the mage teleports and Teleport: Moonglade.
local TELEPORTS = { [8690] = "hearth", [556] = "hearth" }
for _, id in ipairs({ 3561, 3562, 3563, 3565, 3566, 3567, 32271, 32272, 33690, 35715, 18960 }) do
    TELEPORTS[id] = "spell"
end

FG:On("UNIT_SPELLCAST_SUCCEEDED", function(unit, _, spellID)
    if unit ~= "player" then return end
    local mode = TELEPORTS[FG.Readable(spellID) or 0]
    if mode then cast = { mode = mode, time = time() } end
end)

--- Whether the player was carried between two samples: moved without moving by themselves.
local function Carried(from, to)
    local speed = GetUnitSpeed and FG.Readable((GetUnitSpeed("player")))
    if speed ~= 0 then return false end
    return from.map ~= to.map or math.abs(from.x - to.x) + math.abs(from.y - to.y) > 0.05
end

local function TrackPosition()
    if not char or UnitOnTaxi("player") then return end
    local mapID, x, y = FG.PlayerPosition()
    if not x then return end
    local now = time()
    local sample = { map = mapID, x = x, y = y, continent = Continent(mapID), time = now }
    if last and sample.continent and last.continent and sample.continent ~= last.continent then
        local from, mode = last, nil
        if cast and cast.time >= last.time - 2 then
            mode = cast.mode
        elseif departure ~= last and departure.continent == last.continent and last.time - departure.time < 600 then
            from, mode = departure, "ride"
        end
        Record("transit", nil, nil, nil, {
            fromMap = from.map,
            fromX = from.x,
            fromY = from.y,
            seconds = now - from.time,
            mode = mode,
        }, mapID, x, y)
        cast = nil
        departure = sample
    elseif not last or not Carried(last, sample) then
        departure = sample
    end
    last = sample
end

FG:On("PLAYER_LOGIN", function()
    C_Timer.NewTicker(5, function() FG:Try("transit", TrackPosition) end)
end)
FG:On("ZONE_CHANGED_NEW_AREA", TrackPosition)

function Collector:PrintStatus()
    if not char then return end
    local counts = {}
    for _, e in ipairs(char.events) do
        local event = RowEvent(e) or "?"
        counts[event] = (counts[event] or 0) + 1
    end
    local parts = {}
    for event, n in pairs(counts) do
        table.insert(parts, event .. "=" .. n)
    end
    table.sort(parts)
    local sent, pending = SentSeq(), 0
    for _, e in ipairs(char.events) do
        if RowSeq(e) > sent then pending = pending + 1 end
    end
    FG:Print("%d events recorded, %d not sent yet (%s)", #char.events, pending, table.concat(parts, ", "))
end

--- This character's race and class, and its rows the app does not have yet (the rows saved as
--- tables by older versions are left out).
function Collector:Pending()
    if not char then return nil end
    local sent, rows = SentSeq(), {}
    for _, e in ipairs(char.events) do
        if type(e) == "string" and RowSeq(e) > sent then rows[#rows + 1] = e end
    end
    return { race = char.race, class = char.class, events = rows }
end

function Collector:Reset()
    if char then
        wipe(char.events)
        wipe(seen)
        FG:Print("collected events cleared")
    end
end
