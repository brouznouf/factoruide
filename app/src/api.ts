// Typed access to the Tauri backend (app/src-tauri/src/main.rs).
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** Planner tunables, mirror of `fg_route::Params`. */
export interface Params {
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
  pull_levels: number;
  quest_log_size: number;
  flight_learn_radius: number;
  farm_on_way: boolean;
  farm_corridor: number;
  farm_below: number;
  farm_above: number;
  camp_radius: number;
  allow_elite: boolean;
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
  gear_floor: number;
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
  zone_stay: number;
  forced_dungeons: number[];
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
  professions?: ProfessionGoal[];
  /** Language of the guide texts (English where a name is not translated). */
  locale?: string | null;
  /** Classes of the other players leveling together (empty: solo). */
  group?: string[];
  /** The character as the addon recorded it in game: the guide goes on from there. */
  start?: StartState | null;
}

/** A point of a game map (uiMapID, percent coordinates). */
export interface MapPos {
  map: number;
  x: number;
  y: number;
}

/** A character as the addon recorded it (`/fg profile`, or at each logout); mirror of `fg_route::job::StartState`. */
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
  text: string;
  target?: { kind: string; id: number; name: string };
  zone?: string;
  map?: { ui_map: number; x: number; y: number };
  /** World position: [continent (map ID), x, y]. */
  world?: [number, number, number];
  level: number;
  time: number;
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
  min_level: number | null;
  max_level: number | null;
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
  races: { key: string; id: number; name: string; faction: string; classes: string[] }[];
  classes: NamedId[];
  zones: { id: number; name: string; names?: Record<string, string>; continent: number }[];
  dungeons: DungeonOption[];
  /** Factions quests give reputation with, most quests first. */
  factions: FactionOption[];
  class_quests: Record<string, string[]>;
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
  /** Install updates on launch in published builds (off by default). */
  auto_update?: boolean;
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
}

export const api = {
  updatesEnabled: () => invoke<boolean>("updates_enabled"),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),
  getOptions: () => invoke<Options>("get_options"),
  /** Computes a guide and saves it as a new version of the guide of the request's name. */
  planRoute: (request: PlanRequest) => invoke<VersionMeta>("plan_route", { request }),
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
  /** Installs this version of the guide in the addon (`null`: removes the guide from it). */
  setInstalled: (guide: string, version: number | null) => invoke<number>("set_installed", { guide, version }),
  deleteVersion: (guide: string, version: number) => invoke<void>("delete_version", { guide, version }),
  deleteGuide: (guide: string) => invoke<void>("delete_guide", { guide }),
  /** Writes the version as a .fgguide file in Downloads; returns its path. */
  exportVersion: (guide: string, version: number) => invoke<string>("export_version", { guide, version }),
  /** Adds a guide received as a .fgguide (or .bqroute) file (its content), as a new version. */
  importGuide: (content: string) => invoke<VersionMeta>("import_guide", { content }),
  mapIndex: () => invoke<MapInfo[]>("map_index"),
  /** Characters the addon recorded in the game client of the current game version, most recently played first. */
  listCharacters: () => invoke<CharacterInfo[]>("list_characters"),
  /** Which guide each character follows ("Name-Realm" -> guide id). */
  listAssignments: () => invoke<Record<string, string>>("list_assignments"),
  /** Make a character follow a guide (null: none) and write the addon; returns its number of guides. */
  assignGuide: (character: string, guide: string | null) => invoke<number>("assign_guide", { character, guide }),
  /** Take a character out of the list (deleted in game; back if played again): it no longer follows a guide. */
  forgetCharacter: (character: string) => invoke<void>("forget_character", { character }),
  /** A map image (WebP), as an object URL. */
  mapImage: (id: number) =>
    invoke<ArrayBuffer>("map_image", { id }).then((bytes) =>
      URL.createObjectURL(new Blob([bytes], { type: "image/webp" })),
    ),
  exportRequest: (request: PlanRequest) => invoke<string>("export_request", { request }),
  /** Copies the addon into the game with the installed guides. */
  /** Returns the number of guides installed. */
  installAddon: () => invoke<number>("install_addon"),
  /** Whether Polestar is installed in WoW Forever. */
  polestarInstalled: () => invoke<boolean>("polestar_installed"),
  /** Whether the addon in the game has the current settings. */
  addonSynced: () => invoke<boolean>("addon_synced"),
  /** Open a page of the project's repository in the browser. */
  openLink: (url: string) => invoke<void>("open_link", { url }),
  /** Game version folders (_classic_beta_...) found in a WoW folder. */
  listClients: (wowDir: string) => invoke<string[]>("list_clients", { wowDir }),
  listEditions: () => invoke<EditionInfo[]>("list_editions"),
  setEdition: (edition: Edition) => invoke<void>("set_edition", { edition }),
  contributionPreview: () => invoke<ContributionPreview>("contribution_preview"),
  /** Sends what is new, in up to 3 documents (rejects with "nothing_to_send", "rate_limited" or the server's message when none was accepted). */
  sendContribution: () => invoke<SendReport>("send_contribution"),
  /** Sends a problem report to the contribution server (rejects with "rate_limited" or the reason). */
  sendIssue: (draft: IssueDraft) => invoke<"accepted" | "already_received">("send_issue", { draft }),
  /** Turns the automatic contributions on or off (that setting only). */
  setAutoContribute: (on: boolean) => invoke<void>("set_auto_contribute", { on }),
  /** Opens a version in the guide editor (`locale`: language of the lines and names shown). */
  editOpen: (guide: string, version: number, locale: string) => invoke<EditOpened>("edit_open", { guide, version, locale }),
  /** Replays the steps of the guide open in the editor. */
  editSimulate: (steps: RouteStep[], locale: string) => invoke<EditReport>("edit_simulate", { steps, locale }),
  /** Saves the edited steps as a new version (rejected while a step is not possible) and closes the editor. */
  editSave: (steps: RouteStep[]) => invoke<VersionMeta>("edit_save", { steps }),
  /** Leaves the editor without saving. */
  editClose: () => invoke<void>("edit_close"),
  onPlanProgress: (fn: (m: string) => void): Promise<UnlistenFn> =>
    listen<string>("plan-progress", (e) => fn(e.payload)),
};

/** Seconds of game time as "12h34". */
export function fmtTime(seconds: number): string {
  if (!Number.isFinite(seconds)) return "—";
  const m = Math.round(seconds / 60);
  return `${Math.floor(m / 60)}h${String(m % 60).padStart(2, "0")}`;
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
  /** The character learns Astral Recall. */
  recall: boolean;
}

export interface EditOpened {
  steps: RouteStep[];
  /** Read back from the guide's steps (computed before the editor): the time may differ a little. */
  rebuilt: boolean;
  report: EditReport;
  catalog: EditCatalog;
}
