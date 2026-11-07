-- The walk left to the place the guide points at (Pathing:Remaining), dotted in green on the
-- world map and the minimap (`pathMap`): around the steep ground when the guide planned it so,
-- straight otherwise. Not while Shortest Path Forever guides (it draws its own).

local FG = Factoruide

local PathLines = {}
FG.PathLines = PathLines

local DOT = "Interface\\COMMON\\Indicator-Gray"
local GREEN = { 0.29, 0.871, 0.502 }
-- World map: a dot every this much of the map's width, at most MAP_DOTS of them.
local MAP_STEP, MAP_DOTS = 0.008, 400
-- Minimap: a dot every this part of its radius, MINI_DOTS at most; drawn every MINI_EVERY seconds.
local MINI_STEP, MINI_DOTS, MINI_EVERY = 0.1, 60, 0.1
-- The minimap's diameter in yards at each zoom, without C_Minimap.GetViewRadius.
local MINIMAP_YARDS = {
    indoor = { [0] = 300, 240, 180, 120, 80, 50 },
    outdoor = { [0] = 466 + 2 / 3, 400, 333 + 1 / 3, 266 + 2 / 3, 200, 133 + 1 / 3 },
}

local function CVar(name)
    local get = C_CVar and C_CVar.GetCVar or GetCVar
    return get and get(name)
end

--- The character's world position: continent, north, west.
local function Player()
    local map = C_Map.GetBestMapForUnit("player")
    local pos = map and C_Map.GetPlayerMapPosition(map, "player")
    if not pos then return nil end
    return FG.Compass.World(map, pos.x, pos.y)
end

--- The walk to draw: its continent, then its points; nil when there is nothing to draw.
local function Walk()
    if not FG:Get("pathMap") or not FG:Get("uiShown") or not FG.Pathing then return nil end
    local continent, north, west = Player()
    if not continent then return nil end
    local points = FG.Pathing:Remaining(continent, north, west)
    if not points or #points < 2 then return nil end
    return continent, points, north, west
end

