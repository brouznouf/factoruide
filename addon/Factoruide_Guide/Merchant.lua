-- At a merchant: sell grey items, repair, buy the guide's gear and the items its quests ask for,
-- and buy the food, water and ammo of the character's level (classic vendor items), within a budget.

local FG = Factoruide
local T = FG.T

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
                local price = select(11, FG.ItemInfo(info.itemID)) or 0
                total = total + price * (info.stackCount or 1)
                C_Container.UseContainerItem(bag, slot)
            end
        end
    end
    if total > 0 then FG:Print(T("grey items sold: %s"), Money(total)) end
end

local function Repair()
    if not CanMerchantRepair or not CanMerchantRepair() then return end
    local cost, can = GetRepairAllCost()
    if can and cost > 0 and cost <= GetMoney() then
        RepairAllItems()
        FG:Print(T("repaired: %s"), Money(cost))
    end
end

--- What the merchant sells at index `i`: its price, how many one buy gives and how many it has
--- left (-1: as many as wanted), through the client's API of the moment (the classic global is
--- gone from the Forever client).
local function MerchantInfo(i)
    if C_MerchantFrame and C_MerchantFrame.GetItemInfo then
        local info = C_MerchantFrame.GetItemInfo(i)
        if not info then return nil end
        return info.price, info.stackCount, info.numAvailable
    end
    local _, _, price, quantity, available = GetMerchantItemInfo(i)
    return price, quantity, available
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
                local price, quantity = MerchantInfo(sold[id])
                if price then
                    best = {
                        index = sold[id],
                        id = id,
                        level = need,
                        unit = price / math.max(1, quantity or 1),
                        ids = family,
                    }
                end
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
        if entry[1] >= offer.level - 10 then have = have + (FG.ItemCount(entry[2]) or 0) end
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
        local name = FG.ItemInfo(offer.id) or ("item " .. offer.id)
        FG:Print(T("bought: %d × %s"), bought, name)
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
        local subclass = ranged and select(7, FG.ItemInfoInstant(ranged))
        -- Guns (3) shoot bullets; bows (2) and crossbows (18) arrows.
        Stock(subclass == 3 and BULLETS or ARROWS, FG:Get("suppliesAmmo"), budget)
    end
end

--- The items the guide still has to buy for its quests, when this merchant sells them (as many
--- as it has of a limited supply), whatever vendor the guide planned: a vendor out of stock is
--- made up for at the next one met.
local function QuestItems()
    local wanted = FG.Guide and FG.Guide:Purchases() or {}
    if not next(wanted) then return end
    for i = 1, GetMerchantNumItems() do
        local id = GetMerchantItemID and GetMerchantItemID(i)
        local want = id and wanted[id]
        if want then
            local price, quantity, available = MerchantInfo(i)
            local missing = want - (FG.ItemCount(id) or 0)
            local each = math.max(1, quantity or 1)
            local buys = math.ceil(missing / each)
            if available and available >= 0 then buys = math.min(buys, available) end
            buys = math.min(buys, math.floor(GetMoney() / math.max(1, price or 0)))
            for _ = 1, buys do
                BuyMerchantItem(i)
            end
            if buys > 0 then
                FG:Print(T("bought for a quest: %d × %s"), buys * each, FG.ItemInfo(id) or ("item " .. id))
            elseif missing > 0 and available == 0 then
                FG:Print(T("out of stock here: %s"), FG.ItemInfo(id) or ("item " .. id))
            end
        end
    end
end

--- The guide's gear this merchant sells (whatever vendor the guide planned, like the quest
--- items), once it can be paid for: a too short purse says nothing, the next merchant tries
--- again.
local function Gear()
    local wanted = FG.Guide and FG.Guide:GearPurchases() or {}
    if not next(wanted) then return end
    for i = 1, GetMerchantNumItems() do
        local id = GetMerchantItemID and GetMerchantItemID(i)
        if id and wanted[id] then
            local price, _, available = MerchantInfo(i)
            if price and price <= GetMoney() and (available or -1) ~= 0 then
                BuyMerchantItem(i)
                FG:Print(T("bought the guide's gear: %s"), FG.ItemInfo(id) or ("item " .. id))
            end
        end
    end
end

FG:On("MERCHANT_SHOW", function()
    if IsShiftKeyDown() then return end
    QuestItems()
    Gear()
    if FG:Get("autoSellJunk") then SellJunk() end
    if FG:Get("autoRepair") then Repair() end
    if FG:Get("autoSupplies") then Supplies() end
end)
