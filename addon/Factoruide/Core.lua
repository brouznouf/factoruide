local addonName, FG = ...
_G.Factoruide = FG

FG.DB_VERSION = 1

--- Game version of the client: forever (1.16+ builds), classic (1.15), tbc (2.x).
FG.Edition = (function()
    local major, minor = (GetBuildInfo and GetBuildInfo() or "1.60"):match("^(%d+)%.(%d+)")
    major, minor = tonumber(major), tonumber(minor)
    if major == 2 then
        return "tbc"
    elseif major == 1 and minor and minor < 16 then
        return "classic"
    end
    return "forever"
end)()

local frame = CreateFrame("Frame")
FG.frame = frame
local handlers = {}

--- Register a handler for an event; several modules may listen to the same event.
function FG:On(event, fn)
    if not handlers[event] then
        handlers[event] = {}
        frame:RegisterEvent(event)
    end
    table.insert(handlers[event], fn)
end

frame:SetScript("OnEvent", function(_, event, ...)
    for _, fn in ipairs(handlers[event]) do
        local ok, err = pcall(fn, ...)
        if not ok then FG:Debug("error in %s: %s", event, tostring(err)) end
    end
end)

function FG:Print(fmt, ...) print("|cff33ccffFactoruide|r: " .. string.format(fmt, ...)) end

function FG:Debug(fmt, ...)
    if FactoruideDB and FactoruideDB.debug then self:Print(fmt, ...) end
end

--- Values can be "secret" (unreadable by addons) in restricted contexts such as combat.
function FG.Readable(v)
    if v == nil or (issecretvalue and issecretvalue(v)) then return nil end
    return v
end

--- Anchor a frame by its top left corner where it is now, so it grows and shrinks downwards
--- (the buttons of its header stay under the mouse); returns the point to save, nil when the
--- frame has no position yet. The game re-anchors a dragged frame to its closest corner or
--- edge, which can be the bottom.
--- Whether a frame can be moved, resized or shown now. A secure button hanging on it (a quest
--- item button), or on a frame anchored to it, makes it protected: during combat the game blocks
--- these changes ("blocked from an action only available to the Blizzard UI"), so they wait for
--- the end of the fight.
function FG.CanLayout(frame) return not (InCombatLockdown() and frame:IsProtected()) end

-- Name the function the game refused to us (the popup does not), once each, with where it came from.
local blocked = {}
for _, event in ipairs({ "ADDON_ACTION_BLOCKED", "ADDON_ACTION_FORBIDDEN" }) do
    FG:On(event, function(addon, func)
        if addon ~= addonName or blocked[func] then return end
        blocked[func] = true
        FG:Print("%s: %s", event, tostring(func))
        if debugstack then print(debugstack(2, 6, 0)) end
    end)
end

function FG.PinTopLeft(frame)
    local left, top = frame:GetLeft(), frame:GetTop()
    if not left or not top then return nil end
    frame:ClearAllPoints()
    frame:SetPoint("TOPLEFT", UIParent, "BOTTOMLEFT", left, top)
    return { "TOPLEFT", "UIParent", "BOTTOMLEFT", left, top }
end

--- Pin a frame by its top (see `FG.PinTopLeft`) when it is anchored by its bottom or center,
--- as positions saved before were; returns the point to save, or nil when nothing changed.
function FG.PinTopIfNeeded(frame)
    local point = frame:GetPoint()
    if not point or point:find("^TOP") then return nil end
    return FG.PinTopLeft(frame)
end

--- Spell name in the game's language (nil when unknown).
function FG.SpellName(id)
    if C_Spell and C_Spell.GetSpellName then return C_Spell.GetSpellName(id) end
    return GetSpellInfo and (GetSpellInfo(id))
end

--- Item info, from C_Item or the older globals.
function FG.ItemInfo(item)
    if C_Item and C_Item.GetItemInfo then return C_Item.GetItemInfo(item) end
    return GetItemInfo(item)
end

function FG.ItemInfoInstant(item)
    if C_Item and C_Item.GetItemInfoInstant then return C_Item.GetItemInfoInstant(item) end
    return GetItemInfoInstant(item)
end

--- C_QuestLog as the modules use it. The Classic Era client (1.15) lacks the quest log functions
--- of the newer ones (its log is read through GetQuestLogTitle): they are rebuilt here, in a table
--- of our own so the other addons, which test whether C_QuestLog has them, see the game as it is.
local function LogEntry(index)
    local title, level, _, isHeader, isCollapsed, isComplete, frequency, questID = GetQuestLogTitle(index)
    if not title then return nil end
    return {
        title = title,
        level = level,
        isHeader = isHeader and true or false,
        isCollapsed = isCollapsed and true or false,
        questID = questID or 0,
        frequency = frequency,
        state = isComplete, -- 1 complete, -1 failed
    }
end

local function LogIndex(questID)
    local index = GetQuestLogIndexByID and GetQuestLogIndexByID(questID)
    return index and index > 0 and index or nil
end

