-- Records quest data observed in game: who gives and takes quests, where, at which level,
-- how long quests take, kills, loot sources and flight masters. `fg collector` imports it.
--
-- Events are stored as compact rows:
--   { seq, time, event, questID, entityType, entityID, uiMapID, x, y, level, data }

local _, FG = ...
local Collector = {}
FG.Collector = Collector

local char -- this character's saved table
local interaction -- entity the player is currently talking to
local seen = {} -- per-session dedupe for noisy events

local function Record(event, questID, entityType, entityID, data, mapID, x, y)
    if not char then return end
    if not mapID then
        mapID, x, y = FG.PlayerPosition()
    end
    char.seq = char.seq + 1
    table.insert(char.events, {
        char.seq,
        time(),
        event,
        questID or false,
        entityType or false,
        entityID or false,
        mapID or false,
        x or false,
        y or false,
        UnitLevel("player"),
        data or false,
    })
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

local function SnapshotCharacter()
    local _, raceFile, raceID = UnitRace("player")
    local _, classFile = UnitClass("player")
    char.race = raceFile
    char.raceID = raceID
    char.class = classFile
    char.faction = UnitFactionGroup("player")
    char.level = UnitLevel("player")
    local completed = C_QuestLog.GetAllCompletedQuestIDs and C_QuestLog.GetAllCompletedQuestIDs()
    if completed then char.completed = completed end
    local log = {}
    for i = 1, C_QuestLog.GetNumQuestLogEntries() do
        local info = C_QuestLog.GetInfo(i)
        if info and not info.isHeader and info.questID then table.insert(log, info.questID) end
    end
    char.questLog = log
end

FG:On("PLAYER_LOGIN", function()
    local key = FG:CharacterKey()
    FactoruideDB.characters[key] = FactoruideDB.characters[key] or { seq = 0, events = {} }
    char = FactoruideDB.characters[key]
    SnapshotCharacter()
end)

