-- Quest panel replacing Blizzard's tracker: the quests of the log in the route's order (as many as
-- configured), and tabs for the guide's professions, reputations and items to keep. Blizzard's
-- tracker (QuestWatchFrame on Classic, ObjectiveTrackerFrame later) is hidden while
-- `replaceTracker` is on (deferred after combat when the frame is protected).

local FG = Factoruide
local T = FG.T
local QuestLog = FG.QuestLog
local Tracker = {}
FG.Tracker = Tracker
--- Quest IDs shown at the last refresh, in order.
Tracker.shown = {}

local L = {
    title = T("Quests"),
    turnIn = T("Ready"),
    offRoute = T("off route"),
    alongTheWay = T("along the way"),
    failed = T("Failed"),
    empty = T("No quest in progress"),
    notLearned = T("not learned"),
    trainer = T("trainer"),
    lvl = T("lvl"),
}

-- Profession names, for those not learned yet (the game names the others).
local PROFESSION_NAMES = {
    alchemy = T("Alchemy"),
    blacksmithing = T("Blacksmithing"),
    enchanting = T("Enchanting"),
    engineering = T("Engineering"),
    herbalism = T("Herbalism"),
    leatherworking = T("Leatherworking"),
    mining = T("Mining"),
    skinning = T("Skinning"),
    tailoring = T("Tailoring"),
    cooking = T("Cooking"),
    fishing = T("Fishing"),
    first_aid = T("First Aid"),
}

local WIDTH = 300
local Skin = FG.Skin
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
-- A panel with small tabs: the quests in progress (one line each, their objectives on mouse-over),
-- the professions and the reputations the guide has goals for (progress bars), the common items
-- the guide's quests still need (bags, bank). Shown or hidden by a button of the guide (and shown
-- at the bank). Under the full guide's card unless dragged away; dropped back near the card's
-- bottom, it sticks to it again.

local TABS = { "quests", "professions", "reputation", "items" }
local TAB_LABELS = {
    quests = T("Quests"),
    professions = T("Professions"),
    reputation = T("Reputation"),
    items = T("Items"),
}
local T = {
    planned = T("planned %d"),
    goal = T("goal %d"),
    goalStanding = T("goal %s"),
    bank = T("%d in the bank"),
    marked = T("in the bank"),
    take = T("take"),
    leave = T("leave"),
    markHint = T("Click: mark as in the bank (or not)"),
    questHint = T("Click: quest log • Shift-click: the compass goes there"),
    neededBy = T("For:"),
    noItems = T("No item to keep"),
    later = T("%d more for later"),
    laterTip = T(
        "Items the guide needs later, not seen in your bags nor bank yet: they show as their quest comes near. Click: show them all."
    ),
    fewer = T("Only the items useful now"),
}
-- Reputation standings from the bottom of Neutral, and the game's names of them.
local STANDINGS = { -42000, -6000, -3000, 0, 3000, 9000, 21000, 42000 }
local SNAP = 40 -- pixels: dropped this close to the card's bottom, the panel sticks to it

local tabs
-- The panel shows the bank's items while it is open, then the tab it had.
local bankTab
local reputations = {}
-- Every item to keep, not only those owned or needed soon (for the session).
local allItems = false

local function Minimal() return FG:Get("guideMode") == "minimal" end

