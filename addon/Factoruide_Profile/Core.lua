-- The addon comes in two parts sharing this namespace (the global `Factoruide`): Factoruide_Profile
-- (this core, the recordings and the character's profile, always installed) and Factoruide_Guide
-- (the guide and everything around it, which depends on it).
local addonName, FG = ...
_G.Factoruide = FG

FG.DB_VERSION = 1

-- Texts in the language chosen in the addon's settings (`locale`, FactoruideDB.settings), else the
-- game's; the English text when there is no translation. The files of Locales/ fill
-- FG.Locales[locale] = { [English text] = translation }.
FG.Locales = {}

-- The languages the addon can be set to (its options, Display page), besides the game's.
FG.LANGUAGES = { "enUS", "frFR", "deDE", "esES", "esMX", "ptBR", "ruRU", "koKR", "zhCN", "zhTW" }
local known = {}
for _, lang in ipairs(FG.LANGUAGES) do
    known[lang] = true
end

--- The language chosen in the settings, nil for the game's.
function FG.ChosenLang()
    local settings = type(FactoruideDB) == "table" and FactoruideDB.settings
    local lang = type(settings) == "table" and settings.locale
    return known[lang] and lang or nil
end

local function TranslationLocale(lang)
    if lang == "esMX" and not FG.Locales.esMX then return "esES" end
    return lang
end

--- The addon's language (a game locale: "frFR", "deDE"...).
function FG.Lang() return TranslationLocale(FG.ChosenLang() or (GetLocale and GetLocale()) or "enUS") end

--- `text` (English) in the addon's language.
function FG.T(text)
    local strings = FG.Locales[FG.Lang()]
    return strings and strings[text] or text
end

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
-- The Profile addon also uses the legacy skill cache when the Guide addon is absent.
local handlers = { SKILL_LINES_CHANGED = {} }
frame:RegisterEvent("SKILL_LINES_CHANGED")
local readingSkills = false
local legacySkills

--- Register a handler for an event; several modules may listen to the same event.
function FG:On(event, fn)
    if not handlers[event] then
        handlers[event] = {}
        frame:RegisterEvent(event)
    end
    table.insert(handlers[event], fn)
end

--- Count an error in the saved variables (`FactoruideDB.errors`, message -> count): a few short
--- messages, so what fails on one client shows up in its SavedVariables.
local MAX_ERRORS = 20
function FG:Error(where, err)
    local msg = (where .. ": " .. tostring(err)):gsub("Interface[/\\]AddOns[/\\]Factoruide[%w_]*[/\\]", ""):sub(1, 200)
    self:Debug("error in %s", msg)
    if type(FactoruideDB) ~= "table" then return end
    local errors = FactoruideDB.errors or {}
    FactoruideDB.errors = errors
    if errors[msg] then
        errors[msg] = errors[msg] + 1
        return
    end
    local n = 0
    for _ in pairs(errors) do
        n = n + 1
    end
    if n < MAX_ERRORS then errors[msg] = 1 end
end

--- Call `fn` under pcall, errors counted (event handlers, timers).
function FG:Try(where, fn, ...)
    local ok, err = pcall(fn, ...)
    if not ok then self:Error(where, err) end
end

frame:SetScript("OnEvent", function(_, event, ...)
    if event == "SKILL_LINES_CHANGED" then
        -- Header expansion is only a read. Its synchronous notifications must not advance the
        -- guide or schedule readers again. Real skill changes invalidate before any handler runs.
        if readingSkills then return end
        legacySkills = nil
    end
    for _, fn in ipairs(handlers[event]) do
        FG:Try(event, fn, ...)
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
        if type(addon) ~= "string" or not addon:find("^Factoruide") or blocked[func] then return end
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

--- How many of an item the character has in its bags (and its bank with `includeBank`, as the
--- game last saw it).
function FG.ItemCount(item, includeBank)
    if C_Item and C_Item.GetItemCount then return C_Item.GetItemCount(item, includeBank) end
    return GetItemCount(item, includeBank)
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
    -- Classic Era and TBC have GetQuestsCompleted, a set { [questID] = true }: a sorted list here.
    GetAllCompletedQuestIDs = function()
        if not GetQuestsCompleted then return nil end
        local ids = {}
        for id, done in pairs(GetQuestsCompleted() or {}) do
            if done then table.insert(ids, id) end
        end
        table.sort(ids)
        return ids
    end,
}
FG.QuestLog = setmetatable({}, {
    __index = function(_, name) return C_QuestLog[name] or classicLog[name] end,
})

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

function FG:CharacterKey() return UnitName("player") .. "-" .. (GetNormalizedRealmName() or GetRealmName() or "") end

-- English translation keys and legacy names for clients without GetProfessions.
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

local function ReadSkillRows()
    local snapshot = { rows = {}, collapsed = {} }
    for i = 1, GetNumSkillLines() do
        local name, header, expanded, rank, _, _, cap = GetSkillLineInfo(i)
        if header and not expanded then
            snapshot.collapsed[#snapshot.collapsed + 1] = i
        elseif not header and name then
            snapshot.rows[name] = { rank, cap, name }
        end
    end
    return snapshot
end

--- Visible rows need no header changes. If a requested skill is hidden, read all headers once;
--- subsequent Guide, Tracker and Nearby reads reuse that snapshot until a real skill event.
local function LegacyProfessionSkill(line)
    local names = SKILL_NAMES[line] or {}
    local english = names[1]
    local strings = FG.Locales[TranslationLocale((GetLocale and GetLocale()) or "enUS")]
    local localized = strings and english and strings[english]
    local clientName = C_TradeSkillUI
        and C_TradeSkillUI.GetTradeSkillDisplayName
        and C_TradeSkillUI.GetTradeSkillDisplayName(line)
    local function Find()
        local rows = legacySkills.rows
        return rows[clientName] or rows[english] or rows[names[2]] or rows[localized]
    end
    if not legacySkills then legacySkills = ReadSkillRows() end
    local found = Find()
    if
        not found
        and not legacySkills.expanded
        and #legacySkills.collapsed > 0
        and ExpandSkillHeader
        and CollapseSkillHeader
        and not readingSkills
    then
        local collapsed = legacySkills.collapsed
        readingSkills = true
        local ok, err = pcall(function()
            -- Original collapsed-list indexes stay valid when opening from the bottom.
            for i = #collapsed, 1, -1 do
                ExpandSkillHeader(collapsed[i])
            end
            legacySkills = ReadSkillRows()
            legacySkills.expanded = true
            -- Closing from the top restores the original indexes of the later headers.
            for _, index in ipairs(collapsed) do
                CollapseSkillHeader(index)
            end
        end)
        readingSkills = false
        if not ok then
            legacySkills = nil
            error(err)
        end
        found = Find()
    end
    if found then return unpack(found) end
    return nil
end

--- Skill of a profession by skill line ID: rank, max rank, localized name (nil if unknown).
function FG.ProfessionSkill(line)
    -- Older named guide steps use 0 when no supported profession can be identified.
    if not line or line <= 0 then return nil end
    if GetProfessions and GetProfessionInfo then
        for _, index in pairs({ GetProfessions() }) do
            local name, _, rank, maxRank, _, _, skillLine = GetProfessionInfo(index)
            if skillLine == line then return rank, maxRank, name end
        end
    end
    if C_SkillInfo and C_SkillInfo.GetSkillLineInfoByID then
        local skill = C_SkillInfo.GetSkillLineInfoByID(line)
        if skill and not skill.isHeader and skill.rank > 0 then return skill.rank, skill.maxRank, skill.name end
    end
    if GetNumSkillLines and GetSkillLineInfo then return LegacyProfessionSkill(line) end
    return nil
end

-- The reputation panel's rows (Forever: C_Reputation; Classic Era and TBC: the older globals):
-- faction ID, points, header, collapsed, header with points of its own, name.
local function ReputationAPI()
    if C_Reputation and C_Reputation.GetFactionDataByIndex and C_Reputation.GetNumFactions then
        return C_Reputation.GetNumFactions,
            function(i)
                local d = C_Reputation.GetFactionDataByIndex(i)
                if d then
                    return d.factionID, d.currentStanding, d.isHeader, d.isCollapsed, d.isHeaderWithRep, d.name
                end
            end,
            C_Reputation.ExpandFactionHeader,
            C_Reputation.CollapseFactionHeader
    end
    if GetNumFactions and GetFactionInfo then
        return GetNumFactions,
            function(i)
                local name, _, _, _, _, value, _, _, header, collapsed, hasRep, _, _, id = GetFactionInfo(i)
                return id, value, header, collapsed, hasRep, name
            end,
            ExpandFactionHeader,
            CollapseFactionHeader
    end
end

--- Points with every faction the reputation panel lists, counted from the bottom of Neutral:
--- { { id = factionID, value = points }... } (and `name`, in the game's language, with `withNames`).
--- Collapsed headers hide their factions: opened for the reading and closed again, unless the
--- panel is open (the player is using it).
function FG.Reputations(withNames)
    local count, row, expand, collapse = ReputationAPI()
    if not count then return {} end
    local panel = ReputationFrame and ReputationFrame:IsVisible()
    local opened, out = {}, {}
    local i = 1
    while i <= count() do
        local id, value, header, collapsed, hasRep, name = row(i)
        if header and collapsed and expand and not panel then
            expand(i)
            opened[id or name or i] = true
        end
        value = FG.Readable(value)
        if id and value and (not header or hasRep) then
            table.insert(out, { id = id, value = value, name = withNames and name or nil })
        end
        i = i + 1
    end
    if collapse then
        -- From the last row: closing a header leaves the rows before it where they are.
        for k = count(), 1, -1 do
            local id, _, header, _, _, name = row(k)
            if header and opened[id or name or k] then collapse(k) end
        end
    end
    return out
end

FG:On("ADDON_LOADED", function(name)
    if name ~= addonName then return end
    FactoruideDB = FactoruideDB or {}
    FactoruideDB.version = FG.DB_VERSION
    FactoruideDB.characters = FactoruideDB.characters or {}
    -- For the app: the game loaded the addon, when, and which app version installed it (its .toc).
    local metadata = C_AddOns and C_AddOns.GetAddOnMetadata or GetAddOnMetadata
    FactoruideDB.seen = { time = time(), version = metadata and metadata(addonName, "Version") or "?" }
end)

-- The only chat command: `/fg` (or `/factoruide`), with anything after it, opens the addon's
-- pages in the game's options (Options.lua). Everything else is done there, with the guide's
-- buttons or with key bindings.
SLASH_FACTORUIDE1 = "/fg"
SLASH_FACTORUIDE2 = "/factoruide"
SlashCmdList.FACTORUIDE = function() FG.Options:Open() end
