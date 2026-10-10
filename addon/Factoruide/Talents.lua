-- Talent plan: the next talent of the class's leveling plan (TalentBuilds.lua, laid on the
-- game's trees by `fg talents`), shown under the guide while points are unspent; learned in one
-- click, or automatically.

local _, FG = ...

local fr = GetLocale() == "frFR"

local Talents = {}
FG.Talents = Talents

--- Talents of the character: "tab:tier:column" -> { tab, index, name, rank, maxRank }.
local function Known()
    local out = {}
    for tab = 1, (GetNumTalentTabs and GetNumTalentTabs() or 0) do
        for i = 1, GetNumTalents(tab) do
            local name, _, tier, column, rank, maxRank = GetTalentInfo(tab, i)
            if name then
                out[tab .. ":" .. tier .. ":" .. column] =
                    { tab = tab, index = i, name = name, rank = rank, maxRank = maxRank }
            end
        end
    end
    return out
end

--- The next talent of the plan not learned yet, or nil.
function Talents:Next()
    local builds = FG.TalentBuilds and FG.TalentBuilds[FG.Edition]
    local build = builds and builds[select(2, UnitClass("player"))]
    if not build then return nil end
    local known = Known()
    local wanted = {}
    for _, pick in ipairs(build.picks) do
        local key = pick[1] .. ":" .. pick[2] .. ":" .. pick[3]
        wanted[key] = (wanted[key] or 0) + 1
        local t = known[key]
        if t and t.rank < wanted[key] and t.rank < t.maxRank then return t end
    end
    return nil
end

local function Unspent() return UnitCharacterPoints and UnitCharacterPoints("player") or 0 end

local frame
local function UI()
    if frame or not CreateFrame then return frame end
    frame = CreateFrame("Frame", "FactoruideTalents", UIParent, "BackdropTemplate")
    frame:SetSize(260, 26)
    frame.text = frame:CreateFontString(nil, "OVERLAY", "GameFontHighlightSmall")
    frame.text:SetPoint("LEFT", 6, 0)
    frame.button = CreateFrame("Button", nil, frame, "UIPanelButtonTemplate")
    frame.button:SetSize(80, 20)
    frame.button:SetPoint("RIGHT", -2, 0)
    frame.button:SetText(fr and "Apprendre" or "Learn")
    frame.button:SetScript("OnClick", function() Talents:LearnNext() end)
    FG.Skin:Style(frame, "panel", { frame.text })
    FG.Skin:Style(frame.button, "button")
    return frame
end

function Talents:LearnNext()
    local t = self:Next()
    if t and Unspent() > 0 and not InCombatLockdown() then
        LearnTalent(t.tab, t.index)
        FG:Print(fr and "talent appris : %s (%d/%d)" or "talent learned: %s (%d/%d)", t.name, t.rank + 1, t.maxRank)
        return true
    end
    return false
end

function Talents:Refresh()
    if not FG:Get("talentAdvisor") then
        if frame then frame:Hide() end
        return
    end
    if FG:Get("autoLearnTalents") and not InCombatLockdown() then
        for _ = 1, Unspent() do
            if not self:LearnNext() then break end
        end
    end
    local t = Unspent() > 0 and self:Next()
    local f = UI()
    if not f then return end
    if not t then
        f:Hide()
        return
    end
    local box = FG.Guide and FG.Guide:Box()
    f:ClearAllPoints()
    if box then
        f:SetPoint("TOPLEFT", box, "BOTTOMLEFT", 0, -4)
    else
        f:SetPoint("CENTER")
    end
    f.text:SetText(
        string.format("|cffffd100%s|r %s (%d/%d)", fr and "Talent :" or "Talent:", t.name, t.rank + 1, t.maxRank)
    )
    f:Show()
end

for _, event in ipairs({
    "PLAYER_ENTERING_WORLD",
    "PLAYER_LEVEL_UP",
    "CHARACTER_POINTS_CHANGED",
    "PLAYER_REGEN_ENABLED",
}) do
    FG:On(event, function()
        -- Talent data is ready a moment after these events.
        if C_Timer then
            C_Timer.After(0.5, function() Talents:Refresh() end)
        else
            Talents:Refresh()
        end
    end)
end
