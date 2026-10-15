-- Quest tracker replacing Blizzard's: the next quests of the log to do in the route's order
-- (as many as configured), objectives with progress, "turn in" with the NPC, quest item
-- buttons. Blizzard's tracker (QuestWatchFrame on Classic, ObjectiveTrackerFrame later) is hidden
-- while `replaceTracker` is on (deferred after combat when the frame is protected).

local _, FG = ...
local QuestLog = FG.QuestLog
local Tracker = {}
FG.Tracker = Tracker
--- Quest IDs shown at the last refresh, in order.
Tracker.shown = {}

local fr = GetLocale() == "frFR"
local L = {
    title = fr and "Quêtes" or "Quests",
    turnIn = fr and "À rendre" or "Turn in",
    offRoute = fr and "hors route" or "off route",
    alongTheWay = fr and "en parallèle" or "along the way",
    failed = fr and "Échouée" or "Failed",
    empty = fr and "Aucune quête en cours" or "No quest in progress",
    notLearned = fr and "non appris" or "not learned",
    trainer = fr and "entraîneur" or "trainer",
    lvl = fr and "niv." or "lvl",
}

local PROFESSION_NAMES = fr
        and {
            alchemy = "Alchimie",
            blacksmithing = "Forge",
            enchanting = "Enchantement",
            engineering = "Ingénierie",
            herbalism = "Herboristerie",
            leatherworking = "Travail du cuir",
            mining = "Minage",
            skinning = "Dépeçage",
            tailoring = "Couture",
            cooking = "Cuisine",
            fishing = "Pêche",
            first_aid = "Secourisme",
        }
    or {}

local WIDTH = 330
local MAX_BUTTONS = 8

local frame, rows, buttons
local dirty = false
local blizzardHooked = false
local regenFrame

-- Blizzard tracker ------------------------------------------------------------

--- Blizzard's quest trackers of this client.
local function BlizzardTrackers()
    local out = {}
    for _, tracker in ipairs({ ObjectiveTrackerFrame, QuestWatchFrame }) do
        if tracker then table.insert(out, tracker) end
    end
    return out
end

local function ApplyBlizzardVisibility()
    local hide = FG:Get("replaceTracker")
    for _, tracker in ipairs(BlizzardTrackers()) do
        if InCombatLockdown() and (not hide or tracker:IsProtected()) then
            regenFrame:RegisterEvent("PLAYER_REGEN_ENABLED")
            return
        end
    end
    regenFrame:UnregisterEvent("PLAYER_REGEN_ENABLED")
    for _, tracker in ipairs(BlizzardTrackers()) do
        if tracker == QuestWatchFrame then
            -- Laid out by the game with its other frames: hiding it from an addon taints that
            -- layout (blocked actions later in combat). Made invisible instead.
            tracker:SetAlpha(hide and 0 or 1)
            tracker:EnableMouse(not hide)
        elseif hide then
            tracker:Hide()
        elseif tracker.Update then
            tracker:Update()
        end
    end
end

local function HookBlizzard()
    regenFrame = regenFrame or CreateFrame("Frame")
    regenFrame:SetScript("OnEvent", ApplyBlizzardVisibility)
    if not blizzardHooked then
        -- Opening the quest log shows them again.
        for _, tracker in ipairs(BlizzardTrackers()) do
            if tracker ~= QuestWatchFrame then
                tracker:HookScript("OnShow", function()
                    if FG:Get("replaceTracker") then ApplyBlizzardVisibility() end
                end)
            end
        end
        blizzardHooked = true
    end
    ApplyBlizzardVisibility()
end

-- Data -------------------------------------------------------------------------

