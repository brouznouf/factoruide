import L from "leaflet";
import "leaflet/dist/leaflet.css";
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { Difficulty, Leg, MapInfo, RouteStep, Step, Suggestions, WorldPoint } from "../api";
import { useAreaName, useT } from "../i18n";
import { type Card, type Moves, positionAt } from "./guideTimeline";
import type { Clock } from "./playback";
import { aheadInZone, aheadPicked, arrowMarks, stepsShown } from "./mapSteps";
import { type Decoded, decodedTile, decodeTile, labelPoint, loadImage, loadIndex, loadMinimapImage, loadMinimapIndex } from "./WorldMap";

/** What the map draws: the kinds of moves, the step markers, the whole guide or the current zone only,
 * and how many steps of the zone from the current one (null: all of them). */
export interface MapLayers {
  walk: boolean;
  flight: boolean;
  link: boolean;
  markers: boolean;
  scope: "all" | "zone";
  ahead: number | null;
}

export const DEFAULT_LAYERS: MapLayers = { walk: true, flight: true, link: true, markers: true, scope: "zone", ahead: null };

interface Props {
  steps: Step[];
  /** The character's moves (the timeline's, or straight lines between the steps while it loads). */
  moves: Moves;
  cards: Card[];
  titles: string[];
  /** Card under way, when its moves end (a flight or a boat taken at its end goes on after
   * it), and the cards of the zone under way (first, last). */
  current: number;
  currentEnd: number;
  zone: [number, number];
  clock: Clock;
  follow: boolean;
  onFollow: (follow: boolean) => void;
  layers: MapLayers;
  onLayers: (layers: MapLayers) => void;
  onSelect: (card: number) => void;
  /** A card chosen in the list (or with the keys): shown on the map when it does not follow the
   * character (`seq` changes at each choice, the same card chosen again included). */
  focus?: { card: number; seq: number };
  /** The line over the map (what the character is doing). */
  status?: ReactNode;
  /** Editing: what could be added where the guide is watched (quests not taken, objectives and
   * turn-ins forgotten), and adding one of them there. */
  suggestions?: Suggestions | null;
  onPick?: (step: RouteStep) => void;
}

/** A place of a suggestion on the map: its kind, what it says, the step it adds. */
interface Offer {
  key: string;
  x: number;
  y: number;
  kind: "untaken" | "objective" | "turn-in";
  glyph: string;
  label: string;
  step: RouteStep;
  /** A quest's color for the character then (its level against the character's). */
  difficulty?: Difficulty;
  /** Done in a dungeon: drawn as a dungeon (a diamond). */
  dungeon?: boolean;
}

/** Continents (game maps), west to east. */
const CONTINENTS = [1, 0, 530];
/** Zoom range: the closest, times a whole continent in view; the furthest, times all the continents in view. */
const MAX_ZOOM = 64;
const MIN_ZOOM = 0.6;
/** From this zoom (times a whole continent in view) the zones' own maps, sharper, are drawn over the continent's. */
const ZONE_DETAIL = 2;
/** From this zoom (times a whole continent in view) the game's minimap, far more detailed, is drawn instead of the zones' maps. */
const MINIMAP_DETAIL = 3;
/** Pixels a side of a minimap image, the levels it has (level l: 2^l game tiles a side), and a game tile's size in yards. */
const MINIMAP_SIZE = 256;
const MINIMAP_LEVELS = 3;
const TILE = 1600 / 3;
/** The minimap's open sea. */
const MINIMAP_SEA = "#05080a";
/** A zone's map this much over a bigger zone's is a city's (seen from inside, or drawn by that zone's map too). */
const CITY_SHARE = 0.5;
/** Cards before and after the current one with a full marker (the others: a dot). */
const NEAR_BEFORE = 2;
const NEAR_AFTER = 8;
/** Screen pixels between the markers of steps at the same place. */
const SAME_PLACE_GAP = 22;
/** Screen pixels between the arrows telling the way along the route. */
const ARROW_GAP = 70;
/** The stitched world map (`fgd worldmap`, packed with the minimap as game map 947): its tiles'
 * side, the map's pixels per pixel of the game's world map art (its most detailed level, 0), its
 * levels (the two most detailed keep the tiles with land only), and the level drawn whole,
 * decoded at once, under the others. */
const WORLD = 947;
const WORLD_TILE = 512;
const WORLD_SCALE = 16;
const WORLD_LEVELS = 5;
const WORLD_FLOOR = 3;
/** The minimap over the stitched map when zoomed in: its share by zoom (times a whole continent
 * in view), from the first, the painted map leading until the closest zooms. */
const MINIMAP_MIX: [number, number][] = [
  [8, 0.15],
  [24, 0.55],
  [48, 1],
];
/** On the stitched map, the game's own art over the zones when zoomed out, by zoom (times a whole
 * continent in view): the world map's, then the continents' (their big names over the zones a
 * while), the zones alone from the last. Each: (zoom, opacity) points, log-interpolated. */
const WORLD_ART: [number, number][] = [
  [1, 1],
  [1.6, 0],
];
/** The caves and underground places as the game's minimap draws them inside (`fgd caves`, packed
 * as game map `CAVES` + the continent's): over the minimap from a close zoom, by zoom (times a
 * whole continent in view). Ironforge and Undercity, under the terrain, are among them: zoomed
 * in, they show inside as the caves do. */
const CAVES = 100000;
const CAVE_ART: [number, number][] = [
  [9, 0],
  [14, 1],
];
/** Share of a continent art's width its parchment frame takes. */
const ART_FRAME = 0.045;
const CONTINENT_ART: [number, number][] = [
  [1, 0],
  [1.3, 1],
  [1.8, 0.45],
  [3.5, 0],
];
/** WoW Forever's Zephras Isle, not on the world map (it floats in the sky): where its middle is
 * drawn on the stitched map, between the continents (0..1 of the world map), at their scale. */
const ISLE_AT: [number, number] = [0.475, 0.19];
/** Times the continents' scale Zephras Isle is drawn at: a map of its own, it reads better a little bigger. */
const ISLE_SCALE = 1.25;
/** Share of an island's art its torn parchment border takes, cut off inside its frame. */
const ISLE_BORDER = 0.035;
/** Rings of an island's pale halo. */
const ISLE_HALO = 6;
/** The stitched map's open sea, past its edges. */
const WORLD_SEA = "#7d7664";

/**
 * A continent on the map: its game map's image, placed at (dx, dy) and drawn `k` times its size
 * (every continent at the same yards per pixel), and world coordinates (x north, y west) on it.
 */
interface Panel {
  map: number;
  info: MapInfo;
  dx: number;
  dy: number;
  k: number;
  w: number;
  h: number;
  project: (x: number, y: number) => [number, number];
  /** A pixel of the continent's own art (where the zones' outlines are given) on the map. */
  fromArt: (x: number, y: number) => [number, number];
  /** Drawn by the stitched world map's tiles, not by its own art. */
  tiled?: boolean;
}

/**
 * The game version's continents side by side, west to east, the same scale, centred on a line;
 * after them, the zones on a game map of their own that the guide goes to (`used`: game maps),
 * WoW Forever's starting islands (Zephras Isle, the Darkspear Islands).
 */
