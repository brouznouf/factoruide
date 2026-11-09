-- World map: numbered pins on the next steps of the guide (zone and continent maps).

local FG = Factoruide

FGStepPinMixin = CreateFromMixins and CreateFromMixins(MapCanvasPinMixin or {}) or {}

--- For our pins' CheckMouseButtonPassthrough. The map calls it on each pin it acquires (WoW
--- Forever; Classic Era and TBC have none) to let right clicks through (zoom out), with
--- SetPassThroughButtons, a protected function: from our code during a fight (the map drawn
--- again), the game blocks it (ADDON_ACTION_BLOCKED). Skipped then: the pin keeps what it had.
function FG.PinPassthrough(pin, ...)
    if InCombatLockdown() or not (MapCanvasPinMixin and MapCanvasPinMixin.CheckMouseButtonPassthrough) then return end
    MapCanvasPinMixin.CheckMouseButtonPassthrough(pin, ...)
end

FGStepPinMixin.CheckMouseButtonPassthrough = FG.PinPassthrough

--- Pins keep the same size on screen whatever the map's scale, as the game's own: without limits,
--- a pin is sized in the map canvas and grows with it (huge on a map enlarged by another addon).
function FG.PinScale(pin)
    if pin.SetScalingLimits then pin:SetScalingLimits(1, 1.0, 1.2) end
end

function FGStepPinMixin:OnLoad()
    self:UseFrameLevelType("PIN_FRAME_LEVEL_AREA_POI")
    FG.PinScale(self)
end

--- `kind`: "current" (the step to do), "spot" (another place of its mobs, `shown` the one the
--- compass points at), else a next step.
function FGStepPinMixin:OnAcquired(x, y, n, text, kind, shown)
    self:SetPosition(x, y)
    self.Number:SetText(n)
    self.text = text
    local size = kind == "current" and 22 or (kind == "spot" and 12 or 18)
    self:SetSize(size, size)
    if kind == "current" then
        self.Dot:SetVertexColor(1, 0.82, 0)
    elseif kind == "spot" then
        self.Dot:SetVertexColor(shown and 1 or 0.75, shown and 0.82 or 0.75, shown and 0 or 0.75)
    else
        self.Dot:SetVertexColor(unpack(FG.Skin.worldColors.blue))
    end
end

-- The map canvas calls these itself (pins must not set OnEnter/OnLeave scripts).
function FGStepPinMixin:OnMouseEnter()
    GameTooltip:SetOwner(self, "ANCHOR_RIGHT")
    GameTooltip:SetText(self.text or "", 1, 1, 1, 1, true)
    GameTooltip:Show()
end

function FGStepPinMixin:OnMouseLeave() GameTooltip:Hide() end

local Provider = CreateFromMixins and CreateFromMixins(MapCanvasDataProviderMixin or {}) or {}

--- Position of a step on map `mapID` (any map of the same continent), or nil.
local function PositionOn(step, mapID)
    if not step.m or not C_Map.GetWorldPosFromMapPos then return nil end
    if step.m == mapID then return step.x / 100, step.y / 100 end
    local continent, world = C_Map.GetWorldPosFromMapPos(step.m, CreateVector2D(step.x / 100, step.y / 100))
    if not continent then return nil end
    local _, pos = C_Map.GetMapPosFromWorldPos(continent, world, mapID)
    if not pos then return nil end
    local x, y = pos:GetXY()
    if x < 0 or x > 1 or y < 0 or y > 1 then return nil end
    return x, y
end

function Provider:RemoveAllData() self:GetMap():RemoveAllPinsByTemplate("FGStepPinTemplate") end

function Provider:RefreshAllData()
    self:RemoveAllData()
    local route = FG.Guide and FG.Guide:Route()
    if not FG:Get("uiShown") or not FG:Get("mapPins") or not route then return end
    local mapID = self:GetMap():GetMapID()
    local first = FG.Guide:CurrentStep()
    local n, shown = 0, 0
    for i = first, #route.steps do
        local step = route.steps[i]
        if not step.bg then
            n = n + 1
            local x, y = PositionOn(step, mapID)
            if x then
                self:GetMap():AcquirePin("FGStepPinTemplate", x, y, n, step.t, i == first and "current" or nil)
                shown = shown + 1
            end
            if n >= FG:Get("mapPinsCount") then break end
        end
    end
    -- The other places of the mobs of the step the compass points at.
    local places, at = FG.Guide:SpotPlaces()
    for i, place in ipairs(places or {}) do
        local x, y = PositionOn(place, mapID)
        if x and place.alt then self:GetMap():AcquirePin("FGStepPinTemplate", x, y, "", place.t, "spot", i == at) end
    end
end

local MapPins = {}
FG.MapPins = MapPins

local added = false
local function Attach()
    if added or not WorldMapFrame or not WorldMapFrame.AddDataProvider then return end
    added = true
    WorldMapFrame:AddDataProvider(Provider)
end

function MapPins:Refresh()
    Attach()
    if added and WorldMapFrame:IsShown() then Provider:RefreshAllData() end
end

FG:OnSetting("uiShown", function() MapPins:Refresh() end)
FG:On("PLAYER_ENTERING_WORLD", Attach)
FG:On("ADDON_LOADED", function(name)
    if name == "Blizzard_WorldMap" then Attach() end
end)
