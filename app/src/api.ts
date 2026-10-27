// Typed access to the Tauri backend (app/src-tauri/src/main.rs), or in the web build to the
// backend in Web Workers (crates/fg-web, see web/host.ts).
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { download, fromBase64 } from "./web/mode";

/** A file the web backend made for the browser to download (the desktop app writes it itself and gives its path). */
export interface WebFile {
  file: string;
  text?: string;
  base64?: string;
}

/** The path of a file the desktop app wrote, or the name of the one the browser downloads. */
function saved(result: string | WebFile): string {
  if (typeof result === "string") return result;
  download(result.file, result.base64 !== undefined ? fromBase64(result.base64) : (result.text ?? ""));
  return result.file;
}

/** A guide as the text to paste on Polestar's Import page, mirror of `fg_route::export::polestar::ImportString`. */
export interface PolestarString {
  text: string;
  steps: number;
  /** The guide data, decompressed (bytes). */
  bytes: number;
  /** Within what Polestar takes today (60 steps, 50,000 characters). */
  fits: boolean;
}

/** A character's export (Export my character) pasted in the web app. */
/** A quest to choose as a must-have one: its name in the guide's language when known. */
export interface QuestChoice {
  id: number;
  name: string;
  level: number;
  zone: string | null;
}

export interface ProfileImported {
  character: string;
  edition: Edition;
  level: number;
}

/** Planner tunables, mirror of `fg_route::Params`. */
export interface Params {
  /** The zone the route begins in (AreaTable zone), chosen on the starts' preview; 0 or unset: the planner's choice. */
  start_zone?: number;
  run_speed: number;
  mount_level: number;
  mount_bonus: number;
  flight_speed: number;
  detour: number;
  stop_overhead: number;
  flight_overhead: number;
  kill_time: number;
  grind_kill_time: number;
  default_xp_difficulty: number;
  default_kill_count: number;
  default_item_count: number;
  default_drop_chance: number;
  min_start_chance: number;
  object_time: number;
  max_quest_above: number;
  progression: string;
  progression_margin?: number | null;
  pack_power?: number | null;
  elite_pull_levels: number;
  pvp_quests: boolean;
  pvp_mark_time: number;
  /** Crowded server (a launch): escorts, events and targets with few spawns take waiting. */
  crowded: number;
  crowd_event_time: number;
  crowd_players: number;
  crowd_by_level: number[];
  respawn_time: number;
  dynamic_respawn: number;
  pull_levels: number;
  quest_log_size: number;
  flight_learn_radius: number;
  farm_on_way: boolean;
  farm_corridor: number;
  farm_share: number;
  farm_below: number;
  farm_above: number;
  camp_radius: number;
  allow_elite: boolean;
  all_class_quests: boolean;
  solo_elite_kills: number;
  shortcuts: boolean;
  elite_kill_factor: number;
  alns: number;
  excluded_sorts: number[];
  excluded_quests: number[];
  preferred_zones: number[];
  excluded_zones: number[];
  /** Zones whose every quest is done, worth it or not (the guide goes past its level for them). */
  forced_zones: number[];
  preferred_zone_weight: number;
  /** Reputations to reach by the end of the guide (none by default: reputation weighs nothing). */
  reputation_goals: ReputationGoal[];
  /** Seconds each point missing from the goals at the end of the guide is worth. */
  reputation_weight: number;
  min_efficiency: number;
  hearth_cooldown: number;
  hearth_cast: number;
  recall_cooldown: number;
  teleport_cast: number;
  train_every: number;
  train_radius: number;
  train_max_delay: number;
  train_time: number;
  train_min_gain: number;
  train_trip_gain: number;
  bag_weight: number;
  loot_per_kill: number;
  sell_radius: number;
  sell_time: number;
  gather_share: number;
  gear_floor: number;
  /** The player's own class power curve, as (level, levels of power) points: empty for the class's. */
  power_curve?: [number, number][];
  /** The player's own gear dependence: null for the class's. */
  power_gear?: number | null;
  power_scale: number;
  dungeon_loot_share: number;
  gear_value: number;
  mandatory_penalty: number;
  grind_weight: number;
  allow_grind: boolean;
  grind_cap: number;
  farm_cap: number;
  grind_over_weight: number;
  gather_overlap: number;
  gather_radius: number;
  objective_alternatives: number;
  flight_weight: number;
  gold_weight: number;
  zone_change_weight: number;
  abandon_weight: number;
  profession_quest_weight: number;
  zone_stay: number;
  forced_dungeons: number[];
  /** Dungeons run with every quest of theirs the character can take. */
  forced_dungeon_quests: number[];
  excluded_dungeons: number[];
  dungeon_quest_weight: number;
  construction_random: boolean;
  continent_stay: number;
  candidates: number;
  race_mutants: number;
  screen: number;
  qualify_tries: number;
  qualify_min_tries: number;
  round_tries: number;
  final_routes: number;
  final_tries: number;
  /** How much the optimization tries (the calculation presets). */
  effort: number;
  share_every: number;
  fit_penalty: number;
  fit_above_penalty: number;
  efficiency_power: number;
  max_object_loot: number;
  common_item_sources: number;
  common_item_time: number;
  dungeons: boolean;
  dungeon_pickup_ahead: number;
  /** Levels dungeons are run at, around their level (-3 and +3: from 15 to 21 for a level 18 dungeon). */
  dungeon_from: number;
  dungeon_to: number;
  dungeon_xp_share: number;
  construction: string;
  threads: number;
  seed: number;
  /** Players leveling together (set from the request's group). */
  group_size: number;
  group_kill_speed: number;
  group_shared_drops: boolean;
  explore_radius: number;
  edition: Edition;
}

