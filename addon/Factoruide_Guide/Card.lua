-- The full guide: one card in the app's colors. Header (zone, step, level against the plan, time
-- against the plan), the character's XP against the guide's, the current tasks (the one the
-- compass points at in light), the talent to learn, the next steps or the quests to pick up; the
-- tools (previous, next, start over, settings, minimal mode) show when the mouse is over it.
-- Draws what Guide:Refresh describes (see Guide.lua).

local FG = Factoruide
local T = FG.T
local Card = {}
FG.Card = Card

local L = {
    now = T("NOW"),
    next = T("NEXT"),
    steps = T("Steps"),
    pickup = T("To pick up • %d"),
    stepOf = T("Step %d / %d"),
    level = T("Lvl %d"),
    planned = T("planned %d"),
    farm = T("Mobs on the way"),
    talent = T("Talent: %s"),
    learn = T("Learn"),
    ahead = T("Ahead of the level %d checkpoint"),
    skip = T("Skip %d steps"),
    more = T("+ %d steps…"),
    less = T("less"),
    levelMark = T("level %d planned here"),
    showQuests = T("Show the quests"),
    hideQuests = T("Hide the quests"),
    yards = T("%d yd"),
    minimal = T("Minimal mode"),
    noPickup = T("No quest to pick up now"),
}

local WIDTH, PAD = 300, 10
-- Next steps shown, and when unfolded.
local NEXT_LINES, NEXT_MORE = 4, 10
-- Left of the task's text (icon and its gap).
local TEXT_LEFT = 37

local Skin = FG.Skin
local frame, state
local expanded = false

local function SetColor(texture, name, alpha)
    local c = Skin.colors[name]
    texture:SetColorTexture(c[1], c[2], c[3], alpha or 1)
end

local function TextColor(fs, name)
    local c = Skin.colors[name]
    fs:SetTextColor(c[1], c[2], c[3])
end

--- " • 84 m" style chips, colored, after a task's line.
local function Chips(v, distance)
    local out = {}
    for _, chip in ipairs(v.chips or {}) do
        table.insert(out, Skin.Hex(chip.color) .. chip.text .. "|r")
    end
    if distance then table.insert(out, Skin.Hex("accent") .. string.format(L.yards, distance) .. "|r") end
    return table.concat(out, "  ")
end

local function Tooltip(owner, text)
    GameTooltip:SetOwner(owner, "ANCHOR_TOP")
    GameTooltip:SetText(text, 1, 1, 1, 1, true)
    GameTooltip:Show()
end

local function Tool(parent, texture, tooltip, onClick)
    local b = CreateFrame("Button", nil, parent)
    b:SetSize(18, 18)
    local bg = Skin:Fill(b, "BACKGROUND", "text", 0.06)
    bg:SetAllPoints()
    b:SetNormalTexture(texture)
    b:SetHighlightTexture(texture, "ADD")
    b:SetScript("OnClick", onClick)
    b:SetScript("OnEnter", function(self) Tooltip(self, tooltip) end)
    b:SetScript("OnLeave", function() GameTooltip:Hide() end)
    return FG:DragHandle(b)
end

--- A text button: `color` text on a light background.
local function TextButton(parent, size, color)
    local b = CreateFrame("Button", nil, parent)
    b.bg = Skin:Fill(b, "BACKGROUND", "text", 0.06)
    b.bg:SetAllPoints()
    b.label = Skin:Text(b, size, color)
    b.label:SetPoint("CENTER")
    b.SetLabel = function(self, text)
        self.label:SetText(text)
        self:SetSize((tonumber(self.label:GetStringWidth()) or 40) + 14, size + 8)
    end
    return FG:DragHandle(b)
end

-- Rows ------------------------------------------------------------------------------

