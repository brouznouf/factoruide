-- Factoruide_Data, the data addon the desktop app writes its guides into (never published, so
-- that an update of the other addons never erases it). It loads first (an optional dependency)
-- and sets two globals:
--   FactoruideData = [==[ the same Lua text as an import (Factoruide_Guide/Import.lua) ]==]
--   FactoruideDataTime = unix seconds it was written
-- The text sets `Factoruide.Routes[name]` for each guide, `.Services`... and maybe
-- `FactoruideSent = { ["Name-Realm"] = last seq }`, the rows the contribution server has: the
-- collector drops them, with or without the guide part.
--
-- Only that last table is read here, from the text, without running it. The guides are run by
-- the guide part (Import.lua), which keeps them: the game counts memory to the addon whose code
-- makes it, and megabytes of guides made here showed as this addon's. Without the guide part
-- the text is let go at the login.

local _, FG = ...

--- The last seq of each character the server has, from `sent` (FactoruideSent of a text), the
--- valid entries only.
function FG.SentSeqs(sent)
    local out = {}
    for character, seq in pairs(type(sent) == "table" and sent or {}) do
        seq = tonumber(seq)
        if type(character) == "string" and seq then out[character] = seq end
    end
    return out
end

--- The seqs of the last `FactoruideSent = { ["Name-Realm"] = seq, ... }` of a guide text, read as
--- text: the apps write it last, one character a line, names without quotes or backslashes.
function FG.DataSentSeqs(text)
    local out = {}
    if type(text) ~= "string" then return out end
    local at, from = nil, 1
    while true do
        local found = text:find("FactoruideSent", from, true)
        if not found then break end
        at, from = found, found + 1
    end
    local block = at and text:match("^FactoruideSent%s*=%s*(%b{})", at)
    for character, seq in (block or ""):gmatch('%[%s*"([^"\\]*)"%s*%]%s*=%s*(%d+)') do
        out[character] = tonumber(seq)
    end
    return out
end

FG.DataSent = FG.DataSentSeqs(FactoruideData)

-- Every addon is loaded at the login: the text the guide part did not take is let go.
FG:On("PLAYER_LOGIN", function() FactoruideData = nil end)