interface Layout {
  panels: Map<number, Panel>;
  width: number;
  height: number;
  unit: number;
  /** The stitched world map's tiles are drawn (the continents where the world map puts them). */
  tiled: boolean;
}

function layout(index: MapInfo[], used: number[]): Layout {
  const biggest = (maps: MapInfo[]) => maps.sort((a, b) => b.width - a.width)[0];
  const infos = CONTINENTS.map((c) => biggest(index.filter((m) => m.kind === 2 && m.bounds?.map === c))).filter(Boolean);
  const islands = used
    .filter((map) => !CONTINENTS.includes(map))
    .map((map) => biggest(index.filter((m) => (m.kind === 3 || m.kind === 6) && m.bounds?.map === map)))
    .filter(Boolean);
  infos.push(...islands);
  // Yards per pixel of the first one, for all.
  const ypp = (m: MapInfo) => (m.bounds!.max_y - m.bounds!.min_y) / m.width;
  const ref = infos.length ? ypp(infos[0]) : 1;
  const sized = infos.map((info) => {
    const k = ypp(info) / ref;
    return { info, k, w: info.width * k, h: info.height * k };
  });
  const height = Math.max(1, ...sized.map((p) => p.h));
  const panels = new Map<number, Panel>();
  let at = 0;
  for (const { info, k, w, h } of sized) {
    const b = info.bounds!;
    const [dx, dy] = [at, (height - h) / 2];
    panels.set(b.map, {
      map: b.map,
      info,
      dx,
      dy,
      k,
      w,
      h,
      project: (x, y) => [dx + ((b.max_y - y) / (b.max_y - b.min_y)) * w, dy + ((b.max_x - x) / (b.max_x - b.min_x)) * h],
      fromArt: (x, y) => [dx + x * k, dy + y * k],
    });
    at += w;
  }
  return { panels, width: Math.max(1, at), height, unit: Math.max(1, ...sized.map((p) => p.w)), tiled: false };
}

/**
 * The stitched world map's layout: the continents where the game's world map puts them, at the
 * scale of its tiles, and the zones on a game map of their own the guide goes to (Zephras Isle)
 * in the sea between them, framed (it floats in the sky).
 */
function worldLayout(index: MapInfo[], used: number[], world: MapInfo): Layout {
  const [W, H] = [world.width * WORLD_SCALE, world.height * WORLD_SCALE];
  const panels = new Map<number, Panel>();
  let ypp = 1;
  for (const pl of world.placements ?? []) {
    const art = index.filter((m) => m.kind === 2 && m.parent === WORLD && m.bounds?.map === pl.map)[0];
    if (!art?.bounds) continue;
    const [dx, dy, w, h] = [pl.ui[0] * W, pl.ui[1] * H, (pl.ui[2] - pl.ui[0]) * W, (pl.ui[3] - pl.ui[1]) * H];
    ypp = (pl.max_y - pl.min_y) / w;
    const project = (x: number, y: number): [number, number] => [dx + ((pl.max_y - y) / (pl.max_y - pl.min_y)) * w, dy + ((pl.max_x - x) / (pl.max_x - pl.min_x)) * h];
    const ab = art.bounds;
    panels.set(pl.map, {
      map: pl.map,
      info: art,
      dx,
      dy,
      k: 1,
      w,
      h,
      project,
      fromArt: (x, y) => project(ab.max_x - (y / art.height) * (ab.max_x - ab.min_x), ab.max_y - (x / art.width) * (ab.max_y - ab.min_y)),
      tiled: true,
    });
  }
  const continents = [...panels.values()];
  // The zones of the world map the game does not place on it (Zephras Isle): always drawn.
  const floating = index.filter((m) => m.kind === 3 && m.parent === WORLD && m.bounds && !panels.has(m.bounds.map)).map((m) => m.bounds!.map);
  for (const map of new Set([...floating, ...used.filter((m) => !panels.has(m))])) {
    const info = index.filter((m) => (m.kind === 3 || m.kind === 6) && m.bounds?.map === map).sort((a, b) => b.width - a.width)[0];
    if (!info?.bounds) continue;
    const b = info.bounds;
    const [w, h] = [((b.max_y - b.min_y) / ypp) * ISLE_SCALE, ((b.max_x - b.min_x) / ypp) * ISLE_SCALE];
    const [dx, dy] = [ISLE_AT[0] * W - w / 2, ISLE_AT[1] * H - h / 2];
    const k = w / info.width;
    panels.set(map, {
      map,
      info,
      dx,
      dy,
      k,
      w,
      h,
      project: (x, y) => [dx + ((b.max_y - y) / (b.max_y - b.min_y)) * w, dy + ((b.max_x - x) / (b.max_x - b.min_x)) * h],
      fromArt: (x, y) => [dx + x * k, dy + y * k],
    });
  }
  return { panels, width: W, height: H, unit: Math.max(1, ...continents.map((p) => p.w)), tiled: true };
}

/** A world point on the map (null: on a map not drawn, a dungeon's). */
type Project = (p: WorldPoint) => [number, number] | null;

type Point = [number, number];

/** The moves of `legs` as lines by kind, in the map's pixels: a boat or a portal from one
 * continent to another goes straight across (a jump). */
interface Traced {
  lines: { walk: Point[][]; flight: Point[][]; link: Point[][]; jump: Point[][] };
}
function trace(legs: Leg[], project: Project): Traced {
  const out: Traced = { lines: { walk: [], flight: [], link: [], jump: [] } };
  for (const l of legs) {
    const from = project(l.from);
    const to = project(l.to);
    if (!from || !to) continue;
    const via = l.from[0] === l.to[0] ? (l.via ?? []).map(([x, y]) => project([l.from[0], x, y])!) : [];
    const line: Point[] = [from, ...via, to];
    if (l.mode === "walk" || l.mode === "mount") out.lines.walk.push(line);
    else if (l.mode === "flight") out.lines.flight.push(line);
    else if (l.mode === "link") out.lines.link.push(line);
    else out.lines.jump.push(line);
  }
  return out;
}

/** The kind of marker of a card: what most matters among its steps. */
function markerKind(steps: Step[], card: Card): { kind: string; glyph: string } {
  const kinds = new Set(steps.slice(card.first, card.last + 1).map((s) => s.kind));
  if (card.travel) return { kind: "travel", glyph: "➜" };
  if (kinds.has("turnin")) return { kind: "turnin", glyph: "?" };
  if (kinds.has("accept")) return { kind: "accept", glyph: "!" };
  if (kinds.has("dungeon")) return { kind: "dungeon", glyph: "◆" };
  if (kinds.has("objective")) {
    const s = steps.slice(card.first, card.last + 1).find((x) => x.kind === "objective");
    return s?.target?.kind === "npc" ? { kind: "kill", glyph: "✕" } : { kind: "collect", glyph: "◆" };
  }
  if (kinds.has("grind")) return { kind: "grind", glyph: "✕" };
  return { kind: "other", glyph: "•" };
}

/** The map's own coordinates as Leaflet's: a point (x, y) of the map is `[y, x]`, y downwards. */
const CRS = L.extend({}, L.CRS.Simple, { transformation: new L.Transformation(1, 0, 1, 0) }) as L.CRS;
const ll = (x: number, y: number) => L.latLng(y, x);
const box = (r: { x: number; y: number; w: number; h: number }, pad = 0) => L.latLngBounds(ll(r.x - pad, r.y - pad), ll(r.x + r.w + pad, r.y + r.h + pad));
/** A transparent pixel: an image overlay's picture until its own is loaded. */
const BLANK = "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";
const escape = (s: string) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);

