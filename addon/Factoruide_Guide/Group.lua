-- Group guides: one guide for several players (the route's `players`), each following it with
-- its own addon. The players in the party tell each other where they are (step, level, XP) and
-- what they marked done by hand:
--   a step marked done is passed only once every player in the party marked it too;
--   a step for some players only (`mb`: a quest the others did already, players catching up)
--   waits for them, the others come along;
--   the guide shows who is on another step, and when the levels are too far apart.

local FG = Factoruide
local T = FG.T

local PREFIX = "FGGRP1"
-- Steps sent again this often (seconds), and a player counts as gone after this long silent.
local REFRESH = 30
local GONE = 120
-- Message sizes are 255 bytes at most.
local CHUNK = 230
-- Results of C_ChatInfo.SendAddonMessage (Enum.SendAddonMessageResult) worth trying again:
-- AddonMessageThrottle, ChannelThrottle, AddOnMessageLockdown (in combat in an instance).
local RETRY = { [3] = true, [8] = true, [11] = true }

local L = {
    forOthers = T("for %s"),
    skipWaits = T("marked done, waiting for %s"),
    waits = T("done, waiting for %s"),
    behind = T("%s: step %d"),
    ahead = T("%s ahead: step %d"),
    apart = T("levels %.1f apart: make the guide again in the app"),
    otherVersion = T("%s follows another version of the guide"),
    train = T("Train at your class trainer"),
    choose = T("Several players of this group guide are of your race and class: choose yours in /fg, Guides"),
}

local Group = { others = {} }
FG.Group = Group

if C_ChatInfo and C_ChatInfo.RegisterAddonMessagePrefix then C_ChatInfo.RegisterAddonMessagePrefix(PREFIX) end

local route -- the guide followed, as written (the original of the adapted copy)
local me -- our place in route.players (0-based), nil when not a group guide or not in it
local version -- "time:steps" of the guide
local state -- the guide state (step, marks, skippedQuests)
local sent = {} -- what was sent last: step, level, xp, time

--- "Name-Realm" of a message's sender (the realm is left out for ours on some clients).
local function Key(sender)
    if sender:find("-", 1, true) then return sender end
    return sender .. "-" .. GetNormalizedRealmName()
end

--- The name players see (without the realm).
local function Short(key) return (key:gsub("%-.*", "")) end

--- Whether the player of `key` is in our party now.
local function InParty(key)
    if not UnitInParty then return true end
    local name = Ambiguate and Ambiguate(key, "none") or Short(key)
    return UnitInParty(name) or UnitInParty(Short(key)) or false
end

--- The trainer of our class closest to where the guide trains (a train step is the class
--- trainer of the player planned for): FG.Services entry or nil.
local function OwnTrainer(step, class, faction)
    local from
    for _, p in ipairs(FG.Services or {}) do
        if p[1] == "class" and step.id and p[11] == step.id then from = p end
    end
    local best, bestD
    for _, p in ipairs(FG.Services or {}) do
        if p[1] == "class" and p[3] == class and p[2]:find(faction, 1, true) then
            local d
            if from and p[4] == from[4] then
                d = (p[5] - from[5]) ^ 2 + (p[6] - from[6]) ^ 2
            elseif p[7] == step.m then
                d = (p[8] - step.x) ^ 2 + (p[9] - step.y) ^ 2
            end
            if d and (not bestD or d < bestD) then
                best, bestD = p, d
            end
        end
    end
    return best
end

--- The guide as this player follows it: the levels and XP planned for it (`gp`), its own class
--- trainer, no skipping ahead to a checkpoint alone.
local function Adapt(r)
    local class = select(3, UnitClass("player"))
    local own = class == r.class
    local faction = UnitFactionGroup("player") == "Horde" and "H" or "A"
    -- Our class's spells: the guide's own, or those it lists for the group's other classes.
    local spells = own and r.spells or (r.classSpells and r.classSpells[class]) or {}
    -- Our own professions (guides made before they were per player: the guide's).
    local player = r.players[me + 1]
    local professions = player and player.professions or r.professions
    local copy = setmetatable(
        { steps = {}, checkpoints = {}, spells = spells, professions = professions },
        { __index = r }
    )
    for i, s in ipairs(r.steps) do
        local c = {}
        for k, v in pairs(s) do
            c[k] = v
        end
        local level, xp = s.gp and s.gp[2 * me + 1], s.gp and s.gp[2 * me + 2]
        if level then
            c.lvl, c.px = level, xp
            if s.k == "grind" and s.xp then c.xp = xp end
        end
        if s.k == "train" and not own then
            local p = OwnTrainer(s, class, faction)
            c.sp, c.t = nil, L.train
            if p then
                c.m, c.x, c.y, c.n, c.id, c.e = p[7], p[8], p[9], p[10], p[11] ~= 0 and p[11] or nil, "npc"
                c.wp = nil
            end
        end
        copy.steps[i] = c
    end
    return copy
end

--- The players of guide `r` this character can be (0-based places in `r.players`): the one of
--- its name, else those of its race and class (a character the app had no profile of).
function Group:Candidates(r)
    local out = {}
    if not r or not r.players then return out end
    local key = FG:CharacterKey()
    for i, p in ipairs(r.players) do
        if p.n == key then return { i - 1 } end
    end
    local _, _, race = UnitRace("player")
    local _, _, class = UnitClass("player")
    race, class = FG.Readable(race), FG.Readable(class)
    for i, p in ipairs(r.players) do
        if race and class and p.race == race and p.class == class then table.insert(out, i - 1) end
    end
    return out
end

--- Follow guide `r` (nil: none) with guide state `s`: the guide as this player follows it.
--- Several players to be: the one chosen in the options (`s.player`, kept per guide).
function Group:Use(r, s)
    route, state, me, version = r, s, nil, nil
    self.choices = nil
    wipe(self.others)
    sent = {}
    if not r or not r.players then return r end
    local candidates = self:Candidates(r)
    if #candidates == 1 then
        me = candidates[1]
    elseif #candidates > 1 then
        self.choices = candidates
        for _, k in ipairs(candidates) do
            if s and s.player == k then me = k end
        end
    end
    if not me then return r end
    version = string.format("%s:%d", tostring(r.time), #r.steps)
    return Adapt(r)
end

--- Whether a group guide is followed, by one of its players.
function Group:Active() return me ~= nil end

--- The players this character can be in the guide followed, when several (nil otherwise), and
--- the one it is (0-based).
function Group:Choices() return self.choices, me end

--- The name players see of the player at place `k` (0-based) of the guide followed.
function Group:PlayerName(k)
    local p = route and route.players and route.players[k + 1]
    return p and Short(p.n) or "?"
end

--- This character is the player at place `k` (0-based) of the guide followed.
function Group:Choose(k)
    if not state then return end
    state.player = k
    FG.Guide:FollowChoice()
    FG.Options:Refresh()
end

--- The other players in the party following the same guide: { key, name, bit, step, level,
--- xp, skips, marks }.
function Group:Present()
    local out = {}
    if not me then return out end
    local now = GetTime()
    for key, o in pairs(self.others) do
        if o.version == version and o.bit and now - o.seen < GONE and InParty(key) then table.insert(out, o) end
    end
    table.sort(out, function(a, b) return a.bit < b.bit end)
    return out
end

--- Whether a step is for this player (every step outside a group guide).
function Group:ForMe(step) return not me or not step.mb or bit.band(step.mb, bit.lshift(1, me)) ~= 0 end

--- A step marked done by hand is passed once every player in the party marked it too.
function Group:SkipShared(step, index)
    for _, o in ipairs(self:Present()) do
        if not (o.marks[index] or (step.q and o.skips[step.q])) then return false end
    end
    return true
end

--- Names of the players of the party a step is for who have not passed it yet.
function Group:Waiting(step, index)
    local out = {}
    for _, o in ipairs(self:Present()) do
        local mine = not step.mb or bit.band(step.mb, bit.lshift(1, o.bit)) ~= 0
        if mine and (o.step or 0) <= index then table.insert(out, o.name) end
    end
    return out
end

--- A step for other players only: passed once those of them in the party are past it.
function Group:Passed(step, index) return #self:Waiting(step, index) == 0 end

--- Level and XP of the player of place `bit` (0-based) when step `step` is done.
local function Planned(step, b) return step.gp and step.gp[2 * b + 1], step.gp and step.gp[2 * b + 2] end

local function Reached(level, xp, wantLevel, wantXP)
    return level > wantLevel or (level == wantLevel and xp >= (wantXP or 0))
end

--- Players catching up (a grind step for some of them): done once each one in the party is
--- where the guide plans it after the step.
function Group:CaughtUp(step)
    if self:ForMe(step) then
        local level, xp = Planned(step, me)
        if level and not Reached(UnitLevel("player"), UnitXP("player"), level, xp) then return false end
    end
    for _, o in ipairs(self:Present()) do
        local level, xp = Planned(step, o.bit)
        if bit.band(step.mb, bit.lshift(1, o.bit)) ~= 0 and level and o.level then
            if not Reached(o.level, o.xp or 0, level, xp) then return false end
        end
    end
    return true
end

--- How far apart in levels this player and those in the party are.
function Group:Spread()
    local function at(level, xp, max) return level + (max and max > 0 and xp / max or 0) end
    local own = at(UnitLevel("player"), UnitXP("player"), UnitXPMax("player"))
    local lo, hi = own, own
    for _, o in ipairs(self:Present()) do
        if o.level then
            local l = at(o.level, o.xp or 0, o.max)
            lo, hi = math.min(lo, l), math.max(hi, l)
        end
    end
    return hi - lo
end

--- Chips of a step of the current tasks: who it is for, who is still on it, who is elsewhere
--- (`done`: we did it).
function Group:Chips(step, index, chip, done)
    if self.choices and not me and state and index == state.step then chip(L.choose, "orange") end
    if not me then return end
    if done and step.k ~= "objective" and self:ForMe(step) then
        local waiting = self:Waiting(step, index)
        if #waiting > 0 then chip(string.format(L.waits, table.concat(waiting, ", ")), "orange") end
    end
    if not self:ForMe(step) then
        local names = {}
        for i, p in ipairs(route.players) do
            if bit.band(step.mb, bit.lshift(1, i - 1)) ~= 0 then table.insert(names, Short(p.n)) end
        end
        chip(string.format(L.forOthers, table.concat(names, ", ")), "lightBlue")
    end
    local marked = state and (state.marks[index] == "skip" or (step.q and state.skippedQuests[step.q]))
    if marked then
        local waiting = {}
        for _, o in ipairs(self:Present()) do
            if not (o.marks[index] or (step.q and o.skips[step.q])) then table.insert(waiting, o.name) end
        end
        if #waiting > 0 then chip(string.format(L.skipWaits, table.concat(waiting, ", ")), "orange") end
    end
    if state and index == state.step then
        for _, o in ipairs(self:Present()) do
            if o.step and o.step < index then
                chip(string.format(L.behind, o.name, o.step), "orange")
            elseif o.step and o.step > index then
                chip(string.format(L.ahead, o.name, o.step), "lightBlue")
            end
        end
        local now = GetTime()
        for key, o in pairs(self.others) do
            if o.version ~= version and now - o.seen < GONE and InParty(key) then
                chip(string.format(L.otherVersion, o.name), "red")
            end
        end
        local spread = self:Spread()
        if spread > (route.syncMax or 1) then chip(string.format(L.apart, spread), "red") end
    end
end

-- Messages ------------------------------------------------------------------------------------

local queue = {}
local flushing = false

local function Flush()
    flushing = false
    while #queue > 0 do
        local result = C_ChatInfo.SendAddonMessage(PREFIX, queue[1], "PARTY")
        -- Refused for now (throttled, or in combat in an instance): again a little later.
        if type(result) == "number" and RETRY[result] then
            if not flushing and C_Timer then
                flushing = true
                C_Timer.After(5, Flush)
            end
            return
        end
        table.remove(queue, 1)
    end
end

local function Send(message)
    if not C_ChatInfo or not C_ChatInfo.SendAddonMessage or not IsInGroup() or IsInRaid() then return end
    table.insert(queue, message)
    if not flushing then Flush() end
end

--- A list of numbers in messages of their own: "<kind>:<version>:<n>:<a,b,c>", n = 1 first.
local function SendList(kind, values)
    local chunk, n = {}, 1
    local function out()
        Send(string.format("%s:%s:%d:%s", kind, version, n, table.concat(chunk, ",")))
        chunk, n = {}, n + 1
    end
    local size = 0
    for _, v in ipairs(values) do
        local s = tostring(v)
        if size + #s + 1 > CHUNK then
            out()
            size = 0
        end
        table.insert(chunk, s)
        size = size + #s + 1
    end
    if #chunk > 0 or n == 1 then out() end
end

local own -- the first step we have not done ourselves (`Guide:OwnStep`)

--- Where this player is: "S:<version>:<step>:<level>:<xp>:<max>", its step the first one it
--- has not done itself (not the one the guide waits on for the others: they would wait on it).
local function SendStatus()
    if not me or not state then return end
    local level, xp, max = UnitLevel("player"), UnitXP("player"), UnitXPMax("player")
    local step = own or state.step
    sent.step, sent.level, sent.xp, sent.time = step, level, xp, GetTime()
    Send(string.format("S:%s:%d:%d:%d:%d", version, step, level, xp, max))
end

--- What this player marked done by hand: the quests, the other steps.
local function SendMarks()
    if not me or not state then return end
    local quests, steps = {}, {}
    for q, on in pairs(state.skippedQuests) do
        if on then table.insert(quests, q) end
    end
    for i, mark in pairs(state.marks) do
        if mark == "skip" then table.insert(steps, i) end
    end
    table.sort(quests)
    table.sort(steps)
    SendList("Q", quests)
    SendList("M", steps)
end

--- Everything again (a player joined, or asked).
local function SendAll()
    SendStatus()
    SendMarks()
end

--- The marks changed (a step marked done by hand, or undone).
function Group:MarksChanged() SendMarks() end

--- Called every second by the guide with the first step we have not done ourselves: tells the
--- party when it or the XP changed.
function Group:Tick(ownStep)
    own = ownStep
    if not me or not state or not IsInGroup() then return end
    if
        sent.step ~= (own or state.step)
        or sent.level ~= UnitLevel("player")
        or sent.xp ~= UnitXP("player")
        or GetTime() - (sent.time or 0) > REFRESH
    then
        SendStatus()
    end
end

local function Other(key)
    local o = Group.others[key]
    if not o then
        o = { key = key, name = Short(key), skips = {}, marks = {} }
        Group.others[key] = o
        for i, p in ipairs(route and route.players or {}) do
            if p.n == key then o.bit = i - 1 end
        end
    end
    o.seen = GetTime()
    return o
end

local function Receive(message, sender)
    local key = Key(sender)
    if not me or key == FG:CharacterKey() then return end
    local kind, rest = message:match("^(%a):(.*)$")
    if kind == "H" then
        SendAll()
        return
    end
    if not rest then return end
    local time, steps, a, b, c, d = rest:match("^([^:]+):(%d+):([^:]*):?([^:]*):?([^:]*):?([^:]*)$")
    if not time then return end
    local o = Other(key)
    o.version = time .. ":" .. steps
    if kind == "S" then
        o.step, o.level, o.xp, o.max = tonumber(a), tonumber(b), tonumber(c), tonumber(d)
    elseif kind == "Q" or kind == "M" then
        local list = kind == "Q" and "skips" or "marks"
        if tonumber(a) == 1 then o[list] = {} end
        for v in b:gmatch("%d+") do
            o[list][tonumber(v)] = true
        end
    end
    if FG.Guide then FG.Guide:Advance() end
end

FG:On("CHAT_MSG_ADDON", function(prefix, message, _, sender)
    if prefix ~= PREFIX then return end
    Receive(message, sender)
end)

FG:On("GROUP_ROSTER_UPDATE", function()
    if not me then return end
    for key in pairs(Group.others) do
        if not InParty(key) then Group.others[key] = nil end
    end
    -- Someone may have joined: everyone says where they are.
    Send("H:")
    SendAll()
end)

FG:On("PLAYER_ENTERING_WORLD", function()
    if me and IsInGroup() then
        Send("H:")
        SendAll()
    end
end)