local function Line(i)
    local line = rows[i]
    if line then return line end
    line = FG:DragHandle(CreateFrame("Button", nil, frame))
    line:SetHeight(15)
    line:RegisterForClicks("LeftButtonUp")
    line.icon = line:CreateTexture(nil, "ARTWORK")
    line.icon:SetSize(13, 13)
    line.icon:SetPoint("TOPLEFT", 0, -1)
    line.icon:SetTexCoord(0.08, 0.92, 0.08, 0.92)
    line.right = Skin:Text(line, 10, "text2")
    line.right:SetPoint("TOPRIGHT", 0, -2)
    line.right:SetJustifyH("RIGHT")
    line.text = Skin:Text(line, 11, "text")
    line.text:SetPoint("TOPLEFT", 0, -1)
    line.text:SetPoint("RIGHT", line.right, "LEFT", -6, 0)
    -- Progress bar under the line (professions, reputations).
    line.track = Skin:Fill(line, "ARTWORK", "text", 0.08)
    line.track:SetHeight(3)
    line.track:SetPoint("BOTTOMLEFT", 0, 2)
    line.track:SetPoint("BOTTOMRIGHT", 0, 2)
    line.fill = Skin:Fill(line, "OVERLAY", "accent", 1)
    line.fill:SetPoint("TOPLEFT", line.track)
    line.fill:SetPoint("BOTTOMLEFT", line.track)
    line.mark = Skin:Fill(line, "OVERLAY", "gold", 1)
    line.mark:SetSize(2, 7)
    line:SetScript("OnClick", function(self)
        if self.questID then
            if IsShiftKeyDown() and FG.Guide and FG.Guide.PointToQuest then
                FG.Guide:PointToQuest(self.questID)
            elseif QuestMapFrame_OpenToQuestDetails then
                QuestMapFrame_OpenToQuestDetails(self.questID)
            end
        elseif self.itemID and FG.Bank then
            FG.Bank:ToggleMark(self.itemID)
        elseif self.toggleItems then
            allItems = not allItems
            Tracker:Refresh()
        end
    end)
    line:SetScript("OnEnter", function(self)
        if not self.tooltip then return end
        GameTooltip:SetOwner(self, "ANCHOR_LEFT")
        for i, l in ipairs(self.tooltip) do
            if i == 1 then
                GameTooltip:AddLine(l, 1, 1, 1, true)
            else
                GameTooltip:AddLine(l, 0.73, 0.77, 0.84, true)
            end
        end
        GameTooltip:Show()
    end)
    line:SetScript("OnLeave", function() GameTooltip:Hide() end)
    rows[i] = line
    return line
end

--- Secure buttons to use quest items (only re-assigned out of combat).
local function ItemButton(i)
    local b = buttons[i]
    if b then return b end
    b = CreateFrame("Button", "FactoruideItemButton" .. i, frame, "SecureActionButtonTemplate")
    b:SetSize(14, 14)
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

--- Where it was dragged; else under the full guide, or on the right with the minimal one.
local function Place()
    local point = FG.Guide and FG.Guide:Layout().trackerPoint
    local mode = Minimal() and "minimal" or "full"
    local key = point and "saved " .. mode or mode
    if frame.placed == key then return end
    frame.placed = key
    frame:ClearAllPoints()
    if point then
        frame:SetPoint(unpack(point))
    elseif key == "full" and FactoruideTodo then
        frame:SetPoint("TOPLEFT", FactoruideTodo, "BOTTOMLEFT", 0, -6)
    else
        frame:SetPoint("TOPRIGHT", UIParent, "TOPRIGHT", -30, -190)
    end
end

--- Dropped near the bottom of the full guide's card: stuck to it again; else where it was dropped.
local function Dropped(self)
    local state = FG.Guide:Layout()
    local card = FactoruideTodo
    local left, top = self:GetLeft(), self:GetTop()
    if not Minimal() and card and card:IsShown() and left and top and card:GetLeft() and card:GetBottom() then
        local dx, dy = left - card:GetLeft(), top - card:GetBottom()
        if math.abs(dx) < SNAP and math.abs(dy) < SNAP then
            state.trackerPoint = nil
            self.placed = nil
            Place()
            return
        end
    end
    state.trackerPoint = FG.PinTopLeft(self) or state.trackerPoint
    self.placed = "saved " .. (Minimal() and "minimal" or "full")
end

local function Create()
    frame = CreateFrame("Frame", "FactoruideTracker", FG.UIRoot, "BackdropTemplate")
    frame:SetWidth(WIDTH)
    frame:SetClampedToScreen(true)
    -- Its secure quest item buttons make it protected: not moved during combat (FG:Draggable).
    FG:Draggable(frame, Dropped)
    -- Tabs: small words in the header, the one shown in white over a thin line.
    tabs = {}
    for _, key in ipairs(TABS) do
        local b = FG:DragHandle(CreateFrame("Button", nil, frame))
        b:SetHeight(18)
        b.label = Skin:Text(b, 10, "muted")
        b.label:SetPoint("CENTER", 0, 1)
        b.line = Skin:Fill(b, "OVERLAY", "accent", 1)
        b.line:SetHeight(1)
        b.line:SetPoint("BOTTOMLEFT", 2, 0)
        b.line:SetPoint("BOTTOMRIGHT", -2, 0)
        b:SetScript("OnClick", function()
            bankTab = nil
            FG:Save("trackerTab", key)
        end)
        tabs[key] = b
    end
    frame.line = Skin:Fill(frame, "ARTWORK", "line", 1)
    frame.line:SetHeight(1)
    frame.line:SetPoint("TOPLEFT", 1, -24)
    frame.line:SetPoint("TOPRIGHT", -1, -24)
    FG.Skin:Style(frame, "panel")
    rows, buttons = {}, {}
    frame:SetScale(FG:Get("scale"))
    FG:OnSetting("scale", function(v) frame:SetScale(v) end)
    FG:Zoomable(frame)