export interface ProfessionGoal {
  key: string;
  target: number;
  /** Character level at which the profession is taken (default 5): the curve starts there. */
  start_level?: number | null;
  /** Skill to reach by character level, [level, skill]; empty = the profession default curve. */
  milestones?: [number, number][];
}

export interface PlanRequest {
  race: string;
  class: string;
  from_level: number;
  to_level: number;
  name?: string | null;
  params?: Params | null;
  required_class_quests?: string[] | null;
  /** Exact quest IDs or names. Prerequisites are included; omission fails generation. */
  required_quests?: (number | string)[];
  professions?: ProfessionGoal[];
  /** Language of the guide texts (English where a name is not translated). */
  locale?: string | null;
  /** The character as the addon recorded it in game: the guide goes on from there. */
  start?: StartState | null;
  /** The other players of a group guide, as their addon recorded them (empty: solo). */
  mates?: StartState[];
  /** The professions each of `mates` levels, in their order. */
  mate_professions?: ProfessionGoal[][];
}

/** A point of a game map (uiMapID, percent coordinates). */
export interface MapPos {
  map: number;
  x: number;
  y: number;
}

/** A character as the addon recorded it ("Save the profile now", or at each logout); mirror of `fg_route::job::StartState`. */
export interface StartState {
  /** "Name-Realm". */
  name: string;
  /** ChrRaces ID and class file name (MAGE...). */
  race: number;
  class: string;
  faction: string;
  level: number;
  xp: number;
  xp_max: number;
  rested: number;
  resting: boolean;
  completed: number[];
  log: { id: number; complete: boolean; objectives: { text: string; done: number; need: number; finished: boolean }[] }[];
  position: MapPos | null;
  instance: boolean;
  bind: { name: string; position: MapPos | null } | null;
  /** Flight paths known (TaxiNodes IDs). */
  flights: number[];
  /** `flights` is final (deduced and edited in the app): the planner does not deduce them again. */
  flights_final?: boolean;
  professions: { line: number; rank: number }[];
  riding: number;
  gear: number[];
  spells: number[];
  /** Points with each faction the game lists (none: recorded by an older addon). */
  reputation?: { id: number; value: number }[];
  /** Copper. */
  money: number;
  /** When it was recorded (unix seconds). */
  time: number;
}

/** A character found in the game's saved variables. */
export interface CharacterInfo {
  /** Full name when the game gives one (WoW Forever: first and last name). */
  full_name: string | null;
  /** Race key (null: a race this game version does not know). */
  race: string | null;
  class: string;
  /** Professions it has: key and skill. */
  professions: [string, number][];
  /** Another player's, received in a party (`/fg partage`): for group guides, not played here. */
  shared: boolean;
  profile: StartState;
}

export interface Breakdown {
  travel: number;
  flights: number;
  fighting: number;
  grinding: number;
  overhead: number;
  training: number;
  hearths: number;
  missing_class_quests: number;
  /** Quests of the forced zones the guide does not do. */
  missing_zone_quests?: number;
  dungeons: number;
  dungeon_runs: number;
  quest_xp: number;
  mob_xp: number;
  grind_xp: number;
  /** Mobs killed on the way (farming while moving): time and XP. */
  farming?: number;
  farm_xp?: number;
  /** Grinding beyond the share of each level it may give (part of `grinding`). */
  grind_over?: number;
  /** XP from discovering areas. */
  explore_xp?: number;
  /** Copper spent on flights (unknown for guides computed before it was kept). */
  flight_cost?: number;
  /** The objectives' fixed time on top of their kills (respawns, events, buying), part of `fighting`. */
  waiting?: number;
}

export interface Step {
  kind: string;
  /** Grind steps: XP to have in `level` before going on. */
  xp?: number;
  quest?: number;
  quest_name?: string;
  objective?: number;
  /** Done along the way (mobs met going to the next steps): does not stop the guide. */
  bg?: boolean;
  /** A quest done inside the dungeon of the run step before it. */
  inside?: boolean;
  /** Mobs to kill on the way to the step (farming on the way). */
  farm?: number;
  /** The XP they give (rested bonus included): what the addon counts. */
  farm_xp?: number;
  text: string;
  target?: { kind: string; id: number; name: string };
  zone?: string;
  /** The zone's ID (AreaTable). */
  zone_id?: number;
  /** Mobs to target (to kill, or that drop the item): their names. */
  mobs?: string[];
  map?: { ui_map: number; x: number; y: number };
  /** World position: [continent (map ID), x, y]. */
  world?: [number, number, number];
  /** XP the guide has in `level` once the step is done. */
  route_xp?: number;
  /** Checkpoints and training: kept when skipping ahead. */
  keep?: boolean;
  /** Character power (level plus what gear and spells add) and the part of it its gear brings, when known. */
  power?: number;
  gear?: number;
  level: number;
  time: number;
}