/** Zoom levels (each twice closer) a wheel pixel asks for, and the share of the way to it each
 * frame goes. */
const WHEEL_ZOOM = 1 / 300;
const WHEEL_GLIDE = 0.3;
/** Share of the view's size added on each side for the images drawn by view (minimap, caves). */
const VIEW_MARGIN = 0.25;
/** The map's planes, bottom to top (their z-index). */
const PANES: [string, number][] = [
  ["gm-world", 200],
  ["gm-base", 202],
  ["gm-zones", 205],
  ["gm-art", 210],
  ["gm-isle-glow", 213],
  ["gm-isle", 215],
  ["gm-minimap", 220],
  ["gm-caves", 230],
  ["gm-labels", 450],
  ["gm-dot", 650],
];

/** The stitched world map's tiles of one level range, from the minimap pack (game map 947):
 * Leaflet's zoom -l draws level l (512 pixels a tile, 512 x 2^l pixels of the map). */
class WorldTiles extends L.GridLayer {
  have: Set<string>;
  constructor(have: Set<string>, options: L.GridLayerOptions) {
    super(options);
    this.have = have;
  }
  // Not cleared at each view reset (Leaflet's: every zoom step of the wheel's glide, set without
  // its animation): the tiles in place are kept, scaled, until the new ones are there, instead of
  // the grey sea showing at once. (The reset only clears for a change of projection, never made.)
  getEvents() {
    const events = super.getEvents!();
    delete events.viewprereset;
    return events;
  }
  createTile(coords: L.Coords, done: L.DoneCallback): HTMLElement {
    const level = -coords.z;
    const key = `${WORLD}/${level}/${coords.x}/${coords.y}`;
    if (!this.have.has(key)) {
      const empty = document.createElement("div");
      setTimeout(() => done(undefined, empty), 0);
      return empty;
    }
    // A canvas the decoded tile is drawn into: painted the moment it is placed (a copy of an image
    // may wait for the engine to decode it again, the grey sea showing meanwhile).
    const canvas = this.clip(document.createElement("canvas"), coords);
    canvas.width = canvas.height = WORLD_TILE;
    const draw = (img: Decoded) => canvas.getContext("2d")!.drawImage(img, 0, 0, WORLD_TILE, WORLD_TILE);
    const ready = decodedTile(key);
    if (ready) {
      draw(ready);
      setTimeout(() => done(undefined, canvas), 0);
      return canvas;
    }
    decodeTile(WORLD, level, coords.x, coords.y)
      .then((img) => {
        draw(img);
        done(undefined, canvas);
      })
      // A tile that could not be loaded is left blank, the floor under it.
      .catch(() => done(new Error("tile"), canvas));
    return canvas;
  }
  /** A tile over the map's east or south edge cut there (the tiles' padding past it left out). */
  clip<T extends HTMLElement>(img: T, coords: L.Coords): T {
    const span = WORLD_TILE * 2 ** -coords.z;
    // The largest x and y (latitude is y here, the larger the further south).
    const b = (this.options as L.GridLayerOptions).bounds as L.LatLngBounds;
    const right = Math.max(0, 1 - (b.getEast() - coords.x * span) / span);
    const bottom = Math.max(0, 1 - (b.getNorth() - coords.y * span) / span);
    if (right > 0 || bottom > 0) img.style.clipPath = `inset(0 ${(right * 100).toFixed(3)}% ${(bottom * 100).toFixed(3)}% 0)`;
    return img;
  }
}

/** The guide on the game's maps (a Leaflet map: panned and zoomed by the browser's compositor):
 * the route (boats across), the steps, the character moving along it. */
