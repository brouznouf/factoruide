-- The minimal guide: a strip at the top of the screen with the task the compass points at (its
-- progress and distance), the other current tasks as labels under it and the character's XP
-- against the guide's. The next steps and the tools unfold when the mouse is over it. The compass
-- sits on top of it; the quest item and the talent to learn are two buttons near the action bars.
-- Draws what Guide:Refresh describes (see Guide.lua).

local FG = Factoruide
local T = FG.T
local Mini = {}
FG.Mini = Mini

local L = {
    stepOf = T("step %d/%d"),
    done = T("done"),
    farm = T("mobs on the way"),
    next = T("NEXT"),
    pickup = T("TO PICK UP"),
    pickups = T("To pick up • %d"),
    steps = T("Steps"),
    levelMark = T("level %d planned here"),
    more = T("+%d more"),
    yards = T("%d yd"),
    full = T("Full mode"),
    showQuests = T("Show the quests"),
    hideQuests = T("Hide the quests"),
    questItem = T("quest item"),
    talent = T("talent to learn"),
    learn = T("Learn"),
    ahead = T("Ahead of the level %d checkpoint"),
    skip = T("Skip %d steps"),
    noPickup = T("No quest to pick up now"),
}

local STRIP_WIDTH, DROP_WIDTH = 360, 380
local MAX_TAGS = 3
local DROP_LINES = 6

local Skin = FG.Skin
local strip, tags, xpBar, drop, buttons, aheadBar, state

local function TextColor(fs, name)
    local c = Skin.colors[name]
    fs:SetTextColor(c[1], c[2], c[3])
end

local function Tooltip(owner, text)
    GameTooltip:SetOwner(owner, "ANCHOR_BOTTOM")
    GameTooltip:SetText(text, 1, 1, 1, 1, true)
    GameTooltip:Show()
end

local function Movable(f, key, default)
    f:SetClampedToScreen(true)
    f:SetPoint(unpack(state[key] or default))
    FG:Draggable(f, function(self)
        local p, _, rp, x, y = self:GetPoint()
        state[key] = { p, "UIParent", rp, x, y }
    end)
end

-- The strip ------------------------------------------------------------------------

local function CreateStrip()
    strip = CreateFrame("Frame", "FactoruideMini", FG.UIRoot, "BackdropTemplate")
    strip:SetSize(STRIP_WIDTH, 56)
    Movable(strip, "miniPoint", { "TOP", UIParent, "TOP", 0, -46 })
    strip.dot = strip:CreateTexture(nil, "ARTWORK")
    strip.dot:SetSize(6, 6)
    strip.dot:SetPoint("TOPLEFT", 12, -11)
    strip.verb = Skin:Text(strip, 9, "muted")
    strip.verb:SetPoint("LEFT", strip.dot, "RIGHT", 5, 0)
    -- Right of the separator: the distance and the zone, cut to its width.
    strip.dist = Skin:Text(strip, 16, "text")
    strip.dist:SetPoint("TOPRIGHT", -6, -12)
    strip.dist:SetWidth(72)
    strip.dist:SetJustifyH("CENTER")
    strip.zone = Skin:Text(strip, 9, "muted")
    strip.zone:SetPoint("TOPRIGHT", -6, -33)
    strip.zone:SetWidth(72)
    strip.zone:SetJustifyH("CENTER")
    strip.sep = Skin:Fill(strip, "ARTWORK", "line", 1)
    strip.sep:SetWidth(1)
    strip.sep:SetPoint("TOPRIGHT", -84, -8)
    strip.sep:SetPoint("BOTTOMRIGHT", -84, 8)
    strip.count = Skin:Text(strip, 12, "text2")
    strip.count:SetPoint("TOPRIGHT", strip.sep, "TOPLEFT", -10, -9)
    strip.count:SetJustifyH("RIGHT")
    strip.name = Skin:Text(strip, 15, "text")
    strip.name:SetPoint("TOPLEFT", 12, -20)
    strip.name:SetPoint("RIGHT", strip.count, "LEFT", -8, 0)
    strip.sub = Skin:Text(strip, 10, "muted")
    strip.sub:SetPoint("TOPLEFT", 12, -38)
    strip.sub:SetPoint("RIGHT", strip.sep, "LEFT", -10, 0)
    strip.track = Skin:Fill(strip, "ARTWORK", "text", 0.08)
    strip.track:SetHeight(3)
    strip.track:SetPoint("BOTTOMLEFT", 12, 5)
    strip.track:SetPoint("BOTTOMRIGHT", strip.sep, "BOTTOMLEFT", -10, -3)
    strip.fill = Skin:Fill(strip, "OVERLAY", "orange", 1)
    strip.fill:SetPoint("TOPLEFT", strip.track)
    strip.fill:SetPoint("BOTTOMLEFT", strip.track)
    -- Clicks on the strip: left points at the task, right marks it done.
    strip:SetScript("OnMouseUp", function(self, button)
        if not self.index or FG.JustDragged(self) then return end
        if button == "RightButton" then
            FG.Guide:ToggleSkip(self.index)
        else
            FG.Guide:PointToStep(self.index)
        end
    end)
    Skin:Style(strip, "window", { strip.verb, strip.dist, strip.zone, strip.count, strip.name, strip.sub })