/** A profession followed along the guide: its skill at each character level (`curve`, index 0..=60). */
export interface RouteProfession {
  key: string;
  name: string;
  target: number;
  start: number;
  curve: number[];
}

export interface Route {
  name: string;
  race_id: number;
  class_id: number;
  class: string;
  faction: string;
  from_level: number;
  to_level: number;
  total_time: number;
  total_time_text: string;
  quests: number;
  breakdown: Breakdown;
  /** The reputation goals and how far the guide gets (none without goals). */
  reputation?: ReputationReached[];
  professions?: RouteProfession[];
  steps: Step[];
}

export interface PlanOutcome {
  route: Route;
  notes: string[];
}

/** A dungeon, with the levels of most of its quests. */
/** A reputation to reach: points counted from the bottom of Neutral (Honored: 9000). */
export interface ReputationGoal {
  faction: number;
  target: number;
}

/** A faction quests give reputation with: its side (null: neutral) and each race's start. */
export interface FactionOption {
  id: number;
  name: string;
  side: string | null;
  quests: number;
  /** ChrRaces ID -> starting points. */
  starts: Record<string, number>;
}

/** Where a guide starts and ends with the faction of a reputation goal. */
export interface ReputationReached {
  faction: number;
  name: string;
  start: number;
  reached: number;
  target: number;
}

export interface DungeonOption extends NamedId {
  /** Its level (its monsters'): run from `dungeon_from` to `dungeon_to` around it. */
  level: number | null;
}

export interface NamedId {
  id: number;
  key: string;
  name: string;
  /** Translated names by game locale (zones, dungeons). */
  names?: Record<string, string>;
}

export interface Options {
  /** `classes`: class keys the race can play. */
  races: { key: string; id: number; name: string; faction: string; classes: string[]; start_npc: number }[];
  classes: NamedId[];
  zones: { id: number; name: string; names?: Record<string, string>; continent: number }[];
  /** The flight paths of the game version (TaxiNodes), and the sides that can use them. */
  flights: { id: number; name: string; continent: number; alliance: boolean; horde: boolean }[];
  dungeons: DungeonOption[];
  /** Factions quests give reputation with, most quests first. */
  factions: FactionOption[];
  class_quests: Record<string, string[]>;
  /** Each class's power curve and gear dependence, by class name in lower case. */
  class_power?: Record<string, { curve: [number, number][]; gear: number }>;
  params: Params;
  /** Level cap of the game version. */
  max_level: number;
}

export type Edition = "forever" | "classic" | "tbc";

/** A game version the app knows: data bundled (`available`), game client found (`installed`). */
export interface EditionInfo {
  key: Edition;
  name: string;
  available: boolean;
  installed: boolean;
  max_level: number;
}

export interface Settings {
  wow_dir: string;
  /** Contribution server address (unset: the built-in one). */
  contribution_url?: string | null;
  /** Send what is new to the contribution server on its own, once a day at most (off by default). */
  auto_contribute?: boolean;
  /** Game version the app works on. */
  edition: Edition;
  /** App language (also the default guide language); unset: the system language. */
  language?: string | null;
  /** Addon behaviour, written to Config.lua on install. */
  addon: Record<string, boolean | number | string>;
  /** What shows the guides in game on WoW Forever: the addon, or Polestar with a guide pack of them (elsewhere always the addon). */
  guide_addon?: "factoruide" | "polestar";
  /** Language of the guides in the addon (game locale); unset: each guide in the language it was made in. */
  guide_locale?: string | null;
  /** Offer every guide to every character, not only those of its race and class. */
  all_guides?: boolean;
  /** What the addon in the game was installed with (kept as is). */
  addon_installed?: string | null;
  /** Whether the addon's setup questions were answered (or skipped): asked once, then on demand. */
  addon_setup_done?: boolean;
  /** Whether the first launch's questions (where the game is) were answered or skipped. */
  welcome_done?: boolean;
}

/** The game's folder as saved (the one above when a game version's folder was chosen), its
 *  game versions' folders, and what keeps the addon from going into the game. */
export interface WowStatus {
  root: string;
  clients: string[];
  /** missing: no such folder; no_game: no game version in it; no_edition: not the one the app works on. */
  problem: "missing" | "no_game" | "no_edition" | "not_writable" | null;
}

/** A World of Warcraft folder found on this computer. */
export interface FoundGame {
  root: string;
  clients: string[];
}

