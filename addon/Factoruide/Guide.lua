-- Shows the planned route and advances automatically. Two boxes:
--   "To do": the current tasks (steps done at the same place are grouped), with progress
--   "Next": the following steps, as many lines as configured (0 hides it)
-- The native waypoint arrow points at the first unfinished task.

local _, FG = ...
local Guide = {}
FG.Guide = Guide

local fr = GetLocale() == "frFR"
local L = {
    todo = fr and "À faire" or "To do",
    next = fr and "Ensuite" or "Next",
    level = fr and "Niv." or "Lvl",
    planned = fr and "prévu" or "planned",
    noRoute = fr and "Pas de route pour cette race/classe.\nGénérez-en une avec `fg route`."
        or "No route for this race/class.\nGenerate one with `fg route`.",
    done = fr and "Route terminée !" or "Route finished!",
    yards = fr and "m" or "yd",
    resync = fr and "Resynchroniser : aller à la première étape pas encore faite"
        or "Resync: go to the first step not done yet",
    ahead = fr and "En avance : sauter au palier niveau %d (%d étapes)"
        or "Ahead: skip to the level %d checkpoint (%d steps)",
    aheadTip = fr
            and "Vous avez atteint le niveau de ce palier plus tôt que prévu : les étapes jusqu'à lui sont passées, sauf les chaînes qui continuent après, les quêtes de classe, les récompenses prévues et l'entraînement."
        or "You reached this checkpoint's level earlier than planned: the steps up to it are skipped, except chains going on after it, class quests, planned rewards and training.",
    alongTheWay = fr and "(en parallèle)" or "(along the way)",
    elite = fr and "[Élite]" or "[Elite]",
    skipHint = fr and "Clic droit : marquer comme déjà fait (ou annuler)"
        or "Right-click: mark as already done (or undo)",
    skipped = fr and "quête marquée comme faite : %s (/fg skip %d pour annuler)"
        or "quest marked as done: %s (/fg skip %d to undo)",
    unskipped = fr and "quête de nouveau à faire : %s" or "quest to do again: %s",
    stepSkipped = fr and "étape marquée comme faite" or "step marked as done",
}

local KINDS = {
    accept = {
        icon = "Interface\\GossipFrame\\AvailableQuestIcon",
        color = "ffd100",
        label = fr and "Prendre" or "Accept",
    },
    turnin = {
        icon = "Interface\\GossipFrame\\ActiveQuestIcon",
        color = "40ff40",
        label = fr and "Rendre" or "Turn in",
    },
    objective = { icon = "Interface\\Icons\\Ability_DualWield", color = "ff9040", label = fr and "Faire" or "Do" },
    fly = { icon = "Interface\\GossipFrame\\TaxiGossipIcon", color = "40c0ff", label = fr and "Vol" or "Fly" },
    flight_master = {
        icon = "Interface\\GossipFrame\\TaxiGossipIcon",
        color = "40c0ff",
        label = fr and "Point de vol" or "Flight path",
    },
    link = { icon = "Interface\\Icons\\INV_Misc_Map_01", color = "40c0ff", label = "Transport" },
    grind = { icon = "Interface\\Icons\\Ability_Rogue_Ambush", color = "c080ff", label = fr and "Farmer" or "Grind" },
    train = { icon = "Interface\\Icons\\INV_Misc_Book_09", color = "ffffff", label = fr and "Entraîneur" or "Trainer" },
    hearth = {
        icon = "Interface\\Icons\\INV_Misc_Rune_01",
        color = "40c0ff",
        label = fr and "Pierre de foyer" or "Hearthstone",
    },
    bind = { icon = "Interface\\GossipFrame\\BinderGossipIcon", color = "40c0ff", label = fr and "Auberge" or "Inn" },
    dungeon = { icon = "Interface\\Icons\\INV_Misc_Key_10", color = "ff4040", label = fr and "Donjon" or "Dungeon" },
    profession = {
        icon = "Interface\\Icons\\Trade_BlackSmithing",
        color = "e0b050",
        label = fr and "Métier" or "Profession",
    },
    practice = {
        icon = "Interface\\Icons\\INV_Misc_Note_01",
        color = "e0b050",
        label = fr and "Pratique" or "Practice",
    },
}

local GROUP_MAX = 6
-- Steps planned this many levels below the character are skipped.
local OUTDATED_LEVELS = 4
local WIDTH = 330

