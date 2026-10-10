-- Leveling time: the real play time on reaching each level (from /played), compared with the
-- guide's plan (`levels` of the route). Shown in the guide header; `/fg time` for the detail
-- and the estimated end.

local _, FG = ...

local fr = GetLocale() == "frFR"

local Timing = { played = nil, at = nil }
FG.Timing = Timing

local requested = false

-- Asking /played prints it in the chat: hide our own requests.
if ChatFrame_DisplayTimePlayed then
    local display = ChatFrame_DisplayTimePlayed
    ChatFrame_DisplayTimePlayed = function(...)
        if not requested then display(...) end
    end
end

local function Request()
    if RequestTimePlayed then
        requested = true
        RequestTimePlayed()
    end
end

local function Times()
    local state = FG.Guide and FG.Guide:State()
    if not state then return nil end
    state.levelTimes = state.levelTimes or {}
    return state.levelTimes
end

FG:On("TIME_PLAYED_MSG", function(total, thisLevel)
    requested = false
    Timing.played, Timing.at = total, GetTime()
    local times = Times()
    if times then times[UnitLevel("player")] = times[UnitLevel("player")] or (total - thisLevel) end
    if FG.Guide then FG.Guide:Refresh() end
end)

FG:On("PLAYER_ENTERING_WORLD", function(login, reload)
    if login or reload then
        if C_Timer then
            C_Timer.After(3, Request)
        else
            Request()
        end
    end
end)
FG:On("PLAYER_LEVEL_UP", function()
    if C_Timer then
        C_Timer.After(1, Request)
    else
        Request()
    end
end)

local function Duration(seconds)
    local m = math.floor(math.abs(seconds) / 60 + 0.5)
    if m < 60 then return m .. " min" end
    return string.format("%dh%02d", math.floor(m / 60), m % 60)
end

--- Real and planned play time on reaching the current level, when both are known.
function Timing:Compare()
    local route = FG.Guide and FG.Guide:Route()
    local times = Times()
    local level = UnitLevel("player")
    if not route or not route.levels or not times or route.fromLevel ~= 1 then return nil end
    local real, planned = times[level], route.levels[level]
    if not real or not planned or planned <= 0 then return nil end
    return real, planned
end

--- " · −12 min" (ahead of the plan, green) or " · +15 min" (behind, red) for the guide header.
function Timing:Short()
    if not FG:Get("levelTiming") then return "" end
    local real, planned = self:Compare()
    if not real then return "" end
    local delta = real - planned
    local color = delta <= 0 and "40ff40" or "ff6060"
    return string.format("  |cff%s%s%s|r", color, delta <= 0 and "−" or "+", Duration(delta))
end

--- `/fg time`: time per level against the plan, and the estimated end.
function Timing:Print()
    local route = FG.Guide and FG.Guide:Route()
    local times = Times() or {}
    if not route or not route.levels then
        FG:Print(fr and "pas de guide" or "no guide")
        return
    end
    for level = 2, UnitLevel("player") do
        local real, planned = times[level], route.levels[level]
        if real and planned then
            FG:Print(
                "%s %d : %s (%s %s, %s%s)",
                fr and "niveau" or "level",
                level,
                Duration(real),
                fr and "prévu" or "planned",
                Duration(planned),
                real <= planned and "−" or "+",
                Duration(real - planned)
            )
        end
    end
    local real, planned = self:Compare()
    if real then
        FG:Print(
            fr and "fin estimée : %s (prévu %s)" or "estimated end: %s (planned %s)",
            Duration(route.time * real / planned),
            Duration(route.time)
        )
    end
end