--- Points every `step` along the line through `points` ({ a, b } pairs), at most `most`, from its
--- start; `keep(a, b)` (optional) leaves out the legs it refuses.
function PathLines.Sample(points, step, most, keep)
    local out = {}
    for k = 1, #points - 1 do
        local a, b = points[k], points[k + 1]
        if not keep or keep(a, b) then
            local length = math.sqrt((b[1] - a[1]) ^ 2 + (b[2] - a[2]) ^ 2)
            local n = math.floor(length / step)
            for i = 0, n do
                if #out >= most then return out end
                local t = length > 0 and i * step / length or 0
                out[#out + 1] = { a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t }
            end
        end
    end
    return out
end

-- World map ----------------------------------------------------------------------------------

FGWalkDotMixin = CreateFromMixins and CreateFromMixins(MapCanvasPinMixin or {}) or {}
FGWalkDotMixin.CheckMouseButtonPassthrough = FG.PinPassthrough -- not in combat (MapPins.lua)

function FGWalkDotMixin:OnLoad() self:UseFrameLevelType("PIN_FRAME_LEVEL_AREA_POI") end

function FGWalkDotMixin:OnAcquired(x, y)
    self:SetPosition(x, y)
    self.Dot:SetVertexColor(GREEN[1], GREEN[2], GREEN[3])
end

local Provider = CreateFromMixins and CreateFromMixins(MapCanvasDataProviderMixin or {}) or {}

function Provider:RemoveAllData() self:GetMap():RemoveAllPinsByTemplate("FGWalkDotTemplate") end

function Provider:RefreshAllData()
    self:RemoveAllData()
    local continent, points = Walk()
    if not continent or not C_Map.GetMapPosFromWorldPos then return end
    local mapID = self:GetMap():GetMapID()
    local onMap = {}
    for _, p in ipairs(points) do
        local _, pos = C_Map.GetMapPosFromWorldPos(continent, CreateVector2D(p[1], p[2]), mapID)
        if not pos then return end
        local x, y = pos:GetXY()
        onMap[#onMap + 1] = { x, y }
    end
    for _, d in ipairs(PathLines.Sample(onMap, MAP_STEP, MAP_DOTS)) do
        if d[1] >= 0 and d[1] <= 1 and d[2] >= 0 and d[2] <= 1 then
            self:GetMap():AcquirePin("FGWalkDotTemplate", d[1], d[2])
        end
    end
end

local added = false
local function Attach()
    if added or not WorldMapFrame or not WorldMapFrame.AddDataProvider then return end
    added = true
    WorldMapFrame:AddDataProvider(Provider)
end

-- Minimap ------------------------------------------------------------------------------------

--- The minimap's radius in yards.
local function Radius()
    if C_Minimap and C_Minimap.GetViewRadius then return C_Minimap.GetViewRadius() end
    local yards = MINIMAP_YARDS[IsIndoors and IsIndoors() and "indoor" or "outdoor"]
    return (yards[Minimap:GetZoom()] or yards[0]) / 2
end

--- Pixels from the minimap's center of a point `dn` yards north and `dw` west of the character:
--- right, up. `facing` turns it when the minimap rotates (nil: north up).
function PathLines.OnMinimap(dn, dw, pixelsPerYard, facing)
    local x, y = -dw, dn
    if facing then
        local c, s = math.cos(facing), math.sin(facing)
        x, y = x * c + y * s, -x * s + y * c
    end
    return x * pixelsPerYard, y * pixelsPerYard
end

local mini, dots = nil, {}

local function Dot(i)
    local d = dots[i]
    if not d then
        d = mini:CreateTexture(nil, "OVERLAY")
        d:SetTexture(DOT)
        d:SetSize(5, 5)
        d:SetVertexColor(GREEN[1], GREEN[2], GREEN[3])
        dots[i] = d
    end
    return d
end

local function DrawMinimap()
    local continent, points, north, west = Walk()
    local shown = 0
    if continent and Minimap:IsVisible() then
        local radius = Radius()
        local inside = radius * 0.92
        local rotate = CVar("rotateMinimap") == "1"
        local facing = rotate and GetPlayerFacing and GetPlayerFacing() or nil
        local ppy = Minimap:GetWidth() / 2 / radius
        -- Only the legs that come within the minimap.
        local function Near(a, b)
            local dn, dw = b[1] - a[1], b[2] - a[2]
            local length = dn * dn + dw * dw
            local t = length > 0 and math.max(0, math.min(1, ((north - a[1]) * dn + (west - a[2]) * dw) / length)) or 0
            local qn, qw = a[1] + t * dn - north, a[2] + t * dw - west
            return qn * qn + qw * qw <= radius * radius
        end
        for _, p in ipairs(PathLines.Sample(points, radius * MINI_STEP, MINI_DOTS * 4, Near)) do
            local dn, dw = p[1] - north, p[2] - west
            if dn * dn + dw * dw <= inside * inside and shown < MINI_DOTS then
                shown = shown + 1
                local d = Dot(shown)
                d:ClearAllPoints()
                d:SetPoint("CENTER", Minimap, "CENTER", PathLines.OnMinimap(dn, dw, ppy, facing))
                d:Show()
            end
        end
    end
    for i = shown + 1, #dots do
        dots[i]:Hide()
    end
end

local function CreateMinimap()
    if mini or not Minimap or not CreateFrame then return end
    mini = CreateFrame("Frame", "FactoruideMinimapWalk", Minimap)
    mini:SetAllPoints(Minimap)
    mini:SetFrameLevel(Minimap:GetFrameLevel() + 2)
    mini:SetScript("OnUpdate", function(self, elapsed)
        self.wait = (self.wait or 0) - elapsed
        if self.wait > 0 then return end
        self.wait = MINI_EVERY
        local ok, err = pcall(DrawMinimap)
        if not ok then
            FG:Error("minimap walk", err)
            self.wait = 5
        end
    end)
end

-- Both ---------------------------------------------------------------------------------------

--- Draw again (the place or the setting changed); the world map follows the character every second.
function PathLines:Refresh()
    Attach()
    CreateMinimap()
    if added and WorldMapFrame:IsShown() then Provider:RefreshAllData() end
end

FG:On("PLAYER_ENTERING_WORLD", function()
    PathLines:Refresh()
    if C_Timer and not PathLines.ticker then
        PathLines.ticker = C_Timer.NewTicker(1, function()
            if added and WorldMapFrame:IsShown() then FG:Try("map walk", Provider.RefreshAllData, Provider) end
        end)
    end
end)
FG:On("ADDON_LOADED", function(name)
    if name == "Blizzard_WorldMap" then Attach() end
end)
for _, key in ipairs({ "pathMap", "pathMode", "spfLink", "uiShown" }) do
    FG:OnSetting(key, function() PathLines:Refresh() end)
end
