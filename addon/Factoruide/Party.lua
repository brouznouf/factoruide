-- Group sync: members running Factoruide send their quest progress (objectives fulfilled) to
-- the party; the tracker shows theirs next to each objective.

local _, FG = ...

local PREFIX = "FGSYNC1"
local Party = { progress = {} }
FG.Party = Party

if C_ChatInfo and C_ChatInfo.RegisterAddonMessagePrefix then C_ChatInfo.RegisterAddonMessagePrefix(PREFIX) end

--- "id:f1,f2;id:f1" for the quests of the log.
local function Encode()
    local parts = {}
    for i = 1, (GetNumQuestLogEntries and GetNumQuestLogEntries() or 0) do
        local _, _, _, isHeader, _, _, _, questID = GetQuestLogTitle(i)
        if not isHeader and questID and questID > 0 then
            local counts = {}
            for _, o in ipairs(C_QuestLog.GetQuestObjectives(questID) or {}) do
                table.insert(counts, o.finished and "x" or tostring(o.numFulfilled or 0))
            end
            table.insert(parts, questID .. ":" .. table.concat(counts, ","))
        end
    end
    return parts
end

local function Send()
    if not FG:Get("partySync") or not IsInGroup() or IsInRaid() or not C_ChatInfo then return end
    -- Messages are 255 bytes at most: "R" starts a full update, then chunks.
    C_ChatInfo.SendAddonMessage(PREFIX, "R", "PARTY")
    local chunk = ""
    for _, part in ipairs(Encode()) do
        if #chunk + #part + 1 > 240 then
            C_ChatInfo.SendAddonMessage(PREFIX, chunk, "PARTY")
            chunk = ""
        end
        chunk = chunk == "" and part or (chunk .. ";" .. part)
    end
    if chunk ~= "" then C_ChatInfo.SendAddonMessage(PREFIX, chunk, "PARTY") end
end

local scheduled = false
local function SendSoon()
    if scheduled or not C_Timer then return end
    scheduled = true
    C_Timer.After(2, function()
        scheduled = false
        Send()
    end)
end

FG:On("QUEST_LOG_UPDATE", SendSoon)
FG:On("GROUP_ROSTER_UPDATE", function()
    for name in pairs(Party.progress) do
        if not UnitInParty(name) then Party.progress[name] = nil end
    end
    SendSoon()
end)

FG:On("CHAT_MSG_ADDON", function(prefix, message, _, sender)
    if prefix ~= PREFIX then return end
    local name = Ambiguate(sender, "short")
    if name == UnitName("player") then return end
    if message == "R" then
        Party.progress[name] = {}
        return
    end
    local mine = Party.progress[name] or {}
    Party.progress[name] = mine
    for id, counts in message:gmatch("(%d+):([^;]*)") do
        local list = {}
        for c in counts:gmatch("[^,]+") do
            table.insert(list, c)
        end
        mine[tonumber(id)] = list
    end
    if FG.Tracker then FG.Tracker:Refresh() end
end)

--- " (Name 3, Name ✓)" for an objective of a quest the party also has.
function Party:Suffix(questID, objective, required)
    if not FG:Get("partySync") then return "" end
    local parts = {}
    for name, quests in pairs(self.progress) do
        local c = quests[questID] and quests[questID][objective]
        if c then
            local done = c == "x" or (required and tonumber(c) and tonumber(c) >= required)
            table.insert(parts, name .. " " .. (done and "|TInterface\\RaidFrame\\ReadyCheck-Ready:10|t" or c))
        end
    end
    table.sort(parts)
    return #parts > 0 and ("  |cff9d9d9d(" .. table.concat(parts, ", ") .. ")|r") or ""
end