/** What the game did with the addon of the game version the app works on. */
export interface AddonHealth {
  app_version: string;
  /** The version of the addon installed (null: not installed; web: the addon of the last export). */
  installed_version: string | null;
  /** The oldest addon version the app works with, and whether the installed one is older (desktop). */
  required_version?: string;
  outdated?: boolean;
  /** When it was installed (Unix seconds). */
  installed_at: number | null;
  /** Every file of the addon is there. */
  complete: boolean;
  /** The last time the addon ran in game (Unix seconds) and its version. */
  seen: { time: number; version: string } | null;
  /** The game saved its addons' data since the install without running this one: not loaded. */
  not_loaded: boolean;
  /** Characters ("Name - Realm") the addon is turned off for in the game's AddOns list. */
  disabled_for: string[];
  saved_variables: number;
  /** The last install added files the game reads only when it starts. */
  restart_needed: boolean;
  /** Polestar shows the guides (only the profile part is installed). */
  polestar: boolean;
}

/** How the game and the addon are set up, as a report joins it (no account or character name). */
export interface SetupReport {
  wow_dir: string;
  clients: string[];
  problem?: string;
  guide_addon: string;
  addon_version?: string;
  addon_complete: boolean;
  seen_version?: string;
  seen_days_ago?: number;
  not_loaded: boolean;
  disabled: number;
  saved_variables: number;
  restart_needed: boolean;
}

/** Fired once the settings or the game version are saved, or the addon installed. */
export const SETTINGS_CHANGED = "fg-settings-changed";
const settingsChanged = () => {
  window.dispatchEvent(new Event(SETTINGS_CHANGED));
};

