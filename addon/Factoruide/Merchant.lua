-- At a merchant: sell grey items, repair, and buy the food, water and ammo of the character's
-- level (classic vendor items), within a budget.

local _, FG = ...

local fr = GetLocale() == "frFR"

-- Vendor supplies by required level: { level, itemID }, one list per family.
local FOOD = {
    { { 1, 4540 }, { 5, 4541 }, { 15, 4542 }, { 25, 4544 }, { 35, 4601 }, { 45, 8950 } }, -- bread
    { { 1, 117 }, { 5, 2287 }, { 15, 3770 }, { 25, 3771 }, { 35, 4599 }, { 45, 8952 } }, -- meat
    { { 1, 2070 }, { 5, 414 }, { 15, 422 }, { 25, 1707 }, { 35, 3927 }, { 45, 8932 } }, -- cheese
    { { 1, 4536 }, { 5, 4537 }, { 15, 4538 }, { 25, 4539 }, { 35, 4602 }, { 45, 8953 } }, -- fruit
    { { 1, 787 }, { 5, 4592 }, { 15, 4593 }, { 25, 4594 }, { 35, 21552 }, { 45, 8957 } }, -- fish
    { { 1, 4604 }, { 5, 4605 }, { 15, 4606 }, { 25, 4607 }, { 35, 4608 }, { 45, 8948 } }, -- mushrooms
}
local WATER = { { { 1, 159 }, { 5, 1179 }, { 15, 1205 }, { 25, 1708 }, { 35, 1645 }, { 45, 8766 } } }
local ARROWS = { { { 1, 2512 }, { 10, 2515 }, { 25, 3030 }, { 40, 11285 } } }
local BULLETS = { { { 1, 2516 }, { 10, 2519 }, { 25, 3033 }, { 40, 11284 } } }
local NO_MANA = { WARRIOR = true, ROGUE = true }

--- Share of the character's money the automatic purchases may spend.
local BUDGET = 0.3

local function Money(copper) return GetCoinTextureString and GetCoinTextureString(copper) or tostring(copper) end

local function SellJunk()
    if not C_Container then return end
    local total = 0
    for bag = 0, 4 do
        for slot = 1, C_Container.GetContainerNumSlots(bag) do
            local info = C_Container.GetContainerItemInfo(bag, slot)
            if info and info.quality == 0 and not info.hasNoValue then
                local price = select(11, C_Item.GetItemInfo(info.itemID)) or 0
                total = total + price * (info.stackCount or 1)
                C_Container.UseContainerItem(bag, slot)
            end
        end
    end
    if total > 0 then FG:Print(fr and "objets gris vendus : %s" or "grey items sold: %s", Money(total)) end
end

local function Repair()
    if not CanMerchantRepair or not CanMerchantRepair() then return end
    local cost, can = GetRepairAllCost()
    if can and cost > 0 and cost <= GetMoney() then
        RepairAllItems()
        FG:Print(fr and "réparation : %s" or "repaired: %s", Money(cost))
    end
end

--- Best item of these families the character can use, among those the merchant sells:
--- merchant index, item ID, stack size, price per unit.
local function BestOffer(families)
    local level = UnitLevel("player")
    local sold = {}
    for i = 1, GetMerchantNumItems() do
        local id = GetMerchantItemID and GetMerchantItemID(i)
        if id then sold[id] = i end
    end
    local best
    for _, family in ipairs(families) do
        for _, entry in ipairs(family) do
            local need, id = entry[1], entry[2]
            if need <= level and sold[id] and (not best or need > best.level) then
                local _, _, price, quantity = GetMerchantItemInfo(sold[id])
                best =
                    { index = sold[id], id = id, level = need, unit = price / math.max(1, quantity or 1), ids = family }
            end
        end
    end
    return best
end

--- Buy up to `want` units of the best supply of these families (those of its family already
--- in the bags count), within what is left of the budget. Returns the money spent.
local function Stock(families, want, budget)
    local offer = BestOffer(families)
    if not offer or want <= 0 then return 0 end
    local have = 0
    for _, entry in ipairs(offer.ids) do
        if entry[1] >= offer.level - 10 then have = have + GetItemCount(entry[2]) end
    end
    local missing = math.min(want - have, math.floor(budget / math.max(1, offer.unit)))
    local stack = GetMerchantItemMaxStack(offer.index)
    local bought = 0
    while missing > 0 do
        local n = math.min(missing, stack)
        BuyMerchantItem(offer.index, n)
        missing = missing - n
        bought = bought + n
    end
    if bought > 0 then
        local name = C_Item.GetItemInfo(offer.id) or ("item " .. offer.id)
        FG:Print(fr and "acheté : %d × %s" or "bought: %d × %s", bought, name)
    end
    return bought * offer.unit
end

local function Supplies()
    local budget = GetMoney() * BUDGET
    local _, class = UnitClass("player")
    budget = budget - Stock(FOOD, FG:Get("suppliesFood"), budget)
    if not NO_MANA[class] then budget = budget - Stock(WATER, FG:Get("suppliesWater"), budget) end
    if class == "HUNTER" then
        local ranged = GetInventoryItemID("player", 18)
        local subclass = ranged and select(7, C_Item.GetItemInfoInstant(ranged))
        -- Guns (3) shoot bullets; bows (2) and crossbows (18) arrows.
        Stock(subclass == 3 and BULLETS or ARROWS, FG:Get("suppliesAmmo"), budget)
    end
end

FG:On("MERCHANT_SHOW", function()
    if IsShiftKeyDown() then return end
    if FG:Get("autoSellJunk") then SellJunk() end
    if FG:Get("autoRepair") then Repair() end
    if FG:Get("autoSupplies") then Supplies() end
end)