export function GuideMap({ steps, moves, cards, titles, current, currentEnd, zone, clock, follow, onFollow, layers, onLayers, onSelect, focus, status, suggestions, onPick }: Props) {
  const t = useT();
  const areaName = useAreaName();
  const [index, setIndex] = useState<MapInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    loadIndex()
      .then(setIndex)
      .catch((e) => setError(String(e)));
  }, []);

  // The minimap's tiles, and the stitched world map's (game map 947) when the game version has it.
  const [minimap, setMinimap] = useState<Set<string> | null>(null);
  useEffect(() => {
    loadMinimapIndex()
      .then(setMinimap)
      .catch(() => setMinimap(new Set()));
  }, []);

  // The game version's continents: on the stitched world map where it has one, else side by side
  // (the boats between them drawn too).
  const used = useMemo(() => [...new Set(steps.flatMap((s) => (s.world ? [s.world[0]] : [])))].sort((a, b) => a - b).join(), [steps]);
  const world = useMemo(() => {
    if (!index || !minimap) return null;
    const maps = used ? used.split(",").map(Number) : [];
    const worldInfo = index.find((m) => m.id === WORLD && m.placements?.length);
    const tiled = !!worldInfo && [...minimap].some((k) => k.startsWith(`${WORLD}/`));
    return tiled ? worldLayout(index, maps, worldInfo!) : layout(index, maps);
  }, [index, minimap, used]);
  const panels = world?.panels;
  const project = useMemo<Project | null>(() => (panels ? (p) => panels.get(p[0])?.project(p[1], p[2]) ?? null : null), [panels]);
  // The zones' maps (the game versions without the stitched map), and where a city is (a map
  // mostly over a bigger zone's: Stormwind, Undercity), covered with the map of the zone around
  // it instead: the zone's map draws the city too, without the seams of two maps drawn differently.
  const [zones, covers] = useMemo(() => {
    if (!panels) return [[], []];
    const all = [...panels.values()].flatMap((panel) =>
      (index ?? [])
        .filter((m) => m.kind === 3 && m.parent === panel.info.id && m.bounds)
        .map((z) => {
          const b = z.bounds!;
          const [x1, y1] = panel.project(b.max_x, b.max_y);
          const [x2, y2] = panel.project(b.min_x, b.min_y);
          const rect = { x: Math.min(x1, x2), y: Math.min(y1, y2), w: Math.abs(x2 - x1), h: Math.abs(y2 - y1) };
          // Outlines are in the continent image's pixels.
          const rings = z.shape?.length ? z.shape.map((r) => r.map(([x, y]) => panel.fromArt(x, y))) : null;
          return { z, rect, rings, label: rings ? labelPoint(rings) : ([rect.x + rect.w / 2, rect.y + rect.h / 2] as [number, number]) };
        }),
    );
    type Zone = (typeof all)[number];
    const within = (a: Zone, b: Zone) => {
      const w = Math.min(a.rect.x + a.rect.w, b.rect.x + b.rect.w) - Math.max(a.rect.x, b.rect.x);
      const h = Math.min(a.rect.y + a.rect.h, b.rect.y + b.rect.h) - Math.max(a.rect.y, b.rect.y);
      return b !== a && b.rect.w * b.rect.h > a.rect.w * a.rect.h && w > 0 && h > 0 && w * h >= CITY_SHARE * a.rect.w * a.rect.h;
    };
    const cities = all.flatMap((a) => {
      const around = all.find((b) => within(a, b));
      return around && a.rings ? [{ id: a.z.id, area: a.z.area, rect: a.rect, rings: a.rings, around }] : [];
    });
    return [all.filter((a) => !all.some((b) => within(a, b))), cities];
  }, [index, panels]);

  // The Leaflet map, made once; its size; its zoom once settled (`k`: screen pixels a map pixel).
  const host = useRef<HTMLDivElement>(null);
  const mapRef = useRef<L.Map | null>(null);
  const [size, setSize] = useState({ w: 0, h: 0 });
  const [zoom, setZoom] = useState(0);
  const k = 2 ** zoom;
  // Zooms: all the continents in view, and one (what the zooms of the details are relative to).
  const fitK = world && size.w ? Math.min(size.w / world.width, size.h / world.height) : 1;
  const unitK = world && size.w ? Math.min(size.w / world.unit, size.h / world.height) : 1;
  const latest = useRef({ follow, onFollow, unitK, fitK, world });
  latest.current = { follow, onFollow, unitK, fitK, world };
  const settle = useRef(0);
  const zooming = useRef(false);
  const refresh = useRef(() => {});
  const opacities = useRef((_z: number) => {});
  const fitRef = useRef(() => {});
  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const map = L.map(el, {
      crs: CRS,
      center: [0, 0],
      zoom: 0,
      minZoom: -10,
      maxZoom: 10,
      zoomSnap: 0,
      zoomDelta: 0.5,
      // Our own wheel zoom (below): Leaflet's waits for the wheel to stop, then animates.
      scrollWheelZoom: false,
      zoomControl: false,
      attributionControl: false,
      doubleClickZoom: false,
      boxZoom: false,
      keyboard: false,
      fadeAnimation: false,
    });
    for (const [name, z] of PANES) map.createPane(name).style.zIndex = String(z);
    map.getPane("gm-labels")!.style.pointerEvents = "none";
    map.getPane("gm-dot")!.style.pointerEvents = "none";
    mapRef.current = map;
    const schedule = () => {
      if (!settle.current) settle.current = window.setTimeout(() => refresh.current(), 120);
    };
    map.on("zoomstart", () => (zooming.current = true));
    map.on("zoomend", () => (zooming.current = false));
    map.on("moveend zoomend", schedule);
    map.on("zoomanim", (e) => opacities.current((e as L.ZoomAnimEvent).zoom));
    map.on("zoom", () => opacities.current(map.getZoom()));
    // Dragging the map stops following the character; a double click shows the whole map.
    map.on("dragstart", () => latest.current.follow && latest.current.onFollow(false));
    map.on("dblclick", () => {
      latest.current.onFollow(false);
      fitRef.current();
    });
    const ro = new ResizeObserver(() => {
      map.invalidateSize({ pan: false });
      setSize({ w: el.clientWidth, h: el.clientHeight });
    });
    ro.observe(el);
    return () => {
      ro.disconnect();
      clearTimeout(settle.current);
      map.remove();
      mapRef.current = null;
    };
  }, []);

  // The wheel's zoom glides there at once, frame by frame, around the pointer (the middle while
  // following the character), its turns adding up while it moves.
  const glide = useRef<{ z: number; at: L.Point | null; raf: number } | null>(null);
  const zoomTo = useCallback((z: number, at: L.Point | null) => {
    const map = mapRef.current;
    if (!map) return;
    const target = Math.max(map.getMinZoom(), Math.min(map.getMaxZoom(), z));
    if (glide.current) {
      Object.assign(glide.current, { z: target, at });
      return;
    }
    const step = () => {
      const g = glide.current;
      if (!g) return;
      const now = map.getZoom();
      const done = Math.abs(g.z - now) < 0.01;
      const next = done ? g.z : now + (g.z - now) * WHEEL_GLIDE;
      if (g.at && !latest.current.follow) map.setZoomAround(g.at, next, { animate: false });
      else map.setZoom(next, { animate: false });
      if (done) glide.current = null;
      else g.raf = requestAnimationFrame(step);
    };
    glide.current = { z: target, at, raf: requestAnimationFrame(step) };
  }, []);
  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const map = mapRef.current;
      if (!map) return;
      // Lines or pages (some mice) as pixels.
      const dy = e.deltaY * (e.deltaMode === 1 ? 33 : e.deltaMode === 2 ? el.clientHeight : 1);
      const from = glide.current?.z ?? map.getZoom();
      zoomTo(from - dy * WHEEL_ZOOM, map.mouseEventToContainerPoint(e as unknown as MouseEvent));
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => {
      el.removeEventListener("wheel", onWheel);
      cancelAnimationFrame(glide.current?.raf ?? 0);
      glide.current = null;
    };
  }, [zoomTo]);

  // The zoom range and the fixed layers of the map: the stitched world map's tiles and the game's
  // own art over them when zoomed out, or the continents' maps side by side; the islands.
  const art = useRef<{ world?: L.ImageOverlay; continents: L.ImageOverlay[] }>({ continents: [] });
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !world || !panels || !size.w || !minimap) return;
    map.setMinZoom(Math.log2(fitK * MIN_ZOOM));
    map.setMaxZoom(Math.log2(unitK * MAX_ZOOM));
    const group = L.layerGroup().addTo(map);
    art.current = { continents: [] };
    if (world.tiled) {
      const bounds = L.latLngBounds(ll(0, 0), ll(world.width, world.height));
      const common = { tileSize: WORLD_TILE, bounds, noWrap: true, pane: "gm-world", keepBuffer: 4, className: "gm-world-tiles", minZoom: -20, maxZoom: 20 };
      // The coarse level under it all (the open sea the detailed levels leave out), the detailed
      // levels over it.
      // The coarse floor, decoded at once: under it all, shown wherever a detailed tile is not
      // there yet (and where the detailed levels leave out the open sea); the detailed levels
      // over it.
      for (const key of minimap) {
        const [m, level, x, y] = key.split("/").map(Number);
        if (m === WORLD && level === WORLD_FLOOR) decodeTile(m, level, x, y, true).catch(() => {});
      }
      new WorldTiles(minimap, { ...common, minNativeZoom: -WORLD_FLOOR, maxNativeZoom: -WORLD_FLOOR }).addTo(group);
      new WorldTiles(minimap, { ...common, minNativeZoom: -(WORLD_LEVELS - 1), maxNativeZoom: 0 }).addTo(group);
      const worldArt = L.imageOverlay(BLANK, bounds, { pane: "gm-art", interactive: false }).addTo(group);
      loadImage(WORLD).then((src) => worldArt.setUrl(src)).catch(() => {});
      art.current.world = worldArt;
      for (const p of panels.values()) {
        if (p.tiled) {
          const b = p.info.bounds!;
          const [x1, y1] = p.project(b.max_x, b.max_y);
          const [x2, y2] = p.project(b.min_x, b.min_y);
          const o = L.imageOverlay(BLANK, L.latLngBounds(ll(x1, y1), ll(x2, y2)), { pane: "gm-art", interactive: false }).addTo(group);
          // Its parchment frame cut off: only its land, sea and names over the stitched map.
          const el = o.getElement();
          if (el) el.style.clipPath = `inset(${ART_FRAME * 150}% ${ART_FRAME * 100}%)`;
          loadImage(p.info.id).then((src) => o.setUrl(src)).catch(() => {});
          art.current.continents.push(o);
        } else {
          isle(p).addTo(group);
        }
      }
    } else {
      for (const p of panels.values()) {
        const o = L.imageOverlay(BLANK, box({ x: p.dx, y: p.dy, w: p.w, h: p.h }), { pane: "gm-base", interactive: false }).addTo(group);
        loadImage(p.info.id).then((src) => o.setUrl(src)).catch(() => {});
      }
    }
    opacities.current(map.getZoom());
    refresh.current();
    return () => {
      group.remove();
    };
  }, [world, panels, size.w > 0, minimap]); // eslint-disable-line react-hooks/exhaustive-deps

  // The layers by zoom: the game's art over the stitched map when zoomed out, the minimap and the
  // caves over it when zoomed in.
  opacities.current = (z: number) => {
    const map = mapRef.current;
    const { unitK: unit, world: w } = latest.current;
    if (!map || !w) return;
    const at = 2 ** z / unit;
    map.getPane("gm-minimap")!.style.opacity = String(w.tiled ? minimapMix(at) : 1);
    map.getPane("gm-caves")!.style.opacity = String(byZoom(CAVE_ART, at));
    art.current.world?.setOpacity(byZoom(WORLD_ART, at));
    for (const o of art.current.continents) o.setOpacity(byZoom(CONTINENT_ART, at));
  };

  // What is drawn by view: the minimap's tiles and the caves near enough, the zones' maps (the
  // game versions without the stitched map), worked out once the view settles.
  const minimapShown = useRef(new Map<string, L.ImageOverlay>());
  const cavesShown = useRef(new Map<string, L.ImageOverlay>());
  const zonesShown = useRef(new Map<string, L.ImageOverlay>());
  refresh.current = () => {
    settle.current = 0;
    const map = mapRef.current;
    if (!map) return;
    // What depends on the zoom (the arrows' spacing, the zones' names) follows it once it settles.
    setZoom(map.getZoom());
    if (!world || !panels || !minimap || !size.w) return;
    const kk = 2 ** map.getZoom();
    const b = map.getBounds();
    const exact = [b.getWest(), Math.min(b.getNorth(), b.getSouth()), b.getEast(), Math.max(b.getNorth(), b.getSouth())];
    // A margin round the view: what a pan brings is there already.
    const [mw, mh] = [(exact[2] - exact[0]) * VIEW_MARGIN, (exact[3] - exact[1]) * VIEW_MARGIN];
    const view = [exact[0] - mw, exact[1] - mh, exact[2] + mw, exact[3] + mh];
    const near = minimap.size > 0 && kk >= unitK * (world.tiled ? MINIMAP_MIX[0][0] : MINIMAP_DETAIL);
    const tiles = near ? [...panels.values()].flatMap((p) => minimapTiles(p, minimap, kk, view)) : [];
    syncImages(map, minimapShown.current, tiles.map((tile) => ({ key: tile.key, rect: tile.rect, pad: tile.rect.w / 512, load: () => loadMinimapImage(tile.map, tile.level, tile.x, tile.y) })), "gm-minimap");
    // The next level in, for the view (not its margin), loaded before a zoom asks for it.
    if (minimap.size > 0 && kk * 2 >= unitK * (world.tiled ? MINIMAP_MIX[0][0] : MINIMAP_DETAIL))
      for (const p of panels.values()) for (const tile of minimapTiles(p, minimap, kk * 2, exact)) loadMinimapImage(tile.map, tile.level, tile.x, tile.y).catch(() => {});
    const caves = near && kk >= unitK * CAVE_ART[0][0] ? [...panels.values()].flatMap((p) => minimapTiles(p, minimap, kk, view, true)) : [];
    syncImages(map, cavesShown.current, caves.map((tile) => ({ key: tile.key, rect: tile.rect, pad: 0, load: () => loadMinimapImage(tile.map, tile.level, tile.x, tile.y) })), "gm-caves");
    // The old maps: the zones' over the continents' when zoomed in, until the minimap comes.
    const close = !world.tiled && near && tiles.length > 0;
    host.current!.style.background = world.tiled ? WORLD_SEA : close ? MINIMAP_SEA : "";
    const zoned = !world.tiled && !close && kk >= unitK * ZONE_DETAIL;
    const [x0, y0, x1, y1] = view;
    const inView = (r: { x: number; y: number; w: number; h: number }) => r.x < x1 && r.x + r.w > x0 && r.y < y1 && r.y + r.h > y0;
    const zoneImages = zoned
      ? [
          ...zones.filter((z) => inView(z.rect)).map((z) => ({ key: `z${z.z.id}`, rect: z.rect, pad: 0, clip: z.rings && clipOf(z.rings, z.rect), load: () => loadImage(z.z.id) })),
          ...covers.filter((c) => inView(c.around.rect)).map((c) => ({ key: `c${c.id}`, rect: c.around.rect, pad: 0, clip: clipOf(c.rings, c.around.rect), load: () => loadImage(c.around.z.id) })),
        ]
      : [];
    syncImages(map, zonesShown.current, zoneImages, "gm-zones");
  };

  const center = useCallback(
    (p: WorldPoint, kk?: number) => {
      const map = mapRef.current;
      const at = project?.(p);
      if (!map || !at) return;
      map.setView(ll(at[0], at[1]), kk ? Math.log2(kk) : map.getZoom(), { animate: false });
    },
    [project],
  );
  const fit = useCallback(() => {
    const map = mapRef.current;
    if (!map || !world || !size.w) return;
    map.setView(ll(world.width / 2, world.height / 2), Math.log2(fitK), { animate: false });
  }, [world, size, fitK]);
  fitRef.current = fit;

  // At first: the zone under way in view around the character (all the continents when not
  // following).
  const placed = useRef<string>("");
  useEffect(() => {
    if (!world || !project || !size.w || !mapRef.current) return;
    const key = [...world.panels.keys()].join("+");
    if (placed.current === key) return;
    placed.current = key;
    if (!follow) return fit();
    const area = steps[cards[current]?.first]?.zone_id;
    const rect = zones.find((z) => z.z.area === area)?.rect ?? covers.find((c) => c.area === area)?.rect;
    const kk = rect ? Math.min(size.w / rect.w, size.h / rect.h) * 0.95 : unitK * 6;
    center(positionAt(moves, clock.time).at, Math.min(unitK * MAX_ZOOM, kk));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [world, project, size, zones, covers]);

  // A card chosen: its place in the middle (where it starts when it has none), zoomed in to the
  // zones' maps at least. Following, the view goes with the character already.
  useEffect(() => {
    if (!focus || follow || !mapRef.current) return;
    const c = cards[focus.card];
    if (!c) return;
    const kk = 2 ** mapRef.current.getZoom();
    center(c.world ?? positionAt(moves, c.start).at, Math.max(kk, Math.min(unitK * MAX_ZOOM, unitK * ZONE_DETAIL * 1.5)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focus?.seq]);

  // At each moment: the character's dot, and the view following it.
  const dot = useRef<L.Marker | null>(null);
  useEffect(() => {
    const map = mapRef.current;
    if (!map) return;
    const icon = L.divIcon({
      className: "gm-icon",
      iconSize: [40, 40],
      iconAnchor: [20, 20],
      html: '<svg width="40" height="40" class="gm-svg"><g class="gm-dot" transform="translate(20 20)"><circle r="14" class="gm-dot-halo"/><circle r="6.5" class="gm-dot-core"/></g></svg>',
    });
    const m = L.marker([0, 0], { icon, pane: "gm-dot", interactive: false, keyboard: false });
    dot.current = m;
    return () => {
      m.remove();
      dot.current = null;
    };
  }, []);
  const cache = useRef(new Map());
  useEffect(() => {
    cache.current = new Map();
  }, [moves]);
  const draw = useCallback(() => {
    const map = mapRef.current;
    const m = dot.current;
    if (!project || !map || !m) return;
    const { at } = positionAt(moves, clock.time, cache.current);
    const here = project(at);
    if (!here) {
      m.remove();
      return;
    }
    const p = ll(here[0], here[1]);
    m.setLatLng(p);
    if (!map.hasLayer(m)) m.addTo(map);
    // (not while a zoom animates: it would stop it)
    if (latest.current.follow && !zooming.current) map.setView(p, map.getZoom(), { animate: false });
  }, [project, moves, clock]);
  useEffect(() => {
    draw();
    return clock.subscribe(draw);
  }, [clock, draw, follow]);

  const zoomBy = (f: number) => mapRef.current && zoomTo((glide.current?.z ?? mapRef.current.getZoom()) + Math.log2(f), null);

  // The steps drawn: from the current one, as many as chosen among the zone's (the slider).
  const zoneMax = Math.max(1, zone[1] - zone[0] + 1);
  const aheadShown = stepsShown(layers.ahead, zoneMax);
  const first = Math.min(Math.max(current, zone[0]), zone[1]);
  const last = Math.min(zone[1], first + aheadShown - 1);
  // Another zone under way: a count it cannot draw becomes all its steps.
  useEffect(() => {
    if (!cards.length) return;
    const ahead = aheadInZone(layers.ahead, zoneMax);
    if (ahead !== layers.ahead) onLayers({ ...layers, ahead });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [zoneMax, cards.length]);

  // The route: the whole guide faint, the steps drawn of the zone under way, the card under way bright.
  const all = useMemo(() => (project ? trace(moves.legs, project) : null), [moves, project]);
  const span = (from: number, to: number) => moves.legs.filter((l) => l.end > from && l.start < to);
  const zoneTrace = useMemo(
    () => (project && cards[first] && cards[last] ? trace(span(cards[first].start, cards[last].end), project) : null),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [moves, project, cards, first, last],
  );
  const lit = useMemo(
    () => (project && cards[current] ? trace(span(cards[current].start, currentEnd), project) : null),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [moves, project, cards, current, currentEnd],
  );
  // The lines, one SVG path a kind (Leaflet draws them again only once a zoom ends), and the way
  // along the steps drawn: arrows spaced on the screen, whatever the zoom.
  const renderer = useMemo(() => L.svg({ padding: 0.5 }), []);
  useEffect(() => {
    const map = mapRef.current;
    if (!map) return;
    const group = L.layerGroup().addTo(map);
    const lines = (d: Traced | null, cls: string) => {
      if (!d) return;
      const add = (pts: Point[][], kind: string, on: boolean) =>
        on && pts.length && L.polyline(pts.map((line) => line.map(([x, y]) => ll(x, y))), { renderer, className: `${kind} ${cls}`, interactive: false, smoothFactor: 0.5 }).addTo(group);
      add(d.lines.walk, "gm-walk", layers.walk);
      add(d.lines.flight, "gm-flight", layers.flight);
      add(d.lines.link, "gm-link", layers.link);
      add(d.lines.jump, "gm-jump", true);
    };
    if (layers.scope === "all") lines(all, "gm-all");
    lines(zoneTrace, layers.scope === "all" ? "gm-zone" : "gm-zone only");
    lines(lit, "gm-lit-glow");
    lines(lit, "gm-lit");
    if (zoneTrace) {
      const { lines: l } = zoneTrace;
      const marks = arrowMarks([...(layers.walk ? l.walk : []), ...(layers.flight ? l.flight : []), ...(layers.link ? l.link : [])], ARROW_GAP / k);
      const chevrons = marks.map(({ x, y, angle }) => {
        const [c, n] = [Math.cos(angle) / k, Math.sin(angle) / k];
        const at = (px: number, py: number) => ll(x + px * c - py * n, y + px * n + py * c);
        return [at(-4, -4), at(1, 0), at(-4, 4)];
      });
      if (chevrons.length) {
        L.polyline(chevrons, { renderer, className: "gm-arrows gm-arrows-under", interactive: false, smoothFactor: 0 }).addTo(group);
        L.polyline(chevrons, { renderer, className: "gm-arrows", interactive: false, smoothFactor: 0 }).addTo(group);
      }
    }
    return () => {
      group.remove();
    };
  }, [all, zoneTrace, lit, layers.walk, layers.flight, layers.link, layers.scope, k, renderer]);

  // Markers: the cards of the zone under way, those around the current one in full (the next
  // ones first), the others as dots.
  const markers = useMemo(() => {
    if (!project || !layers.markers) return [];
    const out: { n: number; x: number; y: number; dx: number; kind: string; glyph: string; near: boolean }[] = [];
    for (let n = first; n <= last && n < cards.length; n++) {
      const c = cards[n];
      const at = c.world && project(c.world);
      if (!at) continue;
      const [x, y] = at;
      out.push({ n, x, y, dx: 0, ...markerKind(steps, c), near: n >= current - NEAR_BEFORE && n <= current + NEAR_AFTER });
    }
    // Steps at the same place (quests of one NPC): side by side in the guide's order, not hidden under each other.
    const places = new Map<string, typeof out>();
    for (const m of out) {
      const key = `${Math.round(m.x * 4)},${Math.round(m.y * 4)}`;
      places.set(key, [...(places.get(key) ?? []), m]);
    }
    for (const same of places.values()) same.forEach((m, i) => (m.dx = (i - (same.length - 1) / 2) * SAME_PLACE_GAP));
    return out;
  }, [project, layers.markers, first, last, cards, steps, current]);
  const select = useRef(onSelect);
  select.current = onSelect;
  useEffect(() => {
    const map = mapRef.current;
    if (!map) return;
    const group = L.layerGroup().addTo(map);
    for (const m of markers) {
      const r = m.n === current ? 11 : m.near ? 8 : 4;
      const html =
        `<svg width="40" height="40" class="gm-svg"><g class="gm-marker gm-${m.kind}${m.n === current ? " current" : ""}${m.near ? "" : " far"}" transform="translate(20 20)">` +
        `<circle r="${r}"/>${m.near ? `<text text-anchor="middle" dominant-baseline="central">${m.glyph}</text>` : ""}` +
        `<g class="gm-label" transform="translate(16 0)"><text dominant-baseline="central">${m.n + 1}. ${escape(titles[m.n] ?? "")}</text></g></g></svg>`;
      const icon = L.divIcon({ className: "gm-icon", iconSize: [40, 40], iconAnchor: [20 - m.dx, 20], html });
      L.marker(ll(m.x, m.y), { icon, keyboard: false, zIndexOffset: m.n === current ? 2000 : m.near ? 1000 : 0 })
        .on("click", () => select.current(m.n))
        .addTo(group);
    }
    return () => {
      group.remove();
    };
  }, [markers, current, titles]);

  // Editing: the quests not taken ("!"), the objectives forgotten (red) and the turn-ins
  // forgotten ("?" in red), on the continents.
  const offers = useMemo(() => {
    if (!project || !suggestions || !onPick) return [];
    const out: Offer[] = [];
    const on = (p: WorldPoint) => !!project(p);
    for (const q of suggestions.untaken)
      q.places.filter((p) => on(p.at)).forEach((p, i) => {
        const [x, y] = project(p.at)!;
        const label = [`[${q.level}] ${q.name}`, p.name, q.dungeon].filter(Boolean).join(" · ");
        out.push({ key: `u${q.quest}/${i}`, x, y, kind: "untaken", glyph: "!", label, step: { kind: "accept", quest: q.quest }, difficulty: q.difficulty, dungeon: !!q.dungeon });
      });
    for (const o of suggestions.objectives)
      o.places.forEach((p, spot) => {
        if (!on(p.at)) return;
        const [x, y] = project(p.at)!;
        // A dungeon's objective is done in its run: picking it adds the run.
        const step: RouteStep = o.dungeon ? { kind: "dungeon", dungeon: o.dungeon[0] } : { kind: "objective", quest: o.quest, objective: o.objective, spot };
        // One mark per dungeon (its entrance), naming all its objectives left.
        const key = o.dungeon ? `d${o.dungeon[0]}` : `o${o.quest}/${o.objective}/${spot}`;
        const same = o.dungeon && out.find((m) => m.key === key);
        if (same) same.label += ` ; ${o.text}`;
        else out.push({ key, x, y, kind: "objective", glyph: "✕", label: o.dungeon ? `${o.dungeon[1]} : ${o.text}` : `${o.text} · ${o.name}`, step, dungeon: !!o.dungeon });
      });
    for (const q of suggestions.turn_ins)
      q.places.filter((p) => on(p.at)).forEach((p, i) => {
        const [x, y] = project(p.at)!;
        out.push({ key: `t${q.quest}/${i}`, x, y, kind: "turn-in", glyph: "?", label: `${q.name} · ${p.name}`, step: { kind: "turn_in", quest: q.quest } });
      });
    return out;
  }, [project, suggestions, onPick]);
  const pick = useRef(onPick);
  pick.current = onPick;
  useEffect(() => {
    const map = mapRef.current;
    if (!map) return;
    const group = L.layerGroup().addTo(map);
    for (const o of offers) {
      const shape = o.dungeon ? '<rect class="gm-offer-shape" x="-8" y="-8" width="16" height="16" transform="rotate(45)"/>' : '<circle class="gm-offer-shape" r="9"/>';
      const html =
        `<svg width="40" height="40" class="gm-svg"><g class="gm-offer gm-${o.kind}${o.difficulty ? ` gm-diff-${o.difficulty}` : ""}${o.dungeon ? " gm-in-dungeon" : ""}" transform="translate(20 20)">` +
        `${shape}<text text-anchor="middle" dominant-baseline="central">${o.glyph}</text>` +
        `<g class="gm-label" transform="translate(16 0)"><text dominant-baseline="central">${escape(o.label)}</text></g></g></svg>`;
      const icon = L.divIcon({ className: "gm-icon", iconSize: [40, 40], iconAnchor: [20, 20], html });
      L.marker(ll(o.x, o.y), { icon, keyboard: false, zIndexOffset: 500 })
        .on("click", () => pick.current?.(o.step))
        .addTo(group);
    }
    return () => {
      group.remove();
    };
  }, [offers]);

  // The zones' names, once zoomed in a little.
  const named = k >= 1.2 * unitK;
  useEffect(() => {
    const map = mapRef.current;
    if (!map || !named) return;
    const group = L.layerGroup().addTo(map);
    for (const { z, label } of zones) {
      if (!z.area) continue;
      const icon = L.divIcon({ className: "gm-icon", iconSize: [0, 0], html: `<span class="gm-zone-name">${escape(areaName(z))}</span>` });
      L.marker(ll(label[0], label[1]), { icon, pane: "gm-labels", interactive: false, keyboard: false }).addTo(group);
    }
    return () => {
      group.remove();
    };
  }, [zones, named, areaName]);

  return (
    <div className="guide-map">
      <div className="gm-bar">
        <strong>{t("Carte")}</strong>
        <button className={`gm-toggle${follow ? " on" : ""}`} aria-pressed={follow} onClick={() => onFollow(!follow)} title={t("La carte suit le personnage")}>
          <span aria-hidden>◎</span> {t("Suivre le personnage")}
        </button>
        <LayersMenu layers={layers} onLayers={onLayers} />
        <label className="gm-ahead" title={t("Étapes dessinées à partir de l'étape en cours, parmi celles de la zone")}>
          {t("Étapes")}
          <input
            type="range"
            min={1}
            max={zoneMax}
            value={aheadShown}
            disabled={zoneMax <= 1}
            onChange={(e) => onLayers({ ...layers, ahead: aheadPicked(Number(e.target.value), zoneMax) })}
          />
          <span>
            {aheadShown} / {zoneMax}
          </span>
        </label>
        <span className="gm-bar-fill" />
      </div>
      {status && <div className="gm-status">{status}</div>}
      <div className="gm-frame">
        <div className="gm-leaflet" ref={host} role="img" aria-label={t("Carte du guide")} />
        {error && <div className="error">{t("Carte indisponible : {error}", { error })}</div>}
        <div className="gm-legend" aria-hidden>
          <span>
            <i className="gm-walk" /> {t("À pied")}
          </span>
          <span>
            <i className="gm-flight" /> {t("Vol")}
          </span>
          <span>
            <i className="gm-link" /> {t("Bateau")}
          </span>
        </div>
        <div className="gm-zoom">
          <button onClick={() => zoomBy(1.6)} aria-label={t("Zoomer")} title={t("Zoomer")}>
            +
          </button>
          <button onClick={() => zoomBy(1 / 1.6)} aria-label={t("Dézoomer")} title={t("Dézoomer")}>
            −
          </button>
          <button
            onClick={() => {
              onFollow(false);
              fit();
            }}
            aria-label={t("Toute la carte")}
            title={t("Toute la carte")}
          >
            ⤢
          </button>
        </div>
      </div>
    </div>
  );
}

/** An image to show on the map at a rectangle (pixels of the map), possibly cut to an outline. */
interface Shown {
  key: string;
  rect: { x: number; y: number; w: number; h: number };
  pad: number;
  clip?: string | null;
  load: () => Promise<string>;
}

/** The images a pane should show now (the last asked), and those being loaded. */
const wanted = new WeakMap<Map<string, L.ImageOverlay>, Set<string>>();
const loading = new WeakMap<Map<string, L.ImageOverlay>, Set<string>>();

/** Bring a pane's images to `want`: each new one added once its picture is loaded, and those no
 * longer wanted kept under them until all the new ones are there (no hole showing the sea while
 * they load). */
function syncImages(map: L.Map, store: Map<string, L.ImageOverlay>, want: Shown[], pane: string) {
  const keys = new Set(want.map((w) => w.key));
  wanted.set(store, keys);
  const busy = loading.get(store) ?? new Set<string>();
  loading.set(store, busy);
  const prune = () => {
    const now = wanted.get(store);
    if (!now || busy.size) return;
    for (const [key, o] of store)
      if (!now.has(key)) {
        o.remove();
        store.delete(key);
      }
  };
  for (const w of want) {
    if (store.has(w.key) || busy.has(w.key)) continue;
    busy.add(w.key);
    const settled = () => {
      busy.delete(w.key);
      prune();
    };
    w.load()
      .then((src) => {
        // No longer wanted meanwhile: not added.
        if (!wanted.get(store)?.has(w.key)) return settled();
        const o = L.imageOverlay(src, box(w.rect, w.pad), { pane, interactive: false, opacity: 0 });
        o.once("load error", () => {
          o.setOpacity(1);
          settled();
        });
        o.addTo(map);
        const el = o.getElement();
        if (el && w.clip) el.style.clipPath = w.clip;
        store.set(w.key, o);
      })
      .catch(settled);
  }
  prune();
}

/** A CSS clip of an image at `rect` to the biggest of its outline's rings (pixels of the map). */
function clipOf(rings: [number, number][][], rect: { x: number; y: number; w: number; h: number }) {
  const ring = rings.reduce((a, b) => (b.length > a.length ? b : a), rings[0] ?? []);
  if (ring.length < 3) return null;
  const pct = (v: number, o: number, s: number) => `${(((v - o) / s) * 100).toFixed(2)}%`;
  return `polygon(${ring.map(([x, y]) => `${pct(x, rect.x, rect.w)} ${pct(y, rect.y, rect.h)}`).join(",")})`;
}

/** An island in the sky on the stitched world map (Zephras Isle): its art in a gold frame, a pale
 * glow round it, its torn border cut off. */
function isle(p: Panel): L.LayerGroup {
  const group = L.layerGroup();
  const [ix, iy] = [p.w * ISLE_BORDER, p.h * ISLE_BORDER * 1.5];
  const r = { x: p.dx + ix, y: p.dy + iy, w: p.w - 2 * ix, h: p.h - 2 * iy };
  // A pale halo, ring after ring fainter outwards: under the art (over it, it would pale it too).
  const glow = L.svg({ pane: "gm-isle-glow", padding: 0.5 });
  for (let i = ISLE_HALO; i > 0; i--) {
    const g = Math.min(r.w, r.h) * 0.025 * i;
    L.rectangle(box(r, g), { renderer: glow, className: "gm-isle-glow", interactive: false, stroke: false }).addTo(group);
  }
  const renderer = L.svg({ pane: "gm-isle", padding: 0.5 });
  const art = L.imageOverlay(BLANK, box({ x: p.dx, y: p.dy, w: p.w, h: p.h }), { pane: "gm-isle", interactive: false }).addTo(group);
  art.on("add", () => {
    const el = art.getElement();
    if (el) el.style.clipPath = `inset(${ISLE_BORDER * 150}% ${ISLE_BORDER * 100}% round 4%)`;
  });
  loadImage(p.info.id).then((src) => art.setUrl(src)).catch(() => {});
  L.rectangle(box(r), { renderer, className: "gm-isle-frame", interactive: false, fill: false }).addTo(group);
  L.rectangle(box(r), { renderer, className: "gm-isle-frame-in", interactive: false, fill: false }).addTo(group);
  return group;
}

/** The share of the minimap over the stitched world map at zoom `z` (times a whole continent in view). */
function minimapMix(z: number) {
  return byZoom(MINIMAP_MIX, z);
}

/** A value by zoom `z` from (zoom, value) points, log-interpolated, the ends held. */
function byZoom(points: [number, number][], z: number) {
  const i = points.findIndex(([at]) => at > z);
  if (i === 0) return points[0][1];
  if (i < 0) return points[points.length - 1][1];
  const [[z0, m0], [z1, m1]] = [points[i - 1], points[i]];
  return m0 + ((m1 - m0) * Math.log(z / z0)) / Math.log(z1 / z0);
}

/** A minimap image in view: its game map, level and place, and its rectangle on the continent's map. */
interface MinimapTile {
  key: string;
  map: number;
  level: number;
  x: number;
  y: number;
  rect: { x: number; y: number; w: number; h: number };
}

/** The minimap images of a continent over `box` (x0, y0, x1, y1 on the map) at zoom `k`: of the
 * level whose images are drawn closest to their own size. */
function minimapTiles(panel: Panel, have: Set<string>, k: number, [x0, y0, x1, y1]: number[], caves = false): MinimapTile[] {
  // The caves (`fgd caves`) are packed as their own map, one image a game tile.
  const map = caves ? CAVES + panel.map : panel.map;
  // A game tile (x eastwards from world y = 32 tiles, y southwards from world x = 32 tiles) on the
  // map, measured with the panel's own projection (the continent's art, or its place on the
  // stitched world map).
  const [ox, oy] = panel.project(32 * TILE, 32 * TILE);
  const sx = panel.project(32 * TILE, 31 * TILE)[0] - ox;
  const sy = panel.project(31 * TILE, 32 * TILE)[1] - oy;
  const level = caves ? 0 : Math.max(0, Math.min(MINIMAP_LEVELS - 1, Math.round(Math.log2(MINIMAP_SIZE / (sx * k)))));
  const span = 2 ** level;
  const [w, h] = [sx * span, sy * span];
  const out: MinimapTile[] = [];
  for (let gx = Math.max(0, Math.floor((x0 - ox) / w)); gx <= Math.min(64 / span - 1, Math.floor((x1 - ox) / w)); gx++) {
    for (let gy = Math.max(0, Math.floor((y0 - oy) / h)); gy <= Math.min(64 / span - 1, Math.floor((y1 - oy) / h)); gy++) {
      const key = `${map}/${level}/${gx}/${gy}`;
      if (have.has(key)) out.push({ key, map, level, x: gx, y: gy, rect: { x: ox + gx * w, y: oy + gy * h, w, h } });
    }
  }
  return out;
}

/** What the map draws, in a menu. */
function LayersMenu({ layers, onLayers }: { layers: MapLayers; onLayers: (l: MapLayers) => void }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const menu = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: PointerEvent) => !menu.current?.contains(e.target as Node) && setOpen(false);
    document.addEventListener("pointerdown", close);
    return () => document.removeEventListener("pointerdown", close);
  }, [open]);
  const check = (key: "walk" | "flight" | "link" | "markers", label: string) => (
    <label>
      <input type="checkbox" checked={layers[key]} onChange={(e) => onLayers({ ...layers, [key]: e.target.checked })} /> {label}
    </label>
  );
  return (
    <div className="gm-layers" ref={menu}>
      <button aria-expanded={open} onClick={() => setOpen(!open)}>
        {t("Calques")} ▾
      </button>
      {open && (
        <div className="gm-menu" role="group" aria-label={t("Calques")}>
          {check("walk", t("Trajets à pied"))}
          {check("flight", t("Vols"))}
          {check("link", t("Bateaux et zeppelins"))}
          {check("markers", t("Étapes sur la carte"))}
          <hr />
          <label>
            <input type="radio" checked={layers.scope === "all"} onChange={() => onLayers({ ...layers, scope: "all" })} /> {t("Tout le guide")}
          </label>
          <label>
            <input type="radio" checked={layers.scope === "zone"} onChange={() => onLayers({ ...layers, scope: "zone" })} /> {t("La zone en cours")}
          </label>
        </div>
      )}
    </div>
  );
}