export const api = {
  updatesEnabled: () => invoke<boolean>("updates_enabled"),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }).then(settingsChanged),
  getOptions: () => invoke<Options>("get_options"),
  /** Computes a guide and saves it as a new version of the guide of the request's name. */
  planRoute: (request: PlanRequest) => invoke<VersionMeta>("plan_route", { request }),
  /** The flight paths a character probably knows from its quests done in their towns. */
  inferredFlights: (start: StartState) => invoke<number[]>("inferred_flights", { start }),
  /** Quests of the race and class whose ID is `query` or whose name holds it, the closest first. */
  questChoices: (race: string, klass: string, query: string, locale: string | null) =>
    invoke<QuestChoice[]>("quest_choices", { race, class: klass, query, locale }),
  /** The starts the character can choose from (fewer than two: nothing to choose), the best score first. */
  previewStarts: (request: PlanRequest) => invoke<StartPreview>("preview_starts", { request }),
  /** Saves a guide of no steps from the request's character and settings (edited by hand), to fill in the editor. */
  manualRoute: (request: PlanRequest) => invoke<VersionMeta>("manual_route", { request }),
  /** Ends the optimization under way now: `planRoute` saves and returns the best guide so far. */
  stopPlan: () => invoke<void>("stop_plan"),
  /** Gives the generation under way up: `planRoute` then rejects with "cancelled" (nothing saved). */
  cancelPlan: () => invoke<void>("cancel_plan"),
  listGuides: () => invoke<GuideSummary[]>("list_guides"),
  getGuide: (id: string) => invoke<GuideSummary>("get_guide", { id }),
  /** A version, its guide written in `locale` (game locale). */
  getVersion: (guide: string, version: number, locale: string) => invoke<GuideVersion>("get_version", { guide, version, locale }),
  /** Versions `a` and `b` of a guide compared, written in `locale` (game locale). */
  compareVersions: (guide: string, a: number, b: number, locale: string) => invoke<Comparison>("compare_versions", { guide, a, b, locale }),
  /** The version of the guide the global export sends to the game (`null`: the latest). */
  setInstalled: (guide: string, version: number | null) => invoke<void>("set_installed", { guide, version }),
  deleteVersion: (guide: string, version: number) => invoke<void>("delete_version", { guide, version }),
  deleteGuide: (guide: string) => invoke<void>("delete_guide", { guide }),
  /** Writes the version as a .fgguide file in Downloads; returns its path. */
  exportVersion: (guide: string, version: number) => invoke<string | WebFile>("export_version", { guide, version }).then(saved),
  /** Adds a guide received as a .fgguide (or .bqroute) file (its content), as a new version. */
  importGuide: (content: string) => invoke<VersionMeta>("import_guide", { content }),
  mapIndex: () => invoke<MapInfo[]>("map_index"),
  /** Characters the addon recorded in the game client of the current game version, most recently played first. */
  listCharacters: () => invoke<CharacterInfo[]>("list_characters"),
  /** Take a character out of the list (deleted in game; back if played again): it no longer follows a guide. */
  forgetCharacter: (character: string) => invoke<void>("forget_character", { character }),
  /** A map image (WebP), as an object URL. */
  mapImage: (id: number) =>
    invoke<ArrayBuffer>("map_image", { id }).then((bytes) =>
      URL.createObjectURL(new Blob([bytes], { type: "image/webp" })),
    ),
  /** The minimap's images of the current game version: [map, level, x, y] of each (level l: 2^l game tiles a side, x from the west, y from the north). */
  minimapIndex: () => invoke<[number, number, number, number][]>("minimap_index"),
  minimapImage: (map: number, level: number, x: number, y: number) =>
    invoke<ArrayBuffer>("minimap_image", { map, level, x, y }).then((bytes) =>
      URL.createObjectURL(new Blob([bytes], { type: "image/webp" })),
    ),
  exportRequest: (request: PlanRequest) => invoke<string | WebFile>("export_request", { request }).then(saved),
  /** Desktop: writes every guide into the game (the data addon Factoruide_Data); returns their number. */
  installAddon: () => invoke<number>("install_addon").then((n) => (settingsChanged(), n)),
  /** Whether Polestar is installed in WoW Forever. */
  polestarInstalled: () => invoke<boolean>("polestar_installed"),
  /** Export a guide version into a pack folder for Polestar's own tools in `dir` (WoW Forever only): the pack's folder. */
  exportPolestar: (guide: string, version: number, dir: string) => invoke<string | WebFile>("export_polestar", { guide, version, dir }).then(saved),
  /** Whether the addon in the game has the current settings. */
  addonSynced: () => invoke<boolean>("addon_synced"),
  /** Open a page of the project's repository in the browser. */
  openLink: (url: string) => invoke<void>("open_link", { url }),
  /** Game version folders (_classic_beta_...) found in a WoW folder. */
  /** The game's folder (`wowDir`, else the saved one) and what keeps the addon from going into the game. */
  wowStatus: (wowDir?: string) => invoke<WowStatus>("wow_status", { wowDir: wowDir ?? null }),
  /** World of Warcraft folders found on this computer. */
  detectWow: () => invoke<FoundGame[]>("detect_wow"),
  /** What the game did with the addon (installed version, last run, turned off...). */
  addonHealth: () => invoke<AddonHealth>("addon_health"),
  /** What a report joins about the game and the addon. */
  setupReport: () => invoke<SetupReport>("setup_report"),
  listEditions: () => invoke<EditionInfo[]>("list_editions"),
  setEdition: (edition: Edition) => invoke<void>("set_edition", { edition }).then(settingsChanged),
  contributionPreview: () => invoke<ContributionPreview>("contribution_preview"),
  /** Sends what is new, in up to 3 documents (rejects with "nothing_to_send", "rate_limited" or the server's message when none was accepted). */
  sendContribution: () => invoke<SendReport>("send_contribution"),
  /** What players can check in game for the current game version, names in `locale`. */
  needHelp: (locale: string) => invoke<HelpCheck[]>("need_help", { locale }),
  /** Sends a problem report to the contribution server (rejects with "rate_limited" or the reason). */
  sendIssue: (draft: IssueDraft) => invoke<"accepted" | "already_received">("send_issue", { draft }),
  /** Turns the automatic contributions on or off (that setting only). */
  setAutoContribute: (on: boolean) => invoke<void>("set_auto_contribute", { on }),
  /** Opens a version in the guide editor (`locale`: language of the lines and names shown). */
  editOpen: (guide: string, version: number, locale: string) => invoke<EditOpened>("edit_open", { guide, version, locale }),
  /**
   * Replays the steps of the guide open in the editor: its report, a line per step and their
   * timeline. The replay under way gives up for this one (it rejects with "superseded").
   */
  editSimulate: (steps: RouteStep[], locale: string) => invoke<EditView>("edit_simulate", { steps, locale }),
  /**
   * What could be added to the steps after step `after` (null: before the first): quests not
   * taken, objectives and turn-ins forgotten. Gives up for a newer replay or suggestion ("superseded").
   */
  editSuggest: (steps: RouteStep[], after: number | null, locale: string) => invoke<Suggestions>("edit_suggest", { steps, after, locale }),
  /** Saves the edited steps as a new version (rejected while a step is not possible) and closes the editor. */
  editSave: (steps: RouteStep[]) => invoke<VersionMeta>("edit_save", { steps }),
  /** Leaves the editor without saving. */
  editClose: () => invoke<void>("edit_close"),
  /** A version replayed to be watched (`locale`: language of its names). */
  guideTimeline: (guide: string, version: number, locale: string) => invoke<Timeline>("guide_timeline", { guide, version, locale }),
  /** Web build: keeps a character's export (the text of "Export my character"). */
  importProfile: (content: string) => invoke<ProfileImported>("import_profile", { content }).then((r) => (settingsChanged(), r)),
  /** The text to paste in game with the guide import: this guide version (the addon adds it to its guides, or replaces the one of its name). */
  guideText: (guide: string, version: number) =>
    invoke<string | { text: string }>("guide_text", { guide, version }).then((r) => (typeof r === "string" ? r : r.text)),
  /** WoW Forever: the text to paste on Polestar's Import page (/polestar import), this guide version; `fits`: within what Polestar takes today. */
  polestarString: (guide: string, version: number) => invoke<PolestarString>("polestar_string", { guide, version }),
  /** Web build: downloads every guide as the data addon (Factoruide_Data, as the desktop app writes it) in a zip; returns its name. */
  dataZip: () => invoke<WebFile>("data_zip").then(saved),
  /** Web build, WoW Forever: downloads every guide as a Polestar guide pack (a zip of its addon folder); returns its name. */
  polestarPack: () => invoke<WebFile>("polestar_pack").then(saved),
  /** Desktop, WoW Forever: writes every guide into the game as a Polestar guide pack; returns their number. */
  installPolestar: () => invoke<number>("install_polestar").then((n) => (settingsChanged(), n)),
  onPlanProgress: (fn: (m: string) => void): Promise<UnlistenFn> =>
    listen<string>("plan-progress", (e) => fn(e.payload)),
};

