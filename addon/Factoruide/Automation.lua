-- Quest dialog automation (accept, turn in, rewards) and fast looting.
-- Holding Shift while talking to an NPC disables the quest automation.

local _, FG = ...

local function Skip() return IsShiftKeyDown() end

--- Whether a quest may be accepted automatically.
local function Wanted(questID)
    if not FG:Get("autoAccept") then return false end
    if FG:Get("routeQuestsOnly") and FG.Guide and FG.Guide.RouteHasQuest then
        return questID ~= nil and FG.Guide:RouteHasQuest(questID)
    end
    return true
end

FG:On("GOSSIP_SHOW", function()
    if Skip() then return end
    if FG:Get("autoTurnIn") then
        for _, q in ipairs(C_GossipInfo.GetActiveQuests()) do
            if q.isComplete then
                C_GossipInfo.SelectActiveQuest(q.questID)
                return
            end
        end
    end
    for _, q in ipairs(C_GossipInfo.GetAvailableQuests()) do
        if Wanted(q.questID) and not q.repeatable then
            C_GossipInfo.SelectAvailableQuest(q.questID)
            return
        end
    end
end)

FG:On("QUEST_GREETING", function()
    if Skip() then return end
    if FG:Get("autoTurnIn") then
        for i = 1, GetNumActiveQuests() do
            local _, isComplete = GetActiveTitle(i)
            if isComplete then
                SelectActiveQuest(i)
                return
            end
        end
    end
    for i = 1, GetNumAvailableQuests() do
        local _, _, isRepeatable, _, questID = GetAvailableQuestInfo(i)
        if Wanted(questID) and not isRepeatable then
            SelectAvailableQuest(i)
            return
        end
    end
end)

FG:On("QUEST_DETAIL", function()
    if Skip() then return end
    if QuestGetAutoAccept() then
        AcknowledgeAutoAcceptQuest()
    elseif Wanted(GetQuestID()) then
        AcceptQuest()
    end
end)

-- Escort quests shared by a party member.
FG:On("QUEST_ACCEPT_CONFIRM", function()
    if FG:Get("autoAccept") and not Skip() then ConfirmAcceptQuest() end
end)

FG:On("QUEST_PROGRESS", function()
    if FG:Get("autoTurnIn") and not Skip() and IsQuestCompletable() then CompleteQuest() end
end)

--- Reward choice with the best vendor price, or nil when item data is not loaded yet.
local function BestReward(choices)
    local best, bestPrice = nil, -1
    for i = 1, choices do
        local link = GetQuestItemLink("choice", i)
        local price = link and select(11, C_Item.GetItemInfo(link))
        if not price then return nil end
        if price > bestPrice then
            best, bestPrice = i, price
        end
    end
    return best
end

FG:On("QUEST_COMPLETE", function()
    if not FG:Get("autoTurnIn") or Skip() then return end
    local choices = GetNumQuestChoices()
    if choices <= 1 then
        GetQuestReward(choices)
    elseif FG:Get("autoReward") then
        -- The guide's choice first, else the best equipment upgrade, else the best vendor price.
        local choice = (FG.Gear and (FG.Gear:PlannedChoice(choices) or FG.Gear:BestChoice(choices)))
            or BestReward(choices)
        if choice then GetQuestReward(choice) end
    end
end)

-- Fast loot: take every slot as soon as the loot is ready, when auto loot applies.
local lastLoot = 0
FG:On("LOOT_READY", function(autoLoot)
    if not FG:Get("fastLoot") then return end
    local auto = autoLoot or (GetCVarBool("autoLootDefault") ~= IsModifiedClick("AUTOLOOTTOGGLE"))
    if not auto or GetTime() - lastLoot < 0.3 then return end
    lastLoot = GetTime()
    for slot = GetNumLootItems(), 1, -1 do
        LootSlot(slot)
    end
end)

-- Class trainer: learn every available spell we can afford, one at a time (the list
-- refreshes after each purchase). With `trainUseful`, only the spells the guide counts for
-- leveling (the route's list), when the route has one.
local training = false

--- Names of the spells worth learning (nil: all of them).
local function Useful()
    if not FG:Get("trainUseful") then return nil end
    local route = FG.Guide and FG.Guide:Route()
    if not route or not route.spells or #route.spells == 0 then return nil end
    local names = {}
    for _, id in ipairs(route.spells) do
        local name = FG.SpellName(id)
        if name then names[name] = true end
    end
    return names
end

local function BuyNext()
    if not training then return end
    local useful = Useful()
    for i = 1, GetNumTrainerServices() do
        local name, serviceType = GetTrainerServiceInfo(i)
        if serviceType == "available" and GetTrainerServiceCost(i) <= GetMoney() and (not useful or useful[name]) then
            BuyTrainerService(i)
            C_Timer.After(0.4, BuyNext)
            return
        end
    end
    training = false
end

FG:On("TRAINER_SHOW", function()
    if FG:Get("autoTrain") and not Skip() and not IsTradeskillTrainer() then
        training = true
        C_Timer.After(0.2, BuyNext)
    end
end)

FG:On("TRAINER_CLOSED", function() training = false end)

-- Flight master: when the current step is a flight, take it to the step's destination (hold
-- Shift to choose by hand).
FG:On("TAXIMAP_OPENED", function()
    if not FG:Get("autoFly") or IsShiftKeyDown() or not FG.Guide then return end
    local route = FG.Guide:Route()
    local step = route and route.steps[FG.Guide:CurrentStep()]
    if not step or step.k ~= "fly" or not step.id then return end
    if C_TaxiMap and C_TaxiMap.GetAllTaxiNodes and GetTaxiMapID then
        for _, node in ipairs(C_TaxiMap.GetAllTaxiNodes(GetTaxiMapID()) or {}) do
            if node.nodeID == step.id then
                if node.state == Enum.FlightPathState.Reachable then TakeTaxiNode(node.slotIndex) end
                return
            end
        end
    end
end)
