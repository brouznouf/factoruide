-- How the guide takes the character to the place it points at, besides the compass's straight
-- line:
--   the walk the guide planned to the step (`wp`: where it starts, then where it turns around
--   the steep ground, world coordinates), shown on the compass as the way to go while the
--   character is close to it (`pathMode`);
--   on WoW Forever, Shortest Path Forever when it is loaded (`spfLink`): it gets the place and
--   guides there with its own lines, arrow and the game's waypoint (ours is left to it). A
--   journey the player replaces or clears there is not sent again until the place changes or is
--   picked again on the compass.

local FG = Factoruide

local Pathing = {}
FG.Pathing = Pathing

local OWNER = "Factoruide"
-- On the walk closer than this (yards); off it beyond REJOIN, where it is not shown any more.
-- The compass points AHEAD yards further along the walk than its point closest to the character.
local CORRIDOR, REJOIN, AHEAD = 45, 220, 30
-- Objective areas: Shortest Path Forever stays on them (held) and stops guiding inside this many
-- yards.
local AREA = 50
-- Its stop kinds, for the guide's step kinds and the compass's places.
local KINDS = {
    accept = "pickup",
    turnin = "turnin",
    objective = "objective",
    grind = "objective",
    train = "trainer",
    sell = "vendor",
    buy = "vendor",
    bind = "innkeeper",
    hearth = "innkeeper",
    flight_master = "flightmaster",
    fly = "flightmaster",
    dungeon = "dungeon",
    class = "trainer",
    profession = "trainer",
    flight = "flightmaster",
    inn = "innkeeper",
}

local function API()
    local spf = ShortestPathForever
    local api = type(spf) == "table" and spf.API
    if type(api) == "table" and type(api.NavigateRoute) == "function" then return api end
    return nil
end

--- Whether Shortest Path Forever is loaded and the setting sends it the guide's places.
function Pathing:SpfLinked() return FG:Get("spfLink") == true and API() ~= nil end

--- Whether the compass shows the guide's walk ("auto": unless Shortest Path Forever guides).
function Pathing:PathShown()
    local mode = FG:Get("pathMode")
    if mode == "on" then return true end
    if mode == "off" then return false end
    return not self:SpfLinked()
end

-- The walk ---------------------------------------------------------------------------------

-- { continent, points = { { north, west }, ..., the place } } (the place alone when the walk
-- goes straight), the place it leads to, and the leg reached (only goes on: a walk going back on
-- itself is not pointed backwards).
local walk, walkPlace, reached

--- The closest point of the leg from `a` to `b` to (`n`, `w`), and how far it is.
local function Project(a, b, n, w)
    local dn, dw = b[1] - a[1], b[2] - a[2]
    local length = dn * dn + dw * dw
    local t = length > 0 and math.max(0, math.min(1, ((n - a[1]) * dn + (w - a[2]) * dw) / length)) or 0
    local qn, qw = a[1] + t * dn, a[2] + t * dw
    return qn, qw, math.sqrt((n - qn) ^ 2 + (w - qw) ^ 2)
end