/** Seconds of game time as "12h34". */
export function fmtTime(seconds: number): string {
  if (!Number.isFinite(seconds)) return "—";
  const m = Math.round(seconds / 60);
  return `${Math.floor(m / 60)}h${String(m % 60).padStart(2, "0")}`;
}

/** The starts offered, and how many different ones were tried. */
export interface StartPreview {
  tried: number;
  options: StartOption[];
  /** What was tried (flight paths known, each start's zone, time and score). */
  details: string[];
}

/** A zone a start can be forced to, known before any route is computed. */
export interface StartCandidate {
  /** What to set as `params.start_zone` (0: the planner's own choice). */
  start_zone: number;
  ui_map: number;
  /** Quests worth doing there from the start. */
  quests: number;
}

/** A way to begin the guide (`previewStarts`): a route computed lightly from a zone. */
export interface StartOption {
  /** The zone it begins in (AreaTable zone) and its map. */
  zone: number;
  ui_map: number;
  /** What to set as `params.start_zone` to get it (0: the planner's own choice). */
  start_zone: number;
  /** Play time of the guide it leads to, and its score (the time the planner ranks by), in seconds. */
  time: number;
  score: number;
  steps: number;
  quests: number;
  /** Its first steps. */
  first: string[];
}

/** One computation of a guide. */
export interface VersionMeta {
  /** Guide id (slug of its name). */
  guide: string;
  version: number;
  name: string;
  /** Unix seconds. */
  created: number;
  race: string;
  class: string;
  group?: string[];
  from_level: number;
  to_level: number;
  total_time: number;
  total_time_text: string;
  quests: number;
  data: string | null;
  /** Received from someone else (.fgguide file). */
  imported?: boolean;
  /** Edited by hand. */
  edited?: boolean;
  /** The character met in game the guide starts from ("Name-Realm"). */
  character?: string | null;
}

/** A guide and its versions, newest first; `installed`: version in the addon. */
export interface GuideSummary {
  id: string;
  name: string;
  installed: number | null;
  versions: VersionMeta[];
}

export interface GuideVersion {
  meta: VersionMeta;
  request: PlanRequest;
  outcome: PlanOutcome;
}

/** First and last steps (indices) of a block of a comparison. */
export interface DiffSpan {
  first: number;
  last: number;
}

/** What differs between two sides of a step or of a setting (`field`: English, see CompareView). */
export interface DiffChange {
  field: string;
  from: string;
  to: string;
}

/** A step of one guide or both (indices in their steps); mirror of `fg_route::export::diff::Row`. */
export interface DiffRow {
  op: "same" | "changed" | "removed" | "added";
  a: number | null;
  b: number | null;
  changes?: DiffChange[];
  /** A removed step done elsewhere in the second guide (its index there), or an added one done elsewhere in the first. */
  moved?: number;
}

/**
 * Consecutive steps in one zone (or a dungeon run), in the order of the second guide: `moved`
 * at its place there, `moved_away` where it was in the first one (no rows).
 */
export interface DiffBlock {
  status: "same" | "changed" | "added" | "removed" | "moved" | "moved_away";
  /** English name (the steps give it in the language shown). */
  zone: string | null;
  dungeon: boolean;
  a: DiffSpan | null;
  b: DiffSpan | null;
  rows: DiffRow[];
}

export interface RouteDiff {
  summary: {
    added: number;
    removed: number;
    changed: number;
    moved: number;
    blocks_moved: number;
    blocks_added: number;
    blocks_removed: number;
  };
  blocks: DiffBlock[];
}

/** Two versions of a guide compared. */
export interface Comparison {
  a: GuideVersion;
  b: GuideVersion;
  diff: RouteDiff;
  /** Settings that differ (request paths like `params.progression`, and `data`). */
  settings: DiffChange[];
  /** The same as text (`fg diff`), to hand to someone looking for what went wrong. */
  text: string;
}

export interface MapInfo {
  id: number;
  name: string;
  /** 1 world, 2 continent, 3 zone, 4+ dungeon... */
  kind: number;
  parent: number;
  width: number;
  height: number;
  file: string;
  bounds: { map: number; min_x: number; max_x: number; min_y: number; max_y: number } | null;
  /** Game zone (AreaTable ID) of a zone map. */
  area: number | null;
  /** Translated zone names by game locale. */
  names?: Record<string, string>;
  /** Outline on the parent map (pixel rings, even-odd), when known. */
  shape?: [number, number][][];
}

/** What a click on "send" did. */
export interface SendReport {
  /** "already_received": the server already had the last document. */
  status: "accepted" | "already_received";
  /** Documents the server accepted. */
  documents: number;
  /** New data is left for the next send. */
  more: boolean;
}

