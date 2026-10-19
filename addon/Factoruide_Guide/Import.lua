-- "Import a guide…" (the addon's options, `/fg`): guides made in the Factoruide apps pasted in game, one text each (or several
-- guides in one):
--   FGG1:<LibDeflate:EncodeForPrint(LibDeflate:CompressDeflate(lua))>
-- lua = `Factoruide.Routes["name"] = {...}` for each guide, with its `.created` (unix seconds the
-- guide version was computed) and `.version`, the places that serve the characters
-- (`Factoruide.Services`) and maybe `FactoruideSent = { ["Name-Realm"] = last seq }`: the same text
-- the desktop app writes into the data addon Factoruide_Data. It runs in a sandbox
-- (FG.RunGuideText, the profile part's Data.lua) and must leave data only. A `Factoruide.Config`
-- is ignored: the settings are chosen in game. An `.Assignments` (older apps: character -> guide)
-- is kept as the first choice of those characters.
--
-- Imports add up: each guide is kept on its own in FactoruideDB.guides[name] = { text, created,
-- version, time } (text: its table as Lua, run again at each load), the other data of the last
-- import in FactoruideDB.guideExtras = { text, time }. A guide imported again replaces the one of
-- the same name, unless it was computed before it: then the player is asked.
--
-- The guides in use (FG.Routes) are the union of the imported ones, Factoruide_Data's (FG.DataGuide,
-- run by the profile part) and those an older app wrote into Routes.lua: of two with the same name,
-- the one computed last. Each character chooses its guide in the addon's options (main page, Guides;
-- Guide.lua keeps the choice).
--
-- The addon updates on its own (an addon site), apart from the apps: it reads every guide an older
-- app made. A guide's `.format` (none: 1) tells what it needs; one above Import.FORMAT was made for
-- a newer addon: refused at the import, left out of Factoruide_Data's, the player asked to update.

local FG = Factoruide
local T = FG.T
local Import = {}
FG.Import = Import

Import.PREFIX = "FGG1:"