end

-- The other tasks: labels under the strip.
local function Tag(i)
    local tag = tags.list[i]
    if tag then return tag end
    tag = FG:DragHandle(CreateFrame("Button", nil, tags, "BackdropTemplate"))
    tag:SetHeight(18)
    tag:RegisterForClicks("LeftButtonUp", "RightButtonUp")
    tag:SetScript("OnClick", function(self, button)
        if not self.index then return end
        if button == "RightButton" then
            FG.Guide:ToggleSkip(self.index)
        else
            FG.Guide:PointToStep(self.index)
        end
    end)
    tag:SetScript("OnEnter", function(self)
        if self.index then FG.Guide:StepTooltip(self, self.index) end
    end)
    tag:SetScript("OnLeave", function() GameTooltip:Hide() end)
    tag.icon = tag:CreateTexture(nil, "ARTWORK")
    tag.icon:SetSize(14, 14)
    tag.icon:SetPoint("LEFT", 3, 0)
    tag.icon:SetTexCoord(0.08, 0.92, 0.08, 0.92)
    tag.text = Skin:Text(tag, 10, "text")
    tag.text:SetPoint("LEFT", tag.icon, "RIGHT", 5, 0)
    Skin:Style(tag, "panel", { tag.text })
    tags.list[i] = tag
    return tag
end

local function CreateTags()
    tags = CreateFrame("Frame", nil, strip)
    tags:SetSize(DROP_WIDTH + 100, 18)
    tags:SetPoint("TOP", strip, "BOTTOM", 0, -4)
    tags.list = {}
    xpBar = CreateFrame("Frame", nil, strip)
    xpBar:SetSize(300, 3)
    xpBar.bg = Skin:Fill(xpBar, "BACKGROUND", "xpTrack", 0.7)
    xpBar.bg:SetAllPoints()
    xpBar.fill = Skin:Fill(xpBar, "ARTWORK", "xp", 1)
    xpBar.fill:SetPoint("TOPLEFT")
    xpBar.fill:SetPoint("BOTTOMLEFT")
    xpBar.mark = Skin:Fill(xpBar, "OVERLAY", "gold", 1)
    xpBar.mark:SetSize(2, 7)
    xpBar.track = xpBar
    xpBar.box = FG.XpPlan:Box(xpBar)
end

-- The next steps and the tools, unfolded when the mouse is over the guide -------------------

local function DropRow(i)
    local row = drop.rows[i]
    if row then return row end
    row = FG:DragHandle(CreateFrame("Button", nil, drop))
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
    row.dot:SetPoint("LEFT", 12, 0)
    row.text = Skin:Text(row, 10, "text2")
    row.text:SetPoint("LEFT", 23, 0)
    row.text:SetPoint("RIGHT", -12, 0)
    drop.rows[i] = row
    return row
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