local function LogState(questID)
    local index = LogIndex(questID)
    local entry = index and LogEntry(index)
    return entry and entry.state
end

local classicLog = {
    GetNumQuestLogEntries = function() return (GetNumQuestLogEntries()) end,
    GetInfo = LogEntry,
    GetLogIndexForQuestID = LogIndex,
    GetTitleForQuestID = function(questID)
        if C_QuestLog.GetQuestInfo then return C_QuestLog.GetQuestInfo(questID) end
        local index = LogIndex(questID)
        return index and (GetQuestLogTitle(index))
    end,
    IsComplete = function(questID) return LogState(questID) == 1 end,
    IsFailed = function(questID) return LogState(questID) == -1 end,
    IsOnQuest = function(questID) return LogIndex(questID) ~= nil end,
    IsQuestFlaggedCompleted = IsQuestFlaggedCompleted,
}
FG.QuestLog = setmetatable({}, { __index = function(_, name) return C_QuestLog[name] or classicLog[name] end })

--- Parse a GUID into its entity type ("npc", "object", "player") and ID.
function FG.ParseGUID(guid)
    guid = FG.Readable(guid)
    if not guid then return nil end
    local kind, _, _, _, _, id = strsplit("-", guid)
    if kind == "Creature" or kind == "Vehicle" then
        return "npc", tonumber(id)
    elseif kind == "GameObject" then
        return "object", tonumber(id)
    elseif kind == "Player" then
        return "player", nil
    end
    return nil
end

--- Player position as uiMapID, x, y (percent, 2 decimals).
function FG.PlayerPosition()
    local mapID = C_Map.GetBestMapForUnit("player")
    if not mapID then return nil end
    local pos = C_Map.GetPlayerMapPosition(mapID, "player")
    if not pos then return mapID end
    local x, y = pos:GetXY()
    x, y = FG.Readable(x), FG.Readable(y)
    if not x then return mapID end
    return mapID, math.floor(x * 10000 + 0.5) / 100, math.floor(y * 10000 + 0.5) / 100
end

function FG:CharacterKey() return UnitName("player") .. "-" .. GetNormalizedRealmName() end

-- Skill names in the skills list, for clients without GetProfessions (matched by name).
local SKILL_NAMES = {
    [171] = { "Alchemy", "Alchimie" },
    [164] = { "Blacksmithing", "Forge" },
    [333] = { "Enchanting", "Enchantement" },
    [202] = { "Engineering", "Ingénierie" },
    [182] = { "Herbalism", "Herboristerie" },
    [165] = { "Leatherworking", "Travail du cuir" },
    [186] = { "Mining", "Minage" },
    [393] = { "Skinning", "Dépeçage" },
    [197] = { "Tailoring", "Couture" },
    [185] = { "Cooking", "Cuisine" },
    [356] = { "Fishing", "Pêche" },
    [129] = { "First Aid", "Secourisme" },
    [762] = { "Riding", "Monte" },
}

--- Skill of a profession by skill line ID: rank, max rank, localized name (nil if unknown).
function FG.ProfessionSkill(line)
    if GetProfessions and GetProfessionInfo then
        for _, index in pairs({ GetProfessions() }) do
            local name, _, rank, maxRank, _, _, skillLine = GetProfessionInfo(index)
            if skillLine == line then return rank, maxRank, name end
        end
    end
    if GetNumSkillLines and GetSkillLineInfo then
        local names = SKILL_NAMES[line] or {}
        for i = 1, GetNumSkillLines() do
            local name, header, _, rank, _, _, maxRank = GetSkillLineInfo(i)
            if not header and (name == names[1] or name == names[2]) then return rank, maxRank, name end
        end
    end
    return nil
end

FG:On("ADDON_LOADED", function(name)
    if name ~= addonName then return end
    FactoruideDB = FactoruideDB or {}
    FactoruideDB.version = FG.DB_VERSION
    FactoruideDB.characters = FactoruideDB.characters or {}
end)

SLASH_FACTORUIDE1 = "/fg"
SLASH_FACTORUIDE2 = "/factoruide"
SlashCmdList.FACTORUIDE = function(msg)
    local cmd, arg = strsplit(" ", strtrim(msg or ""), 2)
    cmd = (cmd or ""):lower()
    if FG.Guide and FG.Guide:Command(cmd, arg) then
        return
    elseif cmd == "debug" then
        FactoruideDB.debug = not FactoruideDB.debug
        FG:Print("debug %s", FactoruideDB.debug and "on" or "off")
    elseif FG.Collector and (cmd == "" or cmd == "status") then
        FG.Collector:PrintStatus()
    elseif FG.Collector and cmd == "reset" then
        FG.Collector:Reset()
    elseif cmd == "scale" then
        FG:ScaleCommand(arg)
    elseif FG.Profile and (cmd == "profile" or cmd == "profil") then
        FG.Profile:Save()
    else
        FG:Print(
            "/fg config | guide | hide | show | next | prev | step <n> | skip [id] | restart | lines <n> | route | macro | waypoint | scale [size] | status | reset | profile | debug"
        )
    end
end