end

-- Tabs' contents ------------------------------------------------------------------------

--- Sum of the objectives' counts: "4/6", or how many objectives are done.
local function Progress(q)
    local have, need, done, total = 0, 0, 0, 0
    for _, o in ipairs(q.objectives) do
        if o.text and o.text ~= "" then
            total = total + 1
            if o.finished then done = done + 1 end
            if (o.numRequired or 0) > 0 then
                have = have + math.min(o.numFulfilled or 0, o.numRequired)
                need = need + o.numRequired
            end
        end
    end
    if need > 0 then return string.format("%d/%d", have, need) end
    if total > 0 then return string.format("%d/%d", done, total) end
    return ""
end

local function StandingName(points)
    local n = 1
    for i, from in ipairs(STANDINGS) do
        if points >= from then n = i end
    end
    return _G["FACTION_STANDING_LABEL" .. n] or tostring(n)
end

--- Reputations of the guide's goals, read again when the game says they changed.
local function ReadReputations()
    wipe(reputations)
    for _, r in ipairs(FG.Reputations(true)) do
        reputations[r.id] = r
    end
end

--- The tabs that have something to show, and the items to keep.
local function Available(followed, items, later)
    return {
        quests = true,
        professions = FG:Get("professionsShown") and followed and followed.professions and #followed.professions > 0,
        reputation = followed and followed.reputations and #followed.reputations > 0,
        items = #items > 0 or later > 0 or allItems,
    }
end

