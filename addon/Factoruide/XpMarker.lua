-- XP compared with the guide: on the game's XP bar, a mark where the guide expects the character
-- to be at the step shown and a band up to where it expects it once the current tasks are done,
-- the gap and the band's XP in the bar's tooltip. Without a known XP bar (another UI), a thin
-- bar of its own under the guide.

local _, FG = ...

local XpMarker = {}
FG.XpMarker = XpMarker

local fr = GetLocale() == "frFR"
local L = {
    ahead = fr and "Guide : %s xp d'avance" or "Guide: %s xp ahead",
    behind = fr and "Guide : %s xp de retard" or "Guide: %s xp behind",
    onTime = fr and "Guide : dans les temps" or "Guide: on schedule",
    level = fr and "Guide : niveau %d prévu à cette étape" or "Guide: level %d planned at this step",
    span = fr and "Tâches en cours : de %s à %s xp" or "Current tasks: from %s to %s xp",
}

-- XP bars of the game and of the replacement UIs, the first one shown is used.
local BARS = { "ElvUI_ExperienceBar", "MainMenuExpBar", "MainStatusTrackingBarContainer", "StatusTrackingBarManager" }

local mark, markParent, markLine, markEnd, markBand, own, ownFill, hooked

--- Where the guide expects the character before the step shown: its level and XP in it.
function XpMarker:Planned()
    local route = FG.Guide and FG.Guide:Route()
    local state = FG.Guide and FG.Guide:State()
    if not route or not state then return nil end
    local done = route.steps[state.step - 1]
    if done and done.px then return done.lvl, done.px end
    if state.step <= 1 then return route.fromLevel or 1, 0 end
    return nil
end

--- XP `xp` of `level` on the current level's scale (above its size: the next level, below 0: an
--- earlier one), or nil with `level` when it is further away.
local function OnScale(level, xp)
    local here, max = UnitLevel("player"), UnitXPMax("player")
    if level == here then return xp end
    if level == here + 1 then return max + xp end
    if level == here - 1 and xp >= max then return xp - max end
    return nil, level
end

--- The planned XP on the current level's scale (`OnScale`).
function XpMarker:PlannedHere()
    local level, xp = self:Planned()
    if not level then return nil end
    return OnScale(level, xp)
end

