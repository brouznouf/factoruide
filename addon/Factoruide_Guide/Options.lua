-- The guide part's pages in the game's options (under the addon's: Factoruide_Profile/Options.lua).
-- On the main page, the guides in use: the one this character follows, another to follow, start
-- over, delete, import. Then a page per group of the settings' schema (SettingsSchema.lua), its
-- sections as headers, each setting one of the game's controls (check box, slider, drop-down)
-- reading and writing FG:Get / FG:Save, and the addon's language. A change is saved and applied at
-- once; after one applied only after a /reload, the game's dialog offers to reload.

local FG = Factoruide
local T = FG.T
local Options = FG.Options

-- The guides on the main page ---------------------------------------------------------------

local ROW = 44

StaticPopupDialogs["FACTORUIDE_DELETE_GUIDE"] = {
    text = T("Delete the guide %s?"),
    button1 = YES,
    button2 = NO,
    OnAccept = function(_, name)
        if name and FG.Import:Delete(name) then FG:Print(T("guide %s deleted"), name) end
    end,
    timeout = 0,
    whileDead = true,
    hideOnEscape = true,
}

--- "Race Class" of a guide (the game's names, the class in its color), its numbers when unknown.
local function RaceClass(r)
    local race = C_CreatureInfo and C_CreatureInfo.GetRaceInfo and r.race and C_CreatureInfo.GetRaceInfo(r.race)
    local class = C_CreatureInfo and C_CreatureInfo.GetClassInfo and r.class and C_CreatureInfo.GetClassInfo(r.class)
    local raceName = race and race.raceName
    local className, classFile = class and class.className, class and class.classFile
    -- Without C_CreatureInfo: the character's own names when they are its race and class.
    local myRaceName, _, myRace = UnitRace("player")
    local myClassName, myClassFile, myClass = UnitClass("player")
    if not raceName and r.race == FG.Readable(myRace) then raceName = myRaceName end
    if not className and r.class == FG.Readable(myClass) then
        className, classFile = myClassName, myClassFile
    end
    raceName = raceName or (r.race and "#" .. tostring(r.race)) or "?"
    className = className or (r.class and "#" .. tostring(r.class)) or "?"
    local color = classFile and RAID_CLASS_COLORS and RAID_CLASS_COLORS[classFile]
    if color and color.r then
        className = string.format("|cff%02x%02x%02x%s|r", color.r * 255, color.g * 255, color.b * 255, className)
    end
    return raceName .. " " .. className
end

--- The line under a guide's name: who it is for, its levels, when it was computed, where from.
local function GuideDetails(guide)
    local r = guide.route
    local parts = { RaceClass(r) }
    if r.fromLevel and r.toLevel then parts[#parts + 1] = string.format(T("levels %d-%d"), r.fromLevel, r.toLevel) end
    if guide.created and guide.created > 0 and date then
        parts[#parts + 1] = string.format(T("computed on %s"), date("%Y-%m-%d", guide.created))
    end
    if guide.version then parts[#parts + 1] = "v" .. tostring(guide.version) end
    parts[#parts + 1] = guide.source == "import" and T("imported") or T("from the app (managed by the app)")
    return table.concat(parts, "  ·  ")
end

--- A guide's row: its name and details; Follow, Start over (the guide followed), Delete (imported).
local function GuideRow(parent)
    local row = CreateFrame("Frame", nil, parent)
    row:SetHeight(ROW)
    row.lit = row:CreateTexture(nil, "BACKGROUND")
    row.lit:SetAllPoints()
    row.lit:SetColorTexture(1, 0.82, 0, 0.08)
    row.delete = Options.Button(row, T("Delete"), 80, function()
        local name = row.guide and row.guide.name
        if name then StaticPopup_Show("FACTORUIDE_DELETE_GUIDE", name, nil, name) end
    end)
    row.delete:SetPoint("RIGHT", -6, 0)
    row.follow = Options.Button(row, T("Follow"), 90, function()
        if row.guide then FG.Guide:Choose(row.guide.name) end
    end)
    row.follow:SetPoint("RIGHT", row.delete, "LEFT", -4, 0)
    row.restart = Options.Button(
        row,
        T("Start over"),
        90,
        function() FG.Guide:AskReset() end,
        T("Start over: undo the steps and quests marked done by hand, then go to the first step not done yet")
    )
    row.restart:SetPoint("RIGHT", row.follow, "LEFT", -4, 0)
    -- A group guide with several players of our race and class: which one we are (each click
    -- the next).
    row.who = Options.Button(row, "", 140, function()
        local choices, me = FG.Group:Choices()
        if not choices then return end
        local next = choices[1]
        for i, k in ipairs(choices) do
            if k == me then next = choices[i % #choices + 1] end
        end
        FG.Group:Choose(next)
    end, T("Several players of this group guide are of your race and class: which one you are"))
    row.who:SetPoint("RIGHT", row.restart, "LEFT", -4, 0)
    row.name = row:CreateFontString(nil, "ARTWORK", "GameFontHighlight")
    row.name:SetPoint("TOPLEFT", 8, -7)
    row.name:SetPoint("RIGHT", row.who, "LEFT", -8, 0)
    row.name:SetJustifyH("LEFT")
    row.name:SetWordWrap(false)
    row.details = row:CreateFontString(nil, "ARTWORK", "GameFontDisableSmall")
    row.details:SetPoint("TOPLEFT", 8, -25)
    row.details:SetPoint("RIGHT", row.who, "LEFT", -8, 0)
    row.details:SetJustifyH("LEFT")
    row.details:SetWordWrap(false)
    return row
end

local guideRows = {}

Options:AddBlock(function(parent)
    local block = CreateFrame("Frame", nil, parent)
    local heading = Options.Heading(block, T("Guides"))
    heading:SetPoint("TOPLEFT", 0, 0)
    local clear = Options.Button(
        block,
        T("Delete the imported guides"),
        0,
        function() FG.Import:AskClear() end,
        T("Those of the Factoruide desktop app stay.")
    )
    clear:SetPoint("TOPRIGHT", -6, 2)
    local import = Options.Button(
        block,
        T("Import a guide…"),
        0,
        function() FG.Import:Open() end,
        T("Paste a guide copied from the Factoruide web app.")
    )
    import:SetPoint("RIGHT", clear, "LEFT", -4, 0)
    local hint = block:CreateFontString(nil, "ARTWORK", "GameFontHighlightSmall")
    hint:SetPoint("TOPLEFT", heading, "BOTTOMLEFT", 0, -10)
    hint:SetJustifyH("LEFT")
    hint:SetText(T("The guide this character follows: click Follow."))
    local empty = block:CreateFontString(nil, "ARTWORK", "GameFontHighlight")
    empty:SetPoint("TOPLEFT", 0, -50)
    empty:SetPoint("RIGHT", block, "RIGHT", -6, 0)
    empty:SetJustifyH("LEFT")
    empty:SetText(T("No guide yet: import one with the button above, or send it from the Factoruide desktop app."))

    --- Bring the rows to the guides in use and the one followed.
    function block:Refresh()
        local list = FG.Import:List()
        local followed = FG.Guide:RouteName()
        local y = 48
        for i, guide in ipairs(list) do
            local row = guideRows[i] or GuideRow(self)
            guideRows[i] = row
            row.guide = guide
            row:ClearAllPoints()
            row:SetPoint("TOPLEFT", 0, -y)
            row:SetPoint("TOPRIGHT", 0, -y)
            local isFollowed = guide.name == followed
            row.name:SetText(guide.name .. (guide.fits and "" or "  |cff808080" .. T("(another character)") .. "|r"))
            row.details:SetText(GuideDetails(guide))
            row.lit:SetShown(isFollowed)
            row.follow:SetText(isFollowed and T("Followed") or T("Follow"))
            row.follow:SetEnabled(not isFollowed)
            row.restart:SetShown(isFollowed)
            local choices, me = FG.Group:Choices()
            row.who:SetShown(isFollowed and choices ~= nil)
            row.who:SetText(me and string.format(T("Player: %s"), FG.Group:PlayerName(me)) or T("Choose your player"))
            row.delete:SetShown(guide.source == "import")
            row:Show()
            y = y + ROW + 2
        end
        for i = #list + 1, #guideRows do
            guideRows[i].guide = false
            guideRows[i]:Hide()
        end
        empty:SetShown(#list == 0)
        if #list == 0 then y = y + 30 end
        clear:SetEnabled(FG.Import:ImportedCount() > 0)
        self:SetHeight(y)
    end
    return block
end)

--- The guide rows of the main page (`.guide`, `.follow`, `.restart`, `.delete`), for the tests.
function Options:GuideRows() return guideRows end

-- The settings' pages -----------------------------------------------------------------------

local PREFIX = "Factoruide_" -- the settings' variables in the game's options
local VAR_TYPES = { bool = "Boolean", number = "Number", select = "String" }

-- Language names in their own language.
local LANGUAGE_NAMES = {
    enUS = "English",
    frFR = "Français",
    deDE = "Deutsch",
    esES = "Español (España)",
    esMX = "Español (México)",
    ptBR = "Português (Brasil)",
    ruRU = "Русский",
    koKR = "한국어",
    zhCN = "简体中文",
    zhTW = "繁體中文",
}

-- The addon's language, beside the schema's settings (its own values: "" for the game's).
local LANGUAGE = {
    key = "locale",
    kind = "select",
    default = "",
    label = "Addon language",
    help = "The addon's texts. The guides keep the language they were made in.",
    reload = true,
}

local settings = {} -- key -> the game's setting
local writing -- the key a control is writing (no notice back to it)
local reloadAsked

--- Whether the setting `option` depends on is on (or it depends on none).
local function RequirementMet(option)
    if not option.requires then return true end
    local v = FG:Get(option.requires)
    return v ~= false and v ~= "off"
end

--- The choices of a select: { value, label } (the language: the game's, then every language).
local function Choices(option)
    if option ~= LANGUAGE then
        local out = {}
        for _, choice in ipairs(option.options) do
            out[#out + 1] = { value = choice.value, label = T(choice.label) }
        end
        return out
    end
    local game = GetLocale and GetLocale() or "enUS"
    local out = { { value = "", label = string.format(T("Game's language (%s)"), LANGUAGE_NAMES[game] or game) } }
    for _, lang in ipairs(FG.LANGUAGES) do
        out[#out + 1] = { value = lang, label = LANGUAGE_NAMES[lang] }
    end
    return out
end

local function Value(option)
    if option == LANGUAGE then return FG.ChosenLang() or "" end
    return FG:Get(option.key)
end

--- A number as its slider shows it.
local function Shown(option, v)
    if option.percent then return string.format("%d%%", math.floor(v * 100 + 0.5)) end
    if option.step < 1 then return string.format("%.1f", v) end
    return string.format("%d", v)
end

StaticPopupDialogs["FACTORUIDE_RELOAD"] = {
    text = T("Some changes are applied after /reload."),
    button1 = T("Reload the interface"),
    button2 = T("Later"),
    OnAccept = function() ReloadUI() end,
    timeout = 0,
    whileDead = true,
    hideOnEscape = true,
}

--- After a change applied only after a /reload: the game's dialog offers to reload, once for the
--- changes made together (the page's Defaults button changes several).
local function AskReload()
    if reloadAsked then return end
    reloadAsked = true
    C_Timer.After(0, function()
        reloadAsked = false
        StaticPopup_Show("FACTORUIDE_RELOAD")
    end)
end

--- Save a setting chosen in the options (its default: nothing saved).
local function Write(option, v)
    if option.kind == "number" then
        v = option.min + math.floor((v - option.min) / option.step + 0.5) * option.step
        v = math.floor(v * 1e6 + 0.5) / 1e6
    end
    if v == option.default then v = nil end
    writing = option.key
    FG:Save(option.key, v)
    writing = nil
    if option.reload then AskReload() end
end

local function Tooltip(option)
    local text = option.help and T(option.help) or nil
    if option.reload then
        text = (text and text .. "\n\n" or "") .. "|cffff8040" .. T("Applied after /reload.") .. "|r"
    end
    return text
end

--- One setting's control on page `category`; `controls`: the page's, by key (those it depends on).
local function Control(category, option, controls)
    local setting = Settings.RegisterProxySetting(
        category,
        PREFIX .. option.key,
        Settings.VarType[VAR_TYPES[option.kind]],
        T(option.label),
        option.default,
        function() return Value(option) end,
        function(v) Write(option, v) end
    )
    settings[option.key] = setting
    local control
    if option.kind == "bool" then
        control = Settings.CreateCheckbox(category, setting, Tooltip(option))
    elseif option.kind == "number" then
        local slider = Settings.CreateSliderOptions(option.min, option.max, option.step)
        slider:SetLabelFormatter(MinimalSliderWithSteppersMixin.Label.Right, function(v) return Shown(option, v) end)
        control = Settings.CreateSlider(category, setting, slider, Tooltip(option))
    else
        local function Entries()
            local container = Settings.CreateControlTextContainer()
            for _, choice in ipairs(Choices(option)) do
                container:Add(choice.value, choice.label)
            end
            return container:GetData()
        end
        control = Settings.CreateDropdown(category, setting, Entries, Tooltip(option))
    end
    -- Greyed while the setting it needs is off (under it when on the same page).
    if option.requires then
        local function Met() return RequirementMet(option) end
        if controls[option.requires] then
            control:SetParentInitializer(controls[option.requires], Met)
        else
            control:AddModifyPredicate(Met)
        end
    end
    controls[option.key] = control
end

--- A group's page: its sections (the options of this edition only), each with a header when titled.
local function GroupPage(parent, group)
    local category = Settings.RegisterVerticalLayoutSubcategory(parent, T(group.title))
    local layout = SettingsPanel:GetLayout(category)
    local controls = {}
    for _, section in ipairs(group.sections) do
        local options = {}
        for _, option in ipairs(section.options) do
            if not option.only or option.only == FG.Edition then options[#options + 1] = option end
        end
        if #options > 0 then
            if section.title then
                layout:AddInitializer(CreateSettingsListSectionHeaderInitializer(T(section.title)))
            end
            for _, option in ipairs(options) do
                Control(category, option, controls)
                -- The buttons that belong with it (Options:AddButton).
                for _, button in ipairs(Options.buttons[option.key] or {}) do
                    layout:AddInitializer(Options.ButtonInitializer(button))
                end
            end
        end
    end
end

-- The groups' pages, the language at the end of the first one.
Options:AddPage(function(parent)
    for i, group in ipairs(FG.SettingsSchema or {}) do
        if i == 1 then
            local sections = {}
            for _, section in ipairs(group.sections) do
                sections[#sections + 1] = section
            end
            sections[#sections + 1] = { title = "Language", options = { LANGUAGE } }
            group = { title = group.title, sections = sections }
        end
        GroupPage(parent, group)
    end
end)

-- A setting changed elsewhere (the guide's buttons, a key binding) shows in the options.
FG:OnSetting("*", function(key)
    if key == writing or not settings[key] or not (Settings and Settings.NotifyUpdate) then return end
    Settings.NotifyUpdate(PREFIX .. key)
end)