function Tracker:Refresh()
    dirty = false
    if not frame then return end
    -- During combat, a tracker holding quest item buttons (secure) keeps its size and visibility;
    -- its lines still update (see `FG.CanLayout`, refreshed again after the fight).
    local resize = FG.CanLayout(frame)
    local shown = FG:Get("trackerShown") and (FG:Get("questPanel") or bankTab ~= nil)
    Tracker.visible = shown
    if resize then frame:SetShown(shown) end
    if not shown then return end
    if resize then
        Place()
        frame:SetWidth(Minimal() and 270 or WIDTH)
    end
    local canAssign = not InCombatLockdown()
    local followed = FG.Guide and FG.Guide.Route and FG.Guide:Route()
    local items, later = {}, 0
    if FG.Bank then
        items, later = FG.Bank:Items(allItems)
    end
    local available = Available(followed, items, later)
    local tab = bankTab or FG:Get("trackerTab")
    if not available[tab] then tab = "quests" end
    local quests = CollectQuests()

    -- Header: the tabs that have something.
    local counts = { quests = #quests, items = #items }
    local x = 4
    for _, key in ipairs(TABS) do
        local b = tabs[key]
        b:SetShown(available[key] and true or false)
        if available[key] then
            local label = TAB_LABELS[key] .. (counts[key] and ("  " .. counts[key]) or "")
            b.label:SetText(label)
            local c = Skin.colors[key == tab and "text" or "muted"]
            b.label:SetTextColor(c[1], c[2], c[3])
            b.line:SetShown(key == tab)
            b:SetWidth((tonumber(b.label:GetStringWidth()) or 40) + 12)
            b:ClearAllPoints()
            b:SetPoint("TOPLEFT", x, -3)
            x = x + (tonumber(b:GetWidth()) or 50)
        end
    end

    local n, itemIndex, y = 0, 0, -30
    local function add(height)
        n = n + 1
        local line = Line(n)
        line:ClearAllPoints()
        line:SetPoint("TOPLEFT", 10, y)
        line:SetPoint("RIGHT", -10, 0)
        line:SetHeight(height)
        line.questID, line.itemID, line.tooltip, line.toggleItems = nil, nil, nil, nil
        line.icon:Hide()
        line.track:Hide()
        line.fill:Hide()
        line.mark:Hide()
        line.text:ClearAllPoints()
        line.text:SetPoint("TOPLEFT", 0, -1)
        line.text:SetPoint("RIGHT", line.right, "LEFT", -6, 0)
        line.right:ClearAllPoints()
        line.right:SetPoint("TOPRIGHT", 0, -2)
        line.right:SetText("")
        line:Show()
        y = y - height
        return line
    end
    local function Bar(line, share, mark, color)
        line.track:Show()
        line.fill:Show()
        local w = (tonumber(line:GetWidth()) or 0) > 0 and line:GetWidth() or (Minimal() and 250 or WIDTH - 20)
        line.fill:SetWidth(math.max(1, w * math.max(0, math.min(1, share))))
        local c = Skin.colors[color or "accent"]
        line.fill:SetColorTexture(c[1], c[2], c[3], 1)
        line.mark:SetShown(mark ~= nil)
        if mark then
            line.mark:ClearAllPoints()
            line.mark:SetPoint("CENTER", line.track, "LEFT", w * math.max(0, math.min(1, mark)), 0)
        end
    end

    -- The quests in progress, in the guide's order: one line each.
    Tracker.shown = {}
    local max = FG:Get("trackerQuests")
    for _, q in ipairs(quests) do
        if #Tracker.shown >= max then break end
        table.insert(Tracker.shown, q.id)
        if tab == "quests" then
            local line = add(16)
            line.questID = q.id
            local color = q.step and Skin.Ink(DifficultyColor(q.id)) or Skin.Hex("muted"):sub(5)
            local suffix = q.level and ("  " .. Skin.Hex("muted") .. q.level .. "|r") or ""
            if not q.step then suffix = suffix .. "  " .. Skin.Hex("dim") .. L.offRoute .. "|r" end
            if q.step and q.step.bg then suffix = suffix .. "  " .. Skin.Hex("lightBlue") .. L.alongTheWay .. "|r" end
            line.text:SetText(string.format("|cff%s%s|r%s", color, q.title, suffix))
            local who = q.step and q.step.k == "turnin" and q.step.n
            if q.failed then
                line.right:SetText(Skin.Hex("red") .. L.failed .. "|r")
            elseif q.complete then
                line.right:SetText(Skin.Hex("green") .. L.turnIn .. "|r")
            else
                line.right:SetText(Progress(q))
            end
            -- Mouse-over: the objectives.
            local tip = { q.title }
            for k, o in ipairs(q.objectives) do
                if o.text and o.text ~= "" then
                    local text = (o.finished and "|TInterface\\RaidFrame\\ReadyCheck-Ready:12|t " or "- ") .. o.text
                    if FG.Party then
                        text = text
                            .. FG.Party:Suffix(q.id, k, o.numRequired):gsub("|c%x%x%x%x%x%x%x%x", ""):gsub("|r", "")
                    end
                    table.insert(tip, text)
                end
            end
            if q.complete then table.insert(tip, L.turnIn .. (who and (" • " .. who) or "")) end
            table.insert(tip, T.questHint)
            line.tooltip = tip
            -- Usable quest item, re-assigned only out of combat (secure button).
            local link, icon
            if q.index then
                link, icon = GetQuestLogSpecialItemInfo(q.index)
            end
            if link and itemIndex < MAX_BUTTONS then
                itemIndex = itemIndex + 1
                line.right:ClearAllPoints()
                line.right:SetPoint("TOPRIGHT", -18, -2)
                local b = ItemButton(itemIndex)
                if canAssign then
                    b:ClearAllPoints()
                    b:SetPoint("TOPRIGHT", line, "TOPRIGHT", 0, -1)
                    b:SetAttribute("type", "item")
                    b:SetAttribute("item", link)
                    b.link = link
                    b.icon:SetTexture(icon)
                    b:Show()
                end
            end
        end
    end
    if tab == "quests" and #quests == 0 then add(16).text:SetText(Skin.Hex("muted") .. L.empty .. "|r") end

    -- Professions: skill against the guide's curve for the current level and the goal.
    if tab == "professions" then
        local level = UnitLevel("player")
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
            local want, goal = curveAt(level), math.max(1, p.target or curveAt(60))
            local line = add(24)
            line.text:SetText(Skin.Hex("text") .. (name or PROFESSION_NAMES[p.key] or p.name) .. "|r")
            if not rank then
                line.right:SetText(Skin.Hex("muted") .. L.notLearned .. "|r")
            else
                local color = rank >= want and "green" or (want - rank < 15 and "gold" or "red")
                local right = Skin.Hex(color) .. rank .. "|r" .. Skin.Hex("muted") .. " / " .. goal .. "|r"
                if rank >= maxRank and maxRank < math.min(goal, 300) and want >= maxRank then
                    right = Skin.Hex("orange") .. L.trainer .. "|r  " .. right
                end
                line.right:SetText(right)
                Bar(line, rank / goal, want / goal, color)
            end
            line.tooltip = { name or p.name, string.format(T.planned, want), string.format(T.goal, goal) }
        end
    end

    -- Reputations of the guide's goals: points from the bottom of Neutral.
    if tab == "reputation" then
        for _, g in ipairs(followed.reputations) do
            local r = reputations[g.id]
            local points = r and r.value or 0
            local line = add(24)
            line.text:SetText((r and r.name) or g.name)
            local reached = points >= g.target
            line.right:SetText(
                Skin.Hex(reached and "green" or "text2")
                    .. StandingName(points)
                    .. "|r"
                    .. Skin.Hex("muted")
                    .. "  "
                    .. points
                    .. " / "
                    .. g.target
                    .. "|r"
            )
            Bar(line, points / math.max(1, g.target), nil, reached and "green" or "accent")
            line.tooltip = { (r and r.name) or g.name, string.format(T.goalStanding, StandingName(g.target)) }
        end
    end

    -- Items the guide's quests still need: in the bags against the need, and the bank.
    if tab == "items" then
        for _, item in ipairs(items) do
            local line = add(16)
            line.itemID = item.id
            line.icon:Show()
            line.icon:SetTexture(item.icon)
            line.text:ClearAllPoints()
            line.text:SetPoint("TOPLEFT", 18, -1)
            line.text:SetPoint("RIGHT", line.right, "LEFT", -6, 0)
            local text = item.name or ("#" .. item.id)
            if item.hint then
                text = text .. "  " .. Skin.Hex(item.hint == "take" and "green" or "muted") .. T[item.hint] .. "|r"
            end
            line.text:SetText(text)
            local enough = item.bags >= item.need
            local right = Skin.Hex(enough and "green" or "text2") .. item.bags .. "/" .. item.need .. "|r"
            if (item.bank or 0) > 0 then
                right = Skin.Hex("muted") .. string.format(T.bank, item.bank) .. "|r  " .. right
            elseif item.marked then
                right = Skin.Hex("muted") .. T.marked .. "|r  " .. right
            end
            line.right:SetText(right)
            local tip = { item.name or ("#" .. item.id), T.neededBy }
            for _, q in ipairs(item.quests) do
                table.insert(tip, string.format("%s x%d", q.name, q.count))
            end
            table.insert(tip, T.markHint)
            line.tooltip = tip
        end
        if later > 0 or allItems then
            local line = add(16)
            line.toggleItems = true
            local text = allItems and T.fewer or string.format(T.later, later)
            line.text:SetText(Skin.Hex(allItems and "accent" or "dim") .. text .. "|r")
            line.tooltip = not allItems and { text, T.laterTip } or nil
        end
    end

    for i = n + 1, #rows do
        rows[i]:Hide()
    end
    Tracker.lines, Tracker.tab = n, tab
    if canAssign then
        for i = itemIndex + 1, #buttons do
            buttons[i]:Hide()
        end
    end
    if not resize then return end
    frame:SetHeight(-y + 6)
    -- A position saved anchored by the bottom: from now on the tracker grows downwards.
    local pinned = FG.PinTopIfNeeded(frame)
    if pinned then FG.Guide:State().trackerPoint = pinned end
end

--- Show or hide the panel (the guide's button), kept for the next sessions.
function Tracker:Toggle() FG:Save("questPanel", not FG:Get("questPanel")) end

--- While the bank is open: the items tab, shown even when hidden (`false`: back as it was).
function Tracker:ShowItems(on)
    bankTab = on and "items" or nil
    self:Refresh()
end

local function Schedule()
    if not dirty then
        dirty = true
        -- Bags and the quest log change in bursts (looting, selling): one refresh after them.
        C_Timer.After(0.3, function() FG:Try("tracker", Tracker.Refresh, Tracker) end)
    end
end

FG:On("PLAYER_LOGIN", function()
    Create()
    HookBlizzard()
    ReadReputations()
    FG:OnSetting("replaceTracker", ApplyBlizzardVisibility)
    for _, key in ipairs({
        "trackerShown",
        "trackerQuests",
        "trackerAll",
        "professionsShown",
        "guideMode",
        "trackerTab",
        "questPanel",
    }) do
        FG:OnSetting(key, Schedule)
    end
    Schedule()
end)

FG:On("UPDATE_FACTION", function()
    ReadReputations()
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
    "BAG_UPDATE",
}) do
    FG:On(event, Schedule)
end