local function TaskRow(i)
    local row = frame.rows[i]
    if row then return row end
    row = CreateFrame("Button", nil, frame)
    FG:DragHandle(row)
    row:RegisterForClicks("LeftButtonUp", "RightButtonUp")
    row:SetScript("OnClick", function(self, button)
        if not self.index then return end
        if button == "RightButton" then
            FG.Guide:ToggleSkip(self.index)
        else
            FG.Guide:PointToStep(self.index)
        end
    end)
    row:SetScript("OnEnter", function(self) FG.Guide:StepTooltip(self, self.index) end)
    row:SetScript("OnLeave", function() GameTooltip:Hide() end)
    row.light = Skin:Fill(row, "BACKGROUND", "blue", Skin.light and 0.22 or 0.16)
    row.light:SetAllPoints()
    row.edge = Skin:Fill(row, "BORDER", "accent", 1)
    row.edge:SetPoint("TOPLEFT")
    row.edge:SetPoint("BOTTOMLEFT")
    row.edge:SetWidth(2)
    row.icon = row:CreateTexture(nil, "ARTWORK")
    row.icon:SetSize(20, 20)
    row.icon:SetPoint("TOPLEFT", 10, -4)
    row.check = row:CreateTexture(nil, "OVERLAY")
    row.check:SetTexture("Interface\\RaidFrame\\ReadyCheck-Ready")
    row.check:SetSize(14, 14)
    row.check:SetPoint("CENTER", row.icon, "CENTER", 3, -3)
    row.count = Skin:Text(row, 11, "text2")
    row.count:SetPoint("TOPRIGHT", -PAD, -6)
    row.count:SetJustifyH("RIGHT")
    row.name = Skin:Text(row, 12, "text")
    row.name:SetPoint("TOPLEFT", TEXT_LEFT, -5)
    row.name:SetPoint("RIGHT", row.count, "LEFT", -6, 0)
    row.sub = Skin:Text(row, 10, "muted")
    row.sub:SetPoint("TOPLEFT", TEXT_LEFT, -21)
    row.sub:SetPoint("RIGHT", -PAD, 0)
    row.track = Skin:Fill(row, "ARTWORK", "text", 0.08)
    row.track:SetHeight(3)
    row.track:SetPoint("BOTTOMLEFT", TEXT_LEFT, 5)
    row.track:SetPoint("BOTTOMRIGHT", -PAD, 5)
    row.fill = Skin:Fill(row, "OVERLAY", "orange", 1)
    row.fill:SetPoint("TOPLEFT", row.track)
    row.fill:SetPoint("BOTTOMLEFT", row.track)
    -- Mobs on the way: segments between the label and the count.
    row.segs = {}
    frame.rows[i] = row
    return row
end