/** How the last automatic send of a game version went. */
export interface AutoSendStatus {
  edition: Edition;
  name: string;
  /** Unix seconds. */
  time: number;
  status: "accepted" | "already_received" | "rate_limited" | "error";
  message?: string | null;
}

/** A problem the player reports: about a guide (with the steps pointed at) or anything else. */
export interface IssueDraft {
  kind: "guide" | "other";
  message: string;
  /** Who to thank (optional). */
  name?: string;
  language: string;
  guide?: { guide: string; version: number; steps: number[]; locale: string };
  /** Join how the game and the addon are set up (`setupReport`). */
  setup?: boolean;
}

/** A check players can make in game ("Need help" page): is `quest` offered before `before` are turned in? */
export interface HelpCheck {
  /** Stable id, sent with the answer (`forever/offered-before/96259`). */
  id: string;
  quest: { id: number; name: string };
  giver: { id: number; name: string } | null;
  /** The giver's zone and percent coordinates. */
  place: { zone: string; x: number; y: number } | null;
  before: { id: number; name: string }[];
  min_level: number;
  side: "alliance" | "horde" | "both";
}

/** What a contribution of this computer would hold now (current game version): only what was not sent yet. */
export interface ContributionPreview {
  edition: Edition;
  client_dir: string;
  found: boolean;
  quests: number;
  /** Creatures and game objects from the client's caches. */
  entities: number;
  events: number;
  characters: number;
  locales: string[];
  /** Size of the JSON sent (bytes). */
  size: number;
  /** What is new does not fit in one document: the figures are the first one's. */
  more: boolean;
  url: string | null;
  /** When the server last accepted one of this game version (unix seconds). */
  last_sent: number | null;
  /** No send before then (unix seconds): the server asked to wait. */
  retry_after: number | null;
  /** The last automatic send of each game version. */
  automatic: AutoSendStatus[];
}

/** A step of a guide's route by game IDs, what the guide editor changes; mirror of `fg_route::job::RouteStep`. */
export type RouteStep =
  | { kind: "accept"; quest: number }
  /** `farm`: kill every normal mob met on the way to it (beyond the farming cap). */
  | { kind: "objective"; quest: number; objective: number; farm?: boolean; spot?: number }
  | { kind: "turn_in"; quest: number }
  /** Learn a flight path (taxi node ID). */
  | { kind: "flight_master"; node: number }
  /** Bind the hearthstone (innkeeper ID). */
  | { kind: "bind"; inn: number }
  | { kind: "hearth"; recall?: boolean }
  | { kind: "fly"; from: number; to: number }
  | { kind: "dungeon"; dungeon: number }
  /** Boats, zeppelins or trams: board `first`, leave `last` (the same when direct). */
  | { kind: "link"; first: LinkRef; last: LinkRef }
  /** Cast a class teleport (its spell name). */
  | { kind: "teleport"; spell: string }
  /** Learn the class spells at a trainer (NPC ID). */
  | { kind: "train"; trainer: number }
  /** Sell the bags' loot at a vendor (NPC ID). */
  | { kind: "sell"; vendor: number }
  | { kind: "abandon"; quest: number }
  /** Kill mobs of the character's level where it stands: a number, or up to a level. */
  | { kind: "grind"; mobs: number }
  | { kind: "grind_to"; level: number };

/** A transport link one way: its name and the zone it leads to. */
export interface LinkRef {
  name: string;
  to: number;
}

/** Mobs killed on the way to a step. */
export interface Farmed {
  kills: number;
  xp: number;
  seconds: number;
}

/** What a step of an edited guide did. */
export interface StepReport {
  /** done, skipped (nothing to do there) or error (not possible). */
  status: "done" | "skipped" | "error";
  /** Why (English, see the editor's REASONS). */
  reason?: string;
  start: number;
  end: number;
  /** Level after it, and the XP it gave. */
  level: number;
  xp: number;
  /** The line of the guide it writes (none when skipped or not possible). */
  line?: { kind: string; text: string; zone?: string; bg?: boolean; map?: { ui_map: number; x: number; y: number } };
  farm?: Farmed;
  /** An objective without `farm`: what killing every mob on the way would give. */
  farm_all?: Farmed;
  /** Done, but not as the planner would (see the editor's REASONS). */
  warning?: string;
}

/** An edited guide as replayed. */
export interface EditReport {
  total_time: number;
  total_time_text: string;
  breakdown: Breakdown;
  steps: StepReport[];
  errors: number;
  /** Grinding added at the end to reach the target level (seconds). */
  final_grind: number;
  /** Quests taken but never turned in. */
  unfinished: { id: number; name: string }[];
}

export interface CatalogPlace {
  id: number;
  name: string;
  zone: string;
}

/** What can be added to an edited guide. */
export interface EditCatalog {
  quests: { id: number; name: string; level: number; min_level: number; zone: string; objectives: string[] }[];
  inns: CatalogPlace[];
  trainers: CatalogPlace[];
  nodes: CatalogPlace[];
  dungeons: { id: number; name: string; min_level: number; max_level: number }[];
  /** Boats, zeppelins, trams and portals, each one way, with the zones they go from and to. */
  links: { link: LinkRef; from: string; to: string }[];
  /** The character learns Astral Recall. */
  recall: boolean;
}