local function TextButton(parent, text, onClick)
    local b = CreateFrame("Button", nil, parent)
    b.bg = Skin:Fill(b, "BACKGROUND", "text", 0.06)
    b.bg:SetAllPoints()
    b.label = Skin:Text(b, 9, "text2")
    b.label:SetPoint("CENTER")
    b.label:SetText(text)
    b:SetSize((tonumber(b.label:GetStringWidth()) or 50) + 14, 18)
    b:SetScript("OnClick", onClick)
    return FG:DragHandle(b)
end

local function CreateDrop()
    drop = CreateFrame("Frame", "FactoruideMiniNext", strip, "BackdropTemplate")
    drop:SetWidth(DROP_WIDTH)
    -- Its background moves the strip too.
    drop:EnableMouse(true)
    FG:DragHandle(drop)
    drop.rows = {}
    drop.title = Skin:Text(drop, 9, "muted")
    drop.title:SetPoint("TOPLEFT", 12, -7)
    drop.tab = FG:DragHandle(CreateFrame("Button", nil, drop))
    drop.tab:SetSize(120, 14)
    drop.tab:SetPoint("TOPRIGHT", -10, -4)
    drop.tab.label = Skin:Text(drop.tab, 9, "accent")
    drop.tab.label:SetPoint("RIGHT")
    drop.tab.label:SetJustifyH("RIGHT")
    drop.tab:SetScript(
        "OnClick",
        function() FG:Save("nextTab", FG:Get("nextTab") == "pickup" and "next" or "pickup") end
    )
    drop.empty = Skin:Text(drop, 10, "muted")
    drop.line = Skin:Fill(drop, "ARTWORK", "line", 1)
    drop.line:SetHeight(1)
    drop.tools = CreateFrame("Frame", nil, drop)
    drop.tools:SetSize(DROP_WIDTH - 20, 18)
    local GL = FG.Guide.L
    local x = 0
    for _, t in ipairs({
        {
            "Interface\\Buttons\\UI-SpellbookIcon-PrevPage-Up",
            GL.prevStep,
            function() FG.Guide:PrevGroup() end,
        },
        {
            "Interface\\Buttons\\UI-SpellbookIcon-NextPage-Up",
            GL.nextStep,
            function() FG.Guide:NextGroup() end,
        },
        { "Interface\\Buttons\\UI-RefreshButton", GL.resync, function() FG.Guide:AskReset() end },
        { "Interface\\Buttons\\UI-OptionsButton", GL.settings, function() FG:OpenSettings() end },
    }) do
        local b = Tool(drop.tools, t[1], t[2], t[3])
        b:SetPoint("LEFT", x, 0)
        x = x + 20
    end
    local full = TextButton(drop.tools, L.full, function() FG.Guide:ToggleMode() end)
    full:SetPoint("RIGHT")
    drop.quests = TextButton(drop.tools, L.hideQuests, function()
        if FG.Tracker then FG.Tracker:Toggle() end
    end)
    drop.quests:SetPoint("RIGHT", full, "LEFT", -4, 0)
    Skin:Style(drop, "panel", { drop.title, drop.tab.label })
    drop:Hide()
end

-- Ahead of a checkpoint: a line over the compass.
local function CreateAhead()
    aheadBar = FG:DragHandle(CreateFrame("Button", nil, strip, "BackdropTemplate"))
    aheadBar:SetHeight(20)
    aheadBar:SetBackdrop({ bgFile = Skin.WHITE, edgeFile = Skin.WHITE, edgeSize = 1 })
    aheadBar:SetBackdropColor(Skin.colors.bg[1], Skin.colors.bg[2], Skin.colors.bg[3], 0.9)
    aheadBar:SetBackdropBorderColor(1, 0.82, 0, 0.35)
    aheadBar.text = Skin:Text(aheadBar, 10, "gold")
    aheadBar.text:SetPoint("CENTER")
    aheadBar:SetScript("OnClick", function() FG.Guide:SkipAhead() end)
    aheadBar:SetScript("OnEnter", function(self) Tooltip(self, FG.Guide.L.aheadTip) end)
    aheadBar:SetScript("OnLeave", function() GameTooltip:Hide() end)
    aheadBar:Hide()
end

-- The quest item and the talent: two buttons near the action bars --------------------------