--- Where the guide expects the character once the current tasks are done: level and XP in it.
function XpMarker:PlannedAfter()
    local route = FG.Guide and FG.Guide:Route()
    local steps = FG.Guide and FG.Guide:CurrentSteps()
    local last = route and steps and route.steps[steps[#steps]]
    if not last or not last.px then return nil end
    return last.lvl, last.px
end

--- Tooltip line: the XP the current tasks start and end at, on the current level's scale.
function XpMarker:SpanLine()
    local from = self:PlannedHere()
    local level, xp = self:PlannedAfter()
    local to = level and OnScale(level, xp)
    if not from or not to then return nil end
    local n = function(v) return BreakUpLargeNumbers and BreakUpLargeNumbers(v) or tostring(v) end
    return string.format(L.span, n(from), n(to))
end

--- Tooltip line: XP ahead of the guide (positive) or behind it.
function XpMarker:Line()
    local planned, level = self:PlannedHere()
    if not planned then return level and string.format(L.level, level) or nil end
    local gap = UnitXP("player") - planned
    if math.abs(gap) < 1 then return L.onTime end
    local n = BreakUpLargeNumbers and BreakUpLargeNumbers(math.abs(gap)) or tostring(math.abs(gap))
    return string.format(gap > 0 and L.ahead or L.behind, n)
end

local function Bar()
    for _, name in ipairs(BARS) do
        local f = _G[name]
        if f and f.IsShown and f:IsShown() and (tonumber(f:GetWidth()) or 0) > 50 then return f end
    end
end

local function Mark(parent)
    if mark and markParent == parent then return mark end
    mark = mark or CreateFrame("Frame", nil, parent)
    mark:SetParent(parent)
    mark:ClearAllPoints()
    mark:SetAllPoints(parent)
    mark:SetFrameLevel((tonumber(parent:GetFrameLevel()) or 1) + 5)
    markParent = parent
    if not markLine then
        markBand = mark:CreateTexture(nil, "ARTWORK")
        markBand:SetColorTexture(1, 0.82, 0, 0.3)
        markLine = mark:CreateTexture(nil, "OVERLAY")
        markLine:SetColorTexture(1, 0.82, 0, 0.95)
        markLine:SetWidth(2)
        markEnd = mark:CreateTexture(nil, "OVERLAY")
        markEnd:SetColorTexture(1, 0.82, 0, 0.6)
        markEnd:SetWidth(2)
    end
    return mark
end

--- A thin bar under the guide: the character's XP and the guide's mark.
local function OwnBar()
    if own then return own end
    local box = FG.Guide and FG.Guide:Box()
    if not box then return nil end
    own = CreateFrame("Frame", nil, box)
    own:SetHeight(4)
    own:SetPoint("TOPLEFT", box, "BOTTOMLEFT", 1, -1)
    own:SetPoint("TOPRIGHT", box, "BOTTOMRIGHT", -1, -1)
    local bg = own:CreateTexture(nil, "BACKGROUND")
    bg:SetAllPoints()
    bg:SetColorTexture(0, 0, 0, 0.5)
    ownFill = own:CreateTexture(nil, "ARTWORK")
    ownFill:SetPoint("TOPLEFT")
    ownFill:SetPoint("BOTTOMLEFT")
    ownFill:SetColorTexture(0.55, 0.3, 0.9, 0.9)
    own:EnableMouse(true)
    own:SetScript("OnEnter", function(self)
        local line = XpMarker:Line()
        if not line then return end
        GameTooltip:SetOwner(self, "ANCHOR_BOTTOM")
        GameTooltip:SetText(line, 1, 0.82, 0)
        local span = XpMarker:SpanLine()
        if span then GameTooltip:AddLine(span, 1, 0.82, 0) end
        GameTooltip:Show()
    end)
    own:SetScript("OnLeave", function() GameTooltip:Hide() end)
    return own
end

--- The game's tooltip of the XP bar gets the gap with the guide.
local function Hook(bar)
    if hooked == bar or not bar.HookScript then return end
    hooked = bar
    bar:HookScript("OnEnter", function()
        local line = FG:Get("uiShown") and FG:Get("xpMarker") and XpMarker:Line()
        if line and GameTooltip:IsShown() then
            GameTooltip:AddLine(line, 1, 0.82, 0)
            local span = XpMarker:SpanLine()
            if span then GameTooltip:AddLine(span, 1, 0.82, 0) end
            GameTooltip:Show()
        end
    end)
end

function XpMarker:Refresh()
    local planned = self:PlannedHere()
    local max = UnitXPMax("player")
    local on = FG:Get("uiShown")
        and FG:Get("xpMarker")
        and FG:Get("guideShown")
        and max
        and max > 0
        and self:Planned() ~= nil
    local bar = on and Bar()
    if own then own:SetShown(on and not bar) end
    if mark then mark:SetShown(on) end
    if not on then return end
    local parent = bar or OwnBar()
    if not parent then return end
    if bar then Hook(bar) end
    local m = Mark(parent)
    local width = tonumber(parent:GetWidth()) or 0
    if not bar then ownFill:SetWidth(math.max(1, width * math.min(1, UnitXP("player") / max))) end
    -- Further than the next or previous level: at the bar's end, the tooltip tells the level.
    local function X(value, level)
        local share = value and value / max or ((level or 0) > UnitLevel("player") and 1 or 0)
        return width * math.max(0, math.min(1, share))
    end
    local from = X(self:PlannedHere())
    local afterLevel, afterXp = self:PlannedAfter()
    local to = afterLevel and X(OnScale(afterLevel, afterXp)) or from
    markLine:ClearAllPoints()
    markLine:SetPoint("TOP", m, "TOPLEFT", from, 2)
    markLine:SetPoint("BOTTOM", m, "BOTTOMLEFT", from, -2)
    -- The current tasks: from the mark to where they end.
    markEnd:ClearAllPoints()
    markEnd:SetPoint("TOP", m, "TOPLEFT", to, 2)
    markEnd:SetPoint("BOTTOM", m, "BOTTOMLEFT", to, -2)
    markEnd:SetShown(to > from + 2)
    markBand:ClearAllPoints()
    markBand:SetPoint("TOPLEFT", m, "TOPLEFT", from, 0)
    markBand:SetPoint("BOTTOMRIGHT", m, "BOTTOMLEFT", math.max(to, from + 1), 0)
    markBand:SetShown(to > from + 1)
    m:Show()
end

for _, event in ipairs({ "PLAYER_XP_UPDATE", "PLAYER_LEVEL_UP", "PLAYER_ENTERING_WORLD" }) do
    FG:On(event, function() XpMarker:Refresh() end)
end
FG:On("PLAYER_LOGIN", function()
    FG:OnSetting("xpMarker", function() XpMarker:Refresh() end)
    FG:OnSetting("uiShown", function() XpMarker:Refresh() end)
end)
