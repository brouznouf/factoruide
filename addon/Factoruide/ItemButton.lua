-- Quest item button: the item a current step's quest asks to use, one click (or one key: see
-- the game's key bindings, "Factoruide") away. Secure attributes only change out of combat.

local _, FG = ...

local fr = GetLocale() == "frFR"
BINDING_HEADER_FACTORUIDE = "Factoruide"
_G["BINDING_NAME_CLICK FGItemButton:LeftButton"] = fr and "Utiliser l'objet de quête" or "Use the quest item"

local ItemButton = { link = nil, pending = nil }
FG.ItemButton = ItemButton

local button

local function Create()
    if button or not CreateFrame then return button end
    button = CreateFrame("Button", "FGItemButton", UIParent, "SecureActionButtonTemplate")
    button:SetSize(36, 36)
    button:RegisterForClicks("AnyUp", "AnyDown")
    button:SetAttribute("type", "item")
    local icon = button:CreateTexture(nil, "ARTWORK")
    icon:SetAllPoints()
    button.icon = icon
    button:SetNormalTexture("Interface\\Buttons\\UI-Quickslot2")
    local cooldown = CreateFrame("Cooldown", nil, button, "CooldownFrameTemplate")
    cooldown:SetAllPoints()
    button.cooldown = cooldown
    button:SetScript("OnEnter", function(self)
        if ItemButton.link then
            GameTooltip:SetOwner(self, "ANCHOR_LEFT")
            GameTooltip:SetHyperlink(ItemButton.link)
            GameTooltip:Show()
        end
    end)
    button:SetScript("OnLeave", function() GameTooltip:Hide() end)
    FG.Skin:Style(button, "icon")
    button:Hide()
    return button
end

--- Show `link` on the button (nil hides it), now or after combat.
local function Apply(link, texture)
    if InCombatLockdown() then
        ItemButton.pending = { link = link or false, texture = texture }
        return
    end
    ItemButton.pending = nil
    local b = Create()
    if not b then return end
    ItemButton.link = link
    if not link then
        b:Hide()
        return
    end
    local box = FG.Guide and FG.Guide:Box()
    b:ClearAllPoints()
    if box then
        b:SetPoint("TOPRIGHT", box, "TOPLEFT", -6, 0)
    else
        b:SetPoint("CENTER", UIParent, "CENTER", -300, 0)
    end
    b:SetAttribute("item", link)
    b.icon:SetTexture(texture)
    b:Show()
end

--- The first quest item of these steps (quests in the log with a usable item).
function ItemButton:SetSteps(steps)
    if not FG:Get("questItemButton") or not GetQuestLogSpecialItemInfo then return end
    for _, step in ipairs(steps) do
        local index = step.q and GetQuestLogIndexByID and GetQuestLogIndexByID(step.q)
        if index and index > 0 then
            local link, texture = GetQuestLogSpecialItemInfo(index)
            if link then
                if link ~= self.link then Apply(link, texture) end
                return
            end
        end
    end
    if self.link then Apply(nil) end
end

FG:On("PLAYER_REGEN_ENABLED", function()
    local p = ItemButton.pending
    if p then Apply(p.link or nil, p.texture) end
end)

FG:On("BAG_UPDATE_COOLDOWN", function()
    if button and button:IsShown() and ItemButton.link and GetItemCooldown then
        local id = tonumber(ItemButton.link:match("item:(%d+)"))
        local start, duration = GetItemCooldown(id)
        if start then button.cooldown:SetCooldown(start, duration) end
    end
end)
