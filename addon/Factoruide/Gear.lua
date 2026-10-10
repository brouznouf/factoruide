-- Gear advisor: a score per class from item stats, compared with what is equipped in the same
-- slot. Shown in item tooltips and on quest rewards; the automatic reward choice takes the
-- best upgrade first.

local _, FG = ...

local fr = GetLocale() == "frFR"

-- Stat weights per class (leveling, rough): GetItemStats keys.
local S, A, ST, I, SP =
    "ITEM_MOD_STRENGTH_SHORT",
    "ITEM_MOD_AGILITY_SHORT",
    "ITEM_MOD_STAMINA_SHORT",
    "ITEM_MOD_INTELLECT_SHORT",
    "ITEM_MOD_SPIRIT_SHORT"
local AR, DPS, AP = "RESISTANCE0_NAME", "ITEM_MOD_DAMAGE_PER_SECOND_SHORT", "ITEM_MOD_ATTACK_POWER_SHORT"
local SD = "ITEM_MOD_SPELL_DAMAGE_DONE_SHORT"
local WEIGHTS = {
    WARRIOR = { [S] = 1, [A] = 0.6, [ST] = 0.5, [AR] = 0.02, [DPS] = 3, [AP] = 0.5 },
    PALADIN = { [S] = 1, [A] = 0.4, [ST] = 0.5, [I] = 0.4, [SP] = 0.1, [AR] = 0.02, [DPS] = 3, [AP] = 0.4 },
    HUNTER = { [A] = 1, [ST] = 0.4, [I] = 0.3, [S] = 0.1, [AR] = 0.015, [DPS] = 2, [AP] = 0.4 },
    ROGUE = { [A] = 1, [S] = 0.5, [ST] = 0.4, [AR] = 0.015, [DPS] = 3, [AP] = 0.5 },
    PRIEST = { [I] = 1, [SP] = 1, [ST] = 0.5, [AR] = 0.005, [DPS] = 0.5, [SD] = 1 },
    SHAMAN = { [S] = 0.8, [I] = 0.6, [A] = 0.4, [ST] = 0.5, [SP] = 0.2, [AR] = 0.015, [DPS] = 2.5, [AP] = 0.4 },
    MAGE = { [I] = 1, [SP] = 0.8, [ST] = 0.5, [AR] = 0.005, [DPS] = 0.5, [SD] = 1 },
    WARLOCK = { [I] = 1, [SP] = 0.8, [ST] = 0.6, [AR] = 0.005, [DPS] = 0.5, [SD] = 1 },
    DRUID = { [S] = 0.8, [A] = 0.7, [I] = 0.5, [ST] = 0.5, [SP] = 0.3, [AR] = 0.01, [DPS] = 1 },
}

-- Armor (item class 4) subclasses a class wears (cloth 1, leather 2, mail 3, plate 4, shield 6),
-- with the level it starts; weapons (class 2) subclasses it uses.
local ARMOR = {
    WARRIOR = { [1] = 1, [2] = 1, [3] = 1, [4] = 40, [6] = 1 },
    PALADIN = { [1] = 1, [2] = 1, [3] = 1, [4] = 40, [6] = 1 },
    HUNTER = { [1] = 1, [2] = 1, [3] = 40 },
    SHAMAN = { [1] = 1, [2] = 1, [3] = 40, [6] = 1 },
    ROGUE = { [1] = 1, [2] = 1 },
    DRUID = { [1] = 1, [2] = 1 },
    PRIEST = { [1] = 1 },
    MAGE = { [1] = 1 },
    WARLOCK = { [1] = 1 },
}
local WEAPONS = {
    WARRIOR = { 0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 13, 15, 16, 18 },
    PALADIN = { 0, 1, 4, 5, 6, 7, 8 },
    HUNTER = { 0, 1, 2, 3, 6, 7, 8, 10, 13, 15, 16, 18 },
    ROGUE = { 2, 3, 4, 7, 13, 15, 16, 18 },
    PRIEST = { 4, 10, 15, 19 },
    SHAMAN = { 0, 1, 4, 5, 10, 13, 15 },
    MAGE = { 7, 10, 15, 19 },
    WARLOCK = { 7, 10, 15, 19 },
    DRUID = { 4, 5, 10, 13, 15 },
}

-- Equipment slots of an inventory type (the weaker of two slots is replaced).
local SLOTS = {
    INVTYPE_HEAD = { 1 },
    INVTYPE_NECK = { 2 },
    INVTYPE_SHOULDER = { 3 },
    INVTYPE_CHEST = { 5 },
    INVTYPE_ROBE = { 5 },
    INVTYPE_WAIST = { 6 },
    INVTYPE_LEGS = { 7 },
    INVTYPE_FEET = { 8 },
    INVTYPE_WRIST = { 9 },
    INVTYPE_HAND = { 10 },
    INVTYPE_FINGER = { 11, 12 },
    INVTYPE_TRINKET = { 13, 14 },
    INVTYPE_CLOAK = { 15 },
    INVTYPE_WEAPON = { 16 },
    INVTYPE_2HWEAPON = { 16 },
    INVTYPE_WEAPONMAINHAND = { 16 },
    INVTYPE_WEAPONOFFHAND = { 17 },
    INVTYPE_SHIELD = { 17 },
    INVTYPE_HOLDABLE = { 17 },
    INVTYPE_RANGED = { 18 },
    INVTYPE_RANGEDRIGHT = { 18 },
    INVTYPE_THROWN = { 18 },
    INVTYPE_RELIC = { 18 },
}