--- The walk to `place`: the step's planned one when the place is the step's own, else straight.
local function SetWalk(place, step)
    if walkPlace == place then return end
    walk, walkPlace, reached = nil, place, 1
    if not place or not place.m or not place.x then return end
    local continent, north, west = FG.Compass.World(place.m, place.x / 100, place.y / 100)
    if not continent then return end
    local points = {}
    if step and place == step and step.wp and #step.wp >= 2 then
        for i = 1, #step.wp - 1, 2 do
            points[#points + 1] = { step.wp[i], step.wp[i + 1] }
        end
    end
    points[#points + 1] = { north, west }
    walk = { continent = continent, points = points }
end

--- The leg of the walk closest to (`north`, `west`) from the one reached on, the closest point on
--- it and how far it is; nil when the walk goes straight.
local function Closest(north, west)
    local points = walk.points
    local best, bestDistance, bestN, bestW
    for k = reached, #points - 1 do
        local qn, qw, d = Project(points[k], points[k + 1], north, west)
        if not bestDistance or d < bestDistance then
            best, bestDistance, bestN, bestW = k, d, qn, qw
        end
    end
    return best, bestDistance, bestN, bestW
end

--- Where the compass shows the way for a character at (`north`, `west`) of `continent`: AHEAD
--- yards along the walk from its closest point; nil without a walk, off it, or on its last leg
--- (straight to the place). Second value: true when the character is beside the walk, going
--- back to it.
function Pathing:Aim(continent, north, west)
    if not walk or walk.continent ~= continent or not self:PathShown() then return nil end
    local points = walk.points
    local best, bestDistance, bestN, bestW = Closest(north, west)
    if not best or bestDistance > REJOIN then return nil end
    reached = best
    if best == #points - 1 then return nil end
    local n, w, left, k = bestN, bestW, AHEAD, best
    while left > 0 and k < #points do
        local to = points[k + 1]
        local leg = math.sqrt((to[1] - n) ^ 2 + (to[2] - w) ^ 2)
        if leg >= left then
            n, w, left = n + (to[1] - n) * left / leg, w + (to[2] - w) * left / leg, 0
        else
            n, w, left, k = to[1], to[2], left - leg, k + 1
        end
    end
    return n, w, bestDistance > CORRIDOR
end

--- The walk left to the place for a character at (`north`, `west`) of `continent`, to draw it:
--- { { north, west }, ... } from the character (or from the walk's closest point when on it) to
--- the place; the whole walk left when far from it; nil without a walk or with it not shown.
function Pathing:Remaining(continent, north, west)
    if not walk or walk.continent ~= continent or not self:PathShown() then return nil end
    local points = walk.points
    local out = {}
    local best, bestDistance, bestN, bestW = Closest(north, west)
    if not best then
        out[1] = { north, west }
    elseif bestDistance > REJOIN then
        for k = reached, #points - 1 do
            out[#out + 1] = points[k]
        end
    else
        if bestDistance > CORRIDOR then out[1] = { north, west } end
        out[#out + 1] = { bestN, bestW }
        for k = best + 1, #points - 1 do
            out[#out + 1] = points[k]
        end
    end
    out[#out + 1] = points[#points]
    return out
end

-- Shortest Path Forever -------------------------------------------------------------------

-- The place sent (its key), the one the player replaced or cleared there (not sent again), and
-- the subscription to its changes.
local sent, refused, subscribed

local function Key(place) return place.m .. ":" .. place.x .. ":" .. place.y end

local function Changed()
    local api = API()
    if not api or not sent or (api.CurrentStop and api.CurrentStop(OWNER)) then return end
    local why = api.Ended and api.Ended(OWNER)
    -- Arrived: not sent again either, while the place stays the same.
    if why == "replaced" or why == "cleared" then refused = sent end
end

local function Cancel(api)
    if sent and api and api.Cancel then api.Cancel(OWNER) end
    sent = nil
end

-- The NPC standing at the place pointed at.
local targetNpc

--- The place the guide points at (nil: none), with what stands there: { title, kind (a step's
--- or a compass place's), q (its quest), npc (its NPC ID) }.
function Pathing:SetTarget(place, step, info)
    SetWalk(place, step)
    targetNpc = place and info and info.npc or nil
    if FG.NpcMark then FG.NpcMark:SetNpc(targetNpc) end
    local api = self:SpfLinked() and API()
    if not api then return end
    if not subscribed and api.Subscribe then
        subscribed = true
        api.Subscribe(Changed)
    end
    if not place or not place.m or not place.x then
        Cancel(api)
        return
    end
    local key = Key(place)
    if key == sent or key == refused then return end
    info = info or {}
    local stop = { map = place.m, x = place.x / 100, y = place.y / 100, title = info.title, kind = KINDS[info.kind] }
    if info.kind == "objective" or info.kind == "grind" then
        stop.hold, stop.radius = true, AREA
        if info.q then stop.questID = info.q end
    end
    -- Refused in combat, without a position or with its journeys turned off: tried again at the
    -- next refresh.
    if api.NavigateRoute(OWNER, { stop }) then sent = key end
end

--- The NPC standing at the place pointed at (nil: none, or not an NPC).
function Pathing:TargetNpc() return targetNpc end

--- Whether Shortest Path Forever guides to the guide's place now (the game's waypoint is its).
function Pathing:Guiding()
    local api = self:SpfLinked() and API()
    return api ~= nil and sent ~= nil and (not api.CurrentStop or api.CurrentStop(OWNER) ~= nil)
end

--- The place is picked by hand (a click on the compass): sent again, even refused before.
function Pathing:Forget()
    sent, refused = nil, nil
end

FG:OnSetting("spfLink", function(on)
    if not on then Cancel(API()) end
    refused = nil
    if FG.Guide then FG.Guide:Refresh() end
end)
FG:OnSetting("pathMode", function()
    if FG.Guide then FG.Guide:Refresh() end
end)