local function CreateButtons()
    buttons = CreateFrame("Frame", "FactoruideButtons", FG.UIRoot)
    buttons:SetSize(520, 44)
    Movable(buttons, "buttonsPoint", { "BOTTOM", UIParent, "BOTTOM", 0, 150 })
    buttons.slot = CreateFrame("Frame", nil, buttons)
    buttons.slot:SetSize(36, 36)
    buttons.slot:SetPoint("LEFT", 4, 0)
    buttons.itemLabel = CreateFrame("Frame", nil, buttons, "BackdropTemplate")
    buttons.itemLabel:SetHeight(30)
    buttons.itemLabel:SetPoint("LEFT", buttons.slot, "RIGHT", 8, 0)
    buttons.itemLabel.name = Skin:Text(buttons.itemLabel, 10, "text")
    buttons.itemLabel.name:SetPoint("TOPLEFT", 8, -4)
    buttons.itemLabel.kind = Skin:Text(buttons.itemLabel, 9, "muted")
    buttons.itemLabel.kind:SetPoint("TOPLEFT", 8, -16)
    buttons.itemLabel.kind:SetText(L.questItem)
    Skin:Style(buttons.itemLabel, "panel", { buttons.itemLabel.name, buttons.itemLabel.kind })

    local talent = FG:DragHandle(CreateFrame("Button", nil, buttons))
    talent:SetSize(36, 36)
    talent:SetPoint("LEFT", 270, 0)
    talent.icon = talent:CreateTexture(nil, "ARTWORK")
    talent.icon:SetAllPoints()
    talent.icon:SetTexCoord(0.08, 0.92, 0.08, 0.92)
    local ring = Skin:Fill(talent, "BACKGROUND", "accent", 1)
    ring:SetPoint("TOPLEFT", -2, 2)
    ring:SetPoint("BOTTOMRIGHT", 2, -2)
    talent:SetScript("OnClick", function()
        if FG.Talents then FG.Talents:LearnNext() end
    end)
    talent.label = CreateFrame("Frame", nil, talent, "BackdropTemplate")
    talent.label:SetHeight(30)
    talent.label:SetPoint("LEFT", talent, "RIGHT", 8, 0)
    talent.label.name = Skin:Text(talent.label, 10, "text")
    talent.label.name:SetPoint("TOPLEFT", 8, -4)
    talent.label.kind = Skin:Text(talent.label, 9, "muted")
    talent.label.kind:SetPoint("TOPLEFT", 8, -16)
    talent.label.kind:SetText(L.talent)
    Skin:Style(talent.label, "panel", { talent.label.name, talent.label.kind })
    talent.learn = FG:DragHandle(CreateFrame("Button", nil, talent))
    talent.learn.bg = talent.learn:CreateTexture(nil, "BACKGROUND")
    talent.learn.bg:SetAllPoints()
    Skin:AccentLine(talent.learn.bg)
    talent.learn.label = Skin:Text(talent.learn, 10, "text")
    talent.learn.label:SetPoint("CENTER")
    talent.learn.label:SetText(L.learn)
    talent.learn:SetSize((tonumber(talent.learn.label:GetStringWidth()) or 50) + 16, 18)
    talent.learn:SetPoint("LEFT", talent.label, "RIGHT", 6, 0)
    talent.learn:SetScript("OnClick", function()
        if FG.Talents then FG.Talents:LearnNext() end
    end)
    buttons.talent = talent
    buttons:Hide()
end

function Mini:Create(guideState)
    state = guideState
    CreateStrip()
    CreateTags()
    CreateDrop()
    CreateAhead()
    CreateButtons()
    -- The next steps unfold while the mouse is over the guide (a moment more once it left).
    strip:SetScript("OnUpdate", function(self, elapsed)
        self.wait = (self.wait or 0) - elapsed
        if self.wait > 0 then return end
        self.wait = 0.1
        local over = self:IsMouseOver() or tags:IsMouseOver() or (drop:IsShown() and drop:IsMouseOver())
        if over then
            self.left = nil
        elseif drop:IsShown() then
            self.left = (self.left or 0) + 0.1
        end
        local show = over or (self.left ~= nil and self.left < 0.4)
        if show and not drop:IsShown() then
            drop:Show()
        elseif not show and drop:IsShown() then
            drop:Hide()
        end
    end)
    for _, f in ipairs({ strip, buttons }) do
        f:SetScale(FG:Get("scale"))
        FG:Zoomable(f)
    end
    FG:OnSetting("scale", function(v)
        strip:SetScale(v)
        buttons:SetScale(v)
    end)
    strip:Hide()