local Gear = {}
FG.Gear = Gear

local function Class() return select(2, UnitClass("player")) end

--- Score of an item for the character (nil: unknown yet or not for it).
function Gear:Score(link)
    local weights = WEIGHTS[Class()]
    local stats = link and weights and GetItemStats and GetItemStats(link)
    if not stats then return nil end
    local score = 0
    for key, w in pairs(weights) do
        score = score + (stats[key] or 0) * w
    end
    return score
end

--- Whether the character can wear or wield it (at its level).
function Gear:Usable(link)
    local _, _, _, equipLoc, _, classID, subclassID = C_Item.GetItemInfoInstant(link)
    if not SLOTS[equipLoc] then return false end
    local class = Class()
    if classID == 4 and subclassID and subclassID > 0 then
        local from = (ARMOR[class] or {})[subclassID]
        return from ~= nil and UnitLevel("player") >= from
    elseif classID == 2 then
        for _, s in ipairs(WEAPONS[class] or {}) do
            if s == subclassID then return true end
        end
        return false
    end
    return true
end

--- Gain of equipping `link` (score difference, and as a share of what it replaces), or nil.
function Gear:Upgrade(link)
    if not link or not self:Usable(link) then return nil end
    local new = self:Score(link)
    if not new then return nil end
    local equipLoc = select(4, C_Item.GetItemInfoInstant(link))
    local current
    for _, slot in ipairs(SLOTS[equipLoc]) do
        local worn = GetInventoryItemLink("player", slot)
        local s = worn and self:Score(worn) or 0
        -- A two-handed weapon replaces the off hand too.
        if equipLoc == "INVTYPE_2HWEAPON" then
            local off = GetInventoryItemLink("player", 17)
            s = s + (off and self:Score(off) or 0)
        end
        if not current or s < current then current = s end
    end
    local gain = new - (current or 0)
    return gain, current and current > 0 and gain / current or nil
end

--- Index of the reward choice the guide planned for the quest being turned in (the route was
--- computed with that item worn), if any.
function Gear:PlannedChoice(choices)
    local route = FG.Guide and FG.Guide:Route()
    local quest = GetQuestID and GetQuestID()
    if not route or not quest or quest == 0 then return nil end
    for _, s in ipairs(route.steps) do
        if s.k == "turnin" and s.q == quest and s.rw then
            for i = 1, choices do
                local link = GetQuestItemLink("choice", i)
                if link and tonumber(link:match("item:(%d+)")) == s.rw then return i end
            end
        end
    end
    return nil
end

--- Index of the reward choice that is the best upgrade, if any.
function Gear:BestChoice(choices)
    if not FG:Get("gearAdvisor") then return nil end
    local best, bestGain
    for i = 1, choices do
        local gain = self:Upgrade(GetQuestItemLink("choice", i))
        if gain and gain > 0 and (not bestGain or gain > bestGain) then
            best, bestGain = i, gain
        end
    end
    return best
end

local function TooltipLine(tooltip, link)
    if not FG:Get("gearAdvisor") or not link then return end
    local gain, share = Gear:Upgrade(link)
    if gain and gain > 0 then
        local text = share and string.format("+%d %%", math.floor(share * 100 + 0.5))
            or (fr and "emplacement vide" or "empty slot")
        tooltip:AddLine("|cff33ccffFactoruide|r  |cff40ff40" .. (fr and "amélioration " or "upgrade ") .. text .. "|r")
    end
end

if TooltipDataProcessor and Enum and Enum.TooltipDataType then
    TooltipDataProcessor.AddTooltipPostCall(Enum.TooltipDataType.Item, function(tooltip)
        if tooltip.GetItem then TooltipLine(tooltip, select(2, tooltip:GetItem())) end
    end)
elseif GameTooltip and GameTooltip.HookScript then
    GameTooltip:HookScript("OnTooltipSetItem", function(tooltip) TooltipLine(tooltip, select(2, tooltip:GetItem())) end)
end

-- Arrow on the best upgrade among quest rewards.
local arrow
FG:On("QUEST_COMPLETE", function()
    if arrow then arrow:Hide() end
    local choices = GetNumQuestChoices()
    local best = choices > 1 and (Gear:PlannedChoice(choices) or Gear:BestChoice(choices))
    local button = best and _G["QuestInfoRewardsFrameQuestInfoItem" .. best]
    if not button then return end
    arrow = arrow or UIParent:CreateTexture(nil, "OVERLAY")
    arrow:SetParent(button)
    arrow:SetDrawLayer("OVERLAY", 7)
    arrow:SetTexture("Interface\\Buttons\\UI-MicroStream-Green")
    arrow:SetSize(20, 20)
    arrow:ClearAllPoints()
    arrow:SetPoint("TOPLEFT", button, "TOPLEFT", -4, 4)
    arrow:Show()
end)