--- Quests in progress, with the index of their next route step (for sorting).
local function CollectQuests()
    local quests, seen = {}, {}
    local nextStep = FG.Guide and FG.Guide.NextStepByQuest and FG.Guide:NextStepByQuest() or {}
    -- Only the quests being done: their next step is still ahead in the route (or done along
    -- the way); the first ones are kept.
    local current = FG.Guide and FG.Guide.CurrentStep and FG.Guide:CurrentStep() or 1
    local function inProgress(step) return step and (step.bg or step.index >= current) end
    local function add(q, logIndex)
        if seen[q] then return end
        seen[q] = true
        -- The log's line (level, quest item) when it is listed: not under a collapsed header.
        logIndex = logIndex or (QuestLog.GetLogIndexForQuestID and QuestLog.GetLogIndexForQuestID(q))
        local info = logIndex and QuestLog.GetInfo(logIndex)
        if info and info.questID ~= q then
            info, logIndex = nil, nil
        end
        table.insert(quests, {
            id = q,
            index = logIndex,
            title = info and info.title or QuestLog.GetTitleForQuestID(q) or ("#" .. q),
            level = info and info.level,
            complete = QuestLog.IsComplete(q),
            failed = QuestLog.IsFailed and QuestLog.IsFailed(q),
            objectives = QuestLog.GetQuestObjectives(q) or {},
            step = nextStep[q],
        })
    end
    -- The route's quests in the log, from the route: the same list whatever the quest log
    -- window shows (headers collapsed or expanded while it is open).
    for q, step in pairs(nextStep) do
        if inProgress(step) and QuestLog.IsOnQuest(q) then add(q) end
    end
    -- The others, from the log, when the whole log is shown.
    if FG:Get("trackerAll") then
        for i = 1, QuestLog.GetNumQuestLogEntries() do
            local info = QuestLog.GetInfo(i)
            if info and not info.isHeader and not info.isHidden and info.questID then add(info.questID, i) end
        end
    end
    table.sort(quests, function(a, b)
        local sa, sb = a.step and a.step.index or math.huge, b.step and b.step.index or math.huge
        if sa ~= sb then return sa < sb end
        return a.title < b.title
    end)
    return quests
end

local function DifficultyColor(questID)
    if GetDifficultyColor and C_PlayerInfo and C_PlayerInfo.GetContentDifficultyQuestForPlayer then
        local c = GetDifficultyColor(C_PlayerInfo.GetContentDifficultyQuestForPlayer(questID))
        if c then return string.format("%02x%02x%02x", c.r * 255, c.g * 255, c.b * 255) end
    end
    return "ffd100"
end

-- UI ---------------------------------------------------------------------------

local function Line(i)
    local line = rows[i]
    if line then return line end
    line = CreateFrame("Button", nil, frame)
    line:SetHeight(16)
    line:RegisterForClicks("LeftButtonUp")
    line.text = line:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    line.text:SetPoint("LEFT")
    line.text:SetPoint("RIGHT")
    line.text:SetJustifyH("LEFT")
    line.text:SetWordWrap(false)
    FG.Skin:Font(line.text)
    line:SetScript("OnClick", function(self)
        if not self.questID then return end
        if IsShiftKeyDown() and FG.Guide and FG.Guide.PointToQuest then
            FG.Guide:PointToQuest(self.questID)
        elseif QuestMapFrame_OpenToQuestDetails then
            QuestMapFrame_OpenToQuestDetails(self.questID)
        end
    end)
    rows[i] = line
    return line
end

--- Secure buttons to use quest items (only re-assigned out of combat).
local function ItemButton(i)
    local b = buttons[i]
    if b then return b end
    b = CreateFrame("Button", "FactoruideItemButton" .. i, frame, "SecureActionButtonTemplate")
    b:SetSize(20, 20)
    b:RegisterForClicks("AnyUp", "AnyDown")
    b.icon = b:CreateTexture(nil, "ARTWORK")
    b.icon:SetAllPoints()
    b:SetScript("OnEnter", function(self)
        if self.link then
            GameTooltip:SetOwner(self, "ANCHOR_LEFT")
            GameTooltip:SetHyperlink(self.link)
            GameTooltip:Show()
        end
    end)
    b:SetScript("OnLeave", function() GameTooltip:Hide() end)
    FG.Skin:Style(b, "icon")
    b:Hide()
    buttons[i] = b
    return b
end

