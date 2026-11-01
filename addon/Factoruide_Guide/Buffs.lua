-- Buffs (WoW Forever): a guide made with the food or Sleeping Bag XP bonus counts on it being
-- active (`route.buffs`). A warning, now and then, while one is missing.

local FG = Factoruide
local T = FG.T

-- Well Fed's XP boost, and the Well-Rested the Sleeping Bag stacks.
local AURAS = { food = 1243969, bag = 429959 }
local WARN_EVERY = 600

local warned = {}

--- Whether the player has aura `spellId`: nil when the game does not tell (secret values).
local function Has(spellId)
    if not (C_UnitAuras and C_UnitAuras.GetPlayerAuraBySpellID) then return nil end
    local ok, aura = pcall(C_UnitAuras.GetPlayerAuraBySpellID, spellId)
    if not ok or (issecretvalue and issecretvalue(aura)) then return nil end
    return aura ~= nil
end

local function Warn(text)
    if not FG:Get("uiShown") then return end
    if UIErrorsFrame then UIErrorsFrame:AddMessage(text, 1, 0.82, 0.2) end
    FG:Print("%s", text)
end

--- Warns about each buff of the followed guide the player lacks (not in combat, not twice within
--- ten minutes).
function FG:CheckBuffs()
    local route = FG.Guide and FG.Guide:Route()
    if not (route and route.buffs) or InCombatLockdown() or UnitIsDeadOrGhost("player") then return end
    for _, key in ipairs(route.buffs) do
        local spellId = AURAS[key]
        if spellId and Has(spellId) == false and (not warned[key] or GetTime() - warned[key] > WARN_EVERY) then
            warned[key] = GetTime()
            Warn(
                key == "food" and T("The guide counts on a food XP bonus: eat to get the Well Fed buff.")
                    or T("The guide counts on the Sleeping Bag XP bonus: sleep on it to get the Well-Rested buff.")
            )
        end
    end
end

-- The auras are not all known right at the loading screen.
FG:On("PLAYER_ENTERING_WORLD", function()
    if C_Timer then C_Timer.After(5, function() FG:CheckBuffs() end) end
end)
FG:On("PLAYER_REGEN_ENABLED", function() FG:CheckBuffs() end)
FG:On("UNIT_AURA", function(unit)
    if unit == "player" then FG:CheckBuffs() end
end)
