-- Supplies and reset state used by reputation planning. Only carried bag contents are recorded.
local _, FG = ...

-- Numeric daily IDs from the primary Forever quest caches; see data/reputation/forever.json.
-- Filled from that maintained catalog by scripts/update-reputation-data.py.
local DAILY_QUESTS = { -- DAILY_QUEST_IDS
    94004,
    94212,
    94213,
    94214,
    94215,
    94216,
    94217,
    94218,
    94219,
    94220,
    94221,
    94222,
    94223,
    94224,
    94225,
    94226,
    94227,
    94228,
    94229,
    94230,
    94231,
    94232,
    94233,
    94234,
    94235,
    94236,
    94237,
    94238,
    94239,
    94240,
    94241,
    94242,
    94243,
    94244,
    94245,
    94246,
    94247,
    94248,
    94249,
    94250,
    94251,
    94252,
    94253,
    94254,
    94255,
    94256,
    94257,
    94258,
    94259,
    94260,
    94261,
    94262,
    94263,
    94264,
    94265,
    94266,
    94267,
    94268,
    94269,
    94270,
    94271,
    94272,
    94273,
    94274,
    94275,
    94276,
    94277,
    94278,
    94279,
    94280,
    94281,
    94282,
    94283,
    94284,
    94285,
    94286,
    94287,
    94288,
    94289,
    94290,
    94291,
    94292,
    94293,
    94294,
    94295,
    94296,
    94297,
    94298,
    94299,
    94300,
    94301,
    94302,
    94303,
    94304,
    94305,
    94306,
    94307,
    94308,
    94309,
    94310,
    94311,
    94312,
    94313,
    94314,
    94315,
    94316,
    94317,
    94318,
    94319,
    94320,
    94321,
    94322,
    94323,
    94324,
    94325,
    94326,
    94327,
    94328,
    94329,
    94330,
    94331,
    94332,
    94333,
    94334,
    94335,
    94336,
    94337,
    94338,
    94339,
    94340,
    94341,
    94342,
    94343,
    94344,
    94345,
    94346,
    94347,
    94348,
    94349,
    94350,
    94351,
    94352,
    94353,
    94354,
    94355,
    94356,
    94357,
    94358,
    94359,
    94360,
}

function FG.ReputationInventory()
    local modern = C_Container
    local slots = modern and modern.GetContainerNumSlots or GetContainerNumSlots
    local info = modern and modern.GetContainerItemInfo or GetContainerItemInfo
    local itemID = modern and modern.GetContainerItemID or GetContainerItemID
    if not (slots and info) then error("bag inventory API unavailable") end
    local totals = {}
    for bag = 0, NUM_BAG_SLOTS or 4 do
        for slot = 1, slots(bag) do
            local id, count
            if modern then
                local item = info(bag, slot)
                if item then
                    id, count = item.itemID, item.stackCount
                end
            else
                local _, amount, _, _, _, _, link = info(bag, slot)
                count = amount
                id = itemID and itemID(bag, slot) or (link and tonumber(link:match("item:(%d+)")))
            end
            if not id and itemID then id = itemID(bag, slot) end
            id, count = FG.Readable(id), FG.Readable(count)
            if type(id) == "number" and id > 0 and type(count) == "number" and count > 0 then
                totals[id] = (totals[id] or 0) + math.floor(count)
            end
        end
    end
    local out = {}
    for id, count in pairs(totals) do
        out[#out + 1] = { item = id, count = count }
    end
    table.sort(out, function(a, b) return a.item < b.item end)
    return out
end

function FG.ReputationDailySnapshot()
    if FG.Edition ~= "forever" then return nil end
    local flagged = C_QuestLog and C_QuestLog.IsQuestFlaggedCompleted or IsQuestFlaggedCompleted
    if not flagged then return nil end
    local completed = {}
    for _, id in ipairs(DAILY_QUESTS) do
        if flagged(id) then completed[#completed + 1] = id end
    end
    local reset = C_DateAndTime and C_DateAndTime.GetSecondsUntilDailyReset or GetQuestResetTime
    local seconds = reset and FG.Readable(reset())
    local now = GetServerTime and FG.Readable(GetServerTime())
    local resetAt
    if type(now) == "number" and now > 0 and type(seconds) == "number" and seconds > 0 then
        resetAt = math.floor(now + seconds)
    end
    return { completed = completed, reset_at = resetAt }
end
