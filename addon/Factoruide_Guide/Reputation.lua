-- A compact programme is one protected guide step. Saved counters describe actual work,
-- never forecast rewards; only the live faction panel can confirm a reputation target.
local FG = Factoruide
local Rep = {}
FG.Reputation = Rep

local function Fingerprint(value)
    local hash = 5381
    local function add(s)
        for i = 1, #s do
            hash = (hash * 33 + s:byte(i)) % 4294967296
        end
    end
    local function visit(v)
        local kind = type(v)
        add(kind .. ":")
        if kind ~= "table" then
            add(tostring(v))
            add(";")
            return
        end
        local keys = {}
        for k in pairs(v) do
            keys[#keys + 1] = k
        end
        table.sort(keys, function(a, b) return tostring(a) < tostring(b) end)
        for _, k in ipairs(keys) do
            visit(k)
            visit(v[k])
        end
        add("}")
    end
    visit(value)
    return tostring(hash)
end

local fingerprints = setmetatable({}, { __mode = "k" })
local function Context(step, index, create)
    local guide = FG.Guide
    local state, route = guide and guide:State(), guide and guide:Route()
    if not state or not route or not step or step.k ~= "reputation" then return end
    local program = step.rp
    if type(program) ~= "table" then return end
    local fingerprint = fingerprints[program]
    if not fingerprint then
        fingerprint = Fingerprint(program)
        fingerprints[program] = fingerprint
    end
    local key = tostring(state.version) .. ":" .. tostring(index) .. ":" .. fingerprint
    if create then state.reputation = state.reputation or {} end
    local progress = state.reputation and state.reputation[key]
    if not progress and create then
        progress = { phase = 1, cycle = 1, action = 1, units = {}, cycleUnits = {}, quota = {}, warnings = {} }
        state.reputation[key] = progress
    end
    return program, progress
end

local function Warn(progress, text)
    if progress.warnings[text] then return end
    progress.warnings[text] = true
    FG:Print("%s", text)
end

local function LiveStandings()
    local out, names = {}, {}
    local ok, rows = pcall(FG.Reputations, true)
    if ok then
        for _, row in ipairs(rows or {}) do
            if type(row.value) == "number" then
                out[tonumber(row.id)] = row.value
                names[tonumber(row.id)] = row.name
            end
        end
    end
    return out, names
end

local function TargetsMet(targets, standings)
    local any = false
    for id, target in pairs(targets or {}) do
        any = true
        if not standings[tonumber(id)] or standings[tonumber(id)] < target then return false end
    end
    return any
end

-- A standing update can retire the visible occurrence before its completion event arrives.
-- Retain only that exposed occurrence: future actions skipped by the same poll earned nothing.
local function LatchExposure(progress)
    local exposed = progress.exposed
    progress.exposed = nil
    if not exposed or (progress.units[exposed.actionID] or 0) >= exposed.unitIndex then return end
    progress.pending = progress.pending or {}
    for _, pending in ipairs(progress.pending) do
        if pending.key == exposed.key then return end
    end
    progress.pending[#progress.pending + 1] = exposed
end

local function ExposeTurnin(progress, action, source)
    if action.kind ~= "turnin" then
        progress.exposed = nil
        return
    end
    local unitIndex = (progress.units[action.id] or 0) + 1
    progress.exposed = {
        key = table.concat({ progress.phase, progress.cycle, action.id, unitIndex }, ":"),
        phase = progress.phase,
        cycle = progress.cycle,
        actionIndex = progress.action,
        actionID = action.id,
        questID = source.identity.quest_id,
        unitIndex = unitIndex,
        totalLimit = action.total_unit_limit,
        resetAt = progress.resetAt,
        daily = source.reset and source.reset.kind == "daily",
        quotaKey = source.reset and source.reset.quota_key or source.id,
    }
end

local function ProgramNotices(program, progress)
    if progress.noticesShown then return end
    progress.noticesShown = true
    local notes = #(program.result and program.result.warnings or {})
    if notes > 0 then
        Warn(
            progress,
            string.format(
                FG.T("%d reputation planning notes. Review source details and limitations in the Factoruide app."),
                notes
            )
        )
    end
    local used = {}
    for _, phase in ipairs(program.phases or {}) do
        for _, action in ipairs(phase.actions or {}) do
            used[action.source_id] = true
        end
    end
    local estimated = false
    for _, warning in ipairs(program.result and program.result.warnings or {}) do
        if warning:find("Classic-derived reputation rules are forecasts", 1, true) == 1 then
            estimated = true
            break
        end
    end
    for _, source in ipairs(program.sources or {}) do
        if used[source.id] and source.confidence == "classic_estimate" then
            estimated = true
            break
        end
    end
    if estimated then
        Warn(
            progress,
            FG.T("Classic reputation estimate: actual inventory and faction standings determine your result.")
        )
    end
end

local function Finish(program, progress, standings)
    if progress.done then return end
    LatchExposure(progress)
    local _, names = LiveStandings()
    for id, target in pairs(program.result and program.result.targets or {}) do
        local value = standings[tonumber(id)]
        if value == nil then
            Warn(
                progress,
                string.format(
                    FG.T("Reputation #%s: actual standing unavailable; target %s remains unverified."),
                    id,
                    target
                )
            )
        elseif value < target then
            Warn(
                progress,
                string.format(
                    FG.T("Reputation %s: actual %s / %s; shortfall %s. Planned work finished; continue the guide."),
                    names[tonumber(id)] or ("#" .. id),
                    value,
                    target,
                    target - value
                )
            )
        end
    end
    progress.done = true
end

-- A reset only happens after the recorded server boundary AND a fresh supported clock.
-- Missing clock data never grants a fresh quota, even after many local calendar days.
local function ResetWindow(progress)
    local reset = C_DateAndTime and C_DateAndTime.GetSecondsUntilDailyReset or GetQuestResetTime
    local now = GetServerTime and FG.Readable(GetServerTime())
    local seconds = reset and FG.Readable(reset())
    if type(now) ~= "number" or type(seconds) ~= "number" or seconds <= 0 then return false end
    local nextReset = math.floor(now + seconds)
    if not progress.resetAt then
        progress.resetAt = nextReset
        -- Work completed with an unknown clock belongs to the observed current window.
        if progress.unknownDaily then
            progress.lastDailyReset = nextReset
            progress.unknownDaily = nil
        end
    elseif now >= progress.resetAt and nextReset > progress.resetAt then
        progress.resetAt, progress.quota = nextReset, {}
    end
    return true
end

local function Source(program, id)
    for _, source in ipairs(program.sources or {}) do
        if source.id == id then return source end
    end
end

local function QuotaKey(source) return source.reset and source.reset.quota_key or source.id end

local function QuotaUsed(progress, source)
    local key = QuotaKey(source)
    if progress.quota[key] == nil then
        local flagged = C_QuestLog and C_QuestLog.IsQuestFlaggedCompleted or IsQuestFlaggedCompleted
        local id = source.identity and source.identity.quest_id
        -- Query the active source directly: absence from an older captured daily list proves nothing.
        progress.quota[key] = id and flagged and flagged(id) and (source.reset.units_per_reset or 1) or 0
    end
    return progress.quota[key]
end

local function Allowed(source, standings)
    if source.group_policy == "unsupported" then return false end
    local eligibility = source.eligibility or {}
    if eligibility.minimum_level and UnitLevel("player") < eligibility.minimum_level then return false end
    if eligibility.maximum_level and UnitLevel("player") > eligibility.maximum_level then return false end
    for _, id in ipairs(eligibility.prerequisite_quests or {}) do
        if not FG.QuestLog.IsQuestFlaggedCompleted(id) then return false end
    end
    for _, id in ipairs(eligibility.excluded_quests or {}) do
        if FG.QuestLog.IsQuestFlaggedCompleted(id) or FG.QuestLog.IsOnQuest(id) then return false end
    end
    for _, condition in ipairs(eligibility.standing or {}) do
        local value, range = standings[condition.faction_id], condition.standing or {}
        if value == nil or range.minimum and value < range.minimum or range.maximum and value >= range.maximum then
            return false
        end
    end
    return true
end

local function InventoryMet(action)
    local any = false
    for _, target in ipairs(action.completion_targets or {}) do
        if target.kind == "inventory" then
            any = true
            if (FG.ItemCount(target.item_id, false) or 0) < target.count then return false end
        end
    end
    return any
end

local function SupportedAcquisition(action)
    local acquisition = action.acquisition or {}
    if not acquisition.item_id then return false end
    if acquisition.kind == "exact_creature_drop" then return acquisition.npc_id ~= nil end
    if acquisition.kind == "priced_vendor" then
        return acquisition.npc_id ~= nil and acquisition.unit_price_copper ~= nil
    end
    return false
end

local function ActionTargetsMet(action, standings)
    local any = false
    for _, target in ipairs(action.completion_targets or {}) do
        any = true
        if target.kind == "reputation" then
            if not standings[target.faction_id] or standings[target.faction_id] < target.standing then return false end
        elseif target.kind == "inventory" then
            if (FG.ItemCount(target.item_id, false) or 0) < target.count then return false end
        else
            return false
        end
    end
    return any
end

local function AdvanceAction(progress)
    progress.action = progress.action + 1
    progress.cycleUnits = {}
    progress.exposed = nil
end

function Rep:Poll(step, index)
    local isActive = FG.Guide and FG.Guide:CurrentStep() == index
    local program, progress = Context(step, index, isActive)
    if not program then return false end
    if progress and progress.done then return true end
    if not isActive or not progress then return false end
    local standings = LiveStandings()
    if TargetsMet(program.result and program.result.targets, standings) then
        Finish(program, progress, standings)
        return true
    end
    if program.version ~= 1 then
        Warn(progress, FG.T("Unsupported reputation programme version; planned reputation work skipped."))
        Finish(program, progress, standings)
        return true
    end
    ProgramNotices(program, progress)
    local clock = ResetWindow(progress)
    -- Each pass advances at least one action/cycle, or returns a live activity.
    for _ = 1, 1000 do
        local phase = (program.phases or {})[progress.phase]
        if not phase then
            Finish(program, progress, standings)
            return true
        end
        if progress.cycle > (phase.cycles or 0) then
            progress.phase, progress.cycle, progress.action = progress.phase + 1, 1, 1
        elseif not (phase.actions or {})[progress.action] then
            if phase.kind == "daily_circuit" then
                progress.lastDailyReset = progress.resetAt
                if not progress.resetAt then progress.unknownDaily = true end
            end
            progress.cycle, progress.action, progress.cycleUnits = progress.cycle + 1, 1, {}
        elseif
            phase.kind == "daily_circuit"
            and progress.action == 1
            and not progress.cycleStarted
            and (
                progress.unknownDaily
                or progress.lastDailyReset and (not clock or progress.lastDailyReset == progress.resetAt)
            )
        then
            progress.waiting = true
            if not clock then
                Warn(
                    progress,
                    FG.T(
                        "Daily reset time is unknown. Waiting for a verified server reset; no daily quota has been assumed."
                    )
                )
            end
            return false
        else
            progress.waiting = nil
            progress.cycleStarted = true
            local action = phase.actions[progress.action]
            local source = Source(program, action.source_id)
            local identity = source and source.identity or {}
            local units = progress.units[action.id] or 0
            local used = progress.cycleUnits[action.id] or 0
            local need = math.min(action.units_per_cycle or 0, (action.total_unit_limit or 0) - units + used)
            local daily = source and source.reset and source.reset.kind == "daily"
            local targetDone = action.kind ~= "acquire" and ActionTargetsMet(action, standings)
            if need <= 0 or used >= need or targetDone then
                if targetDone then LatchExposure(progress) end
                AdvanceAction(progress)
            elseif
                not source
                or not Allowed(source, standings)
                or action.kind == "turnin" and not identity.quest_id
                or action.kind == "kill" and not identity.npc_id
                or action.kind ~= "turnin" and action.kind ~= "kill" and action.kind ~= "acquire"
            then
                Warn(
                    progress,
                    string.format(
                        FG.T("Reputation source %s is unavailable or unsupported; skipping its planned work."),
                        action.source_id
                    )
                )
                AdvanceAction(progress)
            elseif
                daily
                and QuotaUsed(progress, source) >= (source.reset.units_per_reset or 1)
                and action.kind ~= "acquire"
            then
                Warn(
                    progress,
                    string.format(
                        FG.T(
                            "Daily reputation source %s has no remaining quota in this server reset; this opportunity was skipped."
                        ),
                        source.name or source.id
                    )
                )
                AdvanceAction(progress)
            elseif action.kind == "acquire" then
                if InventoryMet(action) then
                    progress.units[action.id] = units + need
                    AdvanceAction(progress)
                elseif not SupportedAcquisition(action) then
                    Warn(
                        progress,
                        string.format(
                            FG.T("Reputation materials for %s are unavailable; skipping this source."),
                            source.name or source.id
                        )
                    )
                    progress.blocked = progress.blocked or {}
                    progress.blocked[source.id] = true
                    AdvanceAction(progress)
                else
                    return false
                end
            elseif progress.blocked and progress.blocked[source.id] then
                AdvanceAction(progress)
            else
                ExposeTurnin(progress, action, source)
                return false
            end
        end
        -- A completed cycle never carries its 'started' bit into another circuit or phase.
        if progress.action == 1 then progress.cycleStarted = nil end
    end
    Warn(
        progress,
        FG.T("Reputation programme is too large to advance in one update; remaining work will be checked again.")
    )
    return false
end

local function ProgramStates(each)
    local guide = FG.Guide
    local route = guide and guide:Route()
    for index, step in ipairs(route and route.steps or {}) do
        if step.k == "reputation" then
            local _, progress = Context(step, index, false)
            if progress and each(progress) then return true end
        end
    end
    return false
end

-- Explicit navigation and route changes end the ambiguous event window. Recorded work stays.
function Rep:Deactivate()
    ProgramStates(function(progress)
        progress.exposed, progress.pending = nil, nil
    end)
end

local function CountPending(id)
    return ProgramStates(function(progress)
        for index, pending in ipairs(progress.pending or {}) do
            if pending.questID == id then
                table.remove(progress.pending, index)
                local units = progress.units[pending.actionID] or 0
                if units < pending.unitIndex and pending.unitIndex <= pending.totalLimit then
                    progress.units[pending.actionID] = units + 1
                    if
                        progress.phase == pending.phase
                        and progress.cycle == pending.cycle
                        and progress.action == pending.actionIndex
                    then
                        progress.cycleUnits[pending.actionID] = (progress.cycleUnits[pending.actionID] or 0) + 1
                    end
                    -- The completion belongs to the captured server window, never a fresh quota.
                    if pending.daily and pending.resetAt == progress.resetAt then
                        progress.quota[pending.quotaKey] = (progress.quota[pending.quotaKey] or 0) + 1
                    end
                    return true
                end
            end
        end
    end)
end

local function NewQuestInteraction(id)
    id = FG.Readable(id)
    if type(id) ~= "number" then return end
    ProgramStates(function(progress)
        for index = #(progress.pending or {}), 1, -1 do
            if progress.pending[index].questID == id then table.remove(progress.pending, index) end
        end
    end)
end
FG:On("QUEST_DETAIL", function() NewQuestInteraction(GetQuestID()) end)
FG:On("QUEST_ACCEPTED", function(first, second) NewQuestInteraction(second or first) end)

local function ActiveSnapshot()
    local guide = FG.Guide
    local route, index = guide and guide:Route(), guide and guide:CurrentStep()
    local step = route and route.steps[index]
    if not step or step.k ~= "reputation" then return end
    local program, progress = Context(step, index, false)
    if not program or not progress or progress.done then return end
    local phase = (program.phases or {})[progress.phase]
    local action = phase and (phase.actions or {})[progress.action]
    return {
        program = program,
        state = progress,
        phase = phase,
        action = action,
        source = action and Source(program, action.source_id),
        waiting = progress.waiting,
    }
end

function Rep:Active()
    local guide = FG.Guide
    local route, index = guide and guide:Route(), guide and guide:CurrentStep()
    local step = route and route.steps[index]
    if not step or step.k ~= "reputation" or self:Poll(step, index) then return end
    return ActiveSnapshot()
end

function Rep:WantsQuest(id)
    local active = self:Active()
    return active ~= nil
            and not active.waiting
            and active.action
            and active.action.kind == "turnin"
            and active.source
            and active.source.identity
            and active.source.identity.quest_id == id
        or false
end

-- Once a programme's counted deliveries end, a still-open reward panel cannot take more.
-- Ordinary quest lifecycles keep their existing behaviour, including shared setup quest IDs.
function Rep:AutomationAllowed(id)
    local guide = FG.Guide
    local route = guide and guide:Route()
    local current = route and route.steps[guide:CurrentStep()]
    if current and current.k == "reputation" then return self:WantsQuest(id) end
    if current and current.q == id then return true end
    for _, step in ipairs(route and route.steps or {}) do
        for _, source in ipairs(step.rp and step.rp.sources or {}) do
            if source.identity and source.identity.quest_id == id then return false end
        end
    end
    return true
end

local function Count(kind, id)
    if kind == "turnin" and CountPending(id) then return true end
    -- The reward may update standing or daily flags before QUEST_TURNED_IN. Attribute
    -- its event to the saved run before polling can advance to another run.
    local active = ActiveSnapshot()
    if not active or active.waiting or not active.source or active.action.kind ~= kind then return end
    local identity = active.source.identity or {}
    if (kind == "turnin" and identity.quest_id or identity.npc_id) ~= id then return end
    local progress, action = active.state, active.action
    if
        (progress.units[action.id] or 0) >= (action.total_unit_limit or 0)
        or (progress.cycleUnits[action.id] or 0) >= (action.units_per_cycle or 0)
    then
        return
    end
    progress.units[action.id] = (progress.units[action.id] or 0) + 1
    progress.cycleUnits[action.id] = (progress.cycleUnits[action.id] or 0) + 1
    if active.source.reset and active.source.reset.kind == "daily" then
        local key = QuotaKey(active.source)
        progress.quota[key] = (progress.quota[key] or 0) + 1
    end
    return true
end

FG:On("QUEST_TURNED_IN", function(id) Count("turnin", FG.Readable(id)) end)
FG:On("PARTY_KILL", function(attacker, target)
    attacker, target = FG.Readable(attacker), FG.Readable(target)
    if not attacker or attacker ~= FG.Readable(UnitGUID("player")) and attacker ~= FG.Readable(UnitGUID("pet")) then
        return
    end
    local _, npc = FG.ParseGUID(target)
    if Count("kill", npc) and FG.Guide then FG.Guide:Advance() end
end)
-- Faction gains can arrive before QUEST_TURNED_IN. Reconcile standings on the guide's
-- normal ticker (or its quest/kill events), after attributing that completion to its run.

function Rep:Presentation(step, index)
    if not step or step.k ~= "reputation" or not FG.Guide or FG.Guide:CurrentStep() ~= index then return step end
    local active = self:Active()
    if not active or not active.action or not active.source then return step end
    local action, source = active.action, active.source
    local identity = source.identity or {}
    local location = action.location or action.acquisition and action.acquisition.location or source.location or {}
    local out = {}
    for k, v in pairs(step) do
        out[k] = v
    end
    out.m, out.x, out.y = location.map_id, location.map_x, location.map_y
    if not out.x or not out.y then out.m = nil end
    out.id = action.kind == "acquire" and action.acquisition and action.acquisition.npc_id
        or action.kind == "turnin" and (identity.ender_npc_id or identity.giver_npc_id)
        or identity.npc_id
    out.n = action.kind == "kill" and identity.npc_name
        or action.kind == "turnin" and (identity.ender_npc_name or identity.giver_npc_name)
    if action.kind == "acquire" and action.acquisition then
        out.n = action.acquisition.npc_name
            or (action.acquisition.npc_id == identity.npc_id and identity.npc_name or nil)
    end
    out.e = out.n and "npc" or nil
    if active.waiting then
        out.t, out.m, out.e = FG.T("Wait for the next verified daily reset"), nil, nil
    else
        local label = action.kind == "acquire" and FG.T("Gather")
            or action.kind == "kill" and FG.T("Kill")
            or FG.T("Turn in")
        out.t = label .. " " .. (source.name or source.id)
        if source.confidence == "classic_estimate" then out.t = out.t .. " (" .. FG.T("Classic estimate") .. ")" end
    end
    return out
end

function Rep:View(step, index)
    local active = self:Active()
    if not active or not active.action then return self:Presentation(step, index).t end
    local action, progress = active.action, active.state
    local have, need = progress.units[action.id] or 0, action.total_unit_limit or 0
    if action.kind == "acquire" then
        for _, target in ipairs(action.completion_targets or {}) do
            if target.kind == "inventory" then
                have, need = FG.ItemCount(target.item_id, false) or 0, target.count
                break
            end
        end
    end
    local sub = string.format(FG.T("Cycle %d/%d"), progress.cycle, active.phase.cycles or 1)
    if active.waiting then sub = FG.T("Actual server reset required; calendar waiting is not active play time.") end
    return self:Presentation(step, index).t, sub, have, need
end