local state -- per character: { route = name, step = n, point = {...}, flightLearned = id }
local route
local routeQuests = {}
local acceptIndex = {} -- quest -> index of its accept step
-- Whether the shown step was already done when we arrived on it (manual navigation): the
-- guide only moves on when the shown step becomes done.
local arrivedDone = false
local todoBox, nextBox
local waypointStep
local Distance -- defined below, used by IsDone

-- Progress -------------------------------------------------------------------

local function QuestTitle(step) return step.q and C_QuestLog.GetTitleForQuestID(step.q) or nil end

--- Game objective matching the step's target name.
local function FindObjective(step)
    local objectives = C_QuestLog.GetQuestObjectives(step.q)
    if objectives and step.n then
        local name = step.n:lower()
        for _, o in ipairs(objectives) do
            if o.text and o.text:lower():find(name, 1, true) then return o end
        end
    end
end

--- Skill line of a "profession" step (its target is the trainer), from the route professions.
local function ProfessionLine(step)
    if step.line then return step.line end
    for _, p in ipairs(route.professions or {}) do
        if step.t:find(p.name, 1, true) then return p.line end
    end
    return 0
end

local IsDone

--- The quest's accept step was passed without accepting it (skipped by hand, left behind,
--- alternative done): its objectives and turn-in cannot be done.
local function AcceptSkipped(step, index)
    local at = acceptIndex[step.q]
    return at ~= nil
        and at < index
        and not C_QuestLog.IsOnQuest(step.q)
        and not C_QuestLog.IsQuestFlaggedCompleted(step.q)
        and IsDone(route.steps[at], at)
end

function IsDone(step, index)
    local q = step.q
    if state.marks[index or state.step] == "skip" or (q and state.skippedQuests[q]) then
        return true -- marked as done by hand
    end
    -- Planned well below our level: left behind (a character resuming the route, a quest
    -- added to the game after we went through that zone), unless the quest is in the log.
    if step.lvl and UnitLevel("player") - step.lvl >= OUTDATED_LEVELS and not (q and C_QuestLog.IsOnQuest(q)) then
        return true
    end
    -- An exclusive alternative (or the quest a breadcrumb leads to) is done or in progress:
    -- this quest cannot be taken any more.
    if step.alt and not C_QuestLog.IsOnQuest(q) then
        for _, other in ipairs(step.alt) do
            if C_QuestLog.IsQuestFlaggedCompleted(other) or C_QuestLog.IsOnQuest(other) then return true end
        end
    end
    if step.k == "accept" then
        if C_QuestLog.IsOnQuest(q) or C_QuestLog.IsQuestFlaggedCompleted(q) then return true end
        -- Profession quest (optional): skipped without the skill it needs.
        if step.line and step.sk then
            local rank = FG.ProfessionSkill(step.line)
            return rank == nil or rank < step.sk
        end
        return false
    elseif step.k == "objective" then
        if C_QuestLog.IsQuestFlaggedCompleted(q) then return true end
        if AcceptSkipped(step, index or state.step) then return true end
        if not C_QuestLog.IsOnQuest(q) then return false end
        local o = FindObjective(step)
        if o then return o.finished end
        return C_QuestLog.IsComplete(q)
    elseif step.k == "turnin" then
        return C_QuestLog.IsQuestFlaggedCompleted(q) or AcceptSkipped(step, index or state.step)
    elseif step.k == "grind" then
        -- To a level, or to an XP amount in it (grind placed before the end of the level).
        local level = UnitLevel("player")
        return level > step.lvl or (level == step.lvl and (not step.xp or UnitXP("player") >= step.xp))
    elseif step.k == "fly" then
        return UnitOnTaxi("player")
    elseif step.k == "practice" then
        local rank = FG.ProfessionSkill(step.id)
        return rank ~= nil and rank >= step.sk
    elseif
        step.k == "profession"
        and step.sk
        and (select(2, FG.ProfessionSkill(ProfessionLine(step))) or 0) >= step.sk
    then
        return true -- rank already learned
    elseif step.k == "flight_master" or step.k == "train" or step.k == "bind" or step.k == "profession" then
        return state.marks[index or state.step] or false
    elseif step.k == "dungeon" then
        return state.marks[index or state.step] == true
    elseif step.k == "hearth" then
        local d = Distance(step)
        return d ~= nil and d < 100
    elseif step.k == "link" then
        local nextStep = route.steps[(index or state.step) + 1]
        return nextStep and nextStep.m and C_Map.GetBestMapForUnit("player") == nextStep.m or false
    end
    return false