end

function Mini:Frame() return strip end

function Mini:Hide()
    if not strip then return end
    strip:Hide()
    if FG.CanLayout(buttons) then buttons:Hide() end
end

--- Where the quest item button goes: near the action bars, with its name.
function Mini:ItemAnchor(button) button:SetPoint("CENTER", buttons.slot) end

local function Count(v) return v.have and v.need and string.format("%d/%d", v.have, v.need) or "" end

--- The tags under the strip: mobs on the way, then the other tasks (the first ones, then "+n").
local function DrawTags(view, main)
    local list = {}
    for _, v in ipairs(view.tasks) do
        if not v.done and v ~= main then table.insert(list, v) end
    end
    local shown = math.min(#list, MAX_TAGS)
    local n, total = 0, 0
    for i = 1, shown + ((#list > MAX_TAGS) and 1 or 0) do
        local v = list[i]
        n = n + 1
        local tag = Tag(n)
        tag:Show()
        if i > shown then
            tag.index = nil
            tag.icon:Hide()
            tag.text:SetText(string.format(L.more, #list - MAX_TAGS))
            TextColor(tag.text, "accent")
            tag.text:ClearAllPoints()
            tag.text:SetPoint("LEFT", 8, 0)
        elseif v.kind == "farm" then
            tag.index = v.index
            tag.icon:Show()
            tag.icon:SetTexture(v.icon)
            TextColor(tag.text, "purple")
            tag.text:ClearAllPoints()
            tag.text:SetPoint("LEFT", tag.icon, "RIGHT", 5, 0)
            tag.text:SetText(L.farm .. "  " .. v.count)
        else
            tag.index = v.index
            tag.icon:Show()
            tag.icon:SetTexture(v.icon)
            TextColor(tag.text, "text")
            tag.text:ClearAllPoints()
            tag.text:SetPoint("LEFT", tag.icon, "RIGHT", 5, 0)
            local text = v.name .. "  " .. Skin.Hex("text2") .. Count(v) .. "|r"
            for _, chip in ipairs(v.chips or {}) do
                text = text .. "  " .. Skin.Hex(chip.color) .. chip.text .. "|r"
            end
            tag.text:SetText(text)
        end
        local w = math.min(260, (tonumber(tag.text:GetStringWidth()) or 80) + (tag.icon:IsShown() and 30 or 16))
        tag:SetWidth(w)
        total = total + w + 6
    end
    for i = n + 1, #tags.list do
        tags.list[i]:Hide()
    end
    -- Centered under the strip.
    local x = -(total - 6) / 2
    for i = 1, n do
        local tag = tags.list[i]
        tag:ClearAllPoints()
        tag:SetPoint("LEFT", tags, "CENTER", x, 0)
        x = x + (tonumber(tag:GetWidth()) or 0) + 6
    end
    return n > 0
end

local function DrawXp(y)
    xpBar:ClearAllPoints()
    xpBar:SetPoint("TOP", strip, "BOTTOM", 0, y)
    if FG.XpPlan then FG.XpPlan:Draw(xpBar, 300) end
end

local function DrawDrop(view, y)
    drop:ClearAllPoints()
    drop:SetPoint("TOP", strip, "BOTTOM", 0, y)
    local tab = view.nextTab
    drop.title:SetText(tab == "pickup" and L.pickup or L.next)
    drop.tab.label:SetText(tab == "pickup" and L.steps or string.format(L.pickups, #view.pickups))
    local list = tab == "pickup" and view.pickups or view.next
    local dy, n, shown = -22, 0, 0
    for _, v in ipairs(list) do
        if shown >= DROP_LINES then break end
        n = n + 1
        local row = DropRow(n)
        row:Show()
        row:ClearAllPoints()
        row:SetPoint("TOPLEFT", 0, dy)
        row:SetPoint("TOPRIGHT", 0, dy)
        if v.marker then
            row.index = nil
            row.dot:Hide()
            row:SetAlpha(1)
            TextColor(row.text, "gold")
            row.text:SetText(string.format(L.levelMark, v.marker))
            dy = dy - 14
        else
            shown = shown + 1
            row.index = v.index
            row.dot:Show()
            local color = Skin.Ink(v.color)
            local r, g, b = tonumber(color:sub(1, 2), 16), tonumber(color:sub(3, 4), 16), tonumber(color:sub(5, 6), 16)
            row.dot:SetColorTexture(r / 255, g / 255, b / 255, 1)
            TextColor(row.text, "text2")
            local text = v.text .. (v.who and ("  " .. Skin.Hex("dim") .. v.who .. "|r") or "")
            row:SetAlpha(v.done and 0.45 or 1)
            if v.done then text = "|TInterface\\RaidFrame\\ReadyCheck-Ready:10|t " .. text end
            if tab == "pickup" and v.distance then
                text = text
                    .. "  "
                    .. Skin.Hex(v.distance < 100 and "text" or "muted")
                    .. string.format(L.yards, v.distance)
                    .. "|r"
            end
            row.text:SetText(text)
            dy = dy - 15
        end
    end
    for i = n + 1, #drop.rows do
        drop.rows[i]:Hide()
    end
    drop.empty:SetShown(#list == 0)
    if #list == 0 then
        drop.empty:ClearAllPoints()
        drop.empty:SetPoint("TOPLEFT", 12, dy)
        drop.empty:SetText(tab == "pickup" and L.noPickup or "")
        dy = dy - 15
    end
    dy = dy - 4
    drop.line:ClearAllPoints()
    drop.line:SetPoint("TOPLEFT", 1, dy)
    drop.line:SetPoint("TOPRIGHT", -1, dy)
    local label = FG:Get("questPanel") and L.hideQuests or L.showQuests
    drop.quests.label:SetText(label)
    drop.quests:SetWidth((tonumber(drop.quests.label:GetStringWidth()) or 80) + 14)
    drop.quests:SetShown(FG:Get("trackerShown") and true or false)
    drop.tools:ClearAllPoints()
    drop.tools:SetPoint("TOPLEFT", 10, dy - 6)
    drop:SetHeight(-dy + 30)
end

local function DrawButtons(view)
    local item = FG.ItemButton and FG.ItemButton.link
    local talent = view.talent
    if not FG.CanLayout(buttons) then return end
    buttons:SetShown(item ~= nil or talent ~= nil)
    buttons.itemLabel:SetShown(item ~= nil)
    if item then
        buttons.itemLabel.name:SetText(FG.ItemInfo(item) or "")
        buttons.itemLabel:SetWidth(
            math.max(
                tonumber(buttons.itemLabel.name:GetStringWidth()) or 60,
                tonumber(buttons.itemLabel.kind:GetStringWidth()) or 60
            ) + 16
        )
    end
    local t = buttons.talent
    t:SetShown(talent ~= nil)
    if talent then
        t.icon:SetTexture(talent.icon)
        t.label.name:SetText(
            talent.name .. "  " .. Skin.Hex("muted") .. (talent.rank + 1) .. "/" .. talent.maxRank .. "|r"
        )
        t.label:SetWidth(
            math.max(tonumber(t.label.name:GetStringWidth()) or 60, tonumber(t.label.kind:GetStringWidth()) or 60) + 16
        )
    end
end

function Mini:Render(view)
    if not strip then return end
    if view.hidden then
        Mini:Hide()
        return
    end
    strip:Show()
    if FG.Compass then FG.Compass:Attach(strip) end

    -- The task pointed at, else the first one not done.
    local main
    for _, v in ipairs(view.tasks or {}) do
        if v.target then main = v end
    end
    if not main then
        for _, v in ipairs(view.tasks or {}) do
            if not v.done and v.kind ~= "farm" then main = main or v end
        end
    end
    -- Only tasks done (the player went back to them): the first one, marked done.
    if not main then
        for _, v in ipairs(view.tasks or {}) do
            if v.kind ~= "farm" then main = main or v end
        end
    end
    strip.index = main and main.index
    if view.message or not main then
        strip.dot:Hide()
        strip.verb:SetText("FACTORUIDE")
        strip.name:SetText(view.message or "")
        strip.sub:SetText("")
        strip.count:SetText("")
        strip.dist:SetText("")
        strip.zone:SetText("")
        strip.track:Hide()
        strip.fill:Hide()
        tags:Hide()
        xpBar:Hide()
        drop:SetAlpha(0)
        drop:EnableMouse(false)
        aheadBar:Hide()
        if FG.CanLayout(buttons) then buttons:Hide() end
        return
    end
    drop:SetAlpha(1)
    drop:EnableMouse(true)
    tags:Show()
    xpBar:Show()
    local kind = FG.Guide.KINDS[main.kind] or FG.Guide.OTHER
    local r, g, b = 1, 1, 1
    if main.color then
        local color = Skin.Ink(main.color)
        r, g, b =
            tonumber(color:sub(1, 2), 16) / 255,
            tonumber(color:sub(3, 4), 16) / 255,
            tonumber(color:sub(5, 6), 16) / 255
    end
    strip.dot:Show()
    strip.dot:SetColorTexture(r, g, b, 1)
    strip.verb:SetText(
        (kind.label or "")
            .. " • "
            .. string.format(L.stepOf, view.step, view.total)
            .. (main.done and ("  " .. Skin.Hex("green") .. L.done .. "|r") or "")
    )
    strip.name:SetText(main.name)
    strip.name:SetAlpha(main.done and 0.5 or 1)
    strip.count:SetText(Count(main))
    local sub = main.sub or ""
    for _, chip in ipairs(main.chips or {}) do
        sub = sub .. (sub ~= "" and "  " or "") .. Skin.Hex(chip.color) .. chip.text .. "|r"
    end
    strip.sub:SetText(sub)
    -- A task with no place (abandon a quest...): no distance nor zone, the text takes the width.
    local placed = view.target ~= nil and view.target.m ~= nil
    strip.sep:SetShown(placed)
    strip.dist:SetShown(placed)
    strip.zone:SetShown(placed)
    local right = placed and strip.sep or strip
    local edge = placed and "TOPLEFT" or "TOPRIGHT"
    strip.count:ClearAllPoints()
    strip.count:SetPoint("TOPRIGHT", right, edge, placed and -10 or -12, -9)
    strip.sub:SetPoint("RIGHT", right, placed and "LEFT" or "RIGHT", placed and -10 or -12, 0)
    strip.track:SetPoint(
        "BOTTOMRIGHT",
        right,
        placed and "BOTTOMLEFT" or "BOTTOMRIGHT",
        placed and -10 or -12,
        placed and -3 or 5
    )
    strip.dist:SetText(view.distance and string.format(L.yards, math.floor(view.distance + 0.5)) or "")
    strip.zone:SetText(view.zone or "")
    local bar = (main.have and main.need and main.need > 0) and true or false
    strip.track:SetShown(bar)
    strip.fill:SetShown(bar)
    if bar then
        -- The bar runs from the left edge to the separator, or the right edge (see above).
        local w = placed and (STRIP_WIDTH - 12 - 84 - 10) or (STRIP_WIDTH - 24)
        local share = math.max(0, math.min(1, main.have / main.need))
        strip.fill:SetWidth(math.max(1, w * share))
    end

    local y = -4
    if DrawTags(view, main) then y = y - 22 end
    DrawXp(y)
    DrawDrop(view, y - 8)

    aheadBar:SetShown(view.ahead ~= nil)
    if view.ahead then
        aheadBar.text:SetText(
            string.format(L.ahead, view.ahead.lvl) .. "  •  " .. string.format(L.skip, view.ahead.steps)
        )
        aheadBar:SetWidth((tonumber(aheadBar.text:GetStringWidth()) or 200) + 24)
        aheadBar:ClearAllPoints()
        aheadBar:SetPoint("BOTTOM", strip, "TOP", 0, 32)
    end
    DrawButtons(view)
end
