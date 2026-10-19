-- Smoke test of the addon outside the game: minimal WoW API stubs, load files in TOC
-- order, fire login and quest events, check the guide advances.
-- Run from the repository root: luajit addon/tests/smoke.lua
-- `luajit addon/tests/smoke.lua profile`: Factoruide_Profile alone (the guide is another addon's).
local profileOnly = arg and arg[1] == "profile"

local onQuest, completed = {}, {}
local playerLevel = 1
local waypoint
local frames = {}

-- Generic UI object: any unknown method is a no-op returning another stub.
local function stub()
    local o = { events = {} }
    function o:RegisterEvent(e) self.events[e] = true end
    function o:SetScript(name, fn) self[name] = fn end
    function o:SetText(t) self.text = t end
    function o:GetText() return self.text end
    -- The game refuses a missing font.
    function o:SetFontObject(font) assert(font ~= nil, "SetFontObject without a font") end
    -- Strict as the newer clients' API: a boolean, not nil.
    function o:SetShown(shown)
        assert(type(shown) == "boolean", "SetShown(" .. tostring(shown) .. ")")
        self._shown = shown
    end
    function o:GetPoint() return "TOP", nil, "TOP", 0, 0 end
    function o:GetID() return 1 end
    function o:SetFrameLevel(level) self._level = level end
    function o:GetFrameLevel() return rawget(self, "_level") or 1 end
    function o:GetStringWidth() return #(rawget(self, "text") or "") * 5 end
    setmetatable(o, {
        __index = function()
            return function() return stub() end
        end,
    })
    return o
end

-- Frames know their name, parent and whether they are shown (a hidden parent hides its children);
-- in fields of their own, the addon sets its own fields on its frames (`strip.name`...).
CreateFrame = function(kind, name, parent)
    local f = stub()
    f._name, f._parent, f._shown, f._button = name, parent, true, kind == "Button"
    function f:SetShown(value)
        assert(type(value) == "boolean", "SetShown(" .. tostring(value) .. ")")
        self._shown = value
    end
    function f:Show() self._shown = true end
    function f:Hide() self._shown = false end
    function f:SetAlpha(a) self._alpha = a end
    function f:GetAlpha() return self._alpha or 1 end
    function f:IsShown() return self._shown end
    function f:SetEnabled(enabled) self._enabled = enabled end
    function f:IsEnabled() return self._enabled ~= false end
    function f:GetName() return self._name end
    function f:SetParent(p) self._parent = p end
    function f:GetParent() return self._parent end
    function f:HasScript(name) return name ~= "OnClick" or self._button == true end
    function f:GetScript(name) return rawget(self, name) end
    function f:IsVisible()
        local p = self._parent
        return self._shown and (not p or not p.IsVisible or p:IsVisible())
    end
    table.insert(frames, f)
    return f
end
CreateVector2D = function(x, y)
    return {
        GetXY = function() return x, y end,
    }
end
-- The game's options (Blizzard_Settings), recorded: categories and their controls (initializers),
-- the proxy settings by variable, the category opened. Checks what the game checks (value types).
local options = { categories = {}, settings = {}, notified = {}, opened = nil }
MinimalSliderWithSteppersMixin = { Label = { Left = 1, Right = 2 } }
do
    local function Initializer(data)
        local init = { data = data, predicates = {} }
        function init:SetParentInitializer(parent, predicate)
            assert(parent and parent ~= self, "a parent initializer")
            self.parent = parent
            if predicate then table.insert(self.predicates, predicate) end
        end
        function init:AddModifyPredicate(predicate) table.insert(self.predicates, predicate) end
        function init:AddSearchTags() end
        function init:GetSetting() return self.data.setting end
        --- Whether the game lets it be changed now (its predicates).
        function init:Enabled()
            for _, predicate in ipairs(self.predicates) do
                if not predicate() then return false end
            end
            return true
        end
        return init
    end
    local function Category(name, parent, frame)
        local category = { name = name, parent = parent, frame = frame, initializers = {}, subcategories = {} }
        category.id = #options.categories + 1
        function category:GetID() return self.id end
        if parent then
            assert(not parent.registered, "subcategory " .. name .. " added after its category's registration")
            table.insert(parent.subcategories, category)
        end
        table.insert(options.categories, category)
        return category, category
    end
    local function Add(category, init)
        table.insert(category.initializers, init)
        return init
    end
    SettingsPanel = {
        GetLayout = function(_, category)
            return { AddInitializer = function(_, init) Add(category, init) end }
        end,
    }
    Settings = {
        VarType = { Boolean = "boolean", Number = "number", String = "string" },
        RegisterCanvasLayoutCategory = function(frame, name) return Category(name, nil, frame) end,
        RegisterVerticalLayoutCategory = function(name) return Category(name) end,
        RegisterCanvasLayoutSubcategory = function(parent, frame, name) return Category(name, parent, frame) end,
        RegisterVerticalLayoutSubcategory = function(parent, name) return Category(name, parent) end,
        RegisterAddOnCategory = function(category)
            assert(not category.parent, "a top category")
            category.registered = true
        end,
        RegisterProxySetting = function(category, variable, varType, name, default, get, set)
            assert(type(variable) == "string" and type(name) == "string", "variable and name")
            assert(type(default) == varType, variable .. ": default of type " .. type(default) .. ", not " .. varType)
            local setting =
                { variable = variable, varType = varType, name = name, default = default, category = category }
            function setting:GetValue() return get() end
            function setting:SetValue(value)
                assert(type(value) == varType, variable .. ": SetValue of type " .. type(value))
                if get() ~= value then set(value) end
            end
            function setting:SetValueToDefault() self:SetValue(default) end
            assert(not options.settings[variable], "registered twice: " .. variable)
            options.settings[variable] = setting
            return setting
        end,
        CreateCheckbox = function(category, setting, tooltip)
            assert(setting.varType == "boolean", "a check box for a boolean")
            return Add(
                category,
                Initializer({ kind = "checkbox", setting = setting, name = setting.name, tooltip = tooltip })
            )
        end,
        CreateSlider = function(category, setting, sliderOptions, tooltip)
            assert(setting.varType == "number" and sliderOptions, "a slider for a number")
            return Add(
                category,
                Initializer({
                    kind = "slider",
                    setting = setting,
                    name = setting.name,
                    options = sliderOptions,
                    tooltip = tooltip,
                })
            )
        end,
        CreateDropdown = function(category, setting, entries, tooltip)
            assert(type(entries) == "function", "the drop-down's entries")
            return Add(
                category,
                Initializer({
                    kind = "dropdown",
                    setting = setting,
                    name = setting.name,
                    options = entries,
                    tooltip = tooltip,
                })
            )
        end,
        CreateSliderOptions = function(min, max, step)
            local slider = { minValue = min, maxValue = max, steps = (max - min) / step, formatters = {} }
            function slider:SetLabelFormatter(labelType, fn)
                assert(labelType, "a label type")
                self.formatters[labelType] = fn
            end
            return slider
        end,
        CreateControlTextContainer = function()
            local container = { data = {} }
            function container:Add(value, label, tooltip)
                table.insert(self.data, { value = value, label = label, text = label, tooltip = tooltip })
            end
            function container:GetData() return self.data end
            return container
        end,
        NotifyUpdate = function(variable) options.notified[variable] = (options.notified[variable] or 0) + 1 end,
        OpenToCategory = function(id) options.opened = id end,
    }
    CreateSettingsListSectionHeaderInitializer = function(name, tooltip)
        return Initializer({ kind = "header", name = name, tooltip = tooltip })
    end
    CreateSettingsButtonInitializer = function(name, buttonText, onClick, tooltip, addSearchTags)
        assert(addSearchTags ~= nil, "addSearchTags is required")
        return Initializer({
            kind = "button",
            name = name,
            buttonText = buttonText,
            buttonClick = onClick,
            tooltip = tooltip,
        })
    end
end
--- The addon's category (Factoruide) in the game's options, and its page `name`.
function options.Category()
    for _, category in ipairs(options.categories) do
        if category.name == "Factoruide" and not category.parent then return category end
    end
end
function options.Page(name)
    for _, category in ipairs(options.Category().subcategories) do
        if category.name == name then return category end
    end
end
--- Every control of the addon's pages (initializers).
function options.Controls()
    local out = {}
    for _, category in ipairs(options.Category().subcategories) do
        for _, init in ipairs(category.initializers) do
            out[#out + 1] = init
        end
    end
    return out
end
local calls = {}
local function record(name)
    return function(...) calls[name] = { ... } end
end
AcceptQuest = record("AcceptQuest")
CompleteQuest = record("CompleteQuest")
GetQuestReward = record("GetQuestReward")
LootSlot = function(i) calls.loot = (calls.loot or 0) + 1 end
QuestGetAutoAccept = function() return false end
IsShiftKeyDown = function() return false end
IsQuestCompletable = function() return true end
GetNumQuestChoices = function() return 1 end
GetNumLootItems = function() return 3 end
GetCVarBool = function() return true end
IsModifiedClick = function() return false end
GetNumTrainerServices = function() return 0 end
IsTradeskillTrainer = function() return false end
GetMoney = function() return 0 end
IsInInstance = function() return false end
IsInGroup = function() return false end
IsInRaid = function() return false end
ObjectiveTrackerFrame = {
    hidden = false,
    Hide = function(self) self.hidden = true end,
    IsProtected = function() return false end,
    HookScript = function() end,
    Update = function() end,
}
GetQuestLogSpecialItemInfo = function() return nil end
GetDifficultyColor = function() return { r = 1, g = 0.8, b = 0 } end
C_PlayerInfo = {
    GetContentDifficultyQuestForPlayer = function() return 1 end,
}
GameTooltip = { SetOwner = function() end, SetHyperlink = function() end, Show = function() end, Hide = function() end }
-- Items: their class (12: quest items, the default) and how many are in the bags and the bank.
local itemClasses, bags, bank = {}, {}, {}
C_Item = {
    GetItemInfoInstant = function(id) return id, nil, nil, nil, 134400, itemClasses[id] or 12, 0 end,
    GetItemCount = function(id, includeBank) return (bags[id] or 0) + (includeBank and (bank[id] or 0) or 0) end,
}
UIParent = {}
print = print
strsplit = function(sep, s, n)
    local out, start = {}, 1
    while true do
        if n and #out == n - 1 then
            table.insert(out, s:sub(start))
            break
        end
        local i = s:find(sep, start, true)
        if not i then
            table.insert(out, s:sub(start))
            break
        end
        table.insert(out, s:sub(start, i - 1))
        start = i + 1
    end
    return unpack(out)
end
strmatch = string.match
ChatFontNormal = {}
strtrim = function(s) return (s:gsub("^%s+", ""):gsub("%s+$", "")) end
wipe = function(t)
    for k in pairs(t) do
        t[k] = nil
    end
    return t
end
time = os.time
date = os.date
GetTime = function() return os.clock() + 1000 end
GetLocale = function() return "frFR" end
UnitName = function() return "Brouz" end
GetNormalizedRealmName = function() return "Realm" end
UnitRace = function() return "Gnome", "Gnome", 7 end
UnitClass = function() return "Mage", "MAGE", 8 end
UnitFactionGroup = function() return "Alliance" end
UnitLevel = function() return playerLevel end
UnitXP = function() return 0 end
UnitXPMax = function() return 400 end
GetXPExhaustion = function() return 600 end
IsResting = function() return true end
local bindLocation = "Coldridge Valley"
GetBindLocation = function() return bindLocation end
GetInventoryItemID = function(_, slot) return slot == 5 and 6125 or nil end
local reloaded = false
ReloadUI = function() reloaded = true end
UnitGUID = function(u)
    if u == "npc" then return "Creature-0-1-2-3-658-0000" end
    return "Player-1-2"
end
StaticPopupDialogs = {}
StaticPopup_Show = record("StaticPopup_Show")
YES, NO = "Yes", "No"
UnitOnTaxi = function() return false end
UnitIsPlayer = function() return false end
UnitExists = function() return false end
InCombatLockdown = function() return false end
local macros = {}
GetMacroIndexByName = function(name)
    for i, m in pairs(macros) do
        if m.name == name then return i end
    end
    return 0
end
GetNumMacros = function() return 0, #macros end
CreateMacro = function(name, icon, body)
    table.insert(macros, { name = name, body = body })
    return #macros
end
EditMacro = function(i, name, icon, body) macros[i].body = body end
PickupMacro = function() end
issecretvalue = function() return false end
local timers = {}
C_Timer = { NewTicker = function() end, After = function(_, fn) table.insert(timers, fn) end }
--- Run the C_Timer.After callbacks waiting.
local function runTimers()
    local due = timers
    timers = {}
    for _, fn in ipairs(due) do
        fn()
    end
end
C_Map = {
    GetBestMapForUnit = function() return 1426 end,
    GetMapInfo = function(m) return { mapID = m, mapType = m == 1415 and 2 or 3, parentMapID = 1415 } end,
    GetPlayerMapPosition = function()
        return {
            GetXY = function() return 0.2993, 0.712 end,
        }
    end,
    CanSetUserWaypointOnMap = function() return true end,
    SetUserWaypoint = function(p) waypoint = p end,
    ClearUserWaypoint = function() waypoint = nil end,
    GetWorldPosFromMapPos = function(m, v)
        local x, y = v:GetXY()
        return 0, CreateVector2D(x * 1000, y * 1000)
    end,
}
UiMapPoint = {
    CreateFromCoordinates = function(m, x, y) return { m = m, x = x, y = y } end,
}
C_SuperTrack = { SetSuperTrackedUserWaypoint = function() end }
C_QuestLog = {
    GetAllCompletedQuestIDs = function() return {} end,
    GetNumQuestLogEntries = function() return 0 end,
    GetInfo = function() return nil end,
    GetLogIndexForQuestID = function() return nil end,
    IsOnQuest = function(q) return onQuest[q] or false end,
    IsQuestFlaggedCompleted = function(q) return completed[q] or false end,
    IsComplete = function() return false end,
    GetQuestObjectives = function() return { { text = "Tough Wolf Meat: 0/8", finished = false } } end,
    GetTitleForQuestID = function() return nil end,
}
C_GossipInfo = {
    GetAvailableQuests = function() return { { questID = 233, questLevel = 2 } } end,
    GetActiveQuests = function() return {} end,
    SelectAvailableQuest = record("SelectAvailableQuest"),
    SelectActiveQuest = record("SelectActiveQuest"),
}
GetQuestID = function() return 179 end
SlashCmdList = {}

-- Each addon gets its own private table; the guide uses the profile's, the global `Factoruide`.
local function load(addon)
    local private = {}
    for line in io.lines("addon/" .. addon .. "/" .. addon .. ".toc") do
        -- Routes.lua and Services.lua ship empty: the guides of the tests instead, as an older app
        -- version wrote them. Config.lua ships empty too: as an older app wrote it, ignored.
        if line == "Config.lua" then
            assert(loadfile("addon/" .. addon .. "/Config.lua"))(addon, private)
            Factoruide.Config = { autoAccept = false, guideMode = "minimal", locale = "deDE", scale = 1.5 }
        elseif line == "Routes.lua" or line == "Services.lua" then
            assert(loadfile("addon/" .. addon .. "/" .. line))(addon, private)
            assert(loadfile("addon/tests/fixtures/" .. line))(addon, private)
        elseif line:match("%.lua$") then
            local chunk = assert(loadfile("addon/" .. addon .. "/" .. line:gsub("\\", "/")))
            chunk(addon, private)
        end
    end
end
-- Factoruide_Data (written by the desktop app) loads first. Alone with the profile part: what the
-- server has (its FactoruideSent) still drops the collector's rows.
if profileOnly then
    FactoruideData = 'Factoruide.Routes = { x = { steps = {} } }\nFactoruideSent = { ["Brouz-Realm"] = 1 }\n'
    FactoruideDataTime = 1700000000
end
load("Factoruide_Profile")
if not profileOnly then load("Factoruide_Guide") end
local FG = Factoruide

local Press, Key, Click, Option
do
    -- The key bindings (Bindings.xml): their Lua, run as the game runs them on a key press.
    local bindings = {}
    options.bindings = bindings
    if not profileOnly then
        local xml = assert(io.open("addon/Factoruide_Guide/Bindings.xml")):read("*a")
        for name, body in xml:gmatch('<Binding name="([^"]+)"[^>]*>(.-)</Binding>') do
            bindings[name] = body
        end
    end
    function Key(name) assert(loadstring(assert(bindings[name], "no binding " .. name)))() end

    --- A click on the button labelled `label` (English, shown translated) in the addon's options: on
    --- its main page or a row of its other pages.
    --- Shown with its parents (frames of the test's CreateFrame only).
    local function Visible(f)
        while type(f) == "table" do
            if rawget(f, "_shown") == false then return false end
            f = rawget(f, "_parent")
        end
        return true
    end
    function Click(label)
        local text = FG.T(label)
        for _, f in ipairs(frames) do
            if f._button and f:GetText() == text and Visible(f) and f:IsEnabled() then
                return f:GetScript("OnClick")(f, "LeftButton")
            end
        end
        for _, init in ipairs(options.Controls()) do
            if init.data.kind == "button" and init.data.name == text then return init.data.buttonClick() end
        end
        error("no button " .. label)
    end

    --- The setting `variable` of the addon's options, as its control changes it.
    function Option(variable) return assert(options.settings[variable], "no setting " .. variable) end
    local function Toggle(variable)
        local setting = Option(variable)
        setting:SetValue(not setting:GetValue())
    end

    -- What the chat commands did before: a key binding while playing, a button or a check box of the
    -- addon's options otherwise.
    local PRESS = {
        prev = function() Key("FACTORUIDE_PREV") end,
        next = function() Key("FACTORUIDE_NEXT") end,
        skip = function() Key("FACTORUIDE_SKIP") end,
        undo = function() Key("FACTORUIDE_UNDO") end,
        spot = function() Key("FACTORUIDE_SPOT") end,
        mode = function() Key("FACTORUIDE_MODE") end,
        guide = function() Key("FACTORUIDE_GUIDE") end,
        hide = function() Key("FACTORUIDE_UI") end,
        time = function() Key("FACTORUIDE_TIME") end,
        status = function() Click("Recorded events") end,
        reset = function() Click("Clear the recorded events") end,
        export = function() Click("Export my character") end,
        profile = function() Click("Save the profile now") end,
        import = function() Click("Import a guide…") end,
        clear = function() Click("Delete the imported guides") end,
        macro = function() Click("Pick up the target macro") end,
        compassReset = function() Click("Compass back to the top") end,
        restart = function() Click("Start over") end,
        debug = function() Toggle("Factoruide_debug") end,
        lock = function() Toggle("Factoruide_lockFrames") end,
        compass = function() Toggle("Factoruide_compassShown") end,
    }
    function Press(id) assert(PRESS[id], "nothing for " .. id)() end
end

local function fire(event, ...)
    for _, f in ipairs(frames) do
        if f.OnEvent and f.events[event] then f.OnEvent(f, event, ...) end
    end
end

GetAddOnMetadata = function(name, field) return name == "Factoruide_Profile" and field == "Version" and "0.13.0" or nil end
fire("ADDON_LOADED", "Factoruide_Profile")
-- The app learns the game loaded the addon, and which version.
assert(FactoruideDB.seen and FactoruideDB.seen.version == "0.13.0" and FactoruideDB.seen.time, "seen")
if not profileOnly then fire("ADDON_LOADED", "Factoruide_Guide") end
fire("PLAYER_LOGIN")
local char = FactoruideDB.characters["Brouz-Realm"]

-- Another addon shows the guide: this one only records, and does nothing else.
if profileOnly then
    assert(FG.Guide == nil and FG.Routes == nil and FG.Get == nil, "no guide without Factoruide_Guide")
    onQuest[179] = true
    fire("QUEST_DETAIL", 0)
    fire("QUEST_ACCEPTED", 179)
    assert(#char.events >= 2, "collector recorded events")
    assert(FG.DataSent["Brouz-Realm"] == 1 and FactoruideData == nil, "Factoruide_Data's sent seqs read, text let go")
    assert(FG.DataGuide == nil and FG.Routes == nil, "no guide kept without the guide part")
    -- The rows go on after the last the server has.
    assert(tonumber(char.events[1]:match("^(%d+)")) == 2, "numbered after the sent seq: " .. char.events[1])
    assert(char.guide == nil and waypoint == nil, "no guide state, no waypoint")
    assert(next(calls) == nil, "nothing done in game: " .. tostring(next(calls)))
    assert(FG.Options and FG.Options.category, "the options built at login")
    -- The addon's page in the game's options, with the profile part's own: the character on the
    -- main page, the Advanced page. `/fg` (a sub-command too) opens it and does nothing else.
    local category = options.Category()
    assert(category and category.registered and category.frame, "the addon's main page in the game's options")
    assert(#category.subcategories == 1 and options.Page("Avancé"), "only the Advanced page without the guide part")
    SlashCmdList.FACTORUIDE("export")
    assert(options.opened == category:GetID(), "/fg opens the addon's page")
    assert(not (FG.TextWindow:Parts() and FG.TextWindow:Parts():IsShown()), "/fg export opens the options only")
    for _, f in ipairs(frames) do
        assert(not (f._button and f:GetText() == FG.T("Import a guide…")), "no guide button without the guide part")
    end
    -- Debug messages: a check box.
    Press("debug")
    assert(FactoruideDB.debug == true and Option("Factoruide_debug"):GetValue() == true, "debug on")
    Press("debug")
    assert(not FactoruideDB.debug, "debug off")
    -- Clearing the recorded events asks first (the game's dialog).
    local recorded = #char.events
    calls.StaticPopup_Show = nil
    Press("reset")
    assert(#char.events == recorded and calls.StaticPopup_Show[1] == "FACTORUIDE_CLEAR_EVENTS", "asked first")
    StaticPopupDialogs.FACTORUIDE_CLEAR_EVENTS.OnAccept()
    assert(#char.events == 0, "recorded events cleared")
    calls.StaticPopup_Show = nil
    fire("QUEST_ACCEPTED", 179)
    FG.Profile:Snapshot()
    local profile = FactoruideDB.profiles["Brouz-Realm"]
    assert(profile and profile.level == 1 and profile.class == "MAGE", "profile saved")
    -- The export needs nothing of the guide part.
    Press("export")
    local _, edit = FG.TextWindow:Parts()
    local json = LibStub("LibDeflate"):DecompressDeflate(LibStub("LibDeflate"):DecodeForPrint(edit:GetText():sub(6)))
    assert(json and json:find('"character":"Brouz-Realm"', 1, true), "export without the guide")
    assert(not json:find('"collector"', 1, true) and not json:find('"events"', 1, true), "no collector rows")
    assert(next(FactoruideDB.errors or {}) == nil, "no error: " .. tostring(next(FactoruideDB.errors or {})))
    print("profile only ok: " .. #char.events .. " events")
    os.exit(0)
end
assert(char.guide.route == "gnome-mage", "assigned route followed: " .. tostring(char.guide.route))

--- Choose a route for the character, as in the settings window's guides page, and follow it.
local function useRoute(name) FG.Guide:Choose(name) end
assert(char.guide.step == 1)
assert(waypoint and waypoint.m == 1426, "waypoint set")

onQuest[179] = true
fire("QUEST_DETAIL", 0)
fire("QUEST_ACCEPTED", 179)
assert(char.guide.step == 2, "advanced after accept, step " .. char.guide.step)

completed[179] = true
onQuest[179] = nil
fire("QUEST_TURNED_IN", 179, 80, 0)
assert(char.guide.step == 4, "objective and turn-in skipped, step " .. char.guide.step)

-- Back one page: steps 3 to 6 (Sten Stoutarm, Balir Frosthammer) are one group, step 2 another.
Press("prev")
assert(char.guide.step == 2, "the page before, step " .. char.guide.step)
Press("next")
assert(char.guide.step == 3, "the page after it, step " .. char.guide.step)
Press("status")
assert(#char.events >= 2, "collector recorded events")
print("smoke test ok: step " .. char.guide.step .. ", " .. #char.events .. " events")

-- On a page with steps left, the current step is its first one not done; the page stays.
FG.Guide:Advance()
assert(char.guide.step == 4 and FG.Guide:CurrentSteps()[1] == 3, "same page, its step not done yet")
-- Going back to a page done already stays there (refresh and quest events), the resync goes forward.
Press("prev")
FG.Guide:Advance()
assert(char.guide.step == 2, "stays after manual navigation")
fire("QUEST_LOG_UPDATE")
runTimers()
assert(char.guide.step == 2, "quest events do not move a manual position")
FG.Guide:Resync()
assert(char.guide.step == 4, "resync goes to the first step not done")
print("navigation ok")

-- Automation
fire("GOSSIP_SHOW")
assert(calls.SelectAvailableQuest and calls.SelectAvailableQuest[1] == 233, "gossip quest selected")
local detailID = GetQuestID
GetQuestID = function() return 233 end
fire("QUEST_DETAIL", 0)
assert(calls.AcceptQuest, "quest accepted")
GetQuestID = detailID
fire("QUEST_PROGRESS")
assert(calls.CompleteQuest, "quest completed")
fire("QUEST_COMPLETE")
assert(calls.GetQuestReward and calls.GetQuestReward[1] == 1, "single reward taken")
fire("LOOT_READY", false)
assert(calls.loot == 3, "all loot slots taken")
-- The guide's mode: switched in game and kept, the minimal strip shown instead of the card.
Press("mode")
assert(FG:Get("guideMode") == "minimal" and FactoruideDB.settings.guideMode == "minimal", "minimal mode kept")
assert(FG.Guide:Box() == FG.Mini:Frame(), "the minimal strip is the guide")
Press("mode")
assert(FG:Get("guideMode") == "full" and FG.Guide:Box() == FG.Card:Frame(), "back to the full card")

-- The settings of an older app's Config.lua are ignored: the defaults until chosen in game.
assert(FG.Config.autoAccept == false and FG:Get("autoAccept") == true, "Config.lua ignored")
assert(FG:Get("scale") == 1 and FG.Lang() == "frFR", "Config.lua's size and language ignored")
FG:Save("autoAccept", false)
calls.AcceptQuest = nil
fire("QUEST_DETAIL", 0)
assert(not calls.AcceptQuest, "auto accept turned off in game")
print("automation ok")

-- Quest tracker: lists the log, hides Blizzard's tracker.
C_QuestLog.GetNumQuestLogEntries = function() return 1 end
C_QuestLog.GetInfo = function(i) return { questID = 233, title = "Coldridge Valley Mail Delivery", level = 2 } end
onQuest[233] = true
assert(ObjectiveTrackerFrame.hidden, "Blizzard tracker hidden")
FG.Tracker:Refresh()
local next = FG.Guide:NextStepByQuest()
assert(next[233], "quest 233 found in the route")
print("tracker ok")

-- Professions: practice steps are done once the skill reaches their target.
local cooking = 20
GetProfessions = function() return nil, nil, nil, nil, 5, nil end
GetProfessionInfo = function(i) return "Cuisine", 0, cooking, 75, 0, 0, 185 end
FG.Routes["prof-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    professions = {
        { key = "cooking", name = "Cooking", line = 185, target = 300, curve = { 0, 5, 10, 15, 20, 25, 30 } },
    },
    steps = {
        {
            k = "profession",
            t = "Learn Apprentice Cooking with Gremlock",
            e = "npc",
            id = 1699,
            n = "Gremlock",
            sk = 75,
            lvl = 5,
        },
        { k = "practice", t = "Raise Cooking to 25 (~7 min)", e = "skill", id = 185, n = "Cooking", sk = 25, lvl = 5 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
-- No guide chosen and several made for a gnome mage: none followed; `/fg route` only opens the
-- addon's options.
FG.Assignments["Brouz-Realm"] = nil
useRoute(nil)
assert(char.guide.route == nil, "no chosen route, got " .. tostring(char.guide.route))
options.opened = nil
SlashCmdList.FACTORUIDE("route gnome-mage")
assert(char.guide.route == nil, "/fg route does not switch, got " .. tostring(char.guide.route))
assert(options.opened == options.Category():GetID(), "/fg route opens the options")
-- Without a guide, the guide's keys do nothing.
Press("next")
Press("skip")
assert(char.guide.route == nil and char.guide.step, "nothing happens")
useRoute("prof-test")
assert(char.guide.step == 2, "rank already learned, step " .. char.guide.step)
cooking = 25
fire("SKILL_LINES_CHANGED")
assert(char.guide.step == 3, "practice done, step " .. char.guide.step)
playerLevel = 5
FG.Tracker:Refresh()
print("professions ok")

-- Skipping: outdated steps, exclusive alternatives, by hand.
playerLevel = 21
FG.Routes["skip-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "accept", t = "Accept Old Quest", q = 5001, lvl = 1 },
        { k = "accept", t = "Accept Alt Quest", q = 5002, alt = { 5003 }, lvl = 21 },
        { k = "accept", t = "Accept Current Quest", q = 5004, lvl = 21 },
        { k = "turnin", t = "Turn in Current Quest", q = 5004, lvl = 21 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
completed[5003] = true
useRoute("skip-test")
assert(char.guide.step == 3, "outdated and alternative steps skipped, step " .. char.guide.step)
Press("skip")
assert(char.guide.step == 5, "current quest skipped by hand, step " .. char.guide.step)
Press("undo")
assert(char.guide.step == 3, "undo brings the quest back, step " .. char.guide.step)
Press("undo")
assert(char.guide.step == 3, "nothing left to undo, step " .. char.guide.step)
print("skip ok")

-- Progress belongs to its guide: a quest marked done by hand in one is still to do in another,
-- and coming back finds the marks again. A new version keeps the quests, not the step marks.
Press("skip")
FG.Routes["other-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 21,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "accept", t = "Accept Current Quest", q = 5004, lvl = 21 },
        { k = "turnin", t = "Turn in Current Quest", q = 5004, lvl = 21 },
    },
}
useRoute("other-test")
assert(char.guide.step == 1 and not char.guide.skippedQuests[5004], "marked in another guide only")
useRoute("skip-test")
assert(char.guide.step == 5 and char.guide.skippedQuests[5004], "back to the first guide")
char.guide.marks[1] = "skip"
FG.Routes["skip-test"].time = 2
useRoute("other-test")
useRoute("skip-test")
assert(not char.guide.marks[1] and char.guide.skippedQuests[5004], "new version")
print("progress per guide ok")

-- A book for the library (GitHub #29): looting it is the accept, the librarian offers the
-- quest only then, taken and turned in together.
FG.Routes["held-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        {
            k = "accept",
            t = "Loot The Dalaran Digest, Vol. 23",
            q = 78127,
            hd = true,
            e = "item",
            id = 209844,
            lvl = 21,
        },
        { k = "turnin", t = "Accept The Dalaran Digest and turn it in", q = 78127, e = "npc", id = 211033, lvl = 21 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
useRoute("held-test")
assert(char.guide.step == 1, "the book is not in the bags yet")
bags[209844] = 1
fire("BAG_UPDATE_DELAYED")
assert(char.guide.step == 2, "the book looted: on to the librarian, step " .. char.guide.step)
assert(FG.Guide:WantsQuest(78127), "taken at the librarian")
onQuest[78127] = true
assert(not FG.Guide:WantsQuest(78127), "taken")
onQuest[78127], bags[209844], completed[78127] = nil, nil, true
fire("QUEST_TURNED_IN", 78127, 155, 0)
assert(char.guide.step == 3, "turned in, step " .. char.guide.step)
completed[78127] = nil
print("held item ok")

-- A quest of the log the guide does not take is abandoned first: the step is done once it has
-- left the log.
FG.Routes["abandon-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 21,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "abandon", t = "Abandon Unused Quest (not in this guide)", q = 7001, lvl = 21 },
        { k = "accept", t = "Accept Current Quest", q = 5004, lvl = 21 },
    },
}
onQuest[7001] = true
local untitled = C_QuestLog.GetTitleForQuestID
C_QuestLog.GetTitleForQuestID = function(q) return q == 7001 and "Quête inutile" or nil end
useRoute("abandon-test")
assert(char.guide.step == 1, "waits while the quest is in the log, step " .. char.guide.step)
-- The game's name of the quest, then the label and why, once each.
local abandonRow = FG.Guide:Box().rows[1]
local abandonLine = abandonRow.name:GetText() .. " / " .. abandonRow.sub:GetText()
assert(
    abandonLine:find("Abandon", 1, true)
        and abandonLine:find("Quête inutile", 1, true)
        and abandonLine:find("(not in this guide)", 1, true)
        and not abandonLine:find("Unused Quest", 1, true),
    "abandon line: " .. abandonLine
)
C_QuestLog.GetTitleForQuestID = untitled
onQuest[7001] = nil
fire("QUEST_REMOVED", 7001)
assert(char.guide.step == 2, "quest abandoned, step " .. char.guide.step)
print("abandon ok")

-- Checkpoints: reached in level before the planned step, the steps up to it are skipped but the
-- kept ones (a chain going on after it).
playerLevel = 22
FG.Routes["checkpoint-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    checkpoints = { { lvl = 22, step = 5 } },
    steps = {
        { k = "accept", t = "Accept Filler", q = 6001, lvl = 20 },
        { k = "accept", t = "Accept Chain", q = 6002, kp = true, lvl = 20 },
        { k = "turnin", t = "Turn in Filler", q = 6001, lvl = 21 },
        { k = "turnin", t = "Turn in Chain", q = 6002, kp = true, lvl = 21 },
        { k = "accept", t = "Accept New Hub Quest", q = 6003, lvl = 22 },
        { k = "accept", t = "Accept Chain Follow-up", q = 6004, lvl = 22 },
    },
}
useRoute("checkpoint-test")
assert(char.guide.step == 1, "checkpoint route starts at 1, step " .. char.guide.step)
FG.Guide:SkipAhead()
assert(char.guide.marks[1] == "skip" and char.guide.marks[3] == "skip", "filler skipped")
assert(not char.guide.marks[2] and not char.guide.marks[4], "chain kept")
assert(char.guide.step == 2, "on the kept chain, step " .. char.guide.step)
playerLevel = 21
onQuest[6002] = true
FG.Guide:Advance()
FG.Guide:SkipAhead()
assert(not char.guide.marks[4], "no skip below the checkpoint level")
onQuest[6002] = nil
print("checkpoints ok")

-- A quest whose accept was left behind: its turn-in is not asked.
playerLevel = 21
FG.Routes["turnin-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "accept", t = "Accept Keeper of the Flame", q = 6001, lvl = 10 },
        { k = "turnin", t = "Turn in Keeper of the Flame", q = 6001, lvl = 20 },
        { k = "accept", t = "Accept Current Quest", q = 6002, lvl = 21 },
    },
}
useRoute("turnin-test")
assert(char.guide.step == 3, "turn-in of a never accepted quest skipped, step " .. char.guide.step)
print("accept-skipped ok")

-- Background objectives do not block the guide; the tracker keeps them as in progress.
playerLevel = 5
onQuest[7001] = true
FG.Routes["bg-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "accept", t = "Accept Gold Dust Exchange", q = 7001, lvl = 5 },
        { k = "objective", t = "Gold Dust x10", q = 7001, o = 1, n = "Gold Dust", bg = true, lvl = 5 },
        { k = "accept", t = "Accept Next Quest", q = 7002, lvl = 5 },
        { k = "turnin", t = "Turn in Gold Dust Exchange", q = 7001, lvl = 6 },
    },
}
useRoute("bg-test")
assert(char.guide.step == 3, "background objective passed, step " .. char.guide.step)
assert(FG.Guide:NextStepByQuest()[7001].bg, "background quest in progress for the tracker")
print("background ok")

-- In progress: a background objective left behind stays listed, and comes before the turn-in of
-- its quest while not done.
do
    local listed
    local set = FG.TargetMacro.SetSteps
    FG.TargetMacro.SetSteps = function(self, steps)
        listed = steps
        return set(self, steps)
    end
    onQuest[7001], onQuest[7002] = true, true
    FG.Guide:Resync()
    assert(char.guide.step == 4, "on the turn-in, step " .. char.guide.step)
    assert(listed[1] and listed[1].k == "objective" and listed[1].q == 7001, "objective before its turn-in")
    assert(listed[2] and listed[2].k == "turnin", "then the turn-in")
    FG.TargetMacro.SetSteps = set
    onQuest[7001], onQuest[7002] = nil, nil
end
print("in progress ok")

-- Target macro: created on demand, then follows the current step's mobs.
playerLevel = 5
FG.Routes["macro-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        {
            k = "objective",
            t = "Kill Kobold Vermin x8",
            q = 7101,
            o = 1,
            e = "npc",
            id = 6,
            n = "Kobold Vermin",
            mobs = { "Kobold Vermin" },
            lvl = 5,
        },
        { k = "turnin", t = "Turn in Kobold Camp", q = 7101, e = "npc", id = 197, n = "Marshal McBride", lvl = 5 },
    },
}
onQuest[7101] = true
useRoute("macro-test")
Press("macro")
local index = GetMacroIndexByName("FG Cible")
assert(index > 0, "target macro created")
assert(
    macros[index].body:find("/targetexact Kobold Vermin", 1, true),
    "macro targets the step's mobs: " .. macros[index].body
)
-- The skull: WoW Forever blocks SetRaidTarget from a macro's /run (Classic Era and TBC do not).
assert(FG.Edition == "forever" and not macros[index].body:find("/run", 1, true), "no skull on Forever")
FG.Edition = "classic"
Press("next")
assert(
    macros[index].body:find("/targetexact Marshal McBride", 1, true),
    "macro follows the step: " .. macros[index].body
)
assert(macros[index].body:find("SetRaidTarget", 1, true), "skull on Classic")
FG.Edition = "forever"
print("target macro ok")

-- Talent plan: the next talent follows the class plan (mage: Improved Frostbolt x5, then
-- Elemental Precision).
local frostbolt = 2
GetNumTalentTabs = function() return 3 end
GetNumTalents = function(tab) return tab == 3 and 2 or 0 end
GetTalentInfo = function(tab, i)
    if i == 1 then return "Improved Frostbolt", nil, 1, 2, frostbolt, 5 end
    return "Elemental Precision", nil, 1, 3, 0, 3
end
assert(FG.Talents:Next().name == "Improved Frostbolt", "next talent: Improved Frostbolt")
frostbolt = 5
assert(FG.Talents:Next().name == "Elemental Precision", "next talent after 5/5: Elemental Precision")
print("talents ok")

-- Leveling time against the plan.
playerLevel = 3
FG.Routes["timing-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1000,
    levels = { [2] = 100, [3] = 300 },
    steps = {
        { k = "accept", t = "Accept", q = 7201, lvl = 3 },
    },
}
useRoute("timing-test")
FG.Guide:State().levelTimes = { [3] = 240 }
assert(FG.Timing:Short():find("1 min", 1, true), "timing ahead of plan: " .. FG.Timing:Short())
print("timing ok")

-- Flight master: the current fly step's destination is taken.
local taken
Enum = Enum or {}
Enum.FlightPathState = { Current = 0, Reachable = 1, Unreachable = 2 }
GetTaxiMapID = function() return 1415 end
C_TaxiMap = {
    GetAllTaxiNodes = function()
        return { { nodeID = 2, slotIndex = 7, state = 1 }, { nodeID = 6, slotIndex = 3, state = 1 } }
    end,
}
TakeTaxiNode = function(slot) taken = slot end
FG.Routes["fly-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "fly", t = "Fly to Ironforge", e = "taxi", id = 6, n = "Ironforge", lvl = 3 },
    },
}
useRoute("fly-test")
for _, f in ipairs(frames) do
    if f.events.TAXIMAP_OPENED and f.OnEvent then f:OnEvent("TAXIMAP_OPENED") end
end
assert(taken == 3, "flight to the step's node taken")
print("auto fly ok")

-- Teleport: done once the character is there (around the city's class trainer).
FG.Routes["teleport-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "teleport", t = "Teleport to Ironforge", m = 1426, x = 80, y = 20, lvl = 20 },
        { k = "teleport", t = "Teleport to Ironforge", m = 1426, x = 30.1, y = 71.3, lvl = 20 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
useRoute("teleport-test")
fire("QUEST_LOG_UPDATE")
runTimers()
assert(char.guide.step == 1, "far from it: not there yet, step " .. char.guide.step)
char.guide.step = 2
fire("QUEST_LOG_UPDATE")
runTimers()
assert(char.guide.step == 3, "arrived, step " .. char.guide.step)
print("teleport ok")

-- Group sync: a member's progress shows next to the objective.
Ambiguate = function(name) return (name:gsub("%-.*", "")) end
for _, f in ipairs(frames) do
    if f.events.CHAT_MSG_ADDON and f.OnEvent then
        f:OnEvent("CHAT_MSG_ADDON", "FGSYNC1", "R", "PARTY", "Pata-Realm")
        f:OnEvent("CHAT_MSG_ADDON", "FGSYNC1", "7101:3;33:x", "PARTY", "Pata-Realm")
    end
end
assert(FG.Party:Suffix(7101, 1, 8):find("Pata 3", 1, true), "party progress shown: " .. FG.Party:Suffix(7101, 1, 8))
print("party ok")

-- Merchant: grey items are sold.
local sold = 0
C_Container = {
    GetContainerNumSlots = function(bag) return bag == 0 and 2 or 0 end,
    GetContainerItemInfo = function(_, slot)
        return { itemID = 100 + slot, quality = slot == 1 and 0 or 2, stackCount = 1 }
    end,
    UseContainerItem = function() sold = sold + 1 end,
}
C_Item = C_Item or {}
C_Item.GetItemInfo = function() return "Item", nil, nil, nil, nil, nil, nil, nil, nil, nil, 5 end
CanMerchantRepair = function() return false end
for _, f in ipairs(frames) do
    if f.events.MERCHANT_SHOW and f.OnEvent then f:OnEvent("MERCHANT_SHOW") end
end
assert(sold == 1, "one grey item sold, got " .. sold)
print("merchant ok")

-- Skins: a skin registered through the public API styles every frame made so far.
local skinned = {}
FactoruideAPI.RegisterSkin("Test", function(frame, kind) skinned[kind] = (skinned[kind] or 0) + 1 end)
assert(skinned.window == 2, "windows skinned: the card and the minimal strip")
assert((skinned.panel or 0) >= 1, "panels skinned")
assert(#FactoruideAPI.GetFrames() >= 2)
assert(FG.Skin:Suite() == nil, "no UI suite in the tests")
print("skins ok")

-- Compass: straight ahead, to the left, behind (angles counter-clockwise, as the game's facing).
local close = function(a, b) return math.abs(a - b) < 1e-9 end
assert(close(FG.Compass.Angle(10, 0, 0), 0), "north, facing north: ahead")
assert(close(FG.Compass.Angle(0, 10, 0), math.pi / 2), "west, facing north: to the left")
assert(close(FG.Compass.Angle(0, -10, math.pi / 2), math.pi), "east, facing west: behind")
assert(close(FG.Compass.Angle(0, 10, math.pi / 2), 0), "west, facing west: ahead")
assert(close(FG.Compass.Left(FG.Compass.Angle(0, -10, 0)), -90), "east, facing north: 90° to the right")
print("compass angles ok")

-- A step whose mobs live in several places: the arrow moves on from a place where the character
-- waited out of combat (nothing there), and on "Next spot"; around again once all are visited.
do
    local now, fighting = 100, false
    local realTime, realCombat = GetTime, UnitAffectingCombat
    GetTime = function() return now end
    UnitAffectingCombat = function() return fighting end
    FG.Routes["spots"] = {
        race = 7,
        class = 8,
        faction = "Alliance",
        fromLevel = 1,
        toLevel = 2,
        steps = {
            {
                k = "objective",
                t = "Kill Lost Stalker",
                q = 900,
                o = 1,
                m = 1426,
                x = 29.93,
                y = 71.2,
                lvl = 1,
                spots = { { m = 1426, x = 50, y = 50 }, { m = 1426, x = 10, y = 10 } },
            },
        },
    }
    onQuest[900] = true
    local before, beforeStep = char.guide.route, char.guide.step
    useRoute("spots")
    FG.Guide:Refresh()
    assert(waypoint.x == 0.2993, "its own place first")
    now, fighting = 130, true
    FG.Guide:Refresh()
    now, fighting = 145, false
    FG.Guide:Refresh()
    assert(waypoint.x == 0.2993, "fighting there: something to find, stays")
    now = 170
    FG.Guide:Refresh()
    assert(waypoint.x == 0.5 and waypoint.y == 0.5, "waited there out of combat: the closest other place")
    Press("spot")
    assert(waypoint.x == 0.1, "next spot: the next one")
    Press("spot")
    assert(waypoint.x == 0.2993, "all visited: around again from the closest")
    local places, at = FG.Guide:SpotPlaces()
    assert(#places == 3 and at == 1, "the places for the map pins")
    onQuest[900] = nil
    useRoute(before)
    FG.Guide:Go(beforeStep)
    GetTime, UnitAffectingCombat = realTime, realCombat
    print("spots ok")
end

-- An NPC who patrols (Gordo on Tirisfal's road, GitHub #20): its spots are along its path and
-- the arrow points first at the one closest to the character, not at the step's own place.
do
    local realCombat = UnitAffectingCombat
    UnitAffectingCombat = function() return false end
    FG.Routes["patrol"] = {
        race = 7,
        class = 8,
        faction = "Alliance",
        fromLevel = 1,
        toLevel = 2,
        steps = {
            {
                k = "accept",
                t = "Accept Gordo's Task from Gordo (patrols: look along the path)",
                q = 901,
                m = 1426,
                x = 80,
                y = 80,
                lvl = 1,
                pt = true,
                spots = { { m = 1426, x = 50, y = 50 }, { m = 1426, x = 30, y = 70 } },
            },
        },
    }
    local before, beforeStep = char.guide.route, char.guide.step
    useRoute("patrol")
    FG.Guide:Refresh()
    assert(waypoint.x == 0.3 and waypoint.y == 0.7, "the place of its path closest to the character")
    local places, at = FG.Guide:SpotPlaces()
    assert(#places == 3 and at == 3, "its own place and its path's")
    useRoute(before)
    FG.Guide:Go(beforeStep)
    UnitAffectingCombat = realCombat
    print("patrol ok")
end

-- Profile: what the app plans a guide from, saved at login, logout and with "Save the profile now".
local profile = FactoruideDB.profiles["Brouz-Realm"]
assert(profile and profile.level == 1 and profile.race == 7 and profile.class == "MAGE", "profile saved")
assert(profile.rested == 600 and profile.xp_max == 400 and profile.resting, "XP and rested XP")
assert(profile.gear[1] == 6125, "gear worn")
assert(profile.bind.name == "Coldridge Valley" and not profile.bind.position, "hearthstone, place unknown")
bindLocation = "Kharanos"
fire("CHAT_MSG_SYSTEM", "Kharanos is now your home.")
assert(profile.bind.name == "Kharanos" and profile.bind.position.map == 1426, "hearthstone bound here")
C_TaxiMap.GetAllTaxiNodes = function()
    return { { nodeID = 6, state = 0 }, { nodeID = 7, state = 1 }, { nodeID = 8, state = 2 } }
end
fire("TAXIMAP_OPENED")
assert(profile.flights[6] and profile.flights[7] and not profile.flights[8], "flight paths known")
C_QuestLog.GetQuestObjectives = function()
    return { { text = "Tough Wolf Meat: 3/8", numFulfilled = 3, numRequired = 8, finished = false } }
end
C_QuestLog.GetAllCompletedQuestIDs = function() return { 179, 233 } end
Press("profile")
assert(reloaded, "the interface reloads to write the profile")
assert(#profile.completed == 2, "quests done")
assert(profile.log[1].id == 233 and profile.log[1].objectives[1].done == 3, "log with its objectives")
print("profile ok")

-- Window size: Ctrl + mouse wheel over a window, or the setting; saved, nothing: the default.
FG:Save("scale", 1.3)
assert(FG:Get("scale") == 1.3 and FactoruideDB.settings.scale == 1.3, "scale saved")
local wheel = FG.Card:Frame()
fire("MODIFIER_STATE_CHANGED", "LCTRL", 1)
wheel.OnMouseWheel(wheel, 1)
assert(FG:Get("scale") == 1.35, "zoomed in: " .. FG:Get("scale"))
FG:Save("scale", nil)
assert(FG:Get("scale") == 1 and FactoruideDB.settings.scale == nil, "back to the default size")
print("scale ok")

-- A part of the profile the game fails to give keeps its previous value, the error is kept.
local inventory = GetInventoryItemID
GetInventoryItemID = function() error("gone") end
FG.Profile:Snapshot()
assert(profile.gear[1] == 6125 and profile.errors.gear, "gear kept")
assert(#profile.log == 1, "the other parts are saved")
GetInventoryItemID = inventory
FG.Profile:Snapshot()
assert(not profile.errors, "no error left")
print("profile parts ok")

-- Reputations: every faction of the panel, those under a collapsed header too (opened for the
-- reading, closed again), headers without points of their own left out.
local factionRows = {
    { name = "Alliance", header = true, id = 469 },
    { name = "Darnassus", id = 69, value = 4200, parent = "Alliance" },
    { name = "Other", header = true, collapsed = true, id = 169 },
    { name = "Booty Bay", id = 21, value = 2900, parent = "Other" },
}
local function visibleFactions()
    local out, hidden = {}, {}
    for _, f in ipairs(factionRows) do
        if f.header and f.collapsed then hidden[f.name] = true end
        if not (f.parent and hidden[f.parent]) then table.insert(out, f) end
    end
    return out
end
local function setCollapsed(i, collapsed) visibleFactions()[i].collapsed = collapsed end
C_Reputation = {
    GetNumFactions = function() return #visibleFactions() end,
    GetFactionDataByIndex = function(i)
        local f = visibleFactions()[i]
        return f
            and {
                factionID = f.id,
                name = f.name,
                currentStanding = f.value or 0,
                isHeader = f.header or false,
                isCollapsed = f.collapsed or false,
                isHeaderWithRep = false,
            }
    end,
    ExpandFactionHeader = function(i) setCollapsed(i, false) end,
    CollapseFactionHeader = function(i) setCollapsed(i, true) end,
}
FG.Profile:Snapshot()
local function points(list)
    local out = {}
    for _, r in ipairs(list or {}) do
        out[r.id] = r.value
    end
    return out
end
local seen = points(profile.reputation)
assert(seen[69] == 4200 and seen[21] == 2900 and not seen[469] and not seen[169], "reputations, collapsed ones too")
assert(factionRows[3].collapsed, "the collapsed header closed again")
C_Reputation = nil
-- Classic Era and TBC: the older globals.
GetNumFactions = function() return #visibleFactions() end
GetFactionInfo = function(i)
    local f = visibleFactions()[i]
    return f.name, "", 4, 0, 0, f.value or 0, false, false, f.header, f.collapsed, false, false, f.parent ~= nil, f.id
end
ExpandFactionHeader = function(i) setCollapsed(i, false) end
CollapseFactionHeader = function(i) setCollapsed(i, true) end
factionRows[2].value = 4300
FG.Profile:Snapshot()
seen = points(profile.reputation)
assert(seen[69] == 4300 and seen[21] == 2900 and factionRows[3].collapsed, "reputations on Classic")
GetNumFactions, GetFactionInfo, ExpandFactionHeader, CollapseFactionHeader = nil, nil, nil, nil
print("reputations ok")

-- Quests to pick up: the next accepts not done that can be taken now (quests needed done,
-- level reached), the hand-picked one pointed at until done.
playerLevel = 21
FG.Routes["pickup-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "accept", t = "Accept First", q = 6001, m = 1426, x = 30, y = 71, lvl = 21 },
        { k = "accept", t = "Accept Follow-up", q = 6002, pre = { 6001 }, m = 1426, x = 30, y = 72, lvl = 21 },
        { k = "accept", t = "Accept Later", q = 6003, min = 30, m = 1426, x = 31, y = 71, lvl = 21 },
        { k = "accept", t = "Accept Either", q = 6004, pa = { 6005, 6001 }, m = 1426, x = 31, y = 72, lvl = 21 },
        { k = "accept", t = "Accept Close", q = 6006, m = 1426, x = 32, y = 71, lvl = 21 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
FG:Set("pickupLines", 10)
useRoute("pickup-test")
local function pickups() return table.concat(FG.Guide:Pickups(FG:Get("pickupLines")), ",") end
assert(pickups() == "1,5", "only quests that can be taken now: " .. pickups())
completed[6001] = true
assert(pickups() == "2,4,5", "their prerequisites done: " .. pickups())
FG:Set("pickupLines", 1)
assert(pickups() == "2", "as many as configured: " .. pickups())
FG.Guide:PointToStep(5)
assert(waypoint and waypoint.x == 0.32, "hand-picked step pointed at")
FG.Guide:Advance()
assert(waypoint.x == 0.32, "still pointed at after a refresh")
onQuest[6006] = true
FG.Guide:Advance()
assert(waypoint.x ~= 0.32, "back to the route once taken")
print("pickups ok")

-- Grind steps count the XP of the quests complete in the log the route turns in before the
-- level they aim at, not of those it turns in later.
playerLevel = 3
FG.Routes["pending-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "grind", t = "Be level 3 and 300 xp", lvl = 3, xp = 300 },
        { k = "turnin", t = "Turn in Soon", q = 7001, rx = 400, lvl = 3 },
        { k = "turnin", t = "Turn in Reaching 4", q = 7003, rx = 900, lvl = 4 },
        { k = "turnin", t = "Turn in Later", q = 7002, rx = 500, lvl = 5 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
local complete = {}
onQuest[7001], onQuest[7002], onQuest[7003] = true, true, true
local log = { 7002, 7003, 7001 }
C_QuestLog.GetNumQuestLogEntries = function() return #log end
C_QuestLog.GetInfo = function(i) return { questID = log[i], title = "Q" .. log[i], level = 3 } end
local isComplete = C_QuestLog.IsComplete
C_QuestLog.IsComplete = function(q) return complete[q] or false end
useRoute("pending-test")
assert(char.guide.step == 1, "not enough XP yet, step " .. char.guide.step)
complete[7002] = true
FG.Guide:Advance()
assert(char.guide.step == 1, "a quest turned in after the level does not count")
complete[7003] = true
FG.Guide:Advance()
assert(char.guide.step == 2, "the turn-in reaching the level counts, step " .. char.guide.step)
FG.Guide:Go(1)
complete[7003] = nil
complete[7001] = true
FG.Guide:Resync()
assert(char.guide.step == 2, "a quest turned in before the level counts, step " .. char.guide.step)
C_QuestLog.IsComplete = isComplete
print("pending xp ok")

-- Just accepted, the game has no objectives yet and may say the quest complete: the kill step
-- is not skipped for it (Polestar #74, Kolkar Leaders went straight to the hand-in).
FG.Routes["loading-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "accept", t = "Accept Kolkar Leaders", q = 9301, id = 1, m = 1426, x = 29.93, y = 71.2, lvl = 3 },
        { k = "objective", t = "Kill Barak", q = 9301, o = 1, n = "Barak Kodobane", lvl = 3 },
        { k = "turnin", t = "Turn in Kolkar Leaders", q = 9301, rx = 500, id = 2, m = 1426, x = 50, y = 50, lvl = 3 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
local loading, loadingDone = nil, false
local objectivesBefore, numEntries, getInfo =
    C_QuestLog.GetQuestObjectives, C_QuestLog.GetNumQuestLogEntries, C_QuestLog.GetInfo
C_QuestLog.GetQuestObjectives = function(q)
    if q == 9301 then return loading end
    return objectivesBefore(q)
end
C_QuestLog.IsComplete = function(q) return q == 9301 and (loading == nil or loadingDone) end
C_QuestLog.GetNumQuestLogEntries = function() return 1 end
C_QuestLog.GetInfo = function() return { questID = 9301, title = "Kolkar Leaders", level = 3 } end
useRoute("loading-test")
assert(char.guide.step == 1, "accept first, step " .. char.guide.step)
onQuest[9301] = true
-- QUEST_ACCEPTED would add a collector row the checks further down do not expect.
fire("QUEST_LOG_UPDATE")
runTimers()
assert(char.guide.step == 2, "no objectives yet: not done, step " .. char.guide.step)
loading = { { text = "", finished = false } }
fire("QUEST_LOG_UPDATE")
runTimers()
assert(char.guide.step == 2, "blank objectives: not done, step " .. char.guide.step)
loading = { { text = "Barak Kodobane slain: 0/1", finished = false } }
fire("QUEST_LOG_UPDATE")
runTimers()
assert(char.guide.step == 2, "objective not finished, step " .. char.guide.step)
loading, loadingDone = { { text = "Barak Kodobane slain: 1/1", finished = true } }, true
fire("QUEST_LOG_UPDATE")
runTimers()
assert(char.guide.step == 3, "objective finished, step " .. char.guide.step)
-- The same while grinding: its XP is not counted before the objectives are there.
FG.Routes["loading-test"].steps[1] = { k = "grind", t = "Be level 3 and 300 xp", lvl = 3, xp = 300 }
loading, loadingDone = nil, false
useRoute("loading-test")
FG.Guide:Go(1)
FG.Guide:Resync()
assert(char.guide.step == 1, "loading quest XP not counted, step " .. char.guide.step)
loading, loadingDone = { { text = "Barak Kodobane slain: 1/1", finished = true } }, true
FG.Guide:Resync()
assert(char.guide.step > 1, "complete quest XP counted, step " .. char.guide.step)
C_QuestLog.GetQuestObjectives, C_QuestLog.GetNumQuestLogEntries, C_QuestLog.GetInfo =
    objectivesBefore, numEntries, getInfo
C_QuestLog.IsComplete = isComplete
onQuest[9301] = nil
print("objectives loading ok")

-- XP bar mark: where the guide expects the character before the step shown, and the gap.
FG.Routes["xp-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "accept", t = "Accept A", q = 8001, lvl = 3, px = 150 },
        { k = "accept", t = "Accept B", q = 8002, lvl = 3, px = 250 },
        { k = "grind", t = "Grind", lvl = 60, px = 0 },
    },
}
playerLevel = 3
useRoute("xp-test")
onQuest[8001] = true
FG.Guide:Advance()
assert(char.guide.step == 2, "step " .. char.guide.step)
local level, xp = FG.XpMarker:Planned()
assert(level == 3 and xp == 150, "planned before step 2: " .. tostring(level) .. " " .. tostring(xp))
local unitXP = UnitXP
UnitXP = function() return 100 end
assert(FG.XpMarker:Line():find("50"), "50 xp behind: " .. FG.XpMarker:Line())
UnitXP = function() return 350 end
assert(FG.XpMarker:Line():find("200"), "200 xp ahead: " .. FG.XpMarker:Line())
local span = FG.XpMarker:SpanLine()
assert(
    span and span:find("150", 1, true) and span:find("250", 1, true),
    "current tasks from 150 to 250: " .. tostring(span)
)
playerLevel = 1
assert(FG.XpMarker:Line():find("3"), "two levels behind: the planned level")
FG.XpMarker:Refresh()
UnitXP, playerLevel = unitXP, 3
print("xp marker ok")

-- A dungeon run followed by its quests is done on entering the dungeon, then each quest step as
-- the quest progresses; without them (guides made before), once back out of the dungeon.
local inside = false
IsInInstance = function() return inside end
local function dungeonRoute(name, withQuests)
    local steps = { { k = "dungeon", t = "Dungeon: The Deadmines with a group", lvl = 21 } }
    if withQuests then steps[2] = { k = "objective", t = "Red Silk Bandanas", q = 8001, di = true, lvl = 21 } end
    steps[#steps + 1] = { k = "grind", t = "Grind", lvl = 60 }
    FG.Routes[name] =
        { race = 7, class = 8, faction = "Alliance", fromLevel = 1, toLevel = 60, time = 1, steps = steps }
end
onQuest[8001] = true
dungeonRoute("dungeon-test", true)
useRoute("dungeon-test")
assert(char.guide.step == 1, "the run first, step " .. char.guide.step)
inside = true
fire("ZONE_CHANGED_NEW_AREA")
assert(char.guide.step == 2, "run done on entering, step " .. char.guide.step)
inside = false
dungeonRoute("dungeon-old", false)
useRoute("dungeon-old")
inside = true
fire("ZONE_CHANGED_NEW_AREA")
assert(char.guide.step == 1, "an old run is not done inside")
inside = false
fire("ZONE_CHANGED_NEW_AREA")
assert(char.guide.step == 2, "an old run is done back out, step " .. char.guide.step)
onQuest[8001] = nil
print("dungeon ok")

-- Service visits are manual tasks. A dungeon exit must have actually been inside;
-- completing its quests must not send the player out before the run is finished.
FG.Routes["inventory-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "vendor", t = "Sell and repair", lvl = 1 },
        { k = "mailbox", t = "Mail items", lvl = 1 },
        { k = "dungeon", t = "Run dungeon", lvl = 21 },
        { k = "objective", t = "Dungeon quest", q = 8001, di = true, lvl = 21 },
        { k = "dungeon_exit", t = "Finish the run, then leave", lvl = 1 },
        { k = "vendor", t = "Vendor after dungeon", lvl = 1 },
        { k = "mailbox", t = "Mailbox after dungeon", lvl = 1 },
        { k = "grind", t = "Continue route", lvl = 60 },
    },
}
local serviceLevel = playerLevel
playerLevel = 20
onQuest[8001] = true
useRoute("inventory-test")
assert(char.guide.step == 1, "a vendor visit is not skipped by level")
FG.Guide:SkipCurrent()
assert(char.guide.step == 2, "vendor completion leaves mailbox pending")
FG.Guide:SkipCurrent()
assert(char.guide.step == 3, "both visits precede the run")
assert(char.guide.marks[5] == nil, "exit must not complete while still outside")
inside = true
fire("ZONE_CHANGED_NEW_AREA")
assert(char.guide.step == 4 and char.guide.marks[5] == "inside", "entry is remembered for the exit gate")
completed[8001] = true
fire("QUEST_LOG_UPDATE")
runTimers()
assert(char.guide.step == 5, "quest completion waits for the dungeon exit")
useRoute("inventory-test")
assert(char.guide.step == 5 and char.guide.marks[5] == "inside", "pending exit survives progress reload")
inside = false
fire("ZONE_CHANGED_NEW_AREA")
assert(char.guide.step == 6, "only leaving the dungeon releases the service trip")
FG.Guide:SkipCurrent()
assert(char.guide.step == 7, "mailbox is separate from vendor completion")
FG.Guide:SkipCurrent()
assert(char.guide.step == 8, "route continues after both services")
completed[8001], onQuest[8001], playerLevel = nil, nil, serviceLevel
print("inventory visits and dungeon exit ok")

-- Only the guide's quests: those it still takes, not those it abandons, marked done by hand or
-- out of it, nor escorts shared by the group (the event does not tell which quest).
FG:Save("autoAccept", nil)
FG.Routes["accept-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "abandon", t = "Abandon Unwanted (not in this guide)", q = 9001, lvl = 21 },
        { k = "accept", t = "Accept Wanted", q = 9002, lvl = 21 },
        { k = "accept", t = "Accept Skipped", q = 9003, lvl = 21 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
useRoute("accept-test")
local questID = GetQuestID
local function taken(q)
    calls.AcceptQuest = nil
    GetQuestID = function() return q end
    fire("QUEST_DETAIL", 0)
    return calls.AcceptQuest ~= nil
end
assert(not taken(9001), "a quest the guide abandons is not taken")
assert(taken(9002), "a quest of the guide is taken")
FG.Guide:ToggleSkip(3) -- right click on its step
assert(not taken(9003), "a quest marked done by hand is not taken")
assert(not taken(9999), "a quest out of the guide is not taken")
-- Classic and TBC have no quests taken on sight (no QuestGetAutoAccept).
local autoAccept = QuestGetAutoAccept
QuestGetAutoAccept = nil
assert(taken(9002), "a quest of the guide is taken on Classic")
assert(not taken(9999), "a quest out of the guide is not taken on Classic")
QuestGetAutoAccept = autoAccept
-- Quest greeting: a quest of the guide is opened; one without ID (older clients) is opened too,
-- its dialog decides.
local greeting = {}
GetNumAvailableQuests = function() return #greeting end
GetNumActiveQuests = function() return 0 end
GetAvailableQuestInfo = function(i) return false, 0, false, false, greeting[i] end
SelectAvailableQuest = record("SelectAvailableQuest")
local function opened(ids)
    greeting, calls.SelectAvailableQuest = ids, nil
    fire("QUEST_GREETING")
    return calls.SelectAvailableQuest and calls.SelectAvailableQuest[1]
end
assert(opened({ 9999, 9002 }) == 2, "the guide's quest opened")
assert(opened({ 9999 }) == nil, "a quest out of the guide is not opened")
assert(opened({ 0 }) == 1, "a quest without ID is opened")
calls.ConfirmAcceptQuest = nil
fire("QUEST_ACCEPT_CONFIRM", "Friend", "Shared Escort")
assert(not calls.ConfirmAcceptQuest, "a shared escort is not confirmed")
GetQuestID = questID
print("guide quests only ok")

-- Mobs on the way: the XP of the kills of the character (or its pet) counted for the current
-- step. The XP is read on the XP bar, a gain counts when a kill is seen within a second of it.
do
    local savedGetTime, savedXP, savedXPMax, savedLevel = GetTime, UnitXP, UnitXPMax, playerLevel
    local savedGUID = UnitGUID
    UnitGUID = function(u) return u == "pet" and "Pet-0-1-2-3-416-0000000001" or savedGUID(u) end
    -- The game's kill XP messages (the client's own strings).
    COMBATLOG_XPGAIN_FIRSTPERSON = "%s dies, you gain %d experience."
    COMBATLOG_XPGAIN_EXHAUSTION1 = "%s dies, you gain %d experience. (%s exp %s bonus)"
    local clock, barXP, barMax = 5000, 0, 400
    GetTime = function() return clock end
    UnitXP = function() return barXP end
    UnitXPMax = function() return barMax end
    playerLevel = 1
    local function gain(n)
        barXP = barXP + n
        if barXP >= barMax then
            barXP, playerLevel = barXP - barMax, playerLevel + 1
        end
        fire("PLAYER_XP_UPDATE", "player")
    end
    --- The time for the gains to be settled (a kill's signals may come after its XP).
    local function settle()
        clock = clock + 1.5
        runTimers()
        clock = clock + 1.5
        runTimers()
    end
    local function kill(source, dest) fire("PARTY_KILL", source, dest) end
    FG.Routes["farm-test"] = {
        race = 7,
        class = 8,
        faction = "Alliance",
        fromLevel = 1,
        toLevel = 60,
        time = 1,
        steps = { { k = "grind", t = "Grind", lvl = 60, fk = 3, fx = 1300 } },
    }
    useRoute("farm-test")
    fire("PLAYER_ENTERING_WORLD")
    char.guide.farm = nil
    -- A kill then its XP; the XP then the kill and the game's message of the same kill (one gain).
    kill("Player-1-2", "Creature-0-1-2-3-299-0000")
    gain(50)
    settle()
    assert(char.guide.farm and char.guide.farm.step == 1 and char.guide.farm.xp == 50, "a kill's XP counted")
    gain(40)
    kill("Pet-0-1-2-3-416-0000000001", "Creature-0-1-2-3-299-0001")
    fire("CHAT_MSG_COMBAT_XP_GAIN", "Mangy Wolf dies, you gain 40 experience.")
    settle()
    assert(char.guide.farm.xp == 90, "the pet's kill counted once: " .. char.guide.farm.xp)
    -- XP without a kill (a quest turned in, an area discovered): not counted. Nor someone else's
    -- kill, nor a player killed.
    clock = clock + 5
    gain(200)
    kill("Player-9-9", "Creature-0-1-2-3-299-0002")
    settle()
    kill("Player-1-2", "Player-3-4")
    gain(10)
    settle()
    assert(char.guide.farm.xp == 90, "quest XP not counted: " .. char.guide.farm.xp)
    -- A quest turned in right after a kill, its XP read in the same gain: the kill's part only.
    kill("Player-1-2", "Creature-0-1-2-3-299-0003")
    fire("QUEST_TURNED_IN", 1234, 35, 0)
    gain(35 + 35) -- one reading for both
    settle()
    assert(char.guide.farm.xp == 125, "turn-in XP taken off a kill's gain: " .. char.guide.farm.xp)
    -- A kill told by the game's message only (shared in a group, the GUID secret), across a level up.
    assert(barXP == 370, barXP)
    fire("CHAT_MSG_COMBAT_XP_GAIN", "Mangy Wolf dies, you gain 150 experience. (75 exp Rested bonus)")
    gain(150)
    settle()
    assert(playerLevel == 2 and char.guide.farm.xp == 275, "kill XP across a level up: " .. char.guide.farm.xp)
    local rows = FG.Guide:Box().rows
    local farmLine, stepLine = rows[1].count:GetText(), rows[2].name:GetText()
    assert(farmLine == "275/1,300 XP", "a line of its own with the XP: " .. farmLine)
    assert(not stepLine:find("275", 1, true) and stepLine:find("Grind", 1, true), "the step's line apart: " .. stepLine)
    -- Guides made before the XP was exported: the kills are counted.
    FG.Routes["farm-test"].steps[1].fx = nil
    FG.Guide:Refresh()
    farmLine = FG.Guide:Box().rows[1].count:GetText()
    assert(farmLine == "3/3", "guides without the XP count the kills: " .. farmLine)
    -- The mobs of an objective of the guide (killed for it, or dropping its item) are not mobs on
    -- the way while that objective is not finished; then they are.
    FG.Routes["farm-quest-test"] = {
        race = 7,
        class = 8,
        faction = "Alliance",
        fromLevel = 1,
        toLevel = 60,
        time = 1,
        steps = {
            {
                k = "objective",
                t = "Kill Night Web Spider x8",
                n = "Night Web Spider",
                q = 9401,
                o = 1,
                e = "npc",
                id = 1505,
                mi = { 1505 },
                fk = 3,
                fx = 300,
                m = 1431,
                x = 24.8,
                y = 59.7,
                lvl = 1,
            },
            {
                k = "objective",
                t = "Spider Silk x4",
                n = "Spider Silk",
                q = 9402,
                o = 1,
                e = "item",
                id = 9999,
                mi = { 1504 },
                m = 1431,
                x = 24.9,
                y = 59.8,
                lvl = 1,
            },
            { k = "grind", t = "Grind", lvl = 60 },
        },
    }
    playerLevel = 2
    onQuest[9401], onQuest[9402] = true, true
    useRoute("farm-quest-test")
    FG.Guide:Resync()
    char.guide.farm = nil
    kill("Player-1-2", "Creature-0-1-2-3-1505-0000") -- the objective's mob
    gain(30)
    settle()
    kill("Player-1-2", "Creature-0-1-2-3-1504-0001") -- drops the item of an objective
    gain(30)
    settle()
    assert(char.guide.farm == nil, "the mobs of objectives in progress not counted on the way")
    kill("Player-1-2", "Creature-0-1-2-3-299-0002")
    gain(25)
    settle()
    assert(char.guide.farm and char.guide.farm.xp == 25, "another mob counted")
    -- The spiders' objective finished (killing more is farming), then the silk's quest out of the log.
    local objectives = C_QuestLog.GetQuestObjectives
    C_QuestLog.GetQuestObjectives = function(q)
        if q == 9401 then return { { text = "Night Web Spider slain: 8/8", finished = true } } end
        return objectives(q)
    end
    kill("Player-1-2", "Creature-0-1-2-3-1505-0003")
    gain(30)
    settle()
    assert(char.guide.farm.xp == 55, "objective done: its mobs count: " .. char.guide.farm.xp)
    kill("Player-1-2", "Creature-0-1-2-3-1504-0004")
    gain(30)
    settle()
    assert(char.guide.farm.xp == 55, "the silk's objective still in progress")
    onQuest[9402] = nil
    kill("Player-1-2", "Creature-0-1-2-3-1504-0005")
    gain(30)
    settle()
    assert(char.guide.farm.xp == 85, "quest out of the log: its mobs count: " .. char.guide.farm.xp)
    C_QuestLog.GetQuestObjectives = objectives
    onQuest[9401], onQuest[9402] = nil, nil
    GetTime, UnitXP, UnitXPMax, playerLevel, UnitGUID = savedGetTime, savedXP, savedXPMax, savedLevel, savedGUID
end
print("farm on the way ok")

-- Start over: asks when steps or quests were marked by hand, then undoes them.
useRoute("skip-test")
char.guide.skippedQuests[5004] = true
char.guide.marks[1] = "skip"
calls.StaticPopup_Show = nil
Press("restart")
assert(calls.StaticPopup_Show and calls.StaticPopup_Show[1] == "FACTORUIDE_RESET", "asked first")
FG.Guide:Reset()
for q in pairs(char.guide.skippedQuests) do
    error("quest still marked by hand: " .. q)
end
for _, mark in pairs(char.guide.marks) do
    assert(mark ~= "skip", "no step marked by hand left")
end
calls.StaticPopup_Show = nil
FG.Guide:AskReset()
assert(not calls.StaticPopup_Show, "nothing to undo: no question")
-- A step without a quest marked by hand, undone with the window's button.
FG.Guide:ToggleSkip(5)
assert(char.guide.marks[5] == "skip", "step marked")
Press("undo")
assert(char.guide.marks[5] == nil, "step mark undone")
-- The time per level, in the chat.
local printed = {}
local realPrint = print
print = function(text) printed[#printed + 1] = text end
Press("time")
print = realPrint
assert(#printed > 0, "time per level printed")
print("start over ok")

-- The next steps' tab, kept.
FG:Save("nextTab", "pickup")
assert(FG:Get("nextTab") == "pickup" and FactoruideDB.settings.nextTab == "pickup", "tab kept")
FG.Guide:Refresh()
FG:Save("nextTab", nil)
assert(FG:Get("nextTab") == "next", "default back")
print("next steps tab ok")

-- The compass: the target, the other current task, the quest to pick up close by (not the far
-- one) and the next turn-in.
FG.Routes["compass-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "objective", t = "Kill A", n = "A", q = 8001, o = 1, id = 1, m = 1426, x = 29.93, y = 71.2, lvl = 1 },
        { k = "objective", t = "Kill B", n = "B", q = 8002, o = 1, id = 1, m = 1426, x = 29.95, y = 71.25, lvl = 1 },
        { k = "turnin", t = "Turn in A", q = 8001, id = 3, m = 1426, x = 40, y = 40, lvl = 1 },
        { k = "accept", t = "Accept Close", q = 8003, id = 4, m = 1426, x = 30, y = 71.3, lvl = 1 },
        { k = "accept", t = "Accept Far", q = 8004, id = 5, m = 1426, x = 50, y = 50, lvl = 1 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
onQuest[8001], onQuest[8002] = true, true
useRoute("compass-test")
local byIndex = {}
for _, p in ipairs(FG.Compass:Points()) do
    byIndex[p.index] = p
end
assert(byIndex[1] and byIndex[1].main, "the target first")
assert(byIndex[2] and not byIndex[2].main, "the other task")
assert(byIndex[3] and byIndex[3].kind == "turnin", "the next turn-in")
assert(byIndex[4] and byIndex[4].kind == "accept", "the close quest to pick up")
assert(not byIndex[5], "not the far quest to pick up")
-- Clicking the other task's point: the guide points at it.
FG.Guide:PointToStep(2)
assert(FG.Compass:Points()[1].index == 2 and FG.Compass:Points()[1].main, "the point clicked is the target")
onQuest[8001], onQuest[8002] = nil, nil
print("compass points ok")

-- The wind: projected ahead, left and behind like the compass; instead of it with Poulpix UI
-- Immersive ("auto"), or always; gusts towards a far target, a swirl on arrival.
do
    local x, y = FG.Wind.Project(20, 0, 0, 0, 9, 3.2, 0)
    assert(close(x, 0) and y > 0.39, "north, facing north: ahead, under the horizon")
    assert(FG.Wind.Project(0, 10, 0, 0, 9, 3.2, 0) < 0, "west, facing north: to the left")
    assert(FG.Wind.Project(-20, 0, 0, 0, 9, 3.2, 0) == nil, "behind the camera: not drawn")
    assert(FG.Wind.Project(20, 0, 0, 0, 9, 3.2, 1) < 0, "camera over the right shoulder: to the left")

    assert(FG.Wind:Active() == false, "auto without Poulpix UI Immersive: the compass")
    assert(#FG.Compass:Points() > 0 and FG.Wind:Target() == nil)
    C_AddOns = { IsAddOnLoaded = function(name) return name == "Poulpix_UI_Immersive" end }
    assert(FG.Wind:Active() == true, "auto with Poulpix UI Immersive: the wind")
    FG.Guide:Refresh()
    assert(#FG.Compass:Points() == 0 and FG.Wind:Target() ~= nil, "the wind instead of the compass")

    local realCombat = UnitAffectingCombat
    GetPlayerFacing = function() return 0 end
    UnitAffectingCombat = function() return false end
    UnitIsDeadOrGhost = function() return false end
    FG.Guide:PointToStep(3)
    for _ = 1, 20 do
        FG.Wind:Update(0.1)
    end
    assert(FG.Wind:Count() > 0, "a gust towards the far target")
    for _ = 1, 60 do
        FG.Wind:Update(0.1)
    end
    assert(FG.Wind:Count() == 0, "the gust gone, until the next")
    FG.Guide:PointToStep(1)
    FG.Wind:Update(0.1)
    assert(FG.Wind:Count() > 0, "a swirl on arrival")

    FG:Set("windGuide", "off")
    assert(FG.Wind:Active() == false, "off even with Poulpix UI Immersive")
    FG.Guide:Refresh()
    assert(#FG.Compass:Points() > 0 and FG.Wind:Target() == nil, "the compass back")
    FG:Set("windGuide", nil)
    C_AddOns, GetPlayerFacing, UnitIsDeadOrGhost, UnitAffectingCombat = nil, nil, nil, realCombat
    assert(FG.Poulpix.Alpha(0.2, false) == 0.2 and FG.Poulpix.Alpha(0.2, true) == 1, "faded, full under the mouse")
end
print("wind ok")

-- The compass's places (tests/fixtures/Services.lua): for a gnome mage, the mage trainer, not
-- another class's nor the Horde's, the flight masters; not the trainer of a profession neither
-- learned nor planned, the innkeepers (off by default), the stable masters (hunters), the far
-- ones. A kind turned on shows; a profession the guide plans shows its trainers.
do
    local function Names()
        local names = {}
        for _, p in ipairs(FG.Nearby:Around(0, 299.3, 712)) do
            names[p.name] = p
        end
        return names
    end
    local names = Names()
    assert(names["Marryk Nurribit"] and names["Marryk Nurribit"].service == "class", "the mage trainer")
    assert(not names["Thran Khorman"] and not names["Horde Mage"], "not another class's nor the Horde's")
    assert(names["Flight Master"], "flight masters by default")
    assert(not names["Innkeeper"] and not names["Stable Master"], "no innkeepers by default, stables for hunters")
    assert(not names["Far Mage Trainer"] and not names["Smith"], "not the far ones nor other professions")
    FG:Set("compassInn", true)
    assert(Names()["Innkeeper"], "innkeepers turned on")
    FG:Set("compassInn", nil)
    local route = FG.Guide:Route()
    route.professions = { { line = 197 } }
    FG.Nearby:Forget()
    assert(Names()["Tailor"] and not Names()["Smith"], "the trainers of a profession the guide plans")
    route.professions = nil
    FG.Nearby:Forget()

    -- A click on a place: the compass and the game's waypoint point at it, until reached.
    FG.Guide:PointToPlace(names["Flight Master"])
    local first = FG.Compass:Points()[1]
    assert(first.main and first.place and first.name == "Flight Master", "the place is the target")
    assert(waypoint and close(waypoint.x, 0.32), "the waypoint on the place")
    local realPosition = C_Map.GetPlayerMapPosition
    C_Map.GetPlayerMapPosition = function()
        return { GetXY = function() return 0.32, 0.712 end }
    end
    FG.Guide:Refresh()
    first = FG.Compass:Points()[1]
    assert(first.main and not first.place and first.index, "reached: back to the guide's step")
    C_Map.GetPlayerMapPosition = realPosition

    -- The compass turned off.
    FG:Set("compassShown", false)
    assert(#FG.Compass:Points() == 0, "no compass")
    FG:Set("compassShown", nil)
    assert(#FG.Compass:Points() > 0, "the compass back")
    -- Hidden and shown in game (the window's compass button), kept.
    Press("compass")
    assert(
        FG:Get("compassShown") == false and FactoruideDB.settings.compassShown == false,
        "compass button: hidden, kept"
    )
    assert(#FG.Compass:Points() == 0, "compass button: no compass")
    Press("compass")
    assert(FG:Get("compassShown") == true and #FG.Compass:Points() > 0, "compass button again: back")
    FG:Save("compassShown", nil)
end
print("compass places ok")

-- The planned walk (`wp`: its start and turns, world yards): the compass points along it, a bit
-- ahead of its closest point; going back to it when beside it; not when far from it, nor on its
-- last leg (straight to the place), nor with the setting off.
FG.Routes["walk-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        {
            k = "accept",
            t = "Accept Far",
            q = 8101,
            id = 6,
            m = 1426,
            x = 40,
            y = 71.2,
            lvl = 1,
            wp = { 299, 712, 299, 800, 400, 800 },
        },
        {
            k = "objective",
            t = "Kill wolves",
            n = "Wolf",
            q = 8102,
            o = 1,
            id = 7,
            m = 1426,
            x = 35,
            y = 71.2,
            lvl = 1,
        },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
useRoute("walk-test")
do
    local n, w, back = FG.Pathing:Aim(0, 299.3, 712)
    assert(n and math.abs(n - 299) < 1 and math.abs(w - 742) < 1 and not back, "along the first leg")
    n, w, back = FG.Pathing:Aim(0, 240, 712)
    assert(n and math.abs(w - 742) < 1 and back, "beside the walk: back to it")
    assert(FG.Pathing:Aim(0, 0, 712) == nil, "far from the walk")
    FG:Set("pathMode", "off")
    assert(FG.Pathing:Aim(0, 299.3, 712) == nil, "the walk turned off")
    FG:Set("pathMode", nil)
    assert(FG.Pathing:Aim(0, 400, 790) == nil, "on the last leg: straight to the place")
    assert(FG.Pathing:Aim(0, 299.3, 712) == nil, "a leg passed is not pointed at again")
end
print("walk ok")

-- Shortest Path Forever: the target sent once, with what stands there; while it guides, no
-- walk on the compass ("auto") and the game's waypoint is its; a journey the player replaces is
-- not sent again until picked on the compass; an objective stays held on its quest's area;
-- unlinked, the journey is cancelled.
do
    local sent, current, ended, subscribers = {}, nil, nil, {}
    ShortestPathForever = {
        API = {
            NavigateRoute = function(owner, stops)
                table.insert(sent, { owner = owner, stop = stops[1] })
                current = 1
                return true
            end,
            CurrentStop = function() return current end,
            Ended = function() return ended end,
            Cancel = function()
                current, ended = nil, "cancelled"
                return true
            end,
            Subscribe = function(fn)
                table.insert(subscribers, fn)
                return function() end
            end,
        },
    }
    FG.Pathing:Forget()
    useRoute("walk-test")
    assert(#sent == 1 and sent[1].owner == "Factoruide", "the target sent")
    local stop = sent[1].stop
    assert(stop.map == 1426 and close(stop.x, 0.4) and close(stop.y, 0.712) and stop.kind == "pickup", "where and what")
    waypoint = nil
    FG.Guide:Refresh()
    assert(#sent == 1, "sent once")
    assert(waypoint == nil, "the game's waypoint left to it")
    assert(FG.Pathing:Aim(0, 299.3, 712) == nil, "no walk on the compass while it guides")

    current, ended = nil, "replaced"
    for _, fn in ipairs(subscribers) do
        fn({ ended = true })
    end
    FG.Guide:Refresh()
    assert(#sent == 1, "the player's journey kept")
    assert(waypoint ~= nil, "the game's waypoint back")
    FG.Pathing:Forget()
    FG.Guide:PointToStep(1)
    assert(#sent == 2, "picked on the compass: sent again")

    onQuest[8102] = true
    FG.Guide:PointToStep(2)
    stop = sent[#sent].stop
    assert(stop.kind == "objective" and stop.hold and stop.questID == 8102 and stop.radius, "an objective held")
    onQuest[8102] = nil

    FG:Set("spfLink", false)
    assert(current == nil and ended == "cancelled", "unlinked: cancelled")
    FG:Set("spfLink", nil)
    ShortestPathForever = nil
    FG.Pathing:Forget()
end
print("shortest path ok")

-- The walk drawn on the maps: what is left of it from where the character is (its closest point
-- on it, then its turns, then the place), dots along it, and where they go on the minimap (north
-- up, or turned with the character).
do
    onQuest[8102] = true
    FG.Guide:PointToStep(2)
    onQuest[8102] = nil
    FG.Guide:PointToStep(1)
    local left = FG.Pathing:Remaining(0, 299.3, 712)
    assert(#left == 4 and math.abs(left[1][1] - 299) < 1 and math.abs(left[1][2] - 712) < 1, "from its closest point")
    assert(left[2][2] == 800 and left[3][1] == 400 and math.abs(left[4][1] - 400) < 1, "its turns, then the place")
    left = FG.Pathing:Remaining(0, 240, 712)
    assert(left[1][1] == 240 and #left == 5, "beside it: from the character")
    FG:Set("pathMode", "off")
    assert(FG.Pathing:Remaining(0, 299.3, 712) == nil, "not drawn when off")
    FG:Set("pathMode", nil)

    local dots = FG.PathLines.Sample({ { 0, 0 }, { 0, 10 } }, 5, 100)
    assert(#dots == 3 and dots[3][2] == 10, "a dot every step")
    assert(#FG.PathLines.Sample({ { 0, 0 }, { 0, 100 } }, 5, 4) == 4, "at most so many")
    local x, y = FG.PathLines.OnMinimap(10, 0, 2, nil)
    assert(close(x, 0) and close(y, 20), "north: up")
    x, y = FG.PathLines.OnMinimap(0, 10, 2, nil)
    assert(close(x, -20) and close(y, 0), "west: left")
    x, y = FG.PathLines.OnMinimap(0, 10, 2, math.pi / 2)
    assert(math.abs(x) < 1e-9 and close(y, 20), "facing west on a turning minimap: west is up")
end
print("walk on the maps ok")

-- The NPC pointed at is marked above its head when its nameplate shows: a place picked on the
-- compass (Marryk Nurribit, NPC 944), on the nameplate showing him, gone with it, back when it
-- shows again.
do
    local realGUID, realExists = UnitGUID, UnitExists
    local plates = { nameplate2 = "Creature-0-1-2-3-299-0000", nameplate3 = "Creature-0-1-2-3-944-0000" }
    UnitGUID = function(u) return plates[u] or realGUID(u) end
    UnitExists = function(u) return plates[u] ~= nil end
    C_NamePlate = { GetNamePlateForUnit = function(u) return plates[u] and CreateFrame("Frame") end }
    local marryk
    for _, p in ipairs(FG.Nearby:Around(0, 299.3, 712)) do
        if p.npc == 944 then marryk = p end
    end
    assert(marryk, "the place knows its NPC")
    -- Out of reach for the arrival test: far from the character.
    marryk.x = 40
    FG.Guide:PointToPlace(marryk)
    assert(FG.NpcMark:Unit() == "nameplate3", "marked on his nameplate")
    plates.nameplate3 = nil
    fire("NAME_PLATE_UNIT_REMOVED", "nameplate3")
    assert(FG.NpcMark:Unit() == nil, "gone with it")
    plates.nameplate3 = "Creature-0-1-2-3-944-0000"
    fire("NAME_PLATE_UNIT_ADDED", "nameplate3")
    assert(FG.NpcMark:Unit() == "nameplate3", "back with it")
    FG.Guide:PointToStep(1)
    assert(FG.NpcMark:Unit() == nil, "another target: no mark")
    UnitGUID, UnitExists, C_NamePlate = realGUID, realExists, nil
    FG.Nearby:Forget()
end
print("npc mark ok")

-- The arrow (off by default): the chevron's frame for a turn (64 frames, 8 a row, counter-
-- clockwise), on the guide's place when turned on.
do
    local f, left, right, top, bottom = FG.Arrow.Frame(0)
    assert(f == 0 and left == 0 and right == 0.125 and top == 0 and bottom == 0.125, "straight ahead: the first")
    f, left, _, top = FG.Arrow.Frame(math.pi / 2)
    assert(f == 16 and left == 0 and top == 0.25, "a quarter turn left: the third row")
    assert(FG.Arrow.Frame(2 * math.pi - 0.01) == 0, "almost a full turn: ahead again")
    assert(FG.Arrow:Target() == nil, "off by default")
    FG:Set("arrowShown", true)
    local t = FG.Arrow:Target()
    assert(t and t.m == 1426 and t.x == 40, "on the guide's place")
    FG:Set("arrowShown", nil)
    assert(FG.Arrow:Target() == nil, "off again")
end
print("arrow ok")

-- The quest panel: one line a quest in both modes; folded, no line.
local logEntries, logInfo = C_QuestLog.GetNumQuestLogEntries, C_QuestLog.GetInfo
C_QuestLog.GetNumQuestLogEntries = function() return 6 end
C_QuestLog.GetInfo = function(i) return { questID = 8100 + i, title = "Quest " .. i, level = 1 } end
FG:Set("trackerAll", true)
FG.Tracker:Refresh()
assert(#FG.Tracker.shown == 6 and FG.Tracker.lines == 6, "a line a quest, got " .. FG.Tracker.lines)
FG:Save("guideMode", "minimal")
FG.Tracker:Refresh()
assert(FG.Tracker.lines == 6, "the same with the minimal guide")
FG:Save("guideMode", nil)
-- The guide's button hides the whole panel (kept), the bank shows it while open.
FG.Tracker:Toggle()
FG.Tracker:Refresh()
assert(not FG.Tracker.visible and FactoruideDB.settings.questPanel == false, "hidden by the guide's button")
FG.Tracker:ShowItems(true)
assert(FG.Tracker.visible, "shown at the bank")
FG.Tracker:ShowItems(false)
assert(not FG.Tracker.visible, "hidden again after the bank")
FG.Tracker:Toggle()
FG.Tracker:Refresh()
assert(FG.Tracker.visible, "shown again")
FG:Set("trackerAll", false)
C_QuestLog.GetNumQuestLogEntries, C_QuestLog.GetInfo = logEntries, logInfo
print("quest panel ok")

-- The windows' size: Ctrl + mouse wheel only, the wheel alone is the camera's.
local scaleBefore = FG:Get("scale")
IsControlKeyDown = function() return false end
FG.Card:Frame():OnMouseWheel(1)
assert(FG:Get("scale") == scaleBefore, "the wheel alone does not resize")
IsControlKeyDown = function() return true end
FG.Card:Frame():OnMouseWheel(1)
assert(FG:Get("scale") > scaleBefore, "Ctrl + wheel resizes")
FG:Save("scale", nil)
print("window size ok")

-- Items to keep: the common items of the quests not turned in yet (not quest items), in the bags
-- and the bank; the bank is read when opened, an item can be marked by hand as being there.
FG.Routes["keep-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "accept", t = "Accept Cloth", q = 9001, id = 1, m = 1426, x = 29.93, y = 71.2, lvl = 1 },
        {
            k = "objective",
            t = "Linen Cloth x10",
            q = 9001,
            o = 1,
            e = "item",
            id = 2589,
            n = "Linen Cloth",
            c = 10,
            lvl = 1,
        },
        { k = "objective", t = "Wolf Meat x8", q = 9001, o = 2, e = "item", id = 750, n = "Wolf Meat", lvl = 1 },
        { k = "turnin", t = "Turn in Cloth", q = 9001, id = 1, m = 1426, x = 29.93, y = 71.2, lvl = 1 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
itemClasses[2589] = 7
onQuest[9001] = true
useRoute("keep-test")
local keep = FG.Bank:Items()
assert(#keep == 1 and keep[1].id == 2589 and keep[1].need == 10, "the cloth, not the quest item")
bags[2589], bank[2589] = 4, 6
assert(FG.Bank:Items()[1].bags == 4 and FG.Bank:Items()[1].bank == nil, "the bank not seen yet")
FG.Bank:ToggleMark(2589)
assert(FG.Bank:Items()[1].marked, "marked by hand as in the bank")
fire("BANKFRAME_OPENED")
local atBank = FG.Bank:Items()[1]
assert(atBank.bank == 6 and not atBank.marked, "the bank read, the mark no longer needed")
assert(atBank.hint == "take", "needed soon: to take")
FG.Tracker:Refresh()
assert(FG.Tracker.tab == "items", "the bank shows the items tab")
fire("BANKFRAME_CLOSED")
FG.Tracker:Refresh()
assert(FG.Tracker.tab == "quests" and FG.Bank:Items()[1].hint == nil, "back to the quests")
-- An item needed much later shows once owned (or when all are asked for).
UnitLevel = function() return 1 end
FG.Routes["keep-test"].steps[3] =
    { k = "objective", t = "Silk x4", q = 9001, o = 2, e = "item", id = 4306, n = "Silk", c = 4, lvl = 20 }
itemClasses[4306] = 7
useRoute("keep-test")
local shownNow, later = FG.Bank:Items()
assert(#shownNow == 1 and later == 1, "the silk needed at 20 waits")
assert(#FG.Bank:Items(true) == 2, "all of them when asked")
bags[4306] = 2
assert(#FG.Bank:Items() == 2, "owned: shown")
bags[4306] = nil
UnitLevel = function() return playerLevel end
completed[9001], onQuest[9001] = true, nil
assert(#FG.Bank:Items(true) == 0, "turned in: nothing to keep")
completed[9001], bags[2589], bank[2589] = nil, nil, nil
print("items to keep ok")

-- Gone back to a step already done: the minimal strip still shows it, marked done.
onQuest[9001] = true
FG:Save("guideMode", "minimal")
useRoute("keep-test")
FG.Guide:Go(1)
local strip = FG.Mini:Frame()
assert(strip.name:GetText() == "Accept Cloth", "the step gone back to: " .. tostring(strip.name:GetText()))
assert(strip.verb:GetText():find("faite", 1, true), "marked done: " .. strip.verb:GetText())
FG:Save("guideMode", nil)
onQuest[9001] = nil
print("minimal strip on a done step ok")

-- The next steps show those done already (greyed), not hidden.
FG.Routes["next-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "accept", t = "Accept A", q = 9101, id = 1, m = 1426, x = 10, y = 10, lvl = 1 },
        { k = "accept", t = "Accept B", q = 9102, id = 2, m = 1426, x = 50, y = 50, lvl = 1 },
        { k = "accept", t = "Accept C", q = 9103, id = 3, m = 1426, x = 60, y = 60, lvl = 1 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
onQuest[9102] = true
useRoute("next-test")
local nextSteps = {}
for _, v in ipairs(FG.Guide.view.next) do
    if v.index then nextSteps[v.index] = v end
end
assert(nextSteps[2] and nextSteps[2].done, "B, done already, listed as done")
assert(nextSteps[3] and not nextSteps[3].done, "C to do")
onQuest[9102] = nil
print("done next steps ok")

-- Pages: two objectives close together are one page, kept until both are done; the turn-in of
-- their quest, elsewhere, is the next page and does not show them again (its progress instead).
FG.Routes["page-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        {
            k = "objective",
            t = "Kill Black Ravager",
            n = "Black Ravager",
            q = 9201,
            o = 1,
            id = 11,
            m = 1431,
            x = 66.8,
            y = 30.5,
            lvl = 1,
        },
        {
            k = "objective",
            t = "Kill Young Black Ravager",
            n = "Young Black Ravager",
            q = 9201,
            o = 2,
            id = 12,
            m = 1431,
            x = 68.3,
            y = 28.3,
            lvl = 1,
        },
        { k = "turnin", t = "Turn in Ira's Dagger", q = 9201, id = 13, m = 1431, x = 75, y = 45, lvl = 1 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
local ravagers = {
    { text = "Black Ravager slain: 0/10", numFulfilled = 0, numRequired = 10, finished = false },
    { text = "Young Black Ravager slain: 0/10", numFulfilled = 0, numRequired = 10, finished = false },
}
local pageObjectives = C_QuestLog.GetQuestObjectives
C_QuestLog.GetQuestObjectives = function(q) return q == 9201 and ravagers or pageObjectives(q) end
onQuest[9201] = true
useRoute("page-test")
local function page()
    local out = {}
    for _, v in ipairs(FG.Guide.view.tasks) do
        table.insert(out, v.index)
    end
    return table.concat(out, ",")
end
assert(page() == "1,2", "the two objectives together: " .. page())
ravagers[1].numFulfilled, ravagers[1].finished = 10, true
fire("QUEST_LOG_UPDATE")
runTimers()
assert(page() == "1,2" and char.guide.step == 2, "the page stays until both are done: " .. page())
Press("next")
assert(page() == "3", "the turn-in alone, not its objectives again: " .. page())
local turnin = FG.Guide.view.tasks[1]
assert(turnin.have == 10 and turnin.need == 20, "the turn-in shows how far the quest is")
Press("prev")
assert(page() == "1,2", "back to the objectives' page: " .. page())
C_QuestLog.GetQuestObjectives = pageObjectives
onQuest[9201] = nil
print("pages ok")

-- Objectives whose names hold one another: "Night Web Spider" is not "Young Night Web Spider",
-- whichever comes first in the log and whatever the text's form (Forever's or Classic's).
FG.Routes["spider-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        {
            k = "objective",
            t = "Kill Young Night Web Spider x10",
            n = "Young Night Web Spider",
            q = 9301,
            o = 1,
            e = "npc",
            id = 1504,
            m = 1431,
            x = 28.6,
            y = 57.7,
            lvl = 1,
        },
        {
            k = "objective",
            t = "Kill Night Web Spider x8",
            n = "Night Web Spider",
            q = 9301,
            o = 2,
            e = "npc",
            id = 1505,
            m = 1431,
            x = 24.8,
            y = 59.7,
            lvl = 1,
        },
        { k = "turnin", t = "Turn in Night Web's Hollow", q = 9301, id = 13, m = 1431, x = 32, y = 66, lvl = 1 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
for _, texts in ipairs({
    { "10/10 Young Night Web Spider slain", "2/8 Night Web Spider slain", young = 1 },
    { "Young Night Web Spider slain: 10/10", "Night Web Spider slain: 2/8", young = 1 },
    { "2/8 Night Web Spider slain", "10/10 Young Night Web Spider slain", young = 2 },
}) do
    local spiders = {}
    for i, text in ipairs(texts) do
        local young = i == texts.young
        spiders[i] = {
            text = text,
            numFulfilled = young and 10 or 2,
            numRequired = young and 10 or 8,
            finished = young,
        }
    end
    C_QuestLog.GetQuestObjectives = function(q) return q == 9301 and spiders or pageObjectives(q) end
    onQuest[9301] = true
    useRoute("spider-test")
    FG.Guide:Resync()
    assert(char.guide.step == 2, "the older spiders still to kill (" .. texts[1] .. "), step " .. char.guide.step)
    local task = FG.Guide.view.tasks[1]
    assert(task.have == 2 and task.need == 8, "their own count: " .. tostring(task.have) .. "/" .. tostring(task.need))
end
C_QuestLog.GetQuestObjectives = pageObjectives
onQuest[9301] = nil
print("objectives by name ok")

-- Texts: in the language chosen in the settings, else the game's; English without a
-- translation; Mexican Spanish as Spain's.
assert(FG.T("Accept") == "Prendre", "the game's language: " .. FG.T("Accept"))
FG:Save("locale", "deDE")
assert(FG.T("Accept") == "Annehmen" and FG.T("No such text") == "No such text", "the chosen language")
FG:Save("locale", "esMX")
assert(FG.T("Accept") == "Aceptar", "Mexican Spanish")
FG:Save("locale", "enUS")
assert(FG.T("Accept") == "Accept", "English")
FG:Save("locale", "xxXX")
assert(FG.T("Accept") == "Prendre", "an unknown language: the game's")
FG:Save("locale", nil)
print("languages ok")

-- The tracker lists the guide's quests in progress even when the quest log lists none
-- (headers collapsed in the quest log window).
FG.Routes["tracker-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "accept", t = "Accept Logged", q = 7101, lvl = 1 },
        { k = "turnin", t = "Turn in Logged", q = 7101, lvl = 1 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
useRoute("tracker-test")
onQuest[7101] = true
local entries = C_QuestLog.GetNumQuestLogEntries
C_QuestLog.GetNumQuestLogEntries = function() return 0 end
FG.Tracker:Refresh()
assert(FG.Tracker.shown[1] == 7101, "guide quest listed without the log")
C_QuestLog.GetNumQuestLogEntries = entries
onQuest[7101] = nil
print("tracker from the guide ok")

-- During combat, a box made protected by a secure button hanging on it (a quest item button)
-- keeps its size: the game would block the change. Resized after the fight.
local box = FG.Guide:Box()
local resized = false
box.SetHeight = function() resized = true end
box.IsProtected = function() return true end
InCombatLockdown = function() return true end
FG.Guide:Refresh()
assert(not resized, "a protected box resized during combat")
InCombatLockdown = function() return false end
fire("PLAYER_REGEN_ENABLED")
assert(resized, "resized after the fight")
print("combat layout ok")

-- Every window moves when dragged from anywhere on it, its rows and buttons too (GitHub #1); where
-- it was dropped is saved. Nothing moves while the windows are locked, nor a window the game
-- protects during combat. A click on a row still acts, not the release that ends a drag.
local function named(name)
    for _, f in ipairs(frames) do
        if f:GetName() == name then return f end
    end
end
local function childOf(window, test)
    for _, f in ipairs(frames) do
        local p = rawget(f, "_parent")
        while type(p) == "table" and p ~= window do
            p = rawget(p, "_parent")
        end
        if p == window and rawget(f, "OnDragStart") and test(f) then return f end
    end
end
local function watchMoves(window)
    local d = { moves = 0 }
    window.StartMoving = function() d.moves = d.moves + 1 end
    window.StopMovingOrSizing = function() d.stops = (d.stops or 0) + 1 end
    window.GetLeft = function() return 120 end
    window.GetTop = function() return 640 end
    window.GetPoint = function() return "TOPLEFT", UIParent, "BOTTOMLEFT", 120, 640 end
    return d
end
local function drag(handle)
    handle.OnDragStart(handle, "LeftButton")
    handle.OnDragStop(handle)
end
local realDragTime = GetTime
local dragNow = 100
GetTime = function() return dragNow end
local pointed
local realPointToStep = FG.Guide.PointToStep
FG.Guide.PointToStep = function(_, index) pointed = index end

local card = FG.Card:Frame()
local cardMoves = watchMoves(card)
local stepRow = card.rows[1]
assert(stepRow and stepRow.index and stepRow.OnDragStart, "the card's first step row passes drags on")
FG.Guide:State().point = nil
drag(stepRow)
local saved = FG.Guide:State().point
assert(cardMoves.moves == 1 and cardMoves.stops == 1, "dragging a step row moves the card")
assert(saved and saved[1] == "TOPLEFT" and saved[4] == 120 and saved[5] == 640, "the card's place saved")
pointed = nil
stepRow.OnClick(stepRow, "LeftButton")
assert(pointed == nil, "the release ending the drag is no click")
dragNow = dragNow + 1
stepRow.OnClick(stepRow, "LeftButton")
assert(pointed == stepRow.index, "a click on the row still points at its step")
assert(rawget(stepRow, "OnEnter"), "the row keeps its tooltip")
FG:Set("lockFrames", true)
FG.Guide:State().point = nil
drag(stepRow)
drag(card)
assert(cardMoves.moves == 1 and FG.Guide:State().point == nil, "locked windows do not move")
FG:Set("lockFrames", nil)
drag(card)
assert(cardMoves.moves == 2, "the card itself still moves")

-- The quest panel: its lines move it; protected by its secure quest item buttons, not in combat.
local tracker = named("FactoruideTracker")
local trackerMoves = watchMoves(tracker)
local trackerLine = childOf(tracker, function(f) return rawget(f, "track") ~= nil end)
assert(trackerLine, "the quest panel's lines pass drags on")
local secureItem = named("FactoruideItemButton1")
assert(not secureItem or not rawget(secureItem, "OnDragStart"), "secure quest item buttons left alone")
FG.Guide:State().trackerPoint = nil
drag(trackerLine)
assert(trackerMoves.moves == 1, "dragging a quest line moves the quest panel")
assert(FG.Guide:State().trackerPoint and FG.Guide:State().trackerPoint[4] == 120, "the quest panel's place saved")
tracker.IsProtected = function() return true end
InCombatLockdown = function() return true end
drag(trackerLine)
assert(trackerMoves.moves == 1, "a protected window does not move during combat")
InCombatLockdown = function() return false end
drag(trackerLine)
assert(trackerMoves.moves == 2, "it moves again after the fight")
tracker.IsProtected = nil
FG.Guide:State().trackerPoint = nil
tracker.placed = nil
FG.Tracker:Refresh()

-- The minimal guide: its strip, the next steps under it and the compass over it move the strip.
Press("mode")
local strip = FG.Mini:Frame()
local stripMoves = watchMoves(strip)
local drop = named("FactoruideMiniNext")
local dropRow = childOf(drop, function(f) return rawget(f, "dot") ~= nil end)
assert(dropRow, "the next steps under the strip pass drags on")
FG.Guide:State().miniPoint = nil
drag(dropRow)
assert(stripMoves.moves == 1, "dragging the next steps moves the strip")
assert(FG.Guide:State().miniPoint and FG.Guide:State().miniPoint[4] == 120, "the strip's place saved")
pointed = nil
strip.OnMouseUp(strip, "LeftButton")
assert(pointed == nil, "dropping the strip does not point at its step")
dragNow = dragNow + 1
strip.OnMouseUp(strip, "LeftButton")
assert(pointed ~= nil and pointed == strip.index, "a click on the strip still points at its step")
local compass = named("FactoruideCompass")
local compassMoves = watchMoves(compass)
drag(compass)
assert(stripMoves.moves == 2 and compassMoves.moves == 0, "the compass over the strip moves the strip")
Press("mode")
drag(compass)
assert(compassMoves.moves == 1, "on its own, the compass moves itself")
local compassPoint = FG.Guide:State().compassPoint
assert(compassPoint and compassPoint[4] == 120, "the compass's place saved")
FG.Guide:State().compassPoint, FG.Guide:State().miniPoint = nil, nil
FG.Guide.PointToStep = realPointToStep
GetTime = realDragTime
print("drag anywhere ok")

-- The compass: its ends dragged change its width (its middle stays), Ctrl + wheel its own size,
-- kept for the character; a hint on the bar says how; "Compass back to the top" puts it
-- back; "Lock the windows" locks the windows.
do
    local state = FG.Guide:State()
    compass.GetEffectiveScale = function() return 1 end
    compass.GetCenter = function() return 500, 700 end
    compass.SetWidth = function(self, w) self.width = w end
    compass.ClearAllPoints = function(self) self.points = {} end
    compass.SetPoint = function(self, ...) table.insert(self.points, { ... }) end
    compass.GetPoint = function(self) return unpack(self.points[1]) end
    compass.SetScale = function(self, s) self.scale = s end
    local cursor = 100
    local realCursor = GetCursorPosition
    GetCursorPosition = function() return cursor, 0 end
    local left, right = compass.grips[1], compass.grips[2]
    local function widen(grip, dx)
        cursor = 100
        grip.OnMouseDown(grip, "LeftButton")
        cursor = 100 + dx
        if rawget(grip, "OnUpdate") then grip.OnUpdate(grip, 0.03) end
        grip.OnMouseUp(grip, "LeftButton")
    end
    widen(right, 50)
    assert(compass.width == 520 and state.compassWidth == 520, "the right end dragged out: wider on both sides")
    local p = state.compassPoint
    assert(p and p[1] == "CENTER" and p[4] == 500 and p[5] == 700, "its middle stays where it was")
    widen(left, 30)
    assert(state.compassWidth == 460, "the left end dragged in: narrower")
    widen(left, 10000)
    assert(state.compassWidth == 240 and compass.width == 240, "not narrower than its least width")
    FG:Set("lockFrames", true)
    widen(right, 50)
    assert(compass.width == 240, "locked: its ends do not move")
    FG:Set("lockFrames", nil)
    assert(rawget(right, "OnUpdate") == nil, "no sizing left running")

    -- Ctrl + wheel over it: its own size, not the other windows'.
    local windows = FG:Get("scale")
    IsControlKeyDown = function() return false end
    compass.OnMouseWheel(compass, 1)
    assert(state.compassScale == nil, "the wheel alone is the camera's")
    IsControlKeyDown = function() return true end
    compass.OnMouseWheel(compass, 1)
    compass.OnMouseWheel(compass, 1)
    assert(state.compassScale == 1.1 and FG:Get("scale") == windows, "Ctrl + wheel: the compass alone grows")
    assert(math.abs(compass.scale - windows * 1.1) < 1e-9, "its size on top of the windows' size")
    IsControlKeyDown = function() return false end

    -- The hint on the bar.
    local lines = {}
    GameTooltip.AddLine = function(_, text) table.insert(lines, text) end
    local function hint()
        lines = {}
        compass.OnEnter(compass)
        return table.concat(lines, "\n")
    end
    assert(
        hint():find("Glisser : la déplacer", 1, true) and hint():find("Ctrl + molette", 1, true),
        "the hint says how"
    )
    Press("lock")
    assert(FG:Get("lockFrames") == true and FactoruideDB.settings.lockFrames == true, "lock button: kept")
    assert(
        hint():find("Verrouillée", 1, true) and not hint():find("Glisser : la déplacer", 1, true),
        "locked: the hint says so"
    )
    local moves = compassMoves.moves
    drag(compass)
    assert(compassMoves.moves == moves, "locked: it does not move")
    Press("lock")
    assert(FG:Get("lockFrames") == false, "lock button again: unlocked")
    FG:Save("lockFrames", nil)
    GameTooltip.AddLine = nil

    -- Back at the top, at its first size.
    Press("compassReset")
    assert(state.compassPoint == nil and state.compassWidth == nil and state.compassScale == nil, "reset: forgotten")
    assert(compass.width == 420 and math.abs(compass.scale - windows) < 1e-9, "reset: its first width and size")
    p = compass.points[1]
    assert(p and p[1] == "TOP" and p[5] == -14, "reset: at the top of the screen")
    GetCursorPosition = realCursor
end
print("compass layout ok")

-- "Hide the addon's windows" (`/fg`) hides every window and mark of the addon at once (one parent
-- frame), for the session; clicked again it brings them back as they were. Progress and settings
-- are kept.
FG.Routes["visibility-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "accept", t = "Accept Logged", q = 7101, lvl = 1, m = 1426, x = 45, y = 50 },
        { k = "grind", t = "Grind", lvl = 60, m = 1426, x = 46, y = 51 },
    },
}
useRoute("visibility-test")
local root = FG.UIRoot
local stepBeforeHide = char.guide.step
local originalSetShown = root.SetShown
local visibilityWrites = 0
root.SetShown = function(self, shown)
    assert(not InCombatLockdown(), "the addon's parent frame changed during combat")
    visibilityWrites = visibilityWrites + 1
    originalSetShown(self, shown)
end
local oldItemInfo, oldQuestIndex = GetQuestLogSpecialItemInfo, GetQuestLogIndexByID
GetQuestLogIndexByID = function() return 1 end
GetQuestLogSpecialItemInfo = function() return "item:123", 123 end
FG.ItemButton:SetSteps({ { q = 7101 } })
GetQuestLogSpecialItemInfo, GetQuestLogIndexByID = oldItemInfo, oldQuestIndex
-- The XP bar mark sits on the game's XP bar, not on the addon's parent.
MainMenuExpBar = CreateFrame("Frame", "MainMenuExpBar")
MainMenuExpBar.GetWidth = function() return 500 end
MainMenuExpBar.GetFrameLevel = function() return 1 end
FG.XpMarker:Refresh()
local xpMark
for _, f in ipairs(frames) do
    if f:GetParent() == MainMenuExpBar then xpMark = f end
end
assert(xpMark and xpMark:IsShown(), "the XP bar mark shown")
local owned = {}
for _, f in ipairs(frames) do
    if f:GetParent() == root and f:GetName() then owned[f:GetName()] = f end
end
for _, name in ipairs({
    "FactoruideTodo",
    "FactoruideMini",
    "FactoruideButtons",
    "FactoruideCompass",
    "FactoruideWind",
    "FactoruideTracker",
    "FGItemButton",
}) do
    assert(owned[name], name .. " hangs on the addon's parent frame")
end
assert(FG.Card:Frame():IsVisible(), "the guide visible before hiding")
local popupHidden
StaticPopup_Hide = function(which) popupHidden = which end
Press("hide")
assert(not root:IsShown() and not FG:Get("uiShown"), "hidden")
assert(popupHidden == "FACTORUIDE_RESET", "the guide's reset dialog closed")
FG.Guide:Refresh()
FG.Tracker:Refresh()
FG.Talents:Refresh()
FG.XpMarker:Refresh()
for _, f in pairs(owned) do
    assert(not f:IsVisible(), "a refresh does not bring back " .. f:GetName())
end
assert(not xpMark:IsShown(), "no XP bar mark while hidden")
assert(FG:Get("guideShown") and FG:Get("questPanel") and FG:Get("guideMode") == "full", "settings kept")
assert(char.guide.step == stepBeforeHide, "progress kept")
assert(FactoruideDB.settings.uiShown == nil, "hidden for the session only")
Press("hide")
assert(root:IsShown() and FG:Get("uiShown"), "shown again")
assert(FG.Card:Frame():IsVisible() and xpMark:IsShown(), "the guide and the XP bar mark back")
-- "Hide the guide" stays the guide's own switch: shown again with the rest, it stays hidden.
Press("guide")
Press("hide")
Press("hide")
assert(not FG:Get("guideShown") and root:IsShown(), "the guide alone still hidden")
assert(not FG.Card:Frame():IsShown(), "the guide's card stays hidden")
Press("guide")
assert(FG.Card:Frame():IsVisible(), "the guide back")

-- In combat (the quest item buttons are secure), the change waits for the end of the fight; the
-- button says what comes, the last click wins.
InCombatLockdown = function() return true end
local writesBeforeCombat = visibilityWrites
Press("hide")
assert(root:IsShown() and FG:Get("uiShown"), "hide waits during combat")
assert(visibilityWrites == writesBeforeCombat, "nothing changed during combat")
assert(not FG:UIWanted(), "hidden after the fight")
InCombatLockdown = function() return false end
fire("PLAYER_REGEN_ENABLED")
assert(not root:IsShown(), "hidden after the fight")
InCombatLockdown = function() return true end
Press("hide")
Press("hide")
InCombatLockdown = function() return false end
fire("PLAYER_REGEN_ENABLED")
assert(not root:IsShown(), "the last click wins")
InCombatLockdown = function() return true end
Press("hide")
assert(not root:IsShown(), "show waits during combat")
InCombatLockdown = function() return false end
fire("PLAYER_REGEN_ENABLED")
assert(root:IsShown(), "shown after the fight")
fire("PLAYER_REGEN_ENABLED")
assert(root:IsShown(), "nothing pending any more")
root.SetShown = originalSetShown

-- The world map's pins and the game's waypoint go at once and stay away on later refreshes.
-- `pins`: the steps' pins (the walk's dots have their own template).
local provider, pins = nil, 0
WorldMapFrame = {
    AddDataProvider = function(self, p)
        provider = provider or p
        p.GetMap = function() return self end
    end,
    IsShown = function() return true end,
    RemoveAllPinsByTemplate = function(_, template)
        if template == "FGStepPinTemplate" then pins = 0 end
    end,
    GetMapID = function() return 1426 end,
    AcquirePin = function(_, template)
        if template == "FGStepPinTemplate" then pins = pins + 1 end
    end,
}
FG.MapPins:Refresh()
assert(pins > 0 and waypoint, "map pins and waypoint shown")
Press("hide")
assert(not waypoint, "the waypoint cleared on hide")
FG.Guide:Refresh()
assert(not waypoint, "a refresh does not set the waypoint again while hidden")
assert(pins == 0, "map pins removed on hide")
provider:RefreshAllData()
assert(pins == 0, "a map refresh adds no pin while hidden")

-- No safety alert while hidden.
local alerts = 0
UIErrorsFrame = { AddMessage = function() alerts = alerts + 1 end }
local health = 10
UnitHealth = function() return health end
UnitHealthMax = function() return 100 end
UnitIsDeadOrGhost = function() return false end
fire("UNIT_HEALTH", "player")
assert(alerts == 0, "no low health alert while hidden")
Press("hide")
assert(pins > 0 and waypoint, "map pins and waypoint back on show")
health = 100
fire("UNIT_HEALTH", "player")
health = 10
fire("UNIT_HEALTH", "player")
assert(alerts == 1, "the low health alert back on show")
WorldMapFrame.IsShown = function() return false end
UIErrorsFrame, UnitHealth, UnitHealthMax, UnitIsDeadOrGhost, MainMenuExpBar = nil, nil, nil, nil, nil
StaticPopup_Hide = nil
FG.XpMarker:Refresh()
print("whole addon hide and show ok")

-- Classic Era (1.15) has no C_QuestLog log functions: the quest log is read through the older
-- globals, and the guide still draws.
local modern = {}
for _, name in ipairs({
    "GetNumQuestLogEntries",
    "GetInfo",
    "GetLogIndexForQuestID",
    "GetTitleForQuestID",
    "IsComplete",
}) do
    modern[name] = C_QuestLog[name]
    C_QuestLog[name] = nil
end
local classicLog = {
    { "Northshire", nil, nil, true, false, nil, nil, 0 },
    { "A Threat Within", 1, nil, false, false, 1, nil, 783 },
    { "Wolves Across the Border", 2, nil, false, false, nil, nil, 33 },
    { "Lost Necklace", 3, nil, false, false, -1, nil, 85 },
}
GetNumQuestLogEntries = function() return #classicLog, 3 end
GetQuestLogTitle = function(i) return unpack(classicLog[i] or {}) end
GetQuestLogIndexByID = function(q)
    for i, e in ipairs(classicLog) do
        if e[8] == q then return i end
    end
    return 0
end
local log = FG.QuestLog
assert(log.GetNumQuestLogEntries() == 4, "entries")
assert(log.GetInfo(1).isHeader and log.GetInfo(2).questID == 783, "headers and quests")
assert(log.GetLogIndexForQuestID(33) == 3 and log.GetLogIndexForQuestID(99) == nil, "log index")
assert(log.GetTitleForQuestID(33) == "Wolves Across the Border", "title")
assert(log.IsComplete(783) and not log.IsComplete(33) and log.IsFailed(85), "complete and failed")
FG.Guide:Refresh()
FG.Tracker:Refresh()
for name, fn in pairs(modern) do
    C_QuestLog[name] = fn
end
print("classic era quest log ok")

-- Collector: rows as the contribution format expects them, on Forever and Classic payloads.
-- Rows are strings (seq|time|event|quest|entityType|entityID|map|x|y|level|data): decoded here
-- into the tables they replace, as the app reads them (keys typed as in its schemas).
local BOOL_KEYS = { complete = true, trivial = true, repeatable = true, elite = true, pet = true, died = true }
local PAIR_KEYS = { items = true, nodes = true }
local TEXT_KEYS = { title = true, name = true, mode = true }
local function decodeRow(row)
    if type(row) ~= "string" then return row end
    local f = {}
    for field in (row .. "|"):gmatch("([^|]*)|") do
        f[#f + 1] = field
    end
    assert(#f == 11, "11 fields: " .. row)
    local function num(v) return v ~= "" and assert(tonumber(v), row) or false end
    local data = false
    if f[11] ~= "" then
        data = {}
        for pair in f[11]:gmatch("[^;]+") do
            local k, v = pair:match("^([^=]+)=(.*)$")
            assert(k and data[k] == nil, "data pair: " .. pair)
            if BOOL_KEYS[k] then
                assert(v == "1", "booleans written as 1: " .. pair)
                data[k] = true
            elseif PAIR_KEYS[k] then
                data[k] = {}
                for a, b in v:gmatch("(%d+):(%d+)") do
                    table.insert(data[k], { tonumber(a), tonumber(b) })
                end
            elseif TEXT_KEYS[k] then
                data[k] = v:gsub("%%(%x%x)", function(h) return string.char(tonumber(h, 16)) end)
            else
                data[k] = assert(tonumber(v), "number: " .. pair)
            end
        end
    end
    local entityType = f[5] ~= "" and f[5] or false
    return {
        num(f[1]),
        num(f[2]),
        f[3],
        num(f[4]),
        entityType,
        num(f[6]),
        num(f[7]),
        num(f[8]),
        num(f[9]),
        num(f[10]),
        data,
    }
end
local function lastRow(event)
    for i = #char.events, 1, -1 do
        local row = decodeRow(char.events[i])
        if row[3] == event then return row, i end
    end
end
local function rowsSince(event, from)
    local out = {}
    for i = from + 1, #char.events do
        local row = decodeRow(char.events[i])
        if row[3] == event then table.insert(out, row) end
    end
    return out
end
local accepted = lastRow("accept")
assert(accepted and accepted[4] == 179, "Forever accept: (questID)")

-- Classic Era and TBC: QUEST_ACCEPTED gives (questLogIndex, questID), the log has no C_QuestLog.
for name in pairs(modern) do
    C_QuestLog[name] = nil
end
fire("QUEST_ACCEPTED", 3, 33)
accepted = lastRow("accept")
assert(accepted[4] == 33, "Classic accept: quest ID, not the log index: " .. tostring(accepted[4]))
assert(accepted[11].title == "Wolves Across the Border" and accepted[11].level == 2, "Classic accept title")

-- Quest greeting: Classic Era and TBC have no GetActiveQuestID, the title is found in the log.
local greetingActive = { "Wolves Across the Border", "Not In The Log" }
GetNumActiveQuests = function() return #greetingActive end
GetActiveTitle = function(i) return greetingActive[i], false end
GetActiveQuestID = nil
greeting = {}
fire("QUEST_GREETING")
local active = lastRow("active")
assert(active and active[4] == 33 and active[5] == "npc" and active[6] == 658, "Classic greeting active quest")
GetActiveQuestID = function(i) return i == 1 and 783 or 0 end
fire("QUEST_GREETING")
assert(lastRow("active")[4] == 783, "Forever greeting active quest")
GetNumActiveQuests = function() return 0 end

-- Classic Era: the completed quests come from GetQuestsCompleted.
local allCompleted = C_QuestLog.GetAllCompletedQuestIDs
C_QuestLog.GetAllCompletedQuestIDs = nil
GetQuestsCompleted = function() return { [33] = true, [12] = true } end
fire("PLAYER_LOGOUT")
assert(char.completed[1] == 12 and char.completed[2] == 33 and #char.completed == 2, "Classic completed quests")
C_QuestLog.GetAllCompletedQuestIDs = allCompleted
for name, fn in pairs(modern) do
    C_QuestLog[name] = fn
end
print("collector quests ok")

-- Turn-ins are not recorded as removed, in either order; an abandon is.
runTimers() -- those queued so far (tracker, party, talents...)
local mark = #char.events
fire("QUEST_TURNED_IN", 4001, 100, 0)
fire("QUEST_REMOVED", 4001)
fire("QUEST_REMOVED", 4002) -- Classic: removed before turned in
fire("QUEST_TURNED_IN", 4002, 100, 0)
fire("QUEST_REMOVED", 4003)
runTimers()
local removed = rowsSince("removed", mark)
assert(#removed == 1 and removed[1][4] == 4003 and removed[1][11] == false, "only the abandon is removed")
print("collector removed ok")

-- Loot: read before fast loot takes the slots, one row per corpse with item quantities (AoE
-- loot), money skipped, a window read once.
local slots = {
    { link = nil, sources = { "Creature-0-1-2-3-299-00A", 1 } }, -- money
    { link = "|Hitem:750::|h", sources = { "Creature-0-1-2-3-299-00A", 1, "Creature-0-1-2-3-300-00B", 2 } },
    { link = "|Hitem:2589::|h", sources = { "Creature-0-1-2-3-299-00A", 3 } },
}
GetNumLootItems = function() return #slots end
GetLootSlotLink = function(i) return slots[i] and slots[i].link end
GetLootSourceInfo = function(i) return unpack(slots[i] and slots[i].sources or {}) end
LootSlot = function(i) slots[i] = nil end
local clock = GetTime
GetTime = function() return clock() + 5000 end
mark = #char.events
fire("LOOT_READY", false)
assert(not slots[1] and not slots[2] and not slots[3], "fast loot took the slots")
local loot = rowsSince("loot", mark)
assert(#loot == 2, "one loot row per corpse, got " .. #loot)
local a, b = loot[1][11].items, loot[2][11].items
assert(
    loot[1][6] == 299 and #a == 2 and a[1][1] == 750 and a[1][2] == 1 and a[2][1] == 2589 and a[2][2] == 3,
    "corpse A"
)
assert(loot[2][6] == 300 and #b == 1 and b[1][1] == 750 and b[1][2] == 2, "corpse B: its own quantity")
fire("LOOT_READY", false)
fire("LOOT_OPENED", false)
assert(#rowsSince("loot", mark) == 2, "a loot window recorded once")
GetTime = clock
print("collector loot ok")

-- Flight master: node IDs and states only, bounded; Classic: the continent's map.
mark = #char.events
fire("TAXIMAP_OPENED")
local taxi = rowsSince("taxi", mark)[1][11]
assert(taxi.map == 1415 and taxi.nodes[1][1] == 6 and taxi.nodes[1][2] == 0 and taxi.nodes[3][2] == 2, "taxi nodes")
local manyNodes = {}
for i = 1, 200 do
    manyNodes[i] = { nodeID = i, name = "A long flight master name " .. i, slotIndex = i }
end
C_TaxiMap.GetAllTaxiNodes = function() return manyNodes end
TaxiNodeGetType = function(slot) return slot == 1 and "CURRENT" or slot == 2 and "REACHABLE" or "DISTANT" end
GetTaxiMapID = nil
fire("TAXIMAP_OPENED")
taxi = lastRow("taxi")[11]
assert(taxi.map == 1415, "Classic: continent map " .. tostring(taxi.map))
assert(#taxi.nodes == 128 and #taxi.nodes[1] == 2, "nodes bounded, ids and states only")
assert(taxi.nodes[1][2] == 0 and taxi.nodes[2][2] == 1 and taxi.nodes[3][2] == 2, "Classic node states")
print("collector taxi ok")

-- Transit: a boat ride is timed from the docked boat, a hearthstone is told apart.
local now, speed, here = 10000, 7, { 1437, 0.10, 0.10 }
local clockTime = time
time = function() return now end
GetUnitSpeed = function() return speed end
local continents = { [1415] = true, [1414] = true }
C_Map.GetMapInfo = function(m)
    if continents[m] then return { mapID = m, mapType = 2, parentMapID = 947 } end
    return { mapID = m, mapType = 3, parentMapID = m < 1415 and 1414 or 1415 }
end
C_Map.GetBestMapForUnit = function() return here[1] end
C_Map.GetPlayerMapPosition = function()
    return {
        GetXY = function() return here[2], here[3] end,
    }
end
local function sample(t, map, x, y, v)
    now, here, speed = t, { map, x, y }, v
    fire("ZONE_CHANGED_NEW_AREA")
end
mark = #char.events
sample(10000, 1437, 0.10, 0.10, 7) -- walking on the dock
sample(10005, 1437, 0.105, 0.10, 0) -- on the boat
sample(10010, 1437, 0.105, 0.10, 0) -- waiting on board
sample(10015, 1437, 0.12, 0.10, 0) -- the boat leaves
sample(10020, 1437, 0.14, 0.10, 0)
sample(10090, 1411, 0.50, 0.50, 0) -- landed on the other continent
local transit = rowsSince("transit", mark)
assert(#transit == 1, "one transit")
local ride = transit[1][11]
assert(ride.mode == "ride" and ride.seconds == 80 and ride.fromMap == 1437 and ride.fromX == 10.5, "boat ride")
sample(10100, 1411, 0.50, 0.50, 0)
fire("UNIT_SPELLCAST_SUCCEEDED", "player", "Cast-1", 8690)
sample(10130, 1426, 0.30, 0.70, 0)
local hearth = lastRow("transit")[11]
assert(hearth.mode == "hearth" and hearth.seconds == 30 and hearth.fromMap == 1411, "hearthstone")
sample(10200, 1426, 0.30, 0.71, 7) -- walking into a portal
sample(10203, 1411, 0.40, 0.40, 0)
local portal = lastRow("transit")[11]
assert(portal.mode == nil and portal.seconds == 3 and portal.fromY == 71, "walked in: no mode")
time = clockTime
print("collector transit ok")

-- Phase 2 recordings: fights, objective progress, guide steps, flights, spawn ages; Forever
-- secret values degrade to the fallbacks without errors.
local clockNow = 50000
local realGetTime = GetTime
GetTime = function() return clockNow end
GetServerTime = function() return 1000000 end
local function spawnGUID(npc, age) return string.format("Creature-0-1-2-3-%d-%010X", npc, (1000000 - age) % 8388608) end
local guids, levels, classes, exists = {}, {}, {}, {}
local unitGUID, unitLevel = UnitGUID, UnitLevel
UnitGUID = function(u)
    if guids[u] ~= nil then return guids[u] or nil end
    return unitGUID(u)
end
UnitLevel = function(u)
    if u == "player" then return playerLevel end
    return levels[u] or 1
end
UnitExists = function(u) return exists[u] or false end
UnitClassification = function(u) return classes[u] or "normal" end
UnitIsDead = function() return false end
UnitCanAttack = function() return true end
UnitName = function(u) return u == "player" and "Brouz" or "Kobold Vermin" end
UnitReaction = function() return 2 end
local secret = {}
issecretvalue = function(v) return v ~= nil and secret[v] == true end
local xpNow, xpMax = 100, 400
UnitXP = function() return xpNow end
UnitXPMax = function() return xpMax end
COMBATLOG_XPGAIN_FIRSTPERSON = "%s dies, you gain %d experience."
COMBATLOG_XPGAIN_EXHAUSTION1 = "%s dies, you gain %d experience. (%s exp %s bonus)"
playerLevel = 5

-- Target read out of combat: a seen row with its spawn age and elite.
local koboldGUID = spawnGUID(299, 90)
guids.target, levels.target, classes.target, exists.target = koboldGUID, 5, "elite", true
here = { 1426, 0.30, 0.70 }
mark = #char.events
fire("PLAYER_TARGET_CHANGED")
local seenRow = rowsSince("seen", mark)[1]
assert(seenRow and seenRow[6] == 299 and seenRow[11].age == 90 and seenRow[11].elite == true, "seen: age and elite")
assert(seenRow[11].name == "Kobold Vermin" and seenRow[11].level == 5, "seen: name and level")
-- The same NPC close by: one row per 5% of the map; elsewhere, a row without the name again.
guids.mouseover, exists.mouseover = spawnGUID(299, 30), true
here = { 1426, 0.301, 0.701 }
fire("UPDATE_MOUSEOVER_UNIT")
fire("UPDATE_MOUSEOVER_UNIT")
here = { 1426, 0.50, 0.50 }
fire("UPDATE_MOUSEOVER_UNIT")
local seenRows = rowsSince("seen", mark)
assert(#seenRows == 2 and seenRows[2][11].name == nil and seenRows[2][11].age == 30, "seen deduped, name once")
-- A secret GUID (Forever, restricted): nothing recorded, no error.
local hiddenGUID = spawnGUID(300, 5)
secret[hiddenGUID] = true
guids.mouseover = hiddenGUID
here = { 1426, 0.70, 0.70 }
fire("UPDATE_MOUSEOVER_UNIT")
assert(#rowsSince("seen", mark) == 2, "secret GUID: no seen row")
-- A static NPC up since the realm restart: seen without its age.
guids.mouseover = spawnGUID(302, 7200)
here = { 1426, 0.90, 0.90 }
fire("UPDATE_MOUSEOVER_UNIT")
local static = rowsSince("seen", mark)[3]
assert(static and static[6] == 302 and static[11].age == nil, "no age past an hour")
-- Safety alerts on a target whose level is secret (a table here: comparing it errors, as a
-- secret does): not compared, no error (checked below).
local secretLevel = {}
secret[secretLevel] = true
guids.target, levels.target = spawnGUID(301, 5), secretLevel
fire("PLAYER_TARGET_CHANGED")
guids.target, levels.target = koboldGUID, 5
fire("PLAYER_TARGET_CHANGED")

-- Fight 1: the target turns secret when the fight begins (the GUID read before is used), the
-- kill is told by the XP message only (secret attacker on PARTY_KILL).
guids.pet, exists.pet = "Pet-0-1-2-3-416-0000000001", true
here = { 1426, 0.40, 0.40 }
secret[koboldGUID] = true
mark = #char.events
fire("PLAYER_REGEN_DISABLED")
secret["Player-1-2"] = true
fire("PARTY_KILL", "Player-1-2", koboldGUID)
secret["Player-1-2"] = nil
fire("CHAT_MSG_COMBAT_XP_GAIN", "Kobold Vermin dies, you gain 45 experience. (45 exp Rested bonus)")
fire("CHAT_MSG_COMBAT_XP_GAIN", "You gain 120 experience.") -- not a kill
fire("PLAYER_TARGET_DIED")
xpNow, clockNow = 145, clockNow + 12
fire("PLAYER_REGEN_ENABLED")
local fight = rowsSince("combat", mark)[1]
assert(fight and fight[5] == "npc" and fight[6] == 299, "fight against the target read before")
assert(fight[7] == 1426 and fight[8] == 40 and fight[9] == 40, "where the fight began")
local f = fight[11]
assert(f.seconds == 12 and f.kills == 1 and f.xp == 45 and f.mobLevel == 5, "fight 1: " .. tostring(f.kills))
assert(f.elite == true and f.pet == true and f.died == nil, "fight 1 flags")
secret[koboldGUID] = nil

-- Fight 2: PARTY_KILL readable (player then pet), counted once each despite the XP messages;
-- XP across a level up.
local wolf1, wolf2 = spawnGUID(525, 600), spawnGUID(525, 50000) -- wolf2: up since the restart
guids.target, levels.target, classes.target = wolf1, 6, "normal"
xpNow, xpMax = 380, 400
mark = #char.events
fire("PLAYER_REGEN_DISABLED")
fire("PARTY_KILL", "Player-1-2", wolf1)
fire("CHAT_MSG_COMBAT_XP_GAIN", "Mangy Wolf dies, you gain 50 experience.")
here = { 1426, 0.42, 0.40 } -- kill rows: one per NPC and spot
fire("PARTY_KILL", "Pet-0-1-2-3-416-0000000001", wolf2)
fire("PARTY_KILL", "Player-9-9", spawnGUID(525, 800)) -- someone else's
playerLevel, xpNow, clockNow = 6, 30, clockNow + 20
fire("PLAYER_REGEN_ENABLED")
f = rowsSince("combat", mark)[1][11]
assert(f.kills == 2 and f.xp == 50 and f.mobLevel == 6 and f.elite == nil, "fight 2: kills " .. tostring(f.kills))
local kills = rowsSince("kill", mark)
assert(#kills == 2 and kills[1][11].age == 600, "kill rows with spawn age")
assert(kills[2][11] == false, "no age past an hour (it would tell the time of day)")

-- Fight 3: nothing killed, short: no row. Fight 4: target died, the player too.
exists.target, exists.pet = false, false
mark = #char.events
fire("PLAYER_REGEN_DISABLED")
clockNow = clockNow + 1
fire("PLAYER_REGEN_ENABLED")
assert(#rowsSince("combat", mark) == 0, "short fight without kill skipped")
fire("PLAYER_REGEN_DISABLED")
fire("PLAYER_TARGET_DIED")
fire("PLAYER_DEAD")
clockNow = clockNow + 8
fire("PLAYER_REGEN_ENABLED")
f = rowsSince("combat", mark)[1]
assert(f[6] == false and f[11].kills == 1 and f[11].died == true and f[11].pet == nil, "fight 4")
-- In an instance: nothing.
IsInInstance = function() return true end
mark = #char.events
fire("PLAYER_REGEN_DISABLED")
fire("PLAYER_TARGET_DIED")
clockNow = clockNow + 10
fire("PLAYER_REGEN_ENABLED")
IsInInstance = function() return false end
assert(#rowsSince("combat", mark) == 0, "no fight recorded in instances")
print("collector combat ok")

-- Objective progress: a row when a count goes up, the log read once per burst of events.
local objectives = { [179] = { { numFulfilled = 2, numRequired = 8 } } }
local questLog = { 179 }
C_QuestLog.GetNumQuestLogEntries = function() return #questLog end
C_QuestLog.GetInfo = function(i) return { questID = questLog[i], title = "Q" .. questLog[i], level = 2 } end
C_QuestLog.GetQuestObjectives = function(q) return objectives[q] end
fire("QUEST_LOG_UPDATE")
runTimers()
mark = #char.events
objectives[179][1].numFulfilled = 4
objectives[180] = { { numFulfilled = 5, numRequired = 8 } }
questLog[2] = 180 -- accepted with some done already: not progress
fire("QUEST_LOG_UPDATE")
fire("QUEST_WATCH_UPDATE")
fire("UNIT_QUEST_LOG_CHANGED", "player")
local queued = #timers
runTimers()
local progress = rowsSince("progress", mark)
assert(#progress == 1 and progress[1][4] == 179, "one progress row, got " .. #progress)
assert(progress[1][11].index == 1 and progress[1][11].value == 4, "objective 1 at 4")
objectives[180][1].numFulfilled = 6
fire("UNIT_QUEST_LOG_CHANGED", "target")
runTimers()
assert(#rowsSince("progress", mark) == 1, "another unit's quest log: not read")
fire("QUEST_LOG_UPDATE")
assert(queued > 0 and #rowsSince("progress", mark) == 1, "read a moment later")
runTimers()
assert(#rowsSince("progress", mark) == 2, "the new quest's progress")
print("collector progress ok")

-- Guide steps: done while playing recorded with their index, not those marked by hand.
FG.Routes["step-test"] = {
    race = 7,
    class = 8,
    faction = "Alliance",
    fromLevel = 1,
    toLevel = 60,
    time = 1,
    steps = {
        { k = "accept", t = "Accept One", q = 9101, lvl = 6 },
        { k = "objective", t = "Boar Meat x4", q = 9101, o = 1, n = "Boar Meat", lvl = 6, d = 240 },
        { k = "train", t = "Train", lvl = 6 },
        { k = "grind", t = "Grind", lvl = 60 },
    },
}
objectives[9101] = { { text = "Boar Meat: 2/4", numFulfilled = 2, numRequired = 4, finished = false } }
useRoute("step-test")
mark = #char.events
onQuest[9101] = true
FG.Guide:Advance()
assert(char.guide.step == 2 and #rowsSince("step", mark) == 0, "an accept has its own row")
objectives[9101][1].finished = true
FG.Guide:Advance()
local steps = rowsSince("step", mark)
assert(#steps == 1 and steps[1][4] == 9101, "step done while playing")
assert(steps[1][11].step == 2 and steps[1][11].planned == 240, "its index and planned time")
Press("skip")
FG.Guide:Advance()
assert(char.guide.step == 4 and #rowsSince("step", mark) == 1, "a step marked by hand is not recorded")
onQuest[9101] = nil
print("collector steps ok")

-- Flights: from the flight master to the landing.
local onTaxi = false
UnitOnTaxi = function() return onTaxi end
here = { 1426, 0.30, 0.70 }
mark = #char.events
fire("PLAYER_CONTROL_LOST")
onTaxi = true
runTimers()
clockNow = clockNow + 125
onTaxi = false
here = { 1453, 0.60, 0.40 }
fire("PLAYER_CONTROL_GAINED")
local flightRow = rowsSince("flight", mark)[1]
assert(flightRow and flightRow[7] == 1453 and flightRow[8] == 60, "landing position")
assert(flightRow[11].fromMap == 1426 and flightRow[11].fromX == 30 and flightRow[11].seconds == 125, "flight")
fire("PLAYER_CONTROL_LOST") -- stunned, not a flight
runTimers()
fire("PLAYER_CONTROL_GAINED")
assert(#rowsSince("flight", mark) == 1, "control lost without a flight")
print("collector flight ok")

-- Quest details carry the XP reward when the client gives it.
GetRewardXP = function() return 450 end
GetQuestID = function() return 9201 end
fire("QUEST_DETAIL", 0)
GetQuestID = questID
assert(lastRow("detail")[4] == 9201 and lastRow("detail")[11].xp == 450, "detail XP")
print("collector detail ok")

-- Rows are strings; game text is percent-encoded where it would break the format.
C_QuestLog.GetLogIndexForQuestID = function() return 1 end
C_QuestLog.GetInfo = function() return { questID = 9301, title = "A|B;C=D%E\nF", level = 7 } end
fire("QUEST_ACCEPTED", 9301)
local raw = char.events[#char.events]
assert(type(raw) == "string" and raw:find("|accept|9301|", 1, true), "a string row: " .. tostring(raw))
assert(raw:find("level=7;title=A%7CB%3BC%3DD%25E%0AF", 1, true), "text encoded: " .. raw)
assert(lastRow("accept")[11].title == "A|B;C=D%E\nF", "and read back")
print("collector rows ok")

GetTime = realGetTime
issecretvalue = function() return false end

-- Handler errors are counted in the saved variables, a few short messages.
for msg in pairs(FactoruideDB.errors or {}) do
    error("handler error: " .. msg)
end
FG:On("FG_TEST", function() error("boom") end)
fire("FG_TEST")
fire("FG_TEST")
local errorKey, errorCount = pairs(FactoruideDB.errors)(FactoruideDB.errors)
assert(errorKey:find("FG_TEST", 1, true) and errorKey:find("boom", 1, true) and errorCount == 2, "error counted")
for i = 1, 30 do
    FG:Error("test", i .. string.rep("x", 300))
end
local kept = 0
for msg in pairs(FactoruideDB.errors) do
    kept = kept + 1
    assert(#msg <= 200, "message shortened")
end
assert(kept == 20, "errors capped, got " .. kept)
print("error counter ok")

-- Rows the app has (Sent.lua) are dropped at login, the seq keeps growing; at most 20000 rows.
FactoruideSent = { ["Brouz-Realm"] = decodeRow(char.events[3])[1] }
local before = #char.events
FG.Collector:Purge()
assert(#char.events == before - 3 and decodeRow(char.events[1])[1] > FactoruideSent["Brouz-Realm"], "sent rows dropped")
local output
local realPrint = print
print = function(s) output = s end
Press("status")
print = realPrint
assert(output:find(#char.events .. " not sent yet", 1, true), "status: " .. output)
for i = 1, 20010 do
    char.events[#char.events + 1] = { char.seq + i, 0, "kill", false, "npc", 6, false, false, false, 1, false }
end
char.seq = char.seq + 20010
FactoruideSent = { ["Brouz-Realm"] = char.seq + 50 } -- saved variables older than what the app has
FG.Collector:Purge()
assert(#char.events == 0 and char.seq == FactoruideSent["Brouz-Realm"], "all sent, the seq follows")
FactoruideSent = nil
FactoruideDB.errors = nil -- full after the error counter test
for i = 1, 20010 do
    char.events[i] = { char.seq + i, 0, "kill", false, "npc", 6, false, false, false, 1, false }
end
char.seq = char.seq + 20010
FG.Collector:Purge()
assert(#char.events == 20000 and char.events[20000][1] == char.seq, "capped, the newest kept")
local capLogged = false
for msg in pairs(FactoruideDB.errors) do
    if msg:find("oldest dropped", 1, true) then capLogged = true end
end
assert(capLogged, "the cap is logged")
print("collector purge ok")

-- In a function of their own: the main chunk is out of locals.
local function ExportImport()
    -- JSON for the profile export: exact text for the usual values, numbers read back the same.
    assert(
        FG.JSON({ b = { 1, 2, 3 }, a = 1, c = {}, d = 'q"\\\n\1', e = 0.1, g = true, h = -3, i = { [2] = "x" } })
            == [[{"a":1,"b":[1,2,3],"c":[],"d":"q\"\\\u000a\u0001","e":0.1,"g":true,"h":-3,"i":{"2":"x"}}]],
        "json: "
            .. FG.JSON({ b = { 1, 2, 3 }, a = 1, c = {}, d = 'q"\\\n\1', e = 0.1, g = true, h = -3, i = { [2] = "x" } })
    )
    for _, v in ipairs({ 1 / 3, 29.93, -1e-7, 1e300, 2 ^ 53 + 2, 1712345678 }) do
        assert(tonumber(FG.JSON(v)) == v, "json number " .. FG.JSON(v))
    end
    assert(FG.JSON(1712345678) == "1712345678" and FG.JSON(0 / 0) == "null", "json integers, NaN")

    --- Whether `text` parses as JSON, and passes the Python `check` (assertions on `d`); nil when
    --- python3 is not there.
    local function PythonJson(text, check)
        if os.execute("python3 -c '' 2>/dev/null") ~= 0 and os.execute("python3 -c '' 2>/dev/null") ~= true then
            return nil
        end
        local path = os.tmpname()
        local f = assert(io.open(path, "wb"))
        f:write(text)
        f:close()
        local script = "import json,sys; d = json.load(open(sys.argv[1], encoding='utf-8')); " .. (check or "")
        local status = os.execute("python3 -I -c " .. string.format("%q", script):gsub("%$", "\\$") .. " " .. path)
        os.remove(path)
        return status == 0 or status == true
    end

    local deflate = LibStub("LibDeflate")

    -- "Export my character": a fresh profile, compressed, in a window to copy (no collector rows).
    local profile = FactoruideDB.profiles["Brouz-Realm"]
    char.events = { "101|5|kill||npc|6|1426|30|70|1|", "102|6|kill||npc|6|1426|30|70|1|", { 103, 0, "kill" } }
    char.events[4] = '104|7|loot||npc|6|1426|30|70|1|name=Wolf "Meat"%0A'
    FactoruideDB.sent = { ["Brouz-Realm"] = 101 }
    profile.flights = { [12] = true, [2] = true, [7] = false }
    playerLevel = 4
    Press("export")
    local window, edit, action, status = FG.TextWindow:Parts()
    assert(window:IsShown() and not action:IsShown(), "export window shown, no second button")
    local exported = edit:GetText()
    -- `FG_PROFILE_EXPORT=<file> luajit addon/tests/smoke.lua`: this export also written into <file>
    -- (the web app's fixtures of the released addons' exports, crates/fg-web/tests/profiles), and
    -- with `FG_SAVED_VARIABLES=<file>` the saved variables as they are now (the desktop app's,
    -- crates/fg-contrib/tests/saved-variables).
    local function Fixture(variable, text)
        if not os.getenv(variable) then return end
        local file = assert(io.open(os.getenv(variable), "wb"))
        file:write(text, "\n")
        file:close()
    end
    Fixture("FG_PROFILE_EXPORT", exported)
    Fixture("FG_SAVED_VARIABLES", "FactoruideDB = " .. assert(FG.Import.Serialize(FactoruideDB)):gsub("^return ", ""))
    assert(exported:sub(1, 5) == "FGP1:" and exported:match("^FGP1:[%w%(%)]+$"), "export prefix and alphabet")
    local json = deflate:DecompressDeflate(deflate:DecodeForPrint(exported:sub(6)))
    assert(json == FG.Export:Json(), "export round trip")
    assert(json:find('"flights":[2,12]', 1, true), "flights as a sorted list")
    assert(not json:find('"collector"', 1, true) and not json:find("|kill|", 1, true), "no collector rows")
    assert(json:find('"level":4', 1, true), "a fresh snapshot")
    local parsed = PythonJson(
        json,
        "assert d['format'] == 'factoruide-profile' and d['version'] == 1 and d['edition'] == 'forever'; "
            .. "assert d['addon'] == '0.13.0' and d['character'] == 'Brouz-Realm'; "
            .. "assert d['profile']['flights'] == [2, 12] and d['profile']['class'] == 'MAGE'; "
            .. "assert sorted(d) == ['addon', 'character', 'edition', 'format', 'profile', 'version']"
    )
    assert(parsed ~= false, "export JSON read by Python")
    print("profile export ok" .. (parsed == nil and " (python3 missing: JSON not parsed)" or ""))

    -- "Import a guide…": guides pasted from the apps, one text each (or several in one); imports add up.
    local fixture = assert(io.open("addon/tests/fixtures/Routes.lua")):read("*a")
    -- Without the older apps' assignments.
    local plain = fixture:gsub("%-%- Which guide each character follows.-}\n", "")
    assert(not plain:find("Assignments", 1, true), "fixture without assignments")
    --- A guide's text as the apps export it: `name`, computed at `created`, for class `class`.
    local function GuideLua(name, created, class, extra)
        local lua = plain:gsub("gnome%-mage", name)
        if class then lua = lua:gsub("class = 8,", "class = " .. class .. ",", 1) end
        return lua
            .. string.format(
                "FG.Routes[%q].created = %d\nFG.Routes[%q].version = 3\nFG.Routes[%q].format = %d\n",
                name,
                created,
                name,
                name,
                FG.Import.FORMAT
            )
            .. (extra or "")
    end
    local function Empty(t) return t == nil or pairs(t)(t) == nil end
    local function Encode(lua) return "FGG1:" .. deflate:EncodeForPrint(deflate:CompressDeflate(lua)) end
    local guideLua = GuideLua(
        "web-mage",
        1000,
        nil,
        'FG.Config = { locale = "enUS", compassShown = false }\n'
            .. 'FactoruideSent = { ["Brouz-Realm"] = 150, ["Other-Realm"] = 3 }\n'
    )
    local guideString = Encode(guideLua)
    Press("import")
    assert(window:IsShown() and action:IsShown() and (edit:GetText() or "") == "", "import window, empty box")
    local function Click(text)
        FG.Import:Open()
        edit:SetText(text)
        action.OnClick()
        return status:GetText()
    end
    reloaded = false
    assert(Click("hello"):find("FGG1:", 1, true), "bad prefix refused")
    assert(Click("FGG1:abc!!$"):find("abîmé", 1, true), "corrupt text refused: " .. status:GetText())
    assert(Click(guideString:sub(1, 200)):find("abîmé", 1, true), "cut text refused")
    local notLua = "FGG1:" .. deflate:EncodeForPrint(deflate:CompressDeflate("FG.Routes = {"))
    assert(Click(notLua):find("pas pu être lu", 1, true), "Lua errors refused")
    -- Made by a newer app for a newer addon (its format above the addon's): refused, the player
    -- told to update the addon. Today's format and none (older apps) are read (ReleasedGuides).
    local newer = GuideLua("web-next", 1000)
        .. string.format('FG.Routes["web-next"].format = %d\n', FG.Import.FORMAT + 1)
    assert(Click(Encode(newer)):find("mettez à jour l'addon", 1, true), "newer format refused: " .. status:GetText())
    assert(not reloaded and Empty(FactoruideDB.guides), "nothing kept from bad strings")
    -- Pasted: the game sends the characters one by one in the same frame (OnChar), the box
    -- itself holds little; the next frame joins them. Spaces and line breaks picked up with the
    -- copy do not matter.
    local pasteTime = GetTime
    GetTime = function() return 42 end
    local text = guideString:sub(1, 100) .. "\n  " .. guideString:sub(101)
    edit:SetText("")
    for i = 1, #text do
        edit.OnChar(edit, text:sub(i, i))
    end
    GetTime = pasteTime
    assert(rawget(edit, "OnUpdate"), "the paste is joined on the next frame")
    edit.OnUpdate(edit)
    assert(not rawget(edit, "OnUpdate"), "joined once")
    assert(edit:GetText():sub(1, 5) == "FGG1:" and #edit:GetText() < 80, "the box shows the paste's beginning only")
    assert(status:GetText():find("Ko", 1, true), "the paste's size told: " .. status:GetText())
    action.OnClick()
    local kept = FactoruideDB.guides["web-mage"]
    assert(reloaded and kept and kept.created == 1000 and kept.version == 3, "guide kept, interface reloaded")
    assert(FactoruideDB.sent["Brouz-Realm"] == 150 and FactoruideDB.sent["Other-Realm"] == 3, "sent seq imported")
    assert(#FG.Collector:Pending().events == 0, "rows up to the imported seq are sent")
    FG.Import:ApplySaved()
    assert(FG.Routes["web-mage"] and FG.Routes["gnome-mage"], "imported guides beside the files'")
    assert(FG.Routes["web-mage"].created == 1000 and #FG.Routes["web-mage"].steps > 10, "the guide read back")
    assert(FG.Import.info["web-mage"].source == "import" and FG.Import.info["gnome-mage"].source == "app", "sources")
    assert(
        FG.Config.locale ~= "enUS" and FG:Get("compassShown") == true and FG.Lang() == "frFR",
        "imported Config ignored"
    )
    useRoute("web-mage")
    assert(FG.Guide:Route() == FG.Routes["web-mage"] and char.guide.route == "web-mage", "imported guide followed")

    -- A second text adds its guide to the first one.
    reloaded = false
    Click(Encode(GuideLua("web-warrior", 1100, 1)))
    assert(reloaded and FactoruideDB.guides["web-mage"] and FactoruideDB.guides["web-warrior"], "imports add up")
    -- The same guide computed later: replaces it, no question; computed at the same time: too.
    calls.StaticPopup_Show = nil
    Click(Encode(GuideLua("web-mage", 1200)))
    assert(FactoruideDB.guides["web-mage"].created == 1200 and not calls.StaticPopup_Show, "newer replaces")
    Click(Encode(GuideLua("web-mage", 1200)))
    assert(FactoruideDB.guides["web-mage"].created == 1200 and not calls.StaticPopup_Show, "same: replaced quietly")
    -- Computed before the one kept: asked; "no" keeps it, "yes" replaces it. The other guides of
    -- the text are kept either way.
    reloaded = false
    Click(Encode(GuideLua("web-mage", 900) .. GuideLua("web-rogue", 1300, 4)))
    local popup = calls.StaticPopup_Show
    assert(popup and popup[1] == "FACTORUIDE_OLDER_GUIDE", "asked")
    assert(popup[2]:find("web-mage", 1, true) and popup[2]:find("1970-01-01", 1, true), "question: " .. popup[2])
    assert(not reloaded and FactoruideDB.guides["web-mage"].created == 1200, "nothing kept before the answer")
    StaticPopupDialogs.FACTORUIDE_OLDER_GUIDE.OnCancel()
    assert(FactoruideDB.guides["web-mage"].created == 1200, "no: kept")
    assert(FactoruideDB.guides["web-rogue"] and reloaded, "the newer guide of the text kept anyway")
    reloaded = false
    calls.StaticPopup_Show = nil
    Click(Encode(GuideLua("web-mage", 900)))
    assert(calls.StaticPopup_Show, "asked again")
    StaticPopupDialogs.FACTORUIDE_OLDER_GUIDE.OnAccept()
    assert(FactoruideDB.guides["web-mage"].created == 900 and reloaded, "yes: replaced")
    -- Answered "no" with nothing else in the text: nothing to reload.
    reloaded = false
    Click(Encode(GuideLua("web-mage", 800)))
    StaticPopupDialogs.FACTORUIDE_OLDER_GUIDE.OnCancel()
    assert(FactoruideDB.guides["web-mage"].created == 900 and not reloaded, "nothing imported")

    -- The sandbox: no globals, no game API, data only.
    local sandbox =
        assert(FG.Import.Run("Factoruide.ok = _G == nil and print == nil and loadstring == nil and string == nil"))
    assert(sandbox.ok == true, "nothing but Factoruide in the sandbox")
    assert(FG.Import.Run("Factoruide.Guide = function() end") == nil, "functions refused")
    assert(FG.Import.Run("Factoruide.x = { [function() end] = 1 }") == nil, "functions as keys refused")
    assert(
        FG.Import.Run("os.exit(1)") == nil and FG.Import.Run(string.dump(function() end)) == nil,
        "no os, no bytecode"
    )
    -- Kept as Lua text: read back the same, a table inside itself refused.
    local value = { 1, nil, 'a\n"b"\0', x = 0.1, [1.5] = true, ["end"] = { -3, 1 / 3, 2 ^ 60 } }
    local back = assert(loadstring(assert(FG.Import.Serialize(value))))()
    assert(back[1] == 1 and back[2] == nil and back[3] == value[3] and back.x == 0.1 and back[1.5], "serialized")
    assert(back["end"][1] == -3 and back["end"][2] == 1 / 3 and back["end"][3] == 2 ^ 60, "numbers exact")
    local loop = {}
    loop.self = loop
    assert(FG.Import.Serialize(loop) == nil, "loops refused")

    -- Which guide a character follows. Only guides kept from here on (no files' guides).
    FG.Import.file = { routes = {}, assignments = {} }
    FactoruideDB.guides, FactoruideDB.guideAssignments = {}, nil
    FG.Import:Apply()
    useRoute(nil)
    assert(char.guide.route == nil, "no guide: none followed")
    -- The only guide made for its race and class: followed by itself.
    Click(Encode(GuideLua("solo", 2000) .. GuideLua("warrior", 2000, 1)))
    FG.Import:Apply()
    FG.Guide:FollowChoice()
    assert(char.guide.route == "solo", "the only gnome mage guide followed: " .. tostring(char.guide.route))
    -- A second one: the guide followed stays; without a choice, none until one is chosen.
    Click(Encode(GuideLua("duo", 2100)))
    FG.Import:Apply()
    FG.Guide:FollowChoice()
    assert(char.guide.route == "solo", "kept following the guide")
    useRoute(nil)
    assert(char.guide.route == nil, "two gnome mage guides: none chosen")
    -- The guides on the options' main page: the gnome mage's first, follow, start over, delete.
    FG.Options:Refresh()
    local rows = FG.Options:GuideRows()
    local function Row(name)
        for _, row in ipairs(rows) do
            if row.guide and row.guide.name == name and row:IsShown() then return row end
        end
    end
    assert(rows[1].guide.fits and rows[2].guide.fits and not rows[3].guide.fits, "this character's guides first")
    assert(Row("warrior").name:GetText():find("autre personnage", 1, true), "another character's guide")
    assert(Row("duo").details:GetText():find("niveaux 1%-60") and Row("duo").delete:IsShown(), "details")
    assert(not Row("duo").restart:IsShown(), "start over: the guide followed only")
    Row("duo").follow.OnClick(Row("duo").follow)
    assert(char.guide.route == "duo" and FG.Guide:Route() == FG.Routes.duo, "chosen in the list")
    assert(Row("duo").follow:GetText() == "Suivi" and not Row("duo").follow:IsEnabled(), "followed")
    assert(Row("solo").follow:GetText() == "Suivre" and Row("solo").follow:IsEnabled(), "another to follow")
    assert(Row("duo").restart:IsShown() and Row("duo").lit._shown, "the guide followed: lit, start over")
    calls.StaticPopup_Show = nil
    char.guide.skippedQuests[5004] = true
    Row("duo").restart.OnClick(Row("duo").restart)
    assert(calls.StaticPopup_Show and calls.StaticPopup_Show[1] == "FACTORUIDE_RESET", "start over: asked")
    char.guide.skippedQuests[5004] = nil
    -- The choice is the character's, kept in its saved state.
    assert(FactoruideDB.characters["Brouz-Realm"].guide.route == "duo", "choice saved per character")
    FG.Guide:FollowChoice()
    assert(char.guide.route == "duo", "and kept")
    -- Delete: asked first (the game's dialog); the guide followed gone, back to the automatic choice.
    calls.StaticPopup_Show = nil
    local delete = Row("duo").delete
    delete.OnClick(delete)
    local asked = calls.StaticPopup_Show
    assert(asked and asked[1] == "FACTORUIDE_DELETE_GUIDE" and asked[2] == "duo", "asks first")
    assert(FactoruideDB.guides.duo, "not before the answer")
    StaticPopupDialogs.FACTORUIDE_DELETE_GUIDE.OnAccept(nil, asked[4])
    assert(not FactoruideDB.guides.duo and not FG.Routes.duo and not Row("duo"), "deleted")
    assert(char.guide.route == "solo", "back to the only gnome mage guide: " .. tostring(char.guide.route))

    -- Factoruide_Data, written by the desktop app: its guides beside the imported ones, of two with
    -- the same name the one computed last. Its text runs in the profile part (Data.lua), before
    -- the guide part loads.
    local dataLua = GuideLua("solo", 5000)
        .. GuideLua("desk-mage", 5000):gsub("local FG = Factoruide\nFG.Routes = FG.Routes or {}\n", "")
        .. 'FactoruideSent = { ["Brouz-Realm"] = 170 }\nFactoruide.Config = { compassShown = false }\n'
    local function LoadData(lua, when)
        FactoruideData, FactoruideDataTime = lua, when
        assert(loadfile("addon/Factoruide_Profile/Data.lua"))("Factoruide_Profile", FG)
        assert(FactoruideData == nil and FG.DataGuide and FG.DataGuide.time == when, "data run, its text let go")
    end
    LoadData(dataLua, 6000)
    FG.Import:ApplySaved()
    assert(FG.Routes["desk-mage"] and FG.Routes.warrior and FG.Routes.solo.created == 5000, "union, newer data wins")
    assert(FG.Import.info.solo.source == "app" and FG.Import.info.warrior.source == "import", "sources")
    assert(FG.DataGuide == nil and FG.DataSent["Brouz-Realm"] == 170, "data taken, its sent seqs kept")
    assert(FG:Get("compassShown") == true, "data's Config ignored")
    FG.Options:Refresh()
    assert(Row("desk-mage") and not Row("desk-mage").delete:IsShown(), "the app's guides are not deleted in game")
    assert(Row("desk-mage").details:GetText():find("géré par l'application", 1, true), "managed by the app")
    assert(not FG.Import:Delete("desk-mage"), "not even by name")
    LoadData(dataLua:gsub("= 5000", "= 10"), 10)
    FG.Import:ApplySaved()
    assert(FG.Routes.solo.created == 2000 and FG.Import.info.solo.source == "import", "newer import wins")
    -- A guide of Factoruide_Data made for a newer addon: left out, named for the login message.
    LoadData(
        GuideLua("desk-next", 5000) .. string.format('FG.Routes["desk-next"].format = %d\n', FG.Import.FORMAT + 1),
        6500
    )
    FG.Import:ApplySaved()
    assert(not FG.Routes["desk-next"] and FG.Import.tooNew[1] == "desk-next", "newer format left out")
    -- Data without `.created` (older apps): the time it was written.
    LoadData(plain:gsub("gnome%-mage", "old-desk"), 7000)
    FG.Import:ApplySaved()
    assert(FG.Import.info["old-desk"].created == 7000, "data's time")
    -- Data that is not data: nothing kept, the error counted.
    FactoruideData, FactoruideDataTime = "Factoruide.f = function() end", 3000
    assert(loadfile("addon/Factoruide_Profile/Data.lua"))("Factoruide_Profile", FG)
    assert(FG.DataGuide == nil and FG.DataFailed, "functions refused in the data")
    FG.DataFailed = nil

    -- An older app's text: the whole text kept before (guideImport) moves to one guide each, its
    -- assignments are the first choice of the characters they name when several guides fit.
    FG.Import.data = nil
    FactoruideDB.guideImport = { text = fixture:gsub("gnome%-mage", "old-mage"), time = 4000 }
    FG.Import:ApplySaved()
    assert(FactoruideDB.guideImport == nil and FactoruideDB.guides["old-mage"].created == 4000, "moved")
    assert(FG.Assignments["Brouz-Realm"] == "old-mage", "assignments kept")
    useRoute(nil)
    assert(char.guide.route == "old-mage", "two gnome mage guides: the one assigned")
    useRoute("solo")
    assert(char.guide.route == "solo", "a choice wins over the assignment")

    -- "Delete the imported guides": asked, then every imported guide deleted.
    calls.StaticPopup_Show = nil
    Press("clear")
    assert(calls.StaticPopup_Show and calls.StaticPopup_Show[1] == "FACTORUIDE_CLEAR_GUIDES", "asked")
    assert(FactoruideDB.guides.solo, "not before the answer")
    StaticPopupDialogs.FACTORUIDE_CLEAR_GUIDES.OnAccept()
    assert(Empty(FactoruideDB.guides) and not FG.Routes.solo and char.guide.route == nil, "imports cleared")
    local ok = pcall(Press, "clear")
    assert(not ok, "nothing left to delete: the button disabled")

    -- Empty generated files (the addon as published) and no import: a message, no guide.
    FG.Services, FG.Config = nil, nil
    FG.Import:Apply()
    local rendered
    local ui = FG.Guide:UI()
    local render = ui.Render
    ui.Render = function(self, view)
        rendered = view
        return render(self, view)
    end
    FG.Guide:FollowChoice()
    FG.Guide:Refresh()
    ui.Render = render
    assert(
        FG.Guide:Route() == nil and rendered and rendered.message == FG.Guide.L.noGuides,
        "no guide: use the web app"
    )
    FG.Guide:Advance()
    Press("next")
    print("guide import ok")
end
ExportImport()

-- The guides the released apps wrote (addon/tests/guides, one file per release whose guide
-- output changed, see its README): each still imported both ways (pasted, Factoruide_Data),
-- listed, followed from its first step shown, and walked through to its end without error.
local function ReleasedGuides()
    local deflate = LibStub("LibDeflate")
    local files = assert(loadfile("addon/tests/guides/index.lua"))()
    assert(#files > 0, "released guides listed")
    local level, quests, done = playerLevel, onQuest, completed
    local errors = {}
    for msg in pairs(FactoruideDB.errors or {}) do
        errors[msg] = true
    end
    for _, file in ipairs(files) do
        local lua = assert(io.open("addon/tests/guides/" .. file)):read("*a")
        local ns = assert(FG.Import.Run(lua), file .. " runs")
        local names = {}
        for name in pairs(ns.Routes) do
            names[#names + 1] = name
        end
        assert(#names > 0, file .. ": its guides")
        for _, way in ipairs({ "import", "app" }) do
            -- Only this guide.
            FactoruideDB.guides, FactoruideDB.guideExtras, FactoruideDB.guideAssignments = {}, nil, nil
            FG.Import.file, FG.Import.data = { routes = {}, assignments = {} }, nil
            if way == "import" then
                local text = "FGG1:" .. deflate:EncodeForPrint(deflate:CompressDeflate(lua))
                local count, err = FG.Import:FromString(text)
                assert(count == #names, file .. " pasted: " .. tostring(err))
            else
                FactoruideData, FactoruideDataTime = lua, 1700000000
                assert(loadfile("addon/Factoruide_Profile/Data.lua"))("Factoruide_Profile", FG)
                assert(FG.DataGuide and not FG.DataFailed, file .. " in Factoruide_Data: " .. tostring(FG.DataFailed))
            end
            FG.Import:ApplySaved()
            local listed = {}
            for _, guide in ipairs(FG.Import:List()) do
                listed[guide.name] = guide
            end
            for _, name in ipairs(names) do
                local what = file .. " (" .. way .. ") " .. name
                assert(listed[name] and listed[name].source == way, what .. ": listed")
                -- A character at level 1 with no quest yet.
                playerLevel, onQuest, completed = 1, {}, {}
                local view
                local ui = FG.Guide:UI()
                local render = ui.Render
                ui.Render = function(self, v)
                    view = v
                    return render(self, v)
                end
                useRoute(name)
                local route = FG.Guide:Route()
                assert(route == FG.Routes[name] and char.guide.route == name, what .. ": followed")
                assert(char.guide.step == 1, what .. ": from its first step, not " .. char.guide.step)
                local first
                for _, task in ipairs(view and view.tasks or {}) do
                    if task.index == 1 then first = task end
                end
                assert(first and first.step == route.steps[1], what .. ": its first step shown")
                -- Its first quest accepted (or the next page): the guide moves on.
                local step = route.steps[1]
                if step.k == "accept" and step.q then
                    onQuest[step.q] = true
                    fire("QUEST_ACCEPTED", step.q)
                else
                    Press("next")
                end
                assert(char.guide.step > 1, what .. ": advanced")
                -- Page after page to its end.
                for _ = 1, #route.steps do
                    if char.guide.step > #route.steps then break end
                    local at = char.guide.step
                    Press("next")
                    assert(char.guide.step > at, what .. ": stuck at step " .. at)
                end
                assert(char.guide.step == #route.steps + 1 and view.message == FG.Guide.L.done, what .. ": to its end")
                ui.Render = render
            end
        end
    end
    for msg in pairs(FactoruideDB.errors or {}) do
        assert(errors[msg], "no error: " .. msg)
    end
    playerLevel, onQuest, completed = level, quests, done
    FactoruideDB.guides, FactoruideDB.guideExtras = {}, nil
    FG.Import.file, FG.Import.data = { routes = {}, assignments = {} }, nil
    FG.Import:ApplySaved()
    useRoute(nil)
    print("released guides ok: " .. #files)
end
ReleasedGuides()

-- The settings: their schema (addon/settings.json, written into SettingsSchema.lua), the window.
local function SettingsTest()
    local tool = assert(loadfile("addon/tools/settings.lua"))("module")
    local generated, current = tool.Compare()
    assert(generated == current, "SettingsSchema.lua differs from settings.json: luajit addon/tools/settings.lua")
    local schema = tool.Decode(assert(io.open("addon/settings.json")):read("*a"))
    local keys = {}
    for _, group in ipairs(schema.groups) do
        for _, section in ipairs(group.sections) do
            for _, option in ipairs(section.options) do
                keys[#keys + 1] = option
            end
        end
    end
    assert(#keys > 50, "every option read: " .. #keys)

    -- Nothing saved: the defaults; a saved value that does not fit: the default (numbers held
    -- within their bounds).
    FactoruideDB.settings = {}
    for _, option in ipairs(keys) do
        assert(FG:Get(option.key) == option.default, "default of " .. option.key)
    end
    FactoruideDB.settings = { scale = "big", guideMode = "weird", autoAccept = 1, trackerQuests = 99 }
    assert(FG:Get("scale") == 1 and FG:Get("guideMode") == "full" and FG:Get("autoAccept") == true, "bad values")
    assert(FG:Get("trackerQuests") == 35, "within bounds")
    -- What the tests before chose (the guide's mode, tabs...) is let go.
    FactoruideDB.settings = {}

    -- Settings kept before the window (FactoruideDB.ui) move over once.
    FactoruideDB.ui = { scale = 1.2, appGuideMode = "full", uiShown = false, compassShown = false }
    FactoruideDB.settings.scale, FactoruideDB.settings.compassShown = nil, nil
    FG.MoveOldSettings()
    assert(FactoruideDB.ui == nil and FactoruideDB.settings.scale == 1.2, "old settings moved")
    assert(FactoruideDB.settings.compassShown == false and FactoruideDB.settings.uiShown == nil, "not the session's")
    assert(FactoruideDB.settings.appGuideMode == nil, "nor the app's leftovers")
    FG:Save("scale", nil)
    FG:Save("compassShown", nil)

    -- The addon's category in the game's options (AddOns), its pages registered with it.
    local category = options.Category()
    assert(category and category.registered and category.frame, "a canvas page in the game's options (AddOns)")
    local pages = {}
    for _, page in ipairs(category.subcategories) do
        pages[#pages + 1] = page.name
    end
    assert(
        table.concat(pages, ",") == "Affichage,Navigation,Automatisation,Conseils,Immersion,Avancé",
        "a page per group, then Advanced: " .. table.concat(pages, ",")
    )
    -- `/fg`, with anything after it: opens the options on the addon's page, nothing else happens.
    local function Now()
        return table.concat({
            tostring(FG:Get("guideMode")),
            tostring(FG:Get("uiShown")),
            tostring(FG:Get("lockFrames")),
            tostring(FG:Get("compassShown")),
            tostring(FG:Get("waypoints")),
            tostring(FG:Get("scale")),
            tostring(FactoruideDB.debug),
            tostring(char.guide.step),
            tostring(#char.events),
            tostring(FG.TextWindow:Parts() and FG.TextWindow:Parts():IsShown()),
        }, " ")
    end
    local before = Now()
    reloaded = false
    for _, old in ipairs({ "", "config", "hide", "next", "skip", "mode", "route x", "time", "reset", "export" }) do
        options.opened = nil
        SlashCmdList.FACTORUIDE(old)
        assert(options.opened == category:GetID(), "/fg " .. old .. " opens the addon's options")
        assert(Now() == before and not reloaded, "/fg " .. old .. " does nothing else: " .. Now())
    end
    assert(SLASH_FACTORUIDE2 == "/factoruide", "/factoruide too")
    -- The guide's options button too; in combat the game refuses: said in the chat instead.
    options.opened = nil
    FG:OpenSettings()
    assert(options.opened == category:GetID(), "the guide's options button")
    options.opened = nil
    InCombatLockdown = function() return true end
    local said
    local realPrint = print
    print = function(text) said = text end
    SlashCmdList.FACTORUIDE("")
    print = realPrint
    InCombatLockdown = function() return false end
    assert(options.opened == nil and said and said:find("combat", 1, true), "in combat: a message")

    -- Every option of the schema is a control of its group's page (those of this edition), under
    -- its section's header, and the language.
    local controls, headers = {}, {}
    for _, page in ipairs(category.subcategories) do
        for _, init in ipairs(page.initializers) do
            local setting = init.data.setting
            if setting then
                controls[setting.variable:gsub("^Factoruide_", "")] = init
                assert(setting.category == page, "the setting of its page")
            elseif init.data.kind == "header" then
                headers[init.data.name] = page.name
            end
        end
    end
    local KINDS = { bool = "checkbox", number = "slider", select = "dropdown" }
    for _, group in ipairs(FG.SettingsSchema) do
        for _, section in ipairs(group.sections) do
            for _, option in ipairs(section.options) do
                local control = controls[option.key]
                if not option.only or option.only == FG.Edition then
                    assert(control and control.data.kind == KINDS[option.kind], "no control for " .. option.key)
                    assert(control.data.name == FG.T(option.label), "label translated: " .. control.data.name)
                    assert(control.data.setting.default == option.default, "default of " .. option.key)
                    if option.help then
                        assert(control.data.tooltip:find(FG.T(option.help), 1, true), "help of " .. option.key)
                    end
                    if section.title then
                        assert(headers[FG.T(section.title)] == FG.T(group.title), "section " .. section.title)
                    end
                else
                    assert(not control, "control of another edition: " .. option.key)
                end
            end
        end
    end
    assert(controls.autoAccept.data.name == "Accepter les quêtes automatiquement", "labels translated")
    assert(controls.locale and controls.locale.data.kind == "dropdown", "the language's control")
    assert(controls.debug, "the debug messages' check box (Advanced)")

    -- A check box: its setting reads FG:Get, writes FG:Save (saved, applied at once; the default
    -- saves nothing).
    local autoAccept = Option("Factoruide_autoAccept")
    assert(autoAccept:GetValue() == true, "read")
    autoAccept:SetValue(false)
    assert(FactoruideDB.settings.autoAccept == false and FG:Get("autoAccept") == false, "unchecked: saved")
    calls.AcceptQuest = nil
    fire("QUEST_DETAIL", 0)
    assert(not calls.AcceptQuest, "applied at once")
    autoAccept:SetValueToDefault()
    assert(FactoruideDB.settings.autoAccept == nil and FG:Get("autoAccept") == true, "the default: nothing saved")
    -- A number: a slider within its bounds, on its steps; a percent shown as such.
    local scale = controls.scale.data.options
    assert(scale.minValue == 0.6 and scale.maxValue == 2 and math.abs(scale.steps - 28) < 1e-9, "the size's slider")
    local label = scale.formatters[MinimalSliderWithSteppersMixin.Label.Right]
    assert(label(0.95) == "95%", "size in percent: " .. label(0.95))
    Option("Factoruide_scale"):SetValue(0.95000001)
    assert(FG:Get("scale") == 0.95 and FactoruideDB.settings.scale == 0.95, "on its steps")
    Option("Factoruide_trackerQuests"):SetValue(9)
    assert(FG:Get("trackerQuests") == 9, "number saved")
    assert(controls.trackerQuests.data.options.formatters[MinimalSliderWithSteppersMixin.Label.Right](9) == "9", "9")
    Option("Factoruide_scale"):SetValueToDefault()
    Option("Factoruide_trackerQuests"):SetValueToDefault()
    -- A select: a drop-down of its choices, translated.
    local styles = controls.windStyle.data.options()
    assert(#styles == 4 and styles[2].value == "mist" and styles[2].label == "Brume basse", "the choices")
    Option("Factoruide_windStyle"):SetValue("mist")
    assert(FG:Get("windStyle") == "mist" and Option("Factoruide_windStyle"):GetValue() == "mist", "select saved")
    Option("Factoruide_windStyle"):SetValueToDefault()
    -- Requires: under the setting it needs, greyed while it is off.
    assert(controls.trackerQuests.parent == controls.trackerShown, "under the setting it needs")
    assert(controls.trackerQuests:Enabled(), "enabled while the tracker is on")
    Option("Factoruide_trackerShown"):SetValue(false)
    assert(not controls.trackerQuests:Enabled() and not controls.replaceTracker:Enabled(), "greyed while off")
    Option("Factoruide_trackerShown"):SetValueToDefault()
    assert(controls.trackerQuests:Enabled(), "back")
    Option("Factoruide_pathMode"):SetValue("off")
    assert(not controls.pathMap:Enabled(), "a select turned off greys too")
    Option("Factoruide_pathMode"):SetValueToDefault()
    -- Applied after a /reload only: said in its help; the game's dialog offers to reload, once for
    -- changes made together.
    assert(controls.uiTheme.data.tooltip:find("/reload", 1, true), "the help says /reload")
    runTimers()
    calls.StaticPopup_Show = nil
    local asked = 0
    StaticPopup_Show = function(which)
        if which == "FACTORUIDE_RELOAD" then asked = asked + 1 end
    end
    Option("Factoruide_autoAccept"):SetValue(false)
    runTimers()
    assert(asked == 0, "no reload for a setting applied at once")
    Option("Factoruide_autoAccept"):SetValueToDefault()
    Option("Factoruide_uiTheme"):SetValue("parchemin")
    Option("Factoruide_suiteSkin"):SetValue(not FG:Get("suiteSkin"))
    assert(FactoruideDB.settings.uiTheme == "parchemin" and asked == 0, "theme saved, asked after the changes")
    runTimers()
    assert(asked == 1, "asked once: " .. asked)
    StaticPopupDialogs.FACTORUIDE_RELOAD.OnAccept()
    assert(reloaded, "the interface reloads")
    reloaded = false
    Option("Factoruide_uiTheme"):SetValueToDefault()
    Option("Factoruide_suiteSkin"):SetValueToDefault()
    runTimers()
    StaticPopup_Show = record("StaticPopup_Show")
    -- The language: the game's, then each language.
    local languages = controls.locale.data.options()
    assert(languages[1].value == "" and languages[1].label:find("Français", 1, true), "the game's first")
    assert(#languages == #FG.LANGUAGES + 1, "then every language")
    Option("Factoruide_locale"):SetValue("enUS")
    assert(FactoruideDB.settings.locale == "enUS" and FG.Lang() == "enUS", "language chosen")
    Option("Factoruide_locale"):SetValue("")
    assert(FactoruideDB.settings.locale == nil and FG.Lang() == "frFR", "the game's again")
    -- Changed elsewhere (a key binding): the options show it; written by its own control: no
    -- notice back to it.
    options.notified = {}
    Press("mode")
    assert(options.notified.Factoruide_guideMode == 1, "the options told of the change")
    Option("Factoruide_guideMode"):SetValue("full")
    assert(options.notified.Factoruide_guideMode == 1, "not for the control's own change")
    -- The buttons beside their settings.
    local function After(key)
        for _, page in ipairs(category.subcategories) do
            for i, init in ipairs(page.initializers) do
                if init.data.setting and init.data.setting.variable == "Factoruide_" .. key then
                    return page.initializers[i + 1].data
                end
            end
        end
    end
    assert(
        After("targetMark").name == "Prendre la macro de ciblage",
        "the macro's button: " .. After("targetMark").name
    )
    assert(After("compassShown").name == "Remettre la boussole en haut", "the compass's button")

    -- The key bindings: each has its name (translated) and does what it says.
    local names = 0
    for name in pairs(options.bindings) do
        local global = _G["BINDING_NAME_" .. name]
        assert(type(global) == "string" and global ~= "", "no name for the binding " .. name)
        names = names + 1
    end
    assert(names == 12 and BINDING_HEADER_FACTORUIDE == "Factoruide", "the bindings: " .. names)
    assert(BINDING_NAME_FACTORUIDE_NEXT == "Étape suivante", "translated: " .. BINDING_NAME_FACTORUIDE_NEXT)
    options.opened = nil
    Key("FACTORUIDE_OPTIONS")
    assert(options.opened == category:GetID(), "a key opens the options")
    local shown = FG:Get("guideShown")
    Key("FACTORUIDE_GUIDE")
    assert(FG:Get("guideShown") == not shown and FactoruideDB.settings.guideShown == not shown, "the guide hidden")
    Key("FACTORUIDE_GUIDE")
    assert(FG:Get("guideShown") == shown, "and shown")
    local gusts = 0
    local gust = FG.Wind.Gust
    FG.Wind.Gust = function() gusts = gusts + 1 end
    Key("FACTORUIDE_WIND")
    FG.Wind.Gust = gust
    assert(gusts == 1, "a gust")
    FG:Save("guideShown", nil)
    FG:Save("guideMode", nil)
    -- Nothing left over: every setting back to its default.
    for _, option in ipairs(keys) do
        assert(FactoruideDB.settings[option.key] == nil, "left saved: " .. option.key)
    end
    for msg in pairs(FactoruideDB.errors or {}) do
        assert(not msg:find("Settings", 1, true), "no error: " .. msg)
    end
    print("settings ok")
end
SettingsTest()