--- Segments of a "mobs on the way" row (a plain bar above 20).
local function Segments(row, have, need)
    local n = math.min(need, 20)
    local left, right = 110, WIDTH - PAD - 40
    local gap = 2
    local w = (right - left - gap * (n - 1)) / n
    local lit = have and math.floor(have / need * n + 0.5) or 0
    for i = 1, math.max(n, #row.segs) do
        local seg = row.segs[i]
        if i <= n then
            if not seg then
                seg = row:CreateTexture(nil, "ARTWORK")
                seg:SetHeight(4)
                row.segs[i] = seg
            end
            seg:ClearAllPoints()
            seg:SetPoint("LEFT", left + (i - 1) * (w + gap), 0)
            seg:SetWidth(w)
            SetColor(seg, "purple", i <= lit and 1 or 0.22)
            seg:Show()
        elseif seg then
            seg:Hide()
        end
    end
end

local function HideSegments(row)
    for _, seg in ipairs(row.segs) do
        seg:Hide()
    end
end

--- Draw a task row; returns its height.
local function DrawTask(row, v, view)
    row.index = v.index
    row:SetAlpha(v.done and 0.45 or 1)
    if v.kind == "farm" then
        row.light:Hide()
        row.edge:Hide()
        row.icon:Hide()
        row.check:Hide()
        row.sub:Hide()
        row.track:Hide()
        row.fill:Hide()
        row.name:SetFontObject(Skin.FontObject(10))
        TextColor(row.name, "purple")
        row.name:SetText(L.farm)
        row.name:ClearAllPoints()
        row.name:SetPoint("LEFT", PAD + 2, 0)
        TextColor(row.count, "purple")
        row.count:ClearAllPoints()
        row.count:SetPoint("RIGHT", -PAD, 0)
        row.count:SetText(v.count)
        Segments(row, v.have, v.need)
        return 18
    end
    HideSegments(row)
    row.light:SetShown(v.target == true)
    row.edge:SetShown(v.target == true)
    row.icon:Show()
    row.icon:SetTexture(v.icon)
    row.icon:SetDesaturated(v.done)
    row.check:SetShown(v.done == true)
    row.name:SetFontObject(Skin.FontObject(12))
    TextColor(row.name, "text")
    row.name:ClearAllPoints()
    row.name:SetPoint("TOPLEFT", TEXT_LEFT, -5)
    row.name:SetPoint("RIGHT", row.count, "LEFT", -6, 0)
    row.name:SetText(v.name)
    TextColor(row.count, v.done and "green" or "text2")
    row.count:ClearAllPoints()
    row.count:SetPoint("TOPRIGHT", -PAD, -6)
    row.count:SetText(v.have and v.need and string.format("%d/%d", v.have, v.need) or "")
    local chips = Chips(v, v.target and view.distance and math.floor(view.distance + 0.5) or nil)
    local sub = v.sub or ""
    if chips ~= "" then sub = sub ~= "" and (sub .. "  " .. chips) or chips end
    row.sub:SetText(sub)
    row.sub:SetShown(sub ~= "" and not v.done)
    local bar = (v.have and v.need and v.need > 0 and not v.done) and true or false
    row.track:SetShown(bar)
    row.fill:SetShown(bar)
    if bar then
        local share = math.max(0, math.min(1, v.have / v.need))
        row.fill:SetWidth(math.max(1, (WIDTH - TEXT_LEFT - PAD) * share))
        SetColor(row.fill, share >= 1 and "green" or "orange")
    end
    if v.done or (sub == "" and not bar) then return 28 end
    return bar and 42 or 36
end

local function NextRow(i)
    local row = frame.nextRows[i]
    if row then return row end
    row = CreateFrame("Button", nil, frame)
    FG:DragHandle(row)
    row:SetHeight(15)
    row:RegisterForClicks("LeftButtonUp", "RightButtonUp")
    row:SetScript("OnClick", function(self, button)
        if not self.index then return end
        if button == "RightButton" then
            FG.Guide:ToggleSkip(self.index)
        else
            FG.Guide:PointToStep(self.index)
        end
    end)
    row:SetScript("OnEnter", function(self)
        if self.index then FG.Guide:StepTooltip(self, self.index) end
    end)
    row:SetScript("OnLeave", function() GameTooltip:Hide() end)
    row.dot = row:CreateTexture(nil, "ARTWORK")
    row.dot:SetSize(5, 5)
    row.dot:SetPoint("LEFT", PAD + 2, 0)
    row.text = Skin:Text(row, 10, "text2")
    row.text:SetPoint("LEFT", PAD + 13, 0)
    row.text:SetPoint("RIGHT", -PAD, 0)
    row.line = Skin:Fill(row, "ARTWORK", "gold", 0.4)
    row.line:SetHeight(1)
    row.line:SetPoint("LEFT", row.text, "RIGHT", 6, 0)
    row.line:SetPoint("RIGHT", -PAD, 0)
    frame.nextRows[i] = row
    return row
end

local function DrawNext(row, v, isPickup)
    row:Show()
    if v.marker then
        row.index = nil
        row.dot:Hide()
        row.text:SetFontObject(Skin.FontObject(9))
        TextColor(row.text, "gold")
        row.text:ClearAllPoints()
        row.text:SetPoint("LEFT", PAD + 2, 0)
        row.text:SetText(string.format(L.levelMark, v.marker))
        row.text:SetWidth(tonumber(row.text:GetStringWidth()) or 80)
        row.line:Show()
        row:SetAlpha(1)
        row:SetHeight(13)
        return 13
    end
    row.index = v.index
    row.line:Hide()
    row.dot:Show()
    local color = Skin.Ink(v.color)
    local r, g, b = tonumber(color:sub(1, 2), 16), tonumber(color:sub(3, 4), 16), tonumber(color:sub(5, 6), 16)
    row.dot:SetColorTexture(r / 255, g / 255, b / 255, 1)
    row.text:SetFontObject(Skin.FontObject(10))
    TextColor(row.text, "text2")
    row.text:ClearAllPoints()
    row.text:SetWidth(0)
    row.text:SetPoint("LEFT", PAD + 13, 0)
    row.text:SetPoint("RIGHT", -PAD, 0)
    local text = v.text
    if v.who then text = text .. "  " .. Skin.Hex("dim") .. v.who .. "|r" end
    -- Done already (the player went back): greyed, checked.
    row:SetAlpha(v.done and 0.45 or 1)
    if v.done then text = "|TInterface\\RaidFrame\\ReadyCheck-Ready:10|t " .. text end
    if isPickup and v.distance then
        text = text
            .. "  "
            .. Skin.Hex(v.distance < 100 and "text" or "muted")
            .. string.format(L.yards, v.distance)
            .. "|r"
    end
    row.text:SetText(text)
    row:SetHeight(15)
    return 15
end

-- The card --------------------------------------------------------------------------

local function Header()
    frame.zone = Skin:Text(frame, 12, "text")
    frame.zone:SetPoint("TOPLEFT", PAD, -10)
    frame.zone:SetPoint("RIGHT", frame, "RIGHT", -110, 0)
    frame.stepLine = Skin:Text(frame, 10, "muted")
    frame.stepLine:SetPoint("TOPLEFT", PAD, -25)
    frame.level = Skin:Text(frame, 15, "text")
    frame.level:SetPoint("TOPRIGHT", -PAD, -8)
    frame.level:SetJustifyH("RIGHT")
    frame.planned = Skin:Text(frame, 10, "text2")
    frame.planned:SetPoint("TOPRIGHT", -PAD, -26)
    frame.planned:SetJustifyH("RIGHT")

    -- Tools, when the mouse is over the card (in place of the level).
    local tools = CreateFrame("Frame", nil, frame)
    tools:SetSize(130, 20)
    tools:SetPoint("TOPRIGHT", -PAD + 2, -10)
    local GL = FG.Guide.L
    local mode = TextButton(tools, 9, "text2")
    mode:SetLabel(L.minimal)
    mode:SetPoint("RIGHT")
    mode:SetScript("OnClick", function() FG.Guide:ToggleMode() end)
    local x = -(tonumber(mode:GetWidth()) or 70) - 4
    for _, t in ipairs({
        { "Interface\\Buttons\\UI-OptionsButton", GL.settings, function() FG:OpenSettings() end },
        { "Interface\\Buttons\\UI-RefreshButton", GL.resync, function() FG.Guide:AskReset() end },
        {
            "Interface\\Buttons\\UI-SpellbookIcon-NextPage-Up",
            GL.nextStep,
            function() FG.Guide:NextGroup() end,
        },
        {
            "Interface\\Buttons\\UI-SpellbookIcon-PrevPage-Up",
            GL.prevStep,
            function() FG.Guide:PrevGroup() end,
        },
    }) do
        local b = Tool(tools, t[1], t[2], t[3])
        b:SetPoint("RIGHT", x, 0)
        x = x - 20
    end
    tools:Hide()
    frame.tools = tools

    -- XP of the character (purple), the guide's before the current tasks (gold mark) and what
    -- they give (hollow gold box).
    frame.xp = Skin:Fill(frame, "ARTWORK", "xpTrack", 1)
    frame.xp:SetHeight(4)
    frame.xpFill = Skin:Fill(frame, "ARTWORK", "xp", 1)
    frame.xpFill:SetPoint("TOPLEFT", frame.xp)
    frame.xpFill:SetPoint("BOTTOMLEFT", frame.xp)
    frame.xpMark = Skin:Fill(frame, "OVERLAY", "gold", 1)
    frame.xpMark:SetSize(2, 8)
    frame.xpBar = { track = frame.xp, fill = frame.xpFill, mark = frame.xpMark, box = FG.XpPlan:Box(frame) }
end

local function Sections()
    frame.nowLabel = Skin:Text(frame, 9, "muted")
    frame.nowLabel:SetText(L.now)
    frame.nextLabel = Skin:Text(frame, 9, "muted")
    frame.nextLabel:SetText(L.next)
    frame.tabs = {}
    for _, key in ipairs({ "pickup", "next" }) do
        local b = TextButton(frame, 10, "muted")
        b:SetScript("OnClick", function() FG:Save("nextTab", key) end)
        frame.tabs[key] = b
    end
    frame.more = FG:DragHandle(CreateFrame("Button", nil, frame))
    frame.more:SetHeight(14)
    frame.more.label = Skin:Text(frame.more, 10, "accent")
    frame.more.label:SetPoint("LEFT")
    frame.more:SetScript("OnClick", function()
        expanded = not expanded
        FG.Guide:Refresh()
    end)
    frame.empty = Skin:Text(frame, 10, "muted")

    -- Ahead of a checkpoint: skip to it.
    local ahead = FG:DragHandle(CreateFrame("Button", nil, frame, "BackdropTemplate"))
    ahead:SetHeight(24)
    ahead:SetBackdrop({ bgFile = Skin.WHITE, edgeFile = Skin.WHITE, edgeSize = 1 })
    ahead:SetBackdropColor(1, 0.82, 0, 0.08)
    ahead:SetBackdropBorderColor(1, 0.82, 0, 0.3)
    ahead.text = Skin:Text(ahead, 10, "gold")
    ahead.text:SetPoint("LEFT", 8, 0)
    ahead.go = Skin:Text(ahead, 10, "gold")
    ahead.go:SetPoint("RIGHT", -8, 0)
    ahead:SetScript("OnClick", function() FG.Guide:SkipAhead() end)
    ahead:SetScript("OnEnter", function(self) Tooltip(self, FG.Guide.L.aheadTip) end)
    ahead:SetScript("OnLeave", function() GameTooltip:Hide() end)
    frame.ahead = ahead

    -- The talent to learn, in one click.
    local talent = CreateFrame("Frame", nil, frame, "BackdropTemplate")
    talent:SetHeight(32)
    talent:SetBackdrop({ bgFile = Skin.WHITE, edgeFile = Skin.WHITE, edgeSize = 1 })
    local blue, accent = Skin.colors.blue, Skin.colors.accent
    talent:SetBackdropColor(blue[1], blue[2], blue[3], 0.14)
    talent:SetBackdropBorderColor(accent[1], accent[2], accent[3], 0.3)
    talent.icon = talent:CreateTexture(nil, "ARTWORK")
    talent.icon:SetSize(22, 22)
    talent.icon:SetPoint("LEFT", 6, 0)
    talent.icon:SetTexCoord(0.08, 0.92, 0.08, 0.92)
    talent.button = FG:DragHandle(CreateFrame("Button", nil, talent))
    talent.button.bg = talent.button:CreateTexture(nil, "BACKGROUND")
    talent.button.bg:SetAllPoints()
    Skin:AccentLine(talent.button.bg)
    talent.button.label = Skin:Text(talent.button, 10, "text")
    talent.button.label:SetPoint("CENTER")
    talent.button.label:SetText(L.learn)
    talent.button:SetSize((tonumber(talent.button.label:GetStringWidth()) or 50) + 16, 18)
    talent.button:SetPoint("RIGHT", -6, 0)
    talent.button:SetScript("OnClick", function()
        if FG.Talents then FG.Talents:LearnNext() end
    end)
    talent.text = Skin:Text(talent, 10, "text")
    talent.text:SetPoint("LEFT", talent.icon, "RIGHT", 8, 0)
    talent.text:SetPoint("RIGHT", talent.button, "LEFT", -6, 0)
    frame.talent = talent

    frame.footLine = Skin:Fill(frame, "ARTWORK", "line", 1)
    frame.footLine:SetHeight(1)
    frame.coords = Skin:Text(frame, 9, "dim")
    -- Shows or hides the quest panel.
    frame.quests = TextButton(frame, 9, "text2")
    frame.quests:SetScript("OnClick", function()
        if FG.Tracker then FG.Tracker:Toggle() end
    end)
    frame.message = Skin:Text(frame, 11, "text2")
    frame.message:SetWordWrap(true)
    frame.message:SetPoint("TOPLEFT", PAD, -46)
    frame.message:SetPoint("RIGHT", -PAD, 0)
end

function Card:Create(guideState)
    state = guideState
    frame = CreateFrame("Frame", "FactoruideTodo", FG.UIRoot, "BackdropTemplate")
    frame:SetWidth(WIDTH)
    frame:SetClampedToScreen(true)
    frame:SetPoint(unpack(state.point or { "TOPRIGHT", UIParent, "TOPRIGHT", -30, -190 }))
    FG:Draggable(frame, function(self) state.point = FG.PinTopLeft(self) or state.point end)
    frame.rows, frame.nextRows = {}, {}
    Header()
    Sections()
    Skin:Style(frame, "window", { frame.zone, frame.stepLine, frame.level, frame.planned })
    -- The tools show while the mouse is over the card.
    frame:SetScript("OnUpdate", function(self, elapsed)
        self.wait = (self.wait or 0) - elapsed
        if self.wait > 0 then return end
        self.wait = 0.1
        local over = self:IsMouseOver() and true or false
        if over == self.over then return end
        self.over = over
        self.tools:SetShown(over)
        self.level:SetShown(not over)
        self.planned:SetShown(not over)
    end)
    frame:SetScale(FG:Get("scale"))
    FG:OnSetting("scale", function(v) frame:SetScale(v) end)
    FG:Zoomable(frame)
    frame:Hide()
end

function Card:Frame() return frame end

function Card:Hide()
    if frame and FG.CanLayout(frame) then frame:Hide() end
end

--- Where the quest item button goes: left of the card, by the tasks.
function Card:ItemAnchor(button) button:SetPoint("TOPRIGHT", frame, "TOPLEFT", -6, -46) end

local function HideAll(list, from)
    for i = from, #list do
        list[i]:Hide()
    end
end

function Card:Render(view)
    if not frame then return end
    local resize = FG.CanLayout(frame)
    if view.hidden then
        if resize then frame:Hide() end
        return
    end
    if resize then frame:Show() end
    if FG.Compass then FG.Compass:Free() end

    local sections = not view.message
    for _, f in ipairs({
        frame.stepLine,
        frame.level,
        frame.planned,
        frame.xp,
        frame.xpFill,
        frame.xpMark,
        frame.nowLabel,
        frame.nextLabel,
        frame.footLine,
        frame.coords,
        frame.more,
    }) do
        f:SetShown(sections)
    end
    frame.message:SetShown(not sections)
    if not sections then
        frame.zone:SetText("Factoruide")
        frame.message:SetText(view.message)
        for _, list in ipairs({ frame.rows, frame.nextRows }) do
            HideAll(list, 1)
        end
        for _, f in ipairs({ frame.ahead, frame.talent, frame.tabs.next, frame.tabs.pickup, frame.empty, frame.quests }) do
            f:Hide()
        end
        if resize then frame:SetHeight(56 + (tonumber(frame.message:GetStringHeight()) or 24)) end
        return
    end

    -- Header.
    frame.zone:SetText(view.zone or "")
    frame.stepLine:SetText(string.format(L.stepOf, view.step, view.total))
    local levelColor = view.level >= view.planned and "green" or (view.level + 1 >= view.planned and "gold" or "red")
    frame.level:SetText(Skin.Hex(levelColor) .. string.format(L.level, view.level) .. "|r")
    frame.planned:SetText(
        string.format(L.planned, view.planned) .. (view.timing ~= "" and (" • " .. view.timing) or "")
    )
    frame.level:SetShown(not frame.over)
    frame.planned:SetShown(not frame.over)

    local y = -42
    -- XP against the guide's.
    frame.xp:ClearAllPoints()
    frame.xp:SetPoint("TOPLEFT", PAD, y)
    frame.xp:SetPoint("TOPRIGHT", -PAD, y)
    if FG.XpPlan then FG.XpPlan:Draw(frame.xpBar, WIDTH - 2 * PAD) end
    y = y - 12

    frame.ahead:SetShown(view.ahead ~= nil)
    if view.ahead then
        frame.ahead:ClearAllPoints()
        frame.ahead:SetPoint("TOPLEFT", PAD - 2, y)
        frame.ahead:SetPoint("TOPRIGHT", -PAD + 2, y)
        frame.ahead.text:SetText(string.format(L.ahead, view.ahead.lvl))
        frame.ahead.go:SetText(string.format(L.skip, view.ahead.steps))
        y = y - 30
    end

    frame.nowLabel:ClearAllPoints()
    frame.nowLabel:SetPoint("TOPLEFT", PAD, y)
    y = y - 14
    for i, v in ipairs(view.tasks) do
        local row = TaskRow(i)
        local h = DrawTask(row, v, view)
        row:ClearAllPoints()
        row:SetPoint("TOPLEFT", 0, y)
        row:SetPoint("TOPRIGHT", 0, y)
        row:SetHeight(h)
        row:Show()
        y = y - h
    end
    HideAll(frame.rows, #view.tasks + 1)

    local t = view.talent
    frame.talent:SetShown(t ~= nil)
    if t then
        frame.talent:ClearAllPoints()
        frame.talent:SetPoint("TOPLEFT", PAD - 2, y - 4)
        frame.talent:SetPoint("TOPRIGHT", -PAD + 2, y - 4)
        frame.talent.icon:SetTexture(t.icon)
        frame.talent.text:SetText(
            string.format(L.talent, t.name) .. "  " .. Skin.Hex("muted") .. (t.rank + 1) .. "/" .. t.maxRank .. "|r"
        )
        y = y - 40
    else
        y = y - 4
    end

    -- Next steps or quests to pick up.
    frame.nextLabel:ClearAllPoints()
    frame.nextLabel:SetPoint("TOPLEFT", PAD, y - 3)
    local tab = view.nextTab
    frame.tabs.next:SetLabel(L.steps)
    frame.tabs.pickup:SetLabel(string.format(L.pickup, #view.pickups))
    frame.tabs.pickup:ClearAllPoints()
    frame.tabs.pickup:SetPoint("TOPRIGHT", -PAD, y)
    frame.tabs.next:ClearAllPoints()
    frame.tabs.next:SetPoint("RIGHT", frame.tabs.pickup, "LEFT", -2, 0)
    for key, b in pairs(frame.tabs) do
        b:Show()
        b.bg:SetShown(key == tab)
        b.bg:SetColorTexture(Skin.colors.line[1], Skin.colors.line[2], Skin.colors.line[3], 1)
        TextColor(b.label, key == tab and "text" or "muted")
    end
    y = y - 20
    local list = tab == "pickup" and view.pickups or view.next
    local limit = expanded and NEXT_MORE or NEXT_LINES
    local shown, n = 0, 0
    for _, v in ipairs(list) do
        if shown >= limit then break end
        n = n + 1
        local row = NextRow(n)
        local h = DrawNext(row, v, tab == "pickup")
        row:ClearAllPoints()
        row:SetPoint("TOPLEFT", 0, y)
        row:SetPoint("TOPRIGHT", 0, y)
        y = y - h
        if not v.marker then shown = shown + 1 end
    end
    HideAll(frame.nextRows, n + 1)
    frame.empty:SetShown(#list == 0)
    if #list == 0 then
        frame.empty:ClearAllPoints()
        frame.empty:SetPoint("TOPLEFT", PAD + 2, y)
        frame.empty:SetText(tab == "pickup" and L.noPickup or "")
        y = y - 15
    end
    local steps = 0
    for _, v in ipairs(list) do
        if not v.marker then steps = steps + 1 end
    end
    local hidden = steps - shown
    frame.more:SetShown(hidden > 0 or expanded)
    if hidden > 0 or expanded then
        frame.more:ClearAllPoints()
        frame.more:SetPoint("TOPLEFT", PAD + 2, y - 2)
        frame.more:SetPoint("TOPRIGHT", -PAD, y - 2)
        frame.more.label:SetText(hidden > 0 and string.format(L.more, hidden) or L.less)
        y = y - 18
    end

    -- Footer: where the target is (and which of its places), the quest panel's button.
    y = y - 4
    frame.footLine:ClearAllPoints()
    frame.footLine:SetPoint("TOPLEFT", 1, y)
    frame.footLine:SetPoint("TOPRIGHT", -1, y)
    frame.coords:ClearAllPoints()
    frame.coords:SetPoint("TOPLEFT", PAD, y - 5)
    frame.coords:SetText((view.coords or "") .. (view.spot and ("  •  " .. view.spot) or ""))
    frame.quests:SetShown(FG:Get("trackerShown") and true or false)
    frame.quests:SetLabel(FG:Get("questPanel") and L.hideQuests or L.showQuests)
    frame.quests:ClearAllPoints()
    frame.quests:SetPoint("TOPRIGHT", -PAD + 4, y - 3)
    y = y - 22

    if resize then
        frame:SetHeight(-y + 2)
        -- A position saved anchored by the bottom: from now on the card grows downwards.
        state.point = FG.PinTopIfNeeded(frame) or state.point
    end
end