end

--- Steps done around the same place as the current one, shown together.
local function CurrentGroup()
    local first = route.steps[state.step]
    local group = { state.step }
    if not first or not first.m then return group end
    for i = state.step + 1, math.min(#route.steps, state.step + 12) do
        local s = route.steps[i]
        if #group >= GROUP_MAX or not s.m or s.m ~= first.m or s.k == "fly" or s.k == "link" or s.k == "grind" then
            break
        end
        local close = math.abs(s.x - first.x) < 2.5 and math.abs(s.y - first.y) < 2.5
        if not close and s.id ~= first.id then break end
        table.insert(group, i)
    end
    return group
end

--- Distance in yards from the player to a step, when both are on the same continent.
function Distance(step)
    if not step.m or not C_Map.GetWorldPosFromMapPos then return nil end
    local playerMap = C_Map.GetBestMapForUnit("player")
    local pos = playerMap and C_Map.GetPlayerMapPosition(playerMap, "player")
    if not pos then return nil end
    local c1, w1 = C_Map.GetWorldPosFromMapPos(playerMap, pos)
    local c2, w2 = C_Map.GetWorldPosFromMapPos(step.m, CreateVector2D(step.x / 100, step.y / 100))
    if not c1 or c1 ~= c2 or not w1 or not w2 then return nil end
    local x1, y1 = w1:GetXY()
    local x2, y2 = w2:GetXY()
    return math.sqrt((x1 - x2) ^ 2 + (y1 - y2) ^ 2)
end

local function SetWaypoint(step)
    if not step or not step.m or not FG:Get("waypoints") or step == waypointStep then return end
    waypointStep = step
    if C_Map.CanSetUserWaypointOnMap(step.m) then
        C_Map.SetUserWaypoint(UiMapPoint.CreateFromCoordinates(step.m, step.x / 100, step.y / 100))
        C_SuperTrack.SetSuperTrackedUserWaypoint(true)
    end
end

-- Text -----------------------------------------------------------------------

local function StepLine(step, withProgress)
    local kind = KINDS[step.k] or { color = "ffffff", label = step.k }
    local title = QuestTitle(step)
    local text
    if step.k == "accept" or step.k == "turnin" then
        text = (title or step.t) .. (step.n and ("  |cff9d9d9d" .. step.n .. "|r") or "")
        -- Reward the guide picked (it counts on wearing it).
        local reward = step.rw and C_Item and C_Item.GetItemNameByID and C_Item.GetItemNameByID(step.rw)
        if reward then text = text .. "  |cff40ff40" .. reward .. "|r" end
    elseif step.k == "train" and step.sp then
        -- Spells worth learning, in the game's language.
        local names = {}
        for _, id in ipairs(step.sp) do
            local name = FG.SpellName(id)
            if not name then break end
            local rank = GetSpellSubtext and GetSpellSubtext(id)
            names[#names + 1] = (rank and rank ~= "") and (name .. " (" .. rank .. ")") or name
        end
        text = #names == #step.sp and ((step.n or "") .. ": " .. table.concat(names, ", ")) or step.t
    elseif step.k == "objective" then
        text = step.t
        if withProgress then
            local o = C_QuestLog.IsOnQuest(step.q) and FindObjective(step)
            if o and o.numRequired and o.numRequired > 0 then
                text = string.format("%s  |cffffffff%d/%d|r", step.n or step.t, o.numFulfilled, o.numRequired)
            end
        end
        if step.bg then text = text .. "  |cff40c0ff" .. L.alongTheWay .. "|r" end
        if title then text = text .. "  |cff9d9d9d" .. title .. "|r" end
        -- Danger: elite mobs, or mobs well above the character.
        if step.el then text = text .. "  |cffff4040" .. L.elite .. "|r" end
        if step.ml and step.ml >= UnitLevel("player") + 3 then
            text = text .. string.format("  |cffff8040%s %d|r", L.level, step.ml)
        end
    elseif step.k == "practice" and withProgress then
        local rank = FG.ProfessionSkill(step.id)
        text = step.t .. (rank and string.format("  |cffffffff%d/%d|r", rank, step.sk) or "")
    else
        text = step.t
    end
    return string.format("|cff%s%s|r  %s", kind.color, kind.label, text)
end

-- UI -------------------------------------------------------------------------

local function CreateBox(name, title, kind)
    local box = CreateFrame("Frame", name, UIParent, "BackdropTemplate")
    box:SetWidth(WIDTH)
    box:SetClampedToScreen(true)

    box.header = box:CreateTexture(nil, "ARTWORK")
    box.header:SetPoint("TOPLEFT", 1, -1)
    box.header:SetPoint("TOPRIGHT", -1, -1)
    box.header:SetHeight(18)

    box.title = box:CreateFontString(nil, "OVERLAY", "GameFontNormalSmall")
    box.title:SetPoint("LEFT", box.header, "LEFT", 6, 0)
    box.title:SetText(title)
    box.info = box:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    box.info:SetPoint("RIGHT", box.header, "RIGHT", -70, 0)
    FG.Skin:Style(box, kind, { box.title, box.info })

    box.rows = {}
    return box
end

local function Row(box, i, big)
    local row = box.rows[i]
    if row then return row end
    row = CreateFrame("Button", nil, box)
    row:SetHeight(big and 20 or 16)
    row:RegisterForClicks("RightButtonUp")
    row:SetScript("OnClick", function(self, button)
        if button == "RightButton" and self.index then Guide:ToggleSkip(self.index) end
    end)
    row:SetScript("OnEnter", function(self)
        local step = self.index and route and route.steps[self.index]
        if not step then return end
        GameTooltip:SetOwner(self, "ANCHOR_LEFT")
        GameTooltip:AddLine(step.t, 1, 1, 1, true)
        GameTooltip:AddLine(L.skipHint, 0.6, 0.6, 0.6, true)
        GameTooltip:Show()
    end)
    row:SetScript("OnLeave", function() GameTooltip:Hide() end)
    row.icon = row:CreateTexture(nil, "ARTWORK")
    row.icon:SetSize(big and 16 or 12, big and 16 or 12)
    row.icon:SetPoint("LEFT")
    row.check = row:CreateTexture(nil, "OVERLAY")
    row.check:SetSize(14, 14)
    row.check:SetPoint("LEFT", row.icon, "LEFT", 2, -2)
    row.check:SetTexture("Interface\\RaidFrame\\ReadyCheck-Ready")
    row.text = row:CreateFontString(nil, "OVERLAY", big and "GameFontHighlight" or "GameFontHighlightSmall")
    row.text:SetPoint("LEFT", row.icon, "RIGHT", 5, 0)
    row.text:SetPoint("RIGHT")
    row.text:SetJustifyH("LEFT")
    row.text:SetWordWrap(false)
    FG.Skin:Font(row.text)
    box.rows[i] = row
    return row
end

local function Layout(box, count, big, footer)
    local h = big and 20 or 16
    for i, row in ipairs(box.rows) do
        if i <= count then
            row:ClearAllPoints()
            row:SetPoint("TOPLEFT", 6, -20 - (i - 1) * h - 2)
            row:SetPoint("RIGHT", -6, 0)
            row:Show()
        else
            row:Hide()
        end
    end
    box:SetHeight(24 + count * h + (footer and 16 or 0))
    -- A position saved anchored by the bottom: from now on the box grows downwards.
    if box == todoBox then state.point = FG.PinTopIfNeeded(box) or state.point end
end

local function HeaderButton(box, text, x, onClick)
    local b = CreateFrame("Button", nil, box)
    b:SetSize(16, 16)
    b:SetPoint("TOPRIGHT", x, -2)
    b:SetNormalFontObject("GameFontNormalSmall")
    b:SetHighlightFontObject("GameFontHighlightSmall")
    if text:find("\\") then
        b:SetNormalTexture(text) -- an icon
        b:SetHighlightTexture(text, "ADD")
    else
        b:SetText(text)
    end
    b:SetScript("OnClick", onClick)
    b:SetScript("OnEnter", function(self)
        if self.tooltip then
            GameTooltip:SetOwner(self, "ANCHOR_TOP")
            GameTooltip:SetText(self.tooltip, 1, 1, 1, 1, true)
            GameTooltip:Show()
        end
    end)
    b:SetScript("OnLeave", function() GameTooltip:Hide() end)
    return b
end

local function CreateUI()
    todoBox = CreateBox("FactoruideTodo", "Factoruide · " .. L.todo, "window")
    todoBox:SetPoint(unpack(state.point or { "TOPRIGHT", UIParent, "TOPRIGHT", -240, -220 }))
    todoBox:SetMovable(true)
    todoBox:EnableMouse(true)
    todoBox:RegisterForDrag("LeftButton")
    todoBox:SetScript("OnDragStart", function(self)
        if not FG:Get("lockFrames") then self:StartMoving() end
    end)
    todoBox:SetScript("OnDragStop", function(self)
        self:StopMovingOrSizing()
        state.point = FG.PinTopLeft(self) or state.point
    end)
    HeaderButton(todoBox, ">", -4, function() Guide:Go(state.step + 1) end)
    HeaderButton(todoBox, "<", -20, function() Guide:Go(state.step - 1) end)
    HeaderButton(todoBox, "Interface\\Buttons\\UI-RefreshButton", -36, function() Guide:Resync() end).tooltip = L.resync
    HeaderButton(todoBox, "*", -52, function() FG:OpenSettings() end)

    todoBox.footer = todoBox:CreateFontString(nil, "OVERLAY", "GameFontDisableSmall")
    todoBox.footer:SetPoint("BOTTOMLEFT", 8, 5)
    todoBox.footer:SetPoint("BOTTOMRIGHT", -8, 5)
    todoBox.footer:SetJustifyH("LEFT")

    -- Above the window, which grows downwards.
    local ahead = CreateFrame("Button", "FactoruideSkipAhead", todoBox, "UIPanelButtonTemplate")
    ahead:SetHeight(22)
    ahead:SetPoint("BOTTOMLEFT", todoBox, "TOPLEFT", 0, 2)
    ahead:SetPoint("BOTTOMRIGHT", todoBox, "TOPRIGHT", 0, 2)
    ahead:SetScript("OnClick", function() Guide:SkipAhead() end)
    ahead:SetScript("OnEnter", function(self)
        GameTooltip:SetOwner(self, "ANCHOR_TOP")
        GameTooltip:SetText(L.aheadTip, 1, 1, 1, 1, true)
        GameTooltip:Show()
    end)
    ahead:SetScript("OnLeave", function() GameTooltip:Hide() end)
    ahead:Hide()
    if FG.Skin then FG.Skin:Style(ahead, "button") end
    todoBox.ahead = ahead

    nextBox = CreateBox("FactoruideNext", L.next, "panel")
    nextBox:SetPoint("TOPLEFT", todoBox, "BOTTOMLEFT", 0, -3)
    HeaderButton(nextBox, "+", -4, function() FG:Set("nextLines", math.min(15, FG:Get("nextLines") + 1)) end)
    HeaderButton(nextBox, "-", -20, function() FG:Set("nextLines", math.max(0, FG:Get("nextLines") - 1)) end)

    local function ApplyScale(v)
        todoBox:SetScale(v)
        nextBox:SetScale(v)
    end
    ApplyScale(FG:Get("scale"))
    FG:OnSetting("scale", ApplyScale)
    FG:OnSetting("nextLines", function() Guide:Refresh() end)
    FG:OnSetting("guideShown", function() Guide:Refresh() end)
    FG:OnSetting("waypoints", function(on)
        waypointStep = nil
        if on then
            Guide:Refresh()
        else
            C_Map.ClearUserWaypoint()
        end
    end)
end

-- Checkpoints -------------------------------------------------------------------

--- The next checkpoint already reached in level, and the steps before it that can be skipped
--- (not kept, not done yet, not the turn-in of a finished quest).
local function Ahead()
    local level = UnitLevel("player")
    for _, c in ipairs(route.checkpoints or {}) do
        if c.step > state.step then
            if level < c.lvl then return nil end
            local skip = {}
            for index = state.step, c.step - 1 do
                local step = route.steps[index]
                local finished = step.k == "turnin" and C_QuestLog.IsOnQuest(step.q) and C_QuestLog.IsComplete(step.q)
                if not step.kp and not step.bg and not finished and not IsDone(step, index) then
                    table.insert(skip, index)
                end
            end
            if #skip == 0 then return nil end
            return c, skip
        end
    end
    return nil
end

--- Skip to the next checkpoint (see Ahead).
function Guide:SkipAhead()
    local c, skip = Ahead()
    if not c then return end
    for _, index in ipairs(skip) do
        state.marks[index] = "skip"
    end
    FG:Print(L.ahead, c.lvl, #skip)
    self:Resync()
end

local function RefreshAhead()
    local c, skip = Ahead()
    todoBox.ahead:SetShown(c ~= nil)
    if c then todoBox.ahead:SetText(string.format(L.ahead, c.lvl, #skip)) end
end

function Guide:Refresh()
    if not todoBox then return end
    local shown = FG:Get("guideShown")
    todoBox:SetShown(shown)
    if not shown then
        nextBox:Hide()
        if FG.Arrow then FG.Arrow:SetTarget(nil) end
        return
    end

    if not route or not route.steps[state.step] then
        todoBox.ahead:Hide()
        if FG.Arrow then FG.Arrow:SetTarget(nil) end
        local row = Row(todoBox, 1, true)
        row.icon:SetTexture(nil)
        row.check:Hide()
        row.text:SetText(route and L.done or L.noRoute)
        Layout(todoBox, 1, true, false)
        todoBox.info:SetText("")
        todoBox.footer:SetText("")
        nextBox:Hide()
        return
    end

    local group = CurrentGroup()
    local target
    local todo = {}
    local rows = 0
    local listed = {}
    local function AddRow(index, done)
        local step = route.steps[index]
        listed[index] = true
        if not done then table.insert(todo, step) end
        rows = rows + 1
        local row = Row(todoBox, rows, true)
        row.index = index
        row.icon:SetTexture((KINDS[step.k] or {}).icon)
        row.icon:SetDesaturated(done)
        row.check:SetShown(done)
        row.text:SetText(StepLine(step, true))
        row.text:SetAlpha(done and 0.45 or 1)
        if not done and not target then target = step end
    end
    for _, index in ipairs(group) do
        local step = route.steps[index]
        local done = IsDone(step, index)
        -- A turn-in of a quest not complete yet: its objectives first.
        if step.k == "turnin" and not done and C_QuestLog.IsOnQuest(step.q) and not C_QuestLog.IsComplete(step.q) then
            for i = 1, index - 1 do
                local o = route.steps[i]
                if o.q == step.q and o.k == "objective" and not listed[i] and not IsDone(o, i) then AddRow(i, false) end
            end
        end
        AddRow(index, done)
    end
    -- Objectives in progress done along the way (left behind, not finished): shown until done.
    for index = 1, state.step - 1 do
        local step = route.steps[index]
        if step.bg and not listed[index] and C_QuestLog.IsOnQuest(step.q) and not IsDone(step, index) then
            AddRow(index, false)
        end
    end
    Layout(todoBox, rows, true, true)
    RefreshAhead()
    if FG.TargetMacro then FG.TargetMacro:SetSteps(todo) end
    if FG.ItemButton then FG.ItemButton:SetSteps(todo) end
    if FG.MapPins then FG.MapPins:Refresh() end
    target = target or route.steps[state.step]

    local level = UnitLevel("player")
    local planned = route.steps[state.step].lvl
    local levelColor = level >= planned and "40ff40" or (level + 1 >= planned and "ffd100" or "ff4040")
    todoBox.info:SetText(
        string.format(
            "%d/%d  |cff%s%s %d|r |cff9d9d9d(%s %d)|r",
            state.step,
            #route.steps,
            levelColor,
            L.level,
            level,
            L.planned,
            planned
        ) .. (FG.Timing and FG.Timing:Short() or "")
    )

    local footer = target.m and string.format("%.1f, %.1f", target.x, target.y) or ""
    local d = Distance(target)
    if d then footer = footer .. string.format("  ·  %d %s", d, L.yards) end
    todoBox.footer:SetText(footer)
    SetWaypoint(target)
    if FG.Arrow then FG.Arrow:SetTarget(target) end

    local lines = FG:Get("nextLines")
    local start = group[#group] + 1
    local count = 0
    for i = 1, lines do
        local step = route.steps[start + i - 1]
        if not step then break end
        count = i
        local row = Row(nextBox, i, false)
        row.index = start + i - 1
        row.icon:SetTexture((KINDS[step.k] or {}).icon)
        row.icon:SetDesaturated(true)
        row.check:Hide()
        local plain = StepLine(step, false):gsub("|c%x%x%x%x%x%x%x%x", ""):gsub("|r", "")
        row.text:SetText("|cffb0b0b0" .. plain .. "|r")
    end
    Layout(nextBox, count, false, false)
    nextBox.info:SetText(count > 0 and string.format("%d", #route.steps - start + 1) or "")
    nextBox:SetShown(lines > 0 and count > 0)
end

-- Navigation -------------------------------------------------------------------

--- Skip forward over finished steps. `fromEvent` resumes after manual navigation.
--- Skip the following steps already done (they are done together, e.g. several quests
--- accepted at once).
local function SkipDone()
    -- Background objectives (done along the way) do not stop the guide: the tracker shows them.
    while route.steps[state.step] and (route.steps[state.step].bg or IsDone(route.steps[state.step], state.step)) do
        state.step = state.step + 1
    end
    arrivedDone = false
end

--- Move on when the shown step has just been done (not when we navigated to a done step).
function Guide:Advance()
    local step = route and route.steps[state.step]
    if step then
        local done = step.bg or IsDone(step, state.step)
        if not done then
            arrivedDone = false
        elseif not arrivedDone then
            state.step = state.step + 1
            SkipDone()
        end
    end
    self:Refresh()
end

--- Go to the first step not done yet (on login, route change, or the resync button).
function Guide:Resync()
    if route then
        state.step = 1
        SkipDone()
    end
    waypointStep = nil
    self:Refresh()
end

function Guide:Go(n)
    if not route then return end
    state.step = math.max(1, math.min(n, #route.steps + 1))
    local here = route.steps[state.step]
    arrivedDone = here ~= nil and (here.bg or IsDone(here, state.step))
    waypointStep = nil
    self:Refresh()
end

function Guide:RouteHasQuest(questID) return routeQuests[questID] or false end

local function UseRoute(name)
    route = FG.Routes and FG.Routes[name]
    state.route = route and name or nil
    wipe(acceptIndex)
    wipe(routeQuests)
    for i, s in ipairs(route and route.steps or {}) do
        if s.q then
            routeQuests[s.q] = true
            if s.k == "accept" and not acceptIndex[s.q] then acceptIndex[s.q] = i end
        end
    end
end

local function PickRoute(char)
    if state.route and FG.Routes and FG.Routes[state.route] then return state.route end
    local classID = select(3, UnitClass("player"))
    local exact, sameRace, sameFaction
    for name, r in pairs(FG.Routes or {}) do
        if r.race == char.raceID and r.class == classID then
            -- Several variants (e.g. "-solo"): the plain one has the shortest name.
            if not exact or #name < #exact or (#name == #exact and name < exact) then exact = name end
        elseif r.race == char.raceID then
            sameRace = sameRace or name
        elseif r.faction == char.faction then
            sameFaction = sameFaction or name
        end
    end
    return exact or sameRace or sameFaction
end

FG:On("PLAYER_LOGIN", function()
    local char = FactoruideDB.characters[FG:CharacterKey()]
    char.guide = char.guide or { step = 1 }
    state = char.guide
    state.shown = nil -- moved to settings in 0.2
    state.flightLearned = nil -- replaced by marks in 0.3
    state.marks = state.marks or {}
    state.skippedQuests = state.skippedQuests or {}
    UseRoute(PickRoute(char))
    CreateUI()
    Guide:Resync()
    C_Timer.NewTicker(1, function() Guide:Advance() end)
end)

-- Visits are done when the matching window opens while the step is on screen.
local function Mark(kind)
    if not route then return end
    for _, index in ipairs(CurrentGroup()) do
        if route.steps[index].k == kind then state.marks[index] = true end
    end
end
FG:On("TAXIMAP_OPENED", function() Mark("flight_master") end)
FG:On("TRAINER_SHOW", function() Mark(IsTradeskillTrainer() and "profession" or "train") end)
FG:On("HEARTHSTONE_BOUND", function() Mark("bind") end)

-- Dungeon steps: done once we entered the instance and came back out.
local function TrackInstance()
    if not route then return end
    local index = state.step
    local step = route.steps[index]
    if not step or step.k ~= "dungeon" then return end
    if IsInInstance() then
        state.marks[index] = "inside"
    elseif state.marks[index] == "inside" then
        state.marks[index] = true
    end
end
FG:On("PLAYER_ENTERING_WORLD", TrackInstance)
FG:On("ZONE_CHANGED_NEW_AREA", TrackInstance)

for _, event in ipairs({
    "QUEST_ACCEPTED",
    "QUEST_TURNED_IN",
    "QUEST_LOG_UPDATE",
    "QUEST_REMOVED",
    "PLAYER_LEVEL_UP",
    "PLAYER_CONTROL_LOST",
    "TAXIMAP_OPENED",
    "ZONE_CHANGED_NEW_AREA",
    "TRAINER_SHOW",
    "HEARTHSTONE_BOUND",
    "SKILL_LINES_CHANGED",
}) do
    FG:On(event, function() Guide:Advance() end)
end

--- Slash commands of the guide (called from Core.lua).
function Guide:Command(cmd, arg)
    if cmd == "guide" then
        FG:Set("guideShown", not FG:Get("guideShown"))
    elseif cmd == "next" then
        self:Go(state.step + 1)
    elseif cmd == "prev" then
        self:Go(state.step - 1)
    elseif cmd == "step" then
        self:Go(tonumber(arg) or state.step)
    elseif cmd == "skip" then
        self:SkipCommand(arg)
    elseif cmd == "lines" then
        FG:Set("nextLines", tonumber(arg) or FG:Get("nextLines"))
    elseif cmd == "time" then
        FG.Timing:Print()
    elseif cmd == "macro" then
        FG.TargetMacro:Pickup()
    elseif cmd == "waypoint" then
        FG:Set("waypoints", not FG:Get("waypoints"))
        FG:Print("waypoints %s", FG:Get("waypoints") and "on" or "off")
    elseif cmd == "config" or cmd == "options" then
        FG:OpenSettings()
    elseif cmd == "route" then
        if arg and FG.Routes and FG.Routes[arg] then
            UseRoute(arg)
            state.marks = {}
            self:Resync()
        else
            local names = {}
            for name in pairs(FG.Routes or {}) do
                table.insert(names, name)
            end
            table.sort(names)
            FG:Print("routes: %s (current: %s)", table.concat(names, ", "), tostring(state.route))
        end
    else
        return false
    end
    return true
end

--- Route followed by the character (nil when none).
function Guide:Route() return route end

--- Saved per-character guide state (also holds the tracker position).
function Guide:State() return state end

--- For each quest, its next step in the route from the current position: { index, k, n }.
function Guide:NextStepByQuest()
    local out = {}
    if not route or not state then return out end
    -- Background objectives left behind and not done yet: in progress along the way.
    for i = 1, math.min(state.step - 1, #route.steps) do
        local s = route.steps[i]
        if s.bg and not out[s.q] and C_QuestLog.IsOnQuest(s.q) and not IsDone(s, i) then
            out[s.q] = { index = i, k = s.k, n = s.n, bg = true }
        end
    end
    for i = state.step, #route.steps do
        local s = route.steps[i]
        if s.q and not out[s.q] and not IsDone(s, i) then out[s.q] = { index = i, k = s.k, n = s.n } end
    end
    -- Quests already behind us in the route still belong to it.
    for i = 1, math.min(state.step - 1, #route.steps) do
        local s = route.steps[i]
        if s.q and not out[s.q] then out[s.q] = { index = i, k = s.k, n = s.n } end
    end
    return out
end

--- Point the waypoint arrow at a quest's next step.
function Guide:PointToQuest(questID)
    local next = self:NextStepByQuest()[questID]
    local step = next and route.steps[next.index]
    if step and step.m and C_Map.CanSetUserWaypointOnMap(step.m) then
        C_Map.SetUserWaypoint(UiMapPoint.CreateFromCoordinates(step.m, step.x / 100, step.y / 100))
        C_SuperTrack.SetSuperTrackedUserWaypoint(true)
    end
end

--- Mark a step as already done by hand (its whole quest for quest steps), or undo it.
function Guide:ToggleSkip(index)
    local step = route and route.steps[index]
    if not step then return end
    if step.q then
        local name = QuestTitle(step) or step.n or tostring(step.q)
        if state.skippedQuests[step.q] then
            state.skippedQuests[step.q] = nil
            FG:Print(L.unskipped, name)
        else
            state.skippedQuests[step.q] = true
            FG:Print(L.skipped, name, step.q)
        end
    else
        state.marks[index] = state.marks[index] ~= "skip" and "skip" or nil
        FG:Print(L.stepSkipped)
    end
    -- Back to the first step not done (an undone quest may be behind us).
    self:Resync()
end

--- `/fg skip [questID]`: mark a quest (or the current step) as done, or undo it.
function Guide:SkipCommand(arg)
    local id = tonumber(arg)
    if not id then return self:ToggleSkip(state.step) end
    for i, s in ipairs(route and route.steps or {}) do
        if s.q == id then return self:ToggleSkip(i) end
    end
    state.skippedQuests[id] = not state.skippedQuests[id] or nil
    self:Resync()
end

--- Index of the step shown (for the tracker).
function Guide:CurrentStep() return state and state.step or 1 end

--- The "To do" window (to attach the item button and the talent hint to it).
function Guide:Box() return todoBox end