--- The newest guide format this addon follows (the apps' GUIDE_FORMAT, fg-route job/routes.rs).
Import.FORMAT = 2

--- Whether guide `r` was made for a newer addon (its format above this addon's).
function Import.TooNew(r) return (tonumber(r.format) or 1) > Import.FORMAT end

--- Run a guide's Lua text in the sandbox: returns the table it filled (its `Factoruide`) and its
--- FactoruideSent, or nil and the error.
Import.Run = FG.RunGuideText

-- What a text sets besides its guides and is not kept with them.
local NOT_EXTRA = { Routes = true, Assignments = true, Config = true }

-- Saving tables as Lua text -----------------------------------------------------------------

local KEYWORDS = {}
for word in
    ("and break do else elseif end false for function if in local nil not or repeat return then true until while"):gmatch(
        "%a+"
    )
do
    KEYWORDS[word] = true
end

local function Number(v)
    if v ~= v then return "0/0" end
    if v == math.huge then return "1/0" end
    if v == -math.huge then return "-1/0" end
    if v == math.floor(v) and math.abs(v) < 2 ^ 53 then return string.format("%d", v) end
    local s = string.format("%.14g", v)
    if tonumber(s) ~= v then s = string.format("%.17g", v) end
    return s
end

local function Write(v, out, open)
    local kind = type(v)
    if kind == "string" then
        out[#out + 1] = string.format("%q", v)
    elseif kind == "number" then
        out[#out + 1] = Number(v)
    elseif kind == "boolean" or kind == "nil" then
        out[#out + 1] = tostring(v)
    elseif kind == "table" then
        if open[v] then error("a table inside itself") end
        open[v] = true
        out[#out + 1] = "{"
        local n = #v
        for i = 1, n do
            Write(v[i], out, open)
            out[#out + 1] = ","
        end
        for k, x in pairs(v) do
            local listed = type(k) == "number" and k >= 1 and k <= n and k == math.floor(k)
            if not listed then
                if type(k) == "string" and k:match("^[%a_][%w_]*$") and not KEYWORDS[k] then
                    out[#out + 1] = k .. "="
                else
                    out[#out + 1] = "["
                    Write(k, out, open)
                    out[#out + 1] = "]="
                end
                Write(x, out, open)
                out[#out + 1] = ","
            end
        end
        out[#out + 1] = "}"
        open[v] = nil
    else
        error("cannot keep a " .. kind)
    end
end

--- `v` (data only) as Lua text giving it back (see Restore), or nil and the error.
function Import.Serialize(v)
    local out = { "return " }
    local ok, err = pcall(Write, v, out, {})
    if not ok then return nil, err end
    return table.concat(out)
end

--- The table a text of Serialize gives back (run with nothing reachable), or nil.
local function Restore(text)
    if type(text) ~= "string" or text:byte(1) == 27 then return nil end
    local chunk = loadstring(text, "=guide")
    if not chunk then return nil end
    setfenv(chunk, {})
    local ok, v = pcall(chunk)
    if ok and type(v) == "table" then return v end
end

-- The guides in use ------------------------------------------------------------------------

local function DB()
    FactoruideDB.guides = type(FactoruideDB.guides) == "table" and FactoruideDB.guides or {}
    return FactoruideDB
end

local function IsGuide(name, r) return type(name) == "string" and type(r) == "table" and type(r.steps) == "table" end

local parsed = {} -- name -> { text, route }: the imported guides run once per text

--- The imported guide `name`'s table, nil when its text cannot be read.
local function Imported(name, entry)
    local cached = parsed[name]
    if cached and cached.text == entry.text then return cached.route end
    local r = Restore(entry.text)
    if not IsGuide(name, r) then
        FG:Error("import", "guide " .. name .. " unreadable")
        return nil
    end
    parsed[name] = { text = entry.text, route = r }
    return r
end

--- Build FG.Routes, FG.Services... from the guides kept, those of Factoruide_Data and the files'.
--- `Import.info[name]` = { source = "import" or "app", created, version } of the guide in use.
function Import:Apply()
    local db = DB()
    -- What the files set (Routes.lua and Services.lua ship empty; older apps wrote them), taken once.
    if not self.file then
        local routes = {}
        for name, r in pairs(type(FG.Routes) == "table" and FG.Routes or {}) do
            routes[name] = r
        end
        self.file = {
            routes = routes,
            assignments = type(FG.Assignments) == "table" and FG.Assignments or {},
            services = FG.Services,
        }
    end
    if FG.DataGuide then
        self.data = FG.DataGuide
        FG.DataGuide = nil
    end
    local data = self.data

    local routes, info, tooNew = {}, {}, {}
    -- Of two with the same name, the one computed last (the later offer when the same). Those
    -- made for a newer addon are left out (told at the login).
    local function Offer(name, r, source, created)
        if not IsGuide(name, r) then return end
        if Import.TooNew(r) then
            tooNew[name] = true
            return
        end
        created = tonumber(created) or 0
        if info[name] and info[name].created > created then return end
        routes[name] = r
        info[name] = { source = source, created = created, version = tonumber(r.version) }
    end
    for name, r in pairs(self.file.routes) do
        Offer(name, r, "app", type(r) == "table" and r.created)
    end
    if data and type(data.ns.Routes) == "table" then
        for name, r in pairs(data.ns.Routes) do
            Offer(name, r, "app", type(r) == "table" and r.created or data.time)
        end
    end
    for name, entry in pairs(db.guides) do
        local r = type(entry) == "table" and Imported(name, entry)
        if r then Offer(name, r, "import", entry.created) end
    end
    FG.Routes, self.info = routes, info
    self.tooNew = {}
    for name in pairs(tooNew) do
        if not routes[name] then self.tooNew[#self.tooNew + 1] = name end
    end
    table.sort(self.tooNew)

    -- The other data (the places): the most recent of each.
    local extras = { { time = -1, ns = { Services = self.file.services } } }
    if data then extras[#extras + 1] = { time = data.time, ns = data.ns } end
    local kept = db.guideExtras
    if type(kept) == "table" then
        local ns = Restore(kept.text)
        if ns then extras[#extras + 1] = { time = tonumber(kept.time) or 0, ns = ns } end
    end
    table.sort(extras, function(a, b) return a.time < b.time end)
    for _, extra in ipairs(extras) do
        for k, v in pairs(extra.ns) do
            if not NOT_EXTRA[k] and type(FG[k]) ~= "function" then FG[k] = v end
        end
    end

    -- Older apps' assignments: the first choice of the characters they name.
    local assignments = {}
    local sources = { self.file.assignments, data and data.ns.Assignments or false, db.guideAssignments }
    for i = 1, 3 do
        local from = sources[i]
        for character, name in pairs(type(from) == "table" and from or {}) do
            if type(character) == "string" and type(name) == "string" then assignments[character] = name end
        end
    end
    FG.Assignments = assignments
end

--- A text run (its `Factoruide`) made ready to keep: { guides = { name -> { text, created,
--- version, older = what is kept when older } }, count, extras, assignments, tooNew = names of the
--- guides made for a newer addon, left out }, or nil and an error. Guides without `.created` take
--- `when`.
local function Prepare(ns, when)
    local db = DB()
    local pending = { guides = {}, count = 0, older = {}, tooNew = {} }
    for name, r in pairs(type(ns.Routes) == "table" and ns.Routes or {}) do
        if IsGuide(name, r) and Import.TooNew(r) then
            pending.tooNew[#pending.tooNew + 1] = name
        elseif IsGuide(name, r) then
            local text, err = Import.Serialize(r)
            if not text then return nil, err end
            local created = tonumber(r.created) or when
            local have = type(db.guides[name]) == "table" and tonumber(db.guides[name].created)
            local guide = { text = text, created = created, version = tonumber(r.version) }
            if have and created < have then
                guide.older = have
                pending.older[#pending.older + 1] = name
            end
            pending.guides[name] = guide
            pending.count = pending.count + 1
        end
    end
    table.sort(pending.older)
    local extras = {}
    for k, v in pairs(ns) do
        if not NOT_EXTRA[k] then extras[k] = v end
    end
    if next(extras) then
        local text, err = Import.Serialize(extras)
        if not text then return nil, err end
        pending.extras = text
    end
    if type(ns.Assignments) == "table" then pending.assignments = ns.Assignments end
    return pending
end

--- Keep a prepared text: its guides (the older ones only with `replaceOlder`), its other data and
--- assignments. Returns how many guides were kept.
local function Keep(pending, replaceOlder, when)
    local db = DB()
    local count = 0
    for name, guide in pairs(pending.guides) do
        if not guide.older or replaceOlder then
            db.guides[name] = { text = guide.text, created = guide.created, version = guide.version, time = when }
            count = count + 1
        end
    end
    if pending.extras then db.guideExtras = { text = pending.extras, time = when } end
    if pending.assignments then
        db.guideAssignments = type(db.guideAssignments) == "table" and db.guideAssignments or {}
        for character, name in pairs(pending.assignments) do
            if type(character) == "string" and type(name) == "string" then db.guideAssignments[character] = name end
        end
    end
    return count
end

--- Before guides were kept one by one: the last import's whole text (FactoruideDB.guideImport).
local function MoveOldImport()
    local old = FactoruideDB.guideImport
    if old == nil then return end
    FactoruideDB.guideImport = nil
    if type(old) ~= "table" or type(old.text) ~= "string" then return end
    local ns, err = Import.Run(old.text)
    local pending = ns and Prepare(ns, tonumber(old.time) or 0)
    if not pending then
        Import.failed = tostring(err)
        return FG:Error("import", err)
    end
    Keep(pending, true, tonumber(old.time) or 0)
end

--- Apply what is kept (at load).
function Import:ApplySaved()
    MoveOldImport()
    self:Apply()
end

local tried = FactoruideDB ~= nil
if tried then Import:ApplySaved() end
-- Without the saved variables yet (outside the game), as soon as they are there.
FG:On("ADDON_LOADED", function(name)
    if name ~= "Factoruide_Guide" or tried then return end
    tried = true
    Import:ApplySaved()
end)

-- The list (the options' main page) ----------------------------------------------------------------

--- Whether guide `r` was made for this character's race and class (and faction).
function Import.Fits(r)
    local _, _, race = UnitRace("player")
    local _, _, class = UnitClass("player")
    race, class = FG.Readable(race), FG.Readable(class)
    local faction = FG.Readable(UnitFactionGroup("player"))
    if type(r) ~= "table" or not race or not class then return false end
    if r.race ~= race or r.class ~= class then return false end
    return not (r.faction and faction) or r.faction == faction
end

--- The guides in use: { name, route, source ("import" or "app"), created, version, fits }, those
--- made for this character first, then by name.
function Import:List()
    local out = {}
    for name, r in pairs(FG.Routes or {}) do
        local info = self.info and self.info[name] or {}
        out[#out + 1] = {
            name = name,
            route = r,
            source = info.source or "app",
            created = info.created,
            version = info.version or tonumber(r.version),
            fits = Import.Fits(r),
        }
    end
    table.sort(out, function(a, b)
        if a.fits ~= b.fits then return a.fits end
        return a.name < b.name
    end)
    return out
end

local function Changed()
    if FG.Guide and FG.Guide.FollowChoice and FG.Guide:State() then FG.Guide:FollowChoice() end
    FG.Options:Refresh()
end

--- Delete the imported guide `name` (those of the app stay: it writes them again). Returns whether
--- one was deleted.
function Import:Delete(name)
    local db = DB()
    if not db.guides[name] then return false end
    db.guides[name] = nil
    parsed[name] = nil
    self:Apply()
    Changed()
    return true
end

--- Delete every imported guide and what came with them. Returns how many there were.
function Import:Clear()
    local db = DB()
    local n = 0
    for _ in pairs(db.guides) do
        n = n + 1
    end
    db.guides, db.guideExtras, db.guideAssignments = {}, nil, nil
    wipe(parsed)
    self:Apply()
    Changed()
    return n
end

-- Importing --------------------------------------------------------------------------------

--- The guide's Lua text out of a pasted string, or nil and a message for the player.
function Import.Decode(s)
    s = (s or ""):gsub("%s", "") -- line breaks or spaces picked up with the copy
    if s == "" then return nil, T("Paste the text first") end
    if s:sub(1, #Import.PREFIX) ~= Import.PREFIX then
        return nil, T("Not a Factoruide guide: the text should start with FGG1:")
    end
    local deflate = LibStub and LibStub("LibDeflate", true)
    if not deflate then return nil, "LibDeflate missing" end
    local ok, text = pcall(function()
        local raw = deflate:DecodeForPrint(s:sub(#Import.PREFIX + 1))
        return raw and deflate:DecompressDeflate(raw)
    end)
    if not ok or type(text) ~= "string" then return nil, T("The text is damaged: copy it again from the web app") end
    return text
end

--- Read a pasted string: its prepared guides (see Prepare; `.older`: the names of those computed
--- before the ones kept), its FactoruideSent kept at once. Nil and a message for the player when
--- it cannot be read.
function Import:Read(s)
    local text, err = Import.Decode(s)
    if not text then return nil, err end
    local ns, sent = Import.Run(text)
    if not ns then return nil, string.format(T("The guide could not be read: %s"), tostring(sent)) end
    local pending, prepareErr = Prepare(ns, time())
    if not pending then return nil, string.format(T("The guide could not be read: %s"), tostring(prepareErr)) end
    -- Made by a newer app: the whole text waits for the addon's update (its guides come together).
    if #pending.tooNew > 0 then
        return nil,
            T("This guide was made by a newer Factoruide app: update the Factoruide addon, then import it again")
    end
    if pending.count == 0 and not pending.extras then return nil, T("No guide in this text") end
    -- What the contribution server already has: the collector drops these rows.
    FactoruideDB.sent = type(FactoruideDB.sent) == "table" and FactoruideDB.sent or {}
    for character, seq in pairs(FG.SentSeqs(sent)) do
        FactoruideDB.sent[character] = math.max(tonumber(FactoruideDB.sent[character]) or 0, seq)
    end
    return pending
end

--- Keep what `Import:Read` gave, the guides computed before those kept only with `replaceOlder`.
--- Returns how many guides were kept.
function Import:Keep(pending, replaceOlder)
    local count = Keep(pending, replaceOlder, time())
    FactoruideDB.guideImportFresh = count
    return count
end

--- Import a pasted string at once (the older guides replaced only with `replaceOlder`): the number
--- of guides kept, or nil and a message for the player.
function Import:FromString(s, replaceOlder)
    local pending, err = self:Read(s)
    if not pending then return nil, err end
    return self:Keep(pending, replaceOlder)
end

local function Day(t) return date and t and t > 0 and date("%Y-%m-%d", t) or "?" end

--- The question asked when guides of the text were computed before the ones kept.
function Import.OlderQuestion(pending)
    local names = pending.older
    if #names == 1 then
        local guide = pending.guides[names[1]]
        return string.format(
            T("The guide %s is older than the one you have (computed on %s, yours on %s): replace it?"),
            names[1],
            Day(guide.created),
            Day(guide.older)
        )
    end
    local shown = {}
    for i = 1, math.min(#names, 4) do
        shown[i] = names[i]
    end
    if #names > 4 then shown[#shown + 1] = "…" end
    return string.format(
        T("%d guides are older than the ones you have (%s): replace them?"),
        #names,
        table.concat(shown, ", ")
    )
end

--- Kept: told, and the interface reloaded to use them (nothing kept: told only).
local function Done(count)
    local window = FG.TextWindow
    if count == 0 then
        FactoruideDB.guideImportFresh = nil
        window:Status(T("nothing imported: you kept your guides"))
        return FG:Print(T("nothing imported: you kept your guides"))
    end
    window:Status(string.format(T("guide imported (%d guides), reloading the interface"), count))
    FG:Print(T("guide imported (%d guides), reloading the interface"), count)
    ReloadUI()
end

local pendingAnswer -- the text waiting for the player's answer about older guides

StaticPopupDialogs["FACTORUIDE_OLDER_GUIDE"] = {
    text = "%s",
    button1 = YES,
    button2 = NO,
    OnAccept = function()
        local pending = pendingAnswer
        pendingAnswer = nil
        if pending then Done(Import:Keep(pending, true)) end
    end,
    OnCancel = function()
        local pending = pendingAnswer
        pendingAnswer = nil
        if pending then Done(Import:Keep(pending, false)) end
    end,
    timeout = 0,
    whileDead = true,
    hideOnEscape = true,
}

--- The pasted string imported from the window: asks first when guides are older than those kept.
function Import:Submit(s)
    local window = FG.TextWindow
    local pending, err = self:Read(s)
    if not pending then return window:Status(err, true) end
    if #pending.older == 0 then return Done(self:Keep(pending, false)) end
    pendingAnswer = pending
    window:Status(T("Answer the question to finish the import"))
    -- The question shows over the game's options, not over the text window: it makes way.
    window:Hide()
    StaticPopup_Show("FACTORUIDE_OLDER_GUIDE", Import.OlderQuestion(pending))
end

--- Open the window to paste a guide into.
function Import:Open()
    FG.TextWindow:Open({
        title = T("Factoruide: guide import"),
        hint = T("Paste here (Ctrl+V) the text copied from the Factoruide web app, then click Import"),
        closeLabel = T("Cancel"),
        button = {
            label = T("Import"),
            onClick = function(text) Import:Submit(text) end,
        },
    })
end

StaticPopupDialogs["FACTORUIDE_CLEAR_GUIDES"] = {
    text = "%s",
    button1 = YES,
    button2 = NO,
    OnAccept = function() FG:Print(T("imported guides deleted (%d)"), Import:Clear()) end,
    timeout = 0,
    whileDead = true,
    hideOnEscape = true,
}

--- How many guides were imported.
function Import:ImportedCount()
    local n = 0
    for _ in pairs(DB().guides) do
        n = n + 1
    end
    return n
end

--- Delete the imported guides, asked first ("Delete the imported guides").
function Import:AskClear()
    local n = self:ImportedCount()
    if n == 0 then return FG:Print(T("no imported guide")) end
    StaticPopup_Show(
        "FACTORUIDE_CLEAR_GUIDES",
        string.format(T("Delete the %d imported guides? Those of the Factoruide app stay."), n)
    )
end

FG:On("PLAYER_LOGIN", function()
    local fresh = FactoruideDB.guideImportFresh
    if Import.failed then
        FG:Print(
            T("the imported guide could not be read (%s): import it again (/fg, Import a guide…)"),
            Import.failed
        )
    elseif fresh then
        FG:Print(T("%d guides imported: choose the one to follow in /fg"), fresh)
    end
    if Import.tooNew and #Import.tooNew > 0 then
        FG:Print(
            T("%s: made by a newer Factoruide app, update the Factoruide addon to follow it"),
            table.concat(Import.tooNew, ", ")
        )
    end
    FactoruideDB.guideImportFresh = nil
end)