FG:On("PLAYER_LOGOUT", function()
    if char then SnapshotCharacter() end
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

FG:On("QUEST_GREETING", function()
    Remember()
    local entityType, entityID = CurrentInteraction()
    if not entityType then return end
    local quests = {}
    for i = 1, GetNumAvailableQuests() do
        local isTrivial, _, isRepeatable, _, questID = GetAvailableQuestInfo(i)
        if questID then
            table.insert(quests, { questID = questID, isTrivial = isTrivial, repeatable = isRepeatable })
        end
    end
    RecordOffers(entityType, entityID, quests)
    for i = 1, GetNumActiveQuests() do
        local questID = GetActiveQuestID(i)
        if questID then RecordOnce("active:" .. entityID .. ":" .. questID, "active", questID, entityType, entityID) end
    end
end)

FG:On("QUEST_DETAIL", function(questStartItemID)
    Remember()
    local questID = GetQuestID()
    if not questID or questID == 0 then return end
    if questStartItemID and questStartItemID ~= 0 then
        RecordOnce("detail:item:" .. questID, "detail", questID, "item", questStartItemID)
    else
        local entityType, entityID = LastInteraction()
        RecordOnce("detail:" .. tostring(entityID) .. ":" .. questID, "detail", questID, entityType, entityID)
    end
end)

FG:On("QUEST_ACCEPTED", function(questID)
    local entityType, entityID = LastInteraction()
    local info = C_QuestLog.GetInfo(C_QuestLog.GetLogIndexForQuestID(questID) or 0)
    Record("accept", questID, entityType, entityID, {
        title = info and info.title,
        level = info and info.level,
    })
    interaction = nil
end)

FG:On("QUEST_PROGRESS", Remember)
FG:On("QUEST_COMPLETE", Remember)

FG:On("QUEST_TURNED_IN", function(questID, xpReward, moneyReward)
    local entityType, entityID = LastInteraction()
    Record("turnin", questID, entityType, entityID, { xp = xpReward, money = moneyReward })
end)

FG:On("QUEST_REMOVED", function(questID, wasReplayQuest)
    -- Also fires on turn-in; the importer keeps it only when no turn-in follows.
    Record("removed", questID, nil, nil, { replay = wasReplayQuest or nil })
end)

FG:On("PLAYER_LEVEL_UP", function(level) Record("level", nil, nil, nil, { level = level }) end)

-- Creature positions: kills and NPCs the player targets or mouses over.
local function RecordUnit(unit, event)
    if UnitIsPlayer(unit) then return end
    local entityType, entityID = FG.ParseGUID(UnitGUID(unit))
    if entityType ~= "npc" then return end
    local mapID, x, y = FG.PlayerPosition()
    if not x then return end
    local key = string.format("%s:%d:%d:%d:%d", event, entityID, mapID, x, y)
    RecordOnce(key, event, nil, "npc", entityID, {
        level = FG.Readable(UnitLevel(unit)),
        name = FG.Readable(UnitName(unit)),
        reaction = FG.Readable(UnitReaction(unit, "player")),
    }, mapID, x, y)
end

FG:On("PLAYER_TARGET_CHANGED", function()
    if UnitExists("target") then RecordUnit("target", "seen") end
end)

FG:On("UPDATE_MOUSEOVER_UNIT", function()
    if not InCombatLockdown() then RecordUnit("mouseover", "seen") end
end)

FG:On("PARTY_KILL", function(attackerGUID, targetGUID)
    if FG.Readable(attackerGUID) ~= UnitGUID("player") then return end
    local entityType, entityID = FG.ParseGUID(targetGUID)
    if entityType == "npc" then
        local mapID, x, y = FG.PlayerPosition()
        if x then
            RecordOnce(
                string.format("kill:%d:%d:%d:%d", entityID, mapID, x, y),
                "kill",
                nil,
                "npc",
                entityID,
                nil,
                mapID,
                x,
                y
            )
        end
    end
end)

FG:On("LOOT_OPENED", function()
    if not GetLootSourceInfo then return end
    local items = {}
    local sourceType, sourceID
    for slot = 1, GetNumLootItems() do
        local link = GetLootSlotLink(slot)
        local itemID = link and tonumber(link:match("item:(%d+)"))
        local guid = GetLootSourceInfo(slot)
        local t, id = FG.ParseGUID(guid)
        if t == "npc" or t == "object" then
            sourceType, sourceID = t, id
        end
        if itemID then table.insert(items, itemID) end
    end
    if sourceType and #items > 0 then Record("loot", nil, sourceType, sourceID, { items = items }) end
end)

FG:On("TAXIMAP_OPENED", function()
    local mapID = GetTaxiMapID and GetTaxiMapID() or C_Map.GetBestMapForUnit("player")
    if not mapID or not C_TaxiMap.GetAllTaxiNodes then return end
    local nodes = {}
    for _, node in ipairs(C_TaxiMap.GetAllTaxiNodes(mapID)) do
        table.insert(nodes, { id = node.nodeID, name = node.name, state = node.state })
    end
    Record("taxi", nil, nil, nil, { map = mapID, nodes = nodes })
end)

-- Transport links: when the continent changes (boat, zeppelin, portal), remember where we
-- left from, where we arrived and how long it took. `fg` turns these into travel links.
local function Continent(mapID)
    local info = mapID and C_Map.GetMapInfo(mapID)
    while info and info.mapType and info.mapType > 2 and info.parentMapID and info.parentMapID ~= 0 do
        info = C_Map.GetMapInfo(info.parentMapID)
    end
    return info and info.mapID
end

local last -- last known position: { map, x, y, continent, time }

local function TrackPosition()
    if not char or UnitOnTaxi("player") then return end
    local mapID, x, y = FG.PlayerPosition()
    if not x then return end
    local continent = Continent(mapID)
    if last and continent and last.continent and continent ~= last.continent then
        Record("transit", nil, nil, nil, {
            fromMap = last.map,
            fromX = last.x,
            fromY = last.y,
            seconds = time() - last.time,
        }, mapID, x, y)
    end
    last = { map = mapID, x = x, y = y, continent = continent, time = time() }
end

FG:On("PLAYER_LOGIN", function() C_Timer.NewTicker(5, TrackPosition) end)
FG:On("ZONE_CHANGED_NEW_AREA", TrackPosition)

function Collector:PrintStatus()
    if not char then return end
    local counts = {}
    for _, e in ipairs(char.events) do
        counts[e[3]] = (counts[e[3]] or 0) + 1
    end
    local parts = {}
    for event, n in pairs(counts) do
        table.insert(parts, event .. "=" .. n)
    end
    table.sort(parts)
    FG:Print("%d events recorded (%s)", #char.events, table.concat(parts, ", "))
end

function Collector:Reset()
    if char then
        wipe(char.events)
        wipe(seen)
        FG:Print("collected events cleared")
    end
end