/** A draft replayed: its report (without lines), a line per step (as the guide writes it; those not done have one of their kind, without place), their timeline. */
export interface EditView {
  report: EditReport;
  lines: Step[];
  timeline: Timeline;
}

/** A place of the world and who or what stands there. */
export interface PlaceAt {
  at: WorldPoint;
  name: string;
}

/** A quest and its places (givers, or where it is turned in). */
export interface QuestAt {
  quest: number;
  name: string;
  level: number;
  /** How hard it is for the character then, as the game colors it. */
  difficulty: Difficulty;
  /** The dungeon its objectives are done in (its name), if any. */
  dungeon?: string;
  places: PlaceAt[];
}

export type Difficulty = "grey" | "green" | "yellow" | "orange" | "red";

/** What could be added after a step of a draft. */
export interface Suggestions {
  /** Quests the draft never takes that the character can take then, the nearest first. */
  untaken: QuestAt[];
  /** Objectives of quests in the log the draft does not do; the index of a place is its spot. */
  objectives: {
    quest: number;
    objective: number;
    name: string;
    text: string;
    /** Done in a dungeon run (its area ID and name): the place is its entrance. */
    dungeon?: [number, string];
    places: PlaceAt[];
  }[];
  /** Quests in the log, their objectives done, that the draft does not turn in. */
  turn_ins: QuestAt[];
}

export interface EditOpened {
  steps: RouteStep[];
  /** Read back from the guide's steps (computed before the editor): the time may differ a little. */
  rebuilt: boolean;
  /** The steps replayed. */
  view: EditView;
  catalog: EditCatalog;
}

/** A world point: [continent (map ID), x, y]. */
export type WorldPoint = [number, number, number];

/** A guide replayed to be watched; mirror of `fg_route::job::Timeline`. */
export interface Timeline {
  /** One per step of the guide, in its order. */
  steps: TimelineStep[];
  /** The character's moves, in time order (none overlap). */
  legs: Leg[];
  /** The character before the first step. */
  start: StartView;
  /** The quests the steps take, do and turn in (and those of the log at the start). */
  quests: QuestInfo[];
  /** The flight paths the character knows or learns. */
  nodes: NodeInfo[];
  /** The inns the hearthstone is bound to. */
  inns: InnInfo[];
  /** XP from each level to the next (index: level). */
  xp_levels: number[];
  log_size: number;
  /** From this level the character rides a mount. */
  mount_level: number;
  /** Steps the replay did the same (the others have a straight walk to them). */
  matched: number;
}

/** A step of the guide as replayed: its time, where the time goes, what changes. */
export interface TimelineStep {
  start: number;
  end: number;
  /** When the character is where the step is done (its moves over). */
  arrive: number;
  /** Seconds of travel, combat, interaction and waiting (absent: none). */
  travel?: number;
  combat?: number;
  interact?: number;
  wait?: number;
  /** The replay does not find it (the data changed since the guide was computed): its split and moves are guessed. */
  estimate?: boolean;
  /** The named place it is done at (a town, a valley). */
  place?: string;
  /** Quests taken, objectives done ([quest, objective]), quests out of the log, flight paths learned (taxi node IDs). */
  accepted?: number[];
  done?: [number, number][];
  removed?: number[];
  flights?: number[];
  /** The inn the hearthstone is bound to from now on (its innkeeper's ID). */
  bind?: number;
  /** When the hearthstone, and Astral Recall, are ready again (used here). */
  hearth?: number;
  recall?: number;
  /** Copper paid for flights. */
  cost?: number;
}

/** How the character moves: walking, mounted, flying, by boat (zeppelin, tram, portal), the hearthstone or a teleport (jumps). */
export type LegMode = "walk" | "mount" | "flight" | "link" | "hearth" | "teleport";

/** A move of the character from `from` to `to`. */
export interface Leg {
  mode: LegMode;
  start: number;
  end: number;
  from: WorldPoint;
  to: WorldPoint;
  /** The points in between ([x, y] on the continent of `from`): the way around the terrain, a flight's way. */
  via?: [number, number][];
}

/** The character before the first step. */
export interface StartView {
  at: WorldPoint;
  log: number[];
  done: [number, number][];
  flights: number[];
  bind: number | null;
  /** The character learns Astral Recall (a second hearthstone). */
  recall: boolean;
}

export interface QuestInfo {
  id: number;
  name: string;
  objectives: ObjectiveInfo[];
}

export interface ObjectiveInfo {
  /** What the step says (Kill Mindless Zombie x8). */
  text: string;
  kind: "kill" | "collect" | "use" | "talk" | "other";
  /** The amount it asks for. */
  count: number;
}

/** A flight path. */
export interface NodeInfo {
  id: number;
  name: string;
  at: WorldPoint;
}

/** An inn: its innkeeper (ID and name) and its town. */
export interface InnInfo {
  id: number;
  name: string;
  place: string;
  at: WorldPoint;
}
