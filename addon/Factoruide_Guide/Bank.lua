-- Items to keep for the guide's quests (Guide:ItemsToKeep), in the bags and in the bank. The bank's
-- count is read each time it is opened (kept per character); an item the addon has not seen there
-- can be marked by hand as being in it (a click in the quest panel's Items tab). At the bank: what
-- to take (needed soon) and what can stay there (needed much later), in the panel and the chat.

local FG = Factoruide
local T = FG.T

local Bank = { open = false }
FG.Bank = Bank

local L = {
    take = T("take from the bank: %s"),
    leave = T("can stay in the bank: %s"),
}

-- Needed within this many levels: taken from the bank (and listed even when none is owned yet);
-- needed this many levels later or more: left there.
local SOON, LATER = 3, 6

--- The character's saved data, nil before the Profile addon made it at login.
local function Char()
    local char = FactoruideDB.characters and FactoruideDB.characters[FG:CharacterKey()]
    if not char then return nil end
    char.bank = char.bank or {}
    char.bankMarks = char.bankMarks or {}
    return char
end

--- The items to keep and where they are: Guide:ItemsToKeep's, with `icon`, `bags` (count in the
--- bags), `bank` (count seen in the bank, nil when not seen), `marked` (in the bank, by hand) and,
--- while the bank is open, `hint`: "take" or "leave". Only those owned (bags, bank) or needed soon
--- unless `all`; the second value is how many others the guide needs later.
function Bank:Items(all)
    local items = FG.Guide and FG.Guide:ItemsToKeep() or {}
    local char = Char()
    local level = UnitLevel("player")
    local shown, later = {}, 0
    if not char then return shown, later end
    for _, item in ipairs(items) do
        local name, _, _, _, _, _, _, _, _, icon = FG.ItemInfo(item.id)
        item.name = name or item.name
        item.icon = icon or select(5, FG.ItemInfoInstant(item.id))
        item.bags = FG.ItemCount(item.id) or 0
        item.bank = char.bank[item.id]
        item.marked = char.bankMarks[item.id] or false
        if self.open then
            if (item.bank or 0) > 0 and item.bags < item.need and item.lvl <= level + SOON then
                item.hint = "take"
            elseif item.bags > 0 and item.lvl >= level + LATER then
                item.hint = "leave"
            end
        end
        if all or item.bags > 0 or (item.bank or 0) > 0 or item.marked or item.lvl <= level + SOON then
            table.insert(shown, item)
        else
            later = later + 1
        end
    end
    return shown, later
end

--- Mark an item as being in the bank (or not), by hand.
function Bank:ToggleMark(id)
    local char = Char()
    if not char then return end
    local marks = char.bankMarks
    marks[id] = not marks[id] or nil
    if FG.Tracker then FG.Tracker:Refresh() end
end

--- Read the bank's count of the items to keep (the game counts the bank while it is open); what
--- was seen replaces the marks by hand.
local function Scan()
    local char = Char()
    if not char then return end
    for _, item in ipairs(FG.Guide and FG.Guide:ItemsToKeep() or {}) do
        local all, bags = FG.ItemCount(item.id, true) or 0, FG.ItemCount(item.id) or 0
        char.bank[item.id] = math.max(0, all - bags)
        char.bankMarks[item.id] = nil
    end
end

local function Names(items, hint)
    local out = {}
    for _, item in ipairs(items) do
        if item.hint == hint then
            local count = hint == "take" and math.min(item.bank or 0, item.need - item.bags) or item.bags
            table.insert(out, string.format("%s x%d", item.name or ("#" .. item.id), count))
        end
    end
    return table.concat(out, ", ")
end

local pending = false
local function Rescan()
    if pending or not Bank.open then return end
    pending = true
    C_Timer.After(0.3, function()
        pending = false
        if not Bank.open then return end
        FG:Try("bank", Scan)
        if FG.Tracker then FG:Try("tracker", FG.Tracker.Refresh, FG.Tracker) end
    end)
end

FG:On("BANKFRAME_OPENED", function()
    Bank.open = true
    Scan()
    local items = Bank:Items()
    local take, leave = Names(items, "take"), Names(items, "leave")
    if take ~= "" then FG:Print(L.take, take) end
    if leave ~= "" then FG:Print(L.leave, leave) end
    if FG.Tracker then FG.Tracker:ShowItems(true) end
end)

FG:On("BANKFRAME_CLOSED", function()
    if not Bank.open then return end
    Bank.open = false
    if FG.Tracker then FG.Tracker:ShowItems(false) end
end)

for _, event in ipairs({ "PLAYERBANKSLOTS_CHANGED", "BAG_UPDATE" }) do
    FG:On(event, Rescan)
end