local function Create()
    frame = CreateFrame("Frame", "FactoruideTracker", FG.UIRoot, "BackdropTemplate")
    frame:SetWidth(WIDTH)
    frame:SetClampedToScreen(true)
    frame:SetMovable(true)
    frame:EnableMouse(true)
    frame:RegisterForDrag("LeftButton")
    frame:SetScript("OnDragStart", function(self)
        if not FG:Get("lockFrames") then self:StartMoving() end
    end)
    frame:SetScript("OnDragStop", function(self)
        self:StopMovingOrSizing()
        local state = FG.Guide:State()
        state.trackerPoint = FG.PinTopLeft(self) or state.trackerPoint
    end)
    local point = FG.Guide and FG.Guide:State().trackerPoint
    if point then
        frame:SetPoint(unpack(point))
    elseif FactoruideNext then
        frame:SetPoint("TOPLEFT", FactoruideNext, "BOTTOMLEFT", 0, -3)
    else
        frame:SetPoint("RIGHT", UIParent, "RIGHT", -60, 0)
    end

    frame.header = frame:CreateTexture(nil, "ARTWORK")
    frame.header:SetPoint("TOPLEFT", 1, -1)
    frame.header:SetPoint("TOPRIGHT", -1, -1)
    frame.header:SetHeight(18)
    frame.title = frame:CreateFontString(nil, "OVERLAY", "GameFontNormalSmall")
    frame.title:SetPoint("LEFT", frame.header, "LEFT", 6, 0)
    frame.count = frame:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    frame.count:SetPoint("RIGHT", frame.header, "RIGHT", -6, 0)
    FG.Skin:Style(frame, "panel", { frame.title, frame.count })

    rows, buttons = {}, {}
    frame:SetScale(FG:Get("scale"))
    FG:OnSetting("scale", function(v) frame:SetScale(v) end)
    FG:Zoomable(frame)
end

