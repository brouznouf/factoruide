-- Big arrow towards the current step's place, turning with the character: green when the step
-- is ahead, red when it is behind, the distance below. Drag it to move it (unless frames are
-- locked). Without a position (instances) or on another continent it only says so.

local _, FG = ...

local Arrow = {}
FG.Arrow = Arrow

local fr = GetLocale() == "frFR"
local TEXTURE = "Interface\\AddOns\\Factoruide\\Media\\Arrow"
local ARRIVED = 8 -- yards
local TWO_PI = 2 * math.pi

local frame, target

--- World position (continent, north, west) of a map point.
local function World(map, x, y)
    if not map or not C_Map.GetWorldPosFromMapPos then return nil end
    local continent, pos = C_Map.GetWorldPosFromMapPos(map, CreateVector2D(x, y))
    if not continent or not pos then return nil end
    local north, west = pos:GetXY()
    return continent, north, west
end

--- Turn of the arrow (radians, counter-clockwise, 0 = straight ahead) towards a point `dn`
--- yards north and `dw` yards west of a character facing `facing` (the game's facing: radians
--- counter-clockwise from north).
function Arrow.Angle(dn, dw, facing) return (math.atan2(dw, dn) - facing) % TWO_PI end

--- Color from green (ahead) through yellow to red (behind); `off` from 0 to 1.
local function Color(off)
    if off < 0.5 then return off * 2, 1, 0 end
    return 1, 2 - off * 2, 0
end

local function Update(self, elapsed)
    self.wait = (self.wait or 0) - elapsed
    if self.wait > 0 then return end
    self.wait = 0.03
    if not target then
        self:Hide()
        return
    end
    local map = C_Map.GetBestMapForUnit("player")
    local pos = map and C_Map.GetPlayerMapPosition(map, "player")
    local c1, pn, pw = World(map, pos and pos.x, pos and pos.y)
    local c2, tn, tw = World(target.m, target.x / 100, target.y / 100)
    if not pos or not c1 or c1 ~= c2 then
        self.arrow:Hide()
        self.check:Hide()
        self.distance:SetText(fr and "ailleurs" or "elsewhere")
        return
    end
    local dn, dw = tn - pn, tw - pw
    local distance = math.sqrt(dn * dn + dw * dw)
    self.distance:SetText(string.format("%d %s", distance, fr and "m" or "yd"))
    if distance < ARRIVED then
        self.arrow:Hide()
        self.check:Show()
        return
    end
    self.check:Hide()
    local facing = GetPlayerFacing and GetPlayerFacing()
    if not facing then
        self.arrow:Hide()
        return
    end
    -- Angles counter-clockwise from north, as the facing; the texture points up.
    local angle = Arrow.Angle(dn, dw, facing)
    self.arrow:SetRotation(angle)
    local off = math.abs(((angle + math.pi) % TWO_PI) - math.pi) / math.pi
    self.arrow:SetVertexColor(Color(off))
    self.arrow:Show()
end

local function Create()
    if frame or not CreateFrame then return frame end
    frame = CreateFrame("Frame", "FactoruideArrow", UIParent)
    frame:SetSize(64, 64)
    frame:SetClampedToScreen(true)
    frame:SetMovable(true)
    frame:EnableMouse(true)
    frame:RegisterForDrag("LeftButton")
    frame:SetScript("OnDragStart", function(self)
        if not FG:Get("lockFrames") then self:StartMoving() end
    end)
    frame:SetScript("OnDragStop", function(self)
        self:StopMovingOrSizing()
        local p, _, rp, x, y = self:GetPoint()
        if FG.Guide then FG.Guide:State().arrowPoint = { p, "UIParent", rp, x, y } end
    end)
    local point = FG.Guide and FG.Guide:State() and FG.Guide:State().arrowPoint
    frame:SetPoint(unpack(point or { "TOP", UIParent, "TOP", 0, -120 }))

    frame.arrow = frame:CreateTexture(nil, "ARTWORK")
    frame.arrow:SetTexture(TEXTURE)
    frame.arrow:SetAllPoints()
    frame.check = frame:CreateTexture(nil, "ARTWORK")
    frame.check:SetTexture("Interface\\RaidFrame\\ReadyCheck-Ready")
    frame.check:SetSize(40, 40)
    frame.check:SetPoint("CENTER")
    frame.check:Hide()
    frame.distance = frame:CreateFontString(nil, "OVERLAY", "GameFontHighlight")
    frame.distance:SetPoint("TOP", frame, "BOTTOM", 0, -2)
    frame.title = frame:CreateFontString(nil, "OVERLAY", "GameFontNormalSmall")
    frame.title:SetPoint("TOP", frame.distance, "BOTTOM", 0, -1)
    frame.title:SetWidth(220)
    frame.title:SetWordWrap(false)
    if FG.Skin then
        FG.Skin:Font(frame.distance)
        FG.Skin:Font(frame.title)
    end

    frame:SetScript("OnUpdate", Update)
    frame:SetScale(FG:Get("scale"))
    FG:OnSetting("scale", function(v) frame:SetScale(v) end)
    FG:OnSetting("arrowShown", function() Arrow:SetTarget(target) end)
    frame:Hide()
    return frame
end

--- Point the arrow at a step (its map, x, y in percent); nil hides it.
function Arrow:SetTarget(step)
    target = step and step.m and step.x and step.y and step or nil
    local f = Create()
    if not f then return end
    if not target or not FG:Get("arrowShown") then
        f:Hide()
        return
    end
    f.title:SetText(step.n or step.zone or "")
    f.wait = 0
    f:Show()
end
