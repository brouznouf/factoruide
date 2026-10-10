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
  crowded: boolean;
  crowd_event_time: number;
  crowd_spawn_time: number;
  crowd_level: number;
  quest_log_size: number;
  flight_learn_radius: number;
  along_corridor: number;
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
  min_efficiency: number;
  hearth_cooldown: number;
  hearth_cast: number;
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
  grind_over_weight: number;
  gather_overlap: number;
  gather_radius: number;
  flight_weight: number;
  gold_weight: number;
  zone_change_weight: number;
  zone_stay: number;
  forced_dungeons: number[];
  excluded_dungeons: number[];
  dungeon_quest_weight: number;
  construction_random: boolean;
  continent_stay: number;
  candidates: number;
  qualify_share: number;
  finalists: number;
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
  time_limit_ms: number;
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
  /** Copper. */
  money: number;
  /** When it was recorded (unix seconds). */
  time: number;
}

/** A character found in the game's saved variables. */
export interface CharacterInfo {
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
  steps: Step[];
}

export interface PlanOutcome {
  route: Route;
  notes: string[];
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
  dungeons: NamedId[];
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
  /** When this computer last sent one (unix seconds). */
  last_contribution?: number | null;
  /** Game version the app works on. */
  edition: Edition;
  /** App language (also the default guide language); unset: the system language. */
  language?: string | null;
  /** Addon behaviour, written to Config.lua on install. */
  addon: Record<string, boolean | number | string>;
  /** Language of the guides in the addon (game locale); unset: each guide in the language it was made in. */
  guide_locale?: string | null;
}

export const api = {
  updatesEnabled: () => invoke<boolean>("updates_enabled"),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<void>("set_settings", { settings }),
  getOptions: () => invoke<Options>("get_options"),
  /** Computes a guide and saves it as a new version of the guide of the request's name. */
  planRoute: (request: PlanRequest) => invoke<VersionMeta>("plan_route", { request }),
  listGuides: () => invoke<GuideSummary[]>("list_guides"),
  getGuide: (id: string) => invoke<GuideSummary>("get_guide", { id }),
  /** A version, its guide written in `locale` (game locale). */
  getVersion: (guide: string, version: number, locale: string) => invoke<GuideVersion>("get_version", { guide, version, locale }),
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
  mapImage: (id: number) => invoke<string>("map_image", { id }),
  exportRequest: (request: PlanRequest) => invoke<string>("export_request", { request }),
  /** Copies the addon into the game with the installed guides. */
  /** Returns the number of guides installed. */
  installAddon: () => invoke<number>("install_addon"),
  /** Game version folders (_classic_beta_...) found in a WoW folder. */
  listClients: (wowDir: string) => invoke<string[]>("list_clients", { wowDir }),
  listEditions: () => invoke<EditionInfo[]>("list_editions"),
  setEdition: (edition: Edition) => invoke<void>("set_edition", { edition }),
  contributionPreview: () => invoke<ContributionPreview>("contribution_preview"),
  /** Sends the contribution: "accepted" or "duplicate" (rejects with "rate_limited" or the server's message). */
  sendContribution: () => invoke<string>("send_contribution"),
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

/** What a contribution of this computer would hold (current game version). */
export interface ContributionPreview {
  edition: Edition;
  client_dir: string;
  found: boolean;
  quests: number;
  events: number;
  characters: number;
  locales: string[];
  /** Size of the JSON sent (bytes). */
  size: number;
  url: string | null;
  last_sent: number | null;
}