function Tracker:Refresh()
    dirty = false
    if not frame then return end
    -- During combat, a tracker holding quest item buttons (secure) keeps its size and visibility;
    -- its lines still update (see `FG.CanLayout`, refreshed again after the fight).
    local resize = FG.CanLayout(frame)
    local shown = FG:Get("trackerShown")
    if resize then frame:SetShown(shown) end
    if not shown then return end

    local quests = CollectQuests()
    local max = FG:Get("trackerQuests")
    local n, used, itemIndex = 0, 0, 0
    local canAssign = not InCombatLockdown()
    local function add(text, questID, indent)
        n = n + 1
        local line = Line(n)
        line:ClearAllPoints()
        line:SetPoint("TOPLEFT", 8 + (indent or 0), -20 - (n - 1) * 16)
        line:SetPoint("RIGHT", -30, 0)
        line.text:SetText(text)
        line.questID = questID
        line:Show()
        return line
    end

    frame.title:SetText("Factoruide · " .. L.title)
    frame.count:SetText(
        string.format("%d/%d", #quests, QuestLog.GetMaxNumQuestsCanAccept and QuestLog.GetMaxNumQuestsCanAccept() or 20)
    )
    -- Professions: skill against the route's curve for the current level.
    local followed = FG.Guide and FG.Guide.Route and FG.Guide:Route()
    if FG:Get("professionsShown") and followed and followed.professions and #followed.professions > 0 then
        local level = UnitLevel("player")
        local goalLevel = math.min(60, (math.floor(level / 5) + 1) * 5)
        local char = FactoruideDB.characters and FactoruideDB.characters[FG:CharacterKey()]
        for _, p in ipairs(followed.professions) do
            local rank, maxRank, name = FG.ProfessionSkill(p.line)
            -- The curve starts when the profession is taken: one taken late (seen just learned
            -- after the route's start level) is followed that many levels later.
            local delay = 0
            if char then
                char.professionsTaken = char.professionsTaken or {}
                if rank and not char.professionsTaken[p.line] then
                    char.professionsTaken[p.line] = rank <= 5 and level or 0
                end
                delay = math.max(0, (char.professionsTaken[p.line] or 0) - (p.start or 0))
            end
            local function curveAt(l) return p.curve[math.max(0, math.min(l - delay, 60)) + 1] or 0 end
            local want = curveAt(level)
            local text
            if not rank then
                text = string.format("|cffe0b050%s|r  |cff9d9d9d%s|r", PROFESSION_NAMES[p.key] or p.name, L.notLearned)
            else
                local color = rank >= want and "40ff40" or (want - rank < 15 and "ffd100" or "ff4040")
                text = string.format("|cffe0b050%s|r  |cff%s%d|r|cff9d9d9d/%d|r", name or p.name, color, rank, want)
                if rank >= maxRank and maxRank < math.min(p.target, 300) and want >= maxRank then
                    text = text .. "  |cffff9040" .. L.trainer .. "|r"
                end
                text = text .. string.format("  |cff9d9d9d%s %d: %d|r", L.lvl, goalLevel, curveAt(goalLevel))
            end
            add(text)
        end
    end

    if #quests == 0 then add("|cff9d9d9d" .. L.empty .. "|r") end
    Tracker.shown = {}
    for _, q in ipairs(quests) do
        if used >= max then break end
        used = used + 1
        table.insert(Tracker.shown, q.id)
        local color = q.step and DifficultyColor(q.id) or "9d9d9d"
        local suffix = ""
        if not q.step then suffix = "  |cff777777(" .. L.offRoute .. ")|r" end
        if q.step and q.step.bg then suffix = "  |cff40c0ff(" .. L.alongTheWay .. ")|r" end
        local level = q.level and string.format("[%d] ", q.level) or ""
        add(string.format("|cff%s%s%s|r%s", color, level, q.title, suffix), q.id)

        -- Usable quest item, re-assigned only out of combat (secure button).
        local link, icon
        if q.index then
            link, icon = GetQuestLogSpecialItemInfo(q.index)
        end
        if link and itemIndex < MAX_BUTTONS then
            itemIndex = itemIndex + 1
            local b = ItemButton(itemIndex)
            if canAssign then
                b:ClearAllPoints()
                b:SetPoint("RIGHT", frame, "TOPRIGHT", -6, -20 - (n - 1) * 16 - 8)
                b:SetAttribute("type", "item")
                b:SetAttribute("item", link)
                b.link = link
                b.icon:SetTexture(icon)
                b:Show()
            end
        end

        if q.failed then
            add("|cffff4040" .. L.failed .. "|r", q.id, 10)
        elseif q.complete then
            local who = q.step and q.step.k == "turnin" and q.step.n
            add("|cff40ff40" .. L.turnIn .. "|r" .. (who and ("  |cff9d9d9d" .. who .. "|r") or ""), q.id, 10)
        else
            for k, o in ipairs(q.objectives) do
                if o.text and o.text ~= "" then
                    local text = o.finished and ("|cff6a6a6a" .. o.text .. "|r") or ("|cffd0d0d0" .. o.text .. "|r")
                    if FG.Party then text = text .. FG.Party:Suffix(q.id, k, o.numRequired) end
                    add((o.finished and "|TInterface\\RaidFrame\\ReadyCheck-Ready:12|t " or "- ") .. text, q.id, 10)
                end
            end
        end
    end
    for i = n + 1, #rows do
        rows[i]:Hide()
    end
    if canAssign then
        for i = itemIndex + 1, #buttons do
            buttons[i]:Hide()
        end
    end
    if not resize then return end
    frame:SetHeight(24 + n * 16)
    -- A position saved anchored by the bottom: from now on the tracker grows downwards.
    local pinned = FG.PinTopIfNeeded(frame)
    if pinned then FG.Guide:State().trackerPoint = pinned end
end

local function Schedule()
    if not dirty then
        dirty = true
        C_Timer.After(0.1, function() Tracker:Refresh() end)
    end
end

FG:On("PLAYER_LOGIN", function()
    Create()
    HookBlizzard()
    FG:OnSetting("replaceTracker", ApplyBlizzardVisibility)
    FG:OnSetting("trackerShown", Schedule)
    FG:OnSetting("trackerQuests", Schedule)
    FG:OnSetting("professionsShown", Schedule)
    Schedule()
end)

for _, event in ipairs({
    "QUEST_LOG_UPDATE",
    "QUEST_ACCEPTED",
    "QUEST_REMOVED",
    "QUEST_TURNED_IN",
    "UNIT_QUEST_LOG_CHANGED",
    "QUEST_WATCH_UPDATE",
    "PLAYER_REGEN_ENABLED",
    "PLAYER_LEVEL_UP",
    "SKILL_LINES_CHANGED",
}) do
    FG:On(event, Schedule)
end
