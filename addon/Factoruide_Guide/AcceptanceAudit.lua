-- Observe every identified planned offer before automation can select a dialog. Acceptance
-- remains the guide's quest-log decision; this module only gives advice after an interaction.
local FG = Factoruide
local Audit = {}
FG.AcceptanceAudit = Audit

local GRACE = 1
local MAX_CLOSING = 16 -- bounded snapshots when several different sources open in one second
local generation = 0
local current
local closing = {}

function Audit:Reset()
    generation = generation + 1
    current, closing = nil, {}
end

local function Route() return FG.Guide and FG.Guide:Route() end

local function Valid(token) return token.live and token.generation == generation and token.route == Route() end

local function Release(token)
    token.live = false
    if current == token then current = nil end
    for i = #closing, 1, -1 do
        if closing[i] == token then table.remove(closing, i) end
    end
end

local function Reconcile(token)
    if not Valid(token) then return Release(token) end
    local missing = {}
    for _, id in ipairs(token.ids) do
        if
            not token.confirmed[id]
            and not token.turnedIn[id]
            and not FG.QuestLog.IsOnQuest(id)
            and (FG.Reputation and FG.Reputation:WantsQuest(id) or not FG.QuestLog.IsQuestFlaggedCompleted(id))
            and FG.Guide:NeedsQuestAcceptance(id)
        then
            local name = token.names[id] or FG.QuestLog.GetTitleForQuestID(id)
            missing[#missing + 1] = name and (name .. " (#" .. id .. ")") or ("#" .. id)
        end
    end
    Release(token) -- deduplicate even if hidden, or a notification sink throws
    if #missing == 0 or not FG:Get("uiShown") then return end
    local text = string.format(
        FG.T("Not accepted: %s. Return to the quest giver and accept them manually."),
        table.concat(missing, ", ")
    )
    if UIErrorsFrame then UIErrorsFrame:AddMessage(text, 1, 0.82, 0.2) end
    FG:Print("%s", text)
end

local function Close(token)
    if not Valid(token) or token.deadline then return end
    token.deadline = GetTime() + GRACE
    token.close = token.close + 1
    local close = token.close
    closing[#closing + 1] = token
    if #closing > MAX_CLOSING then Release(closing[1]) end
    C_Timer.After(GRACE, function()
        if Valid(token) and token.close == close and token.deadline then Reconcile(token) end
    end)
end

-- The predicate guards phase activity, never NPC identity. Older clients can instead expose
-- the existing frames; no frame is created, controlled or hidden by the audit.
local function ManagerOpen()
    local manager = C_PlayerInteractionManager
    local types = Enum and Enum.PlayerInteractionType
    if not (manager and manager.IsInteractingWithNpcOfType and types and types.Gossip and types.QuestGiver) then
        return nil
    end
    return manager.IsInteractingWithNpcOfType(types.Gossip) or manager.IsInteractingWithNpcOfType(types.QuestGiver)
end

local function FramesOpen()
    local known, shown = false, false
    for _, frame in pairs({ GossipFrame, QuestFrame }) do
        if frame and frame.IsShown then
            known = true
            if frame:IsShown() then shown = true end
        end
    end
    if known then return shown end
end

local function Probe(token)
    if not token or not Valid(token) then return end
    local probe = token.close
    C_Timer.After(0, function()
        if current ~= token or not Valid(token) or token.close ~= probe then return end
        local managed = ManagerOpen()
        if managed == true then return end
        local framed = FramesOpen()
        if framed == true then return end
        if managed == nil and framed == nil and (token.gossip or token.quest or token.continuing) then return end
        Close(token)
    end)
end

local function HookFrames()
    for kind, frame in pairs({ gossip = GossipFrame, quest = QuestFrame }) do
        if frame and frame.HookScript and not frame.factoruideAcceptanceHook then
            frame.factoruideAcceptanceHook = true
            frame:HookScript("OnHide", function()
                if current then
                    current[kind], current.continuing = false, false
                    Probe(current)
                end
            end)
        end
    end
end

local function Source(item)
    item = FG.Readable(item)
    if type(item) == "number" and item > 0 then return "item:" .. item end
    local guid = UnitGUID and FG.Readable(UnitGUID("npc"))
    if type(guid) == "string" and guid ~= "" then return guid end
end

local function Open(kind, item, phase)
    local route = Route()
    if not route then return end
    HookFrames()
    local source = Source(item)
    local explicitPhase = phase == true
        and current
        and current.quest
        and not current.deadline
        and (not source or (current.source and current.source:find("^item:")))
    -- Explicit panel transitions can retain or promote an unreadable source snapshot.
    -- Two distinct known sources never join through this continuation evidence.
    local compatible = current and (not source or not current.source or source == current.source)
    local detailTransition = (phase == "detail" or phase == "opening")
        and compatible
        and current
        and not current.deadline
        and (current.gossip or current.greeting or current.opening)
    local same = current
        and Valid(current)
        and (
            explicitPhase
            or detailTransition
            or (source and source == current.source)
            or (compatible and current.continuing)
        )
    if not same then
        local resume
        if source then
            for i = #closing, 1, -1 do
                local token = closing[i]
                if Valid(token) and token.source == source then
                    resume = token
                    table.remove(closing, i) -- preserve it if closing the current source reaches the bound
                    break
                end
            end
        end
        if current then Close(current) end -- previous NPC's snapshot keeps its own deadline
        current = resume
            or {
                generation = generation,
                route = route,
                source = source,
                ids = {},
                names = {},
                seen = {},
                confirmed = {},
                turnedIn = {},
                close = 0,
                live = true,
            }
    end
    local token = current
    token.source = token.source or source
    if token.deadline then
        token.deadline = nil
        for i = #closing, 1, -1 do
            if closing[i] == token then table.remove(closing, i) end
        end
    end
    token.close = token.close + 1 -- cancels probes and timers from an earlier phase/close
    token.gossip, token.quest, token.continuing = kind == "gossip", kind == "quest", false
    token.greeting = phase == "greeting"
    token.opening = phase == "opening"
    return token
end

local function Offer(token, id, name)
    id, name = FG.Readable(id), FG.Readable(name)
    if not token or type(id) ~= "number" or id <= 0 or not FG.Guide:WantsQuest(id) then return end
    if not token.seen[id] then
        token.seen[id] = true
        token.ids[#token.ids + 1] = id
    end
    if type(name) == "string" and name ~= "" then token.names[id] = name end
end

FG:On("GOSSIP_SHOW", function()
    local token = Open("gossip")
    for _, quest in ipairs(C_GossipInfo.GetAvailableQuests() or {}) do
        Offer(token, quest.questID, quest.title)
    end
end)

FG:On("QUEST_GREETING", function()
    local token = Open("quest", nil, "greeting")
    for i = 1, GetNumAvailableQuests() do
        local _, _, _, _, id = GetAvailableQuestInfo(i)
        -- Legacy titles alone cannot safely identify an unaccepted quest.
        Offer(token, id, GetAvailableTitle and GetAvailableTitle(i))
    end
end)

FG:On(
    "QUEST_DETAIL",
    function(item) Offer(Open("quest", item, "detail"), GetQuestID(), GetTitleText and GetTitleText()) end
)
for _, event in ipairs({ "QUEST_PROGRESS", "QUEST_COMPLETE" }) do
    FG:On(event, function() Open("quest", nil, true) end)
end
FG:On("GOSSIP_CLOSED", function(continuing)
    if not current then return end
    current.gossip = false
    current.continuing = continuing == true and not current.quest
    Probe(current)
end)
FG:On("QUEST_FINISHED", function()
    if not current then return end
    current.quest, current.continuing = false, false
    Probe(current)
end)

local function Evidence(id, value, turnedIn)
    id = FG.Readable(id)
    if type(id) ~= "number" or id <= 0 then return end
    local function Set(token)
        if Valid(token) and token.seen[id] then
            if turnedIn then
                token.turnedIn[id] = true
            else
                token.confirmed[id] = value
            end
        end
    end
    if current then Set(current) end
    for _, token in ipairs(closing) do
        Set(token)
    end
end
FG:On("QUEST_ACCEPTED", function(arg1, arg2) Evidence(arg2 or arg1, true) end)
FG:On("QUEST_REMOVED", function(id) Evidence(id, nil) end)
FG:On("QUEST_TURNED_IN", function(id) Evidence(id, true, true) end)

if C_PlayerInteractionManager and C_PlayerInteractionManager.IsInteractingWithNpcOfType then
    FG:On("PLAYER_INTERACTION_MANAGER_FRAME_SHOW", function(kind)
        local types = Enum and Enum.PlayerInteractionType
        if types and kind == types.Gossip then Open("gossip") end
        if types and kind == types.QuestGiver then Open("quest", nil, "opening") end
    end)
    FG:On("PLAYER_INTERACTION_MANAGER_FRAME_HIDE", function(kind)
        if not current then return end
        local types = Enum and Enum.PlayerInteractionType
        if types and (kind == types.Gossip or kind == types.QuestGiver) then
            current.continuing = false
            if kind == types.Gossip then
                current.gossip = false
            else
                current.quest = false
            end
            Probe(current)
        end
    end)
end
for _, event in ipairs({ "PLAYER_LEAVING_WORLD", "PLAYER_LOGOUT" }) do
    FG:On(event, function() Audit:Reset() end)
end
