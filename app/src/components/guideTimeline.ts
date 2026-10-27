// What the guide viewer shows, from a guide's steps and its timeline (`api.guideTimeline`):
// the steps as cards, the character's position at any time, what it is doing,
// and what it holds. Pure functions, shared with the guide editor.
import type { Leg, QuestInfo, Route, Step, Timeline, TimelineStep, WorldPoint } from "../api";

/** A step of the guide, shown as one card, with the steps without a place of their own after it. */
export interface Card {
  /** Index of the card (its number is `n + 1`). */
  n: number;
  /** First and last steps (indexes in the guide's steps). */
  first: number;
  last: number;
  /** When its first step starts and its last one ends. */
  start: number;
  end: number;
  /** Where it is done (its first step with a place). */
  world?: WorldPoint;
  zone?: string;
  /** Travel cards: a flight, a boat, the hearthstone, a teleport. */
  travel: boolean;
  /** The level it reaches (none: no level gained in it). */
  ding: number | null;
  /** Mobs killed on the way to it. */
  farm: number;
}

/** Step kinds that move the character elsewhere: a card of their own. */
const TRAVEL_KINDS = new Set(["fly", "link", "hearth", "teleport", "deathskip"]);

/** Checkpoints: a level to have before going on, nothing done (zero time). */
export const isCheckpoint = (s: Step) => s.kind === "grind" && !!s.keep;

/** Time at which step `i` starts: the end of the one before. */
export const stepStart = (steps: Step[], i: number) => (i > 0 ? steps[i - 1].time : 0);

/** The guide's steps as cards: one a step, in the guide's order (each seen and moved on its own). */
export function buildCards(steps: Step[], fromLevel: number): Card[] {
  const cards: Card[] = [];
  steps.forEach((s, i) => {
    const last = cards[cards.length - 1];
    const travel = TRAVEL_KINDS.has(s.kind);
    // Checkpoints, quests abandoned (no place) and quests done in a dungeon belong to the card
    // before. A step without a place yet (edited, not replayed) keeps its own.
    if (last && (isCheckpoint(s) || (s.kind === "abandon" && !s.world) || s.inside)) {
      last.last = i;
      last.end = s.time;
      last.farm += s.farm ?? 0;
      if (!last.world && s.world) last.world = s.world;
      return;
    }
    cards.push({ n: cards.length, first: i, last: i, start: stepStart(steps, i), end: s.time, world: s.world, zone: s.zone, travel, ding: null, farm: s.farm ?? 0 });
  });
  // Levels reached in each card.
  let level = fromLevel;
  for (const c of cards) {
    const top = Math.max(...steps.slice(c.first, c.last + 1).map((s) => s.level));
    if (top > level) c.ding = top;
    level = Math.max(level, top);
  }
  return cards;
}

/** The card holding step `i`. */
export function cardOf(cards: Card[], i: number): number {
  let lo = 0;
  let hi = cards.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (cards[mid].first <= i) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

/** The step under way at `time`: the first one ending after it (the last one at the end). */
export function stepAt(steps: Step[], time: number): number {
  let lo = 0;
  let hi = steps.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (steps[mid].time > time) hi = mid;
    else lo = mid + 1;
  }
  return lo;
}

/**
 * When the travel of each travel step ends (its index -> time): a flight, a boat, the
 * hearthstone or a teleport starts when its step ends (the step is done once taken) and goes on
 * into the next step's time, which the viewer still shows as the travel step's.
 */
export function travelEnds(steps: Step[], legs: Leg[]): Map<number, number> {
  const out = new Map<number, number>();
  steps.forEach((s, i) => {
    if (!TRAVEL_KINDS.has(s.kind)) return;
    // The first jump or ride from the step's end (after the cast or the wait for the boat).
    for (let k = Math.max(0, legAt(legs, s.time - 1)); k < legs.length && legs[k].start <= s.time + TRAVEL_START; k++) {
      const l = legs[k];
      if (l.start >= s.time - 1 && l.mode !== "walk" && l.mode !== "mount") {
        out.set(i, l.end);
        break;
      }
    }
  });
  return out;
}

/** A travel starts at most this long after its step ends (seconds: the cast, the wait for a boat). */
const TRAVEL_START = 600;

/**
 * A card's title: the NPC of all its steps; for objectives only, what they target (mobs,
 * items, objects); else the named place, else its zone.
 */
export function cardTitle(card: Card, steps: Step[], spans: Spans): string {
  const list = steps.slice(card.first, card.last + 1).filter((s) => !isCheckpoint(s));
  const first = list[0] ?? steps[card.first];
  if (card.travel) return first.target?.name ?? first.text;
  const npcs = new Set(list.map((s) => (s.target?.kind === "npc" && s.kind !== "objective" ? s.target.name : null)));
  if (npcs.size === 1 && !npcs.has(null)) return [...npcs][0]!;
  if (list.every((s) => s.kind === "objective")) {
    const targets = [...new Set(list.map((s) => s.mobs?.[0] ?? s.target?.name).filter(Boolean))];
    if (targets.length) return targets.slice(0, 2).join(" · ");
  }
  const place = spans?.[card.first]?.place;
  if (place) return place;
  return first.target?.name ?? first.zone ?? first.text;
}

/** The timeline of each step shown (none: not replayed yet), or none at all while the replay loads. */
export type Spans = (TimelineStep | undefined)[] | null;

/** Seconds of travel, combat, interaction and waiting of steps `first..=last`. */
export function splitOf(spans: Spans, first: number, last: number) {
  const out = { travel: 0, combat: 0, interact: 0, wait: 0 };
  if (!spans) return out;
  for (let i = first; i <= last; i++) {
    const s = spans[i];
    if (!s) continue;
    out.travel += s.travel ?? 0;
    out.combat += s.combat ?? 0;
    out.interact += s.interact ?? 0;
    out.wait += s.wait ?? 0;
  }
  return out;
}

/** What the character has at a moment of the guide. */
export interface CharState {
  /** Quests in the log, by ID, with their objectives done. */
  log: Map<number, Set<number>>;
  /** Flight paths known (taxi node IDs). */
  flights: Set<number>;
  /** The inn the hearthstone is bound to (its innkeeper's ID). */
  bind: number | null;
  /** When the hearthstone and Astral Recall are ready (game time). */
  hearth: number;
  recall: number;
  /** Copper paid for flights so far. */
  cost: number;
}

const copy = (s: CharState): CharState => ({
  log: new Map([...s.log].map(([q, done]) => [q, new Set(done)])),
  flights: new Set(s.flights),
  bind: s.bind,
  hearth: s.hearth,
  recall: s.recall,
  cost: s.cost,
});

/** Apply what a step changes. */
function apply(s: CharState, step: TimelineStep) {
  for (const q of step.accepted ?? []) if (!s.log.has(q)) s.log.set(q, new Set());
  for (const [q, k] of step.done ?? []) s.log.get(q)?.add(k);
  for (const q of step.removed ?? []) s.log.delete(q);
  for (const f of step.flights ?? []) s.flights.add(f);
  if (step.bind != null) s.bind = step.bind;
  if (step.hearth != null) s.hearth = step.hearth;
  if (step.recall != null) s.recall = step.recall;
  s.cost += step.cost ?? 0;
}

/** Every this many steps a copy of the state is kept: the state after any step is rebuilt from the copy before it. */
const KEEP_EVERY = 64;

/** The character's state after any step of a timeline (-1: before the first), rebuilt quickly. */
export function stateTracker(timeline: Timeline) {
  const start: CharState = {
    log: new Map(timeline.start.log.map((q) => [q, new Set<number>()])),
    flights: new Set(timeline.start.flights),
    bind: timeline.start.bind,
    hearth: 0,
    recall: 0,
    cost: 0,
  };
  for (const [q, k] of timeline.start.done) start.log.get(q)?.add(k);
  const kept: CharState[] = [];
  let s = copy(start);
  timeline.steps.forEach((step, i) => {
    if (i % KEEP_EVERY === 0) kept.push(copy(s));
    apply(s, step);
  });
  return (after: number): CharState => {
    if (after < 0) return copy(start);
    const k = Math.floor((after + 1) / KEEP_EVERY);
    s = copy(kept[Math.min(k, kept.length - 1)]);
    for (let i = Math.min(k, kept.length - 1) * KEEP_EVERY; i <= after && i < timeline.steps.length; i++) apply(s, timeline.steps[i]);
    return s;
  };
}

/** The character's moves: where it starts, and its legs one after the other. */
export interface Moves {
  start: WorldPoint;
  legs: Leg[];
}

/** Moves straight from step to step (while the timeline loads): walking to each place at the start of its step, at running speed. */
export function straightMoves(steps: Step[]): Moves {
  const placed = steps.filter((s) => s.world);
  const legs: Leg[] = [];
  let at = placed[0]?.world ?? ([0, 0, 0] as WorldPoint);
  steps.forEach((s, i) => {
    const w = s.world;
    if (!w || (w[0] === at[0] && w[1] === at[1] && w[2] === at[2])) return;
    const start = stepStart(steps, i);
    // About running speed (7 yards a second): never past the step's end.
    const length = w[0] === at[0] ? Math.min(s.time - start, Math.hypot(w[1] - at[1], w[2] - at[2]) / 7) : 0;
    legs.push({ mode: w[0] === at[0] ? "walk" : "link", start, end: start + length, from: at, to: w });
    at = w;
  });
  return { start: placed[0]?.world ?? at, legs };
}

/** A leg's points from start to end, and the length walked at each. */
export function legPoints(leg: Leg): { points: [number, number][]; lengths: number[] } {
  const points: [number, number][] = [[leg.from[1], leg.from[2]], ...(leg.via ?? []), [leg.to[1], leg.to[2]]];
  const lengths = [0];
  for (let k = 1; k < points.length; k++) lengths.push(lengths[k - 1] + Math.hypot(points[k][0] - points[k - 1][0], points[k][1] - points[k - 1][1]));
  return { points, lengths };
}

/** The last leg started at `time` (-1: none yet). */
export function legAt(legs: Leg[], time: number): number {
  let lo = -1;
  let hi = legs.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (legs[mid].start <= time) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

/** Where the character is at `time`: along the leg under way, else where the last one ended (`cache`: the legs' points). */
export function positionAt(moves: Moves, time: number, cache = new Map<number, ReturnType<typeof legPoints>>()): { at: WorldPoint; leg: number; moving: boolean } {
  const k = legAt(moves.legs, time);
  if (k < 0) return { at: moves.start, leg: -1, moving: false };
  const leg = moves.legs[k];
  if (time >= leg.end || leg.end <= leg.start) return { at: leg.to, leg: k, moving: false };
  // Jumps (the hearthstone, teleports, boats between continents) land at their end.
  if (leg.from[0] !== leg.to[0]) return { at: time - leg.start < (leg.end - leg.start) / 2 ? leg.from : leg.to, leg: k, moving: true };
  let p = cache.get(k);
  if (!p) cache.set(k, (p = legPoints(leg)));
  const total = p.lengths[p.lengths.length - 1];
  const want = ((time - leg.start) / (leg.end - leg.start)) * total;
  let j = 1;
  while (j < p.lengths.length - 1 && p.lengths[j] < want) j++;
  const span = p.lengths[j] - p.lengths[j - 1];
  const f = span > 0 ? (want - p.lengths[j - 1]) / span : 1;
  const [a, b] = [p.points[j - 1], p.points[j]];
  return { at: [leg.from[0], a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f], leg: k, moving: true };
}

/** Total XP of a level and the XP into it. */
function totalXp(levels: number[], level: number, xp: number) {
  let t = xp;
  for (let l = 1; l < level && l < levels.length; l++) t += levels[l];
  return t;
}

/** Level and XP into it at `time` of step `i`: from the step before to the step's, over its action. */
export function xpAt(route: Route, timeline: Timeline | null, i: number, time: number): { level: number; xp: number; next: number } {
  const steps = route.steps;
  const before = i > 0 ? steps[i - 1] : null;
  const after = steps[i];
  const levels = timeline?.xp_levels ?? [];
  const next = (l: number) => levels[l] ?? 0;
  const from = before ? { level: before.level, xp: before.route_xp ?? 0 } : { level: route.from_level, xp: 0 };
  const t = timeline?.steps[i];
  // No step there (a guide being written with none yet): where the character starts.
  if (!timeline || !levels.length || !after || !t) return { ...from, next: next(from.level) };
  const to = { level: after.level, xp: after.route_xp ?? 0 };
  const begin = Math.max(t.arrive, t.start);
  const f = time >= t.end ? 1 : time <= begin || t.end <= begin ? 0 : (time - begin) / (t.end - begin);
  let total = totalXp(levels, from.level, from.xp) + (totalXp(levels, to.level, to.xp) - totalXp(levels, from.level, from.xp)) * Math.max(0, f);
  let level = 1;
  while (level < levels.length - 1 && total >= levels[level]) total -= levels[level++];
  return { level, xp: Math.round(total), next: next(level) };
}

/** What the character is doing at `time`: a verb (French key, see VERBS) and what it does it to. */
export interface Activity {
  verb: string;
  what: string;
}

/** The quest of a step by ID, with the objective it is about. */
export function questOf(quests: Map<number, QuestInfo>, s: Step) {
  const q = s.quest != null ? quests.get(s.quest) : undefined;
  return { quest: q, objective: q && s.objective != null ? q.objectives[s.objective] : undefined };
}

/** What the character does at `time` during step `i` (after its moves, or on one of them); `title`: the title of its card. */
export function activityAt(route: Route, timeline: Timeline, quests: Map<number, QuestInfo>, i: number, title: string, time: number, leg: number, moving: boolean): Activity {
  const s = route.steps[i];
  if (moving && leg >= 0) {
    const l = timeline.legs[leg];
    if (l.mode === "flight") {
      const node = timeline.nodes.find((n) => n.at[0] === l.to[0] && Math.hypot(n.at[1] - l.to[1], n.at[2] - l.to[2]) < 30);
      return { verb: "En vol vers", what: node?.name ?? "" };
    }
    if (l.mode === "link") {
      const link = route.steps.slice(0, i + 1).reverse().find((x) => x.kind === "link");
      return { verb: "En transport", what: link?.target?.name ?? link?.text ?? "" };
    }
    return { verb: "En route vers", what: title };
  }
  const t = timeline.steps[i];
  // No step there (a guide being written with none yet): only the grind to the target level.
  if (!s || !t) return { verb: "Farme", what: "" };
  if (time < t.arrive) {
    const next = timeline.legs[leg + 1];
    if (next?.mode === "hearth") return { verb: "Pierre de foyer", what: "" };
    if (next?.mode === "link") return { verb: "Attend le départ", what: "" };
    return { verb: "En route vers", what: title };
  }
  const { quest, objective } = questOf(quests, s);
  const target = s.target?.name ?? "";
  switch (s.kind) {
    case "accept":
      return { verb: "Prend", what: quest?.name ?? s.quest_name ?? "" };
    case "turnin":
      return { verb: "Rend", what: quest?.name ?? s.quest_name ?? "" };
    case "objective": {
      const verb = { kill: "Tue", collect: "Récupère", use: "Utilise", talk: "Parle à", other: "Fait" }[objective?.kind ?? "other"];
      return { verb, what: objective?.kind === "other" ? (objective.text ?? s.text) : target || s.text };
    }
    case "grind":
      return { verb: "Farme", what: s.zone ?? "" };
    case "dungeon":
      return { verb: "En donjon", what: target };
    case "train":
      return { verb: "S'entraîne", what: target };
    case "sell":
      return { verb: "Vend", what: target };
    case "flight_master":
      return { verb: "Point de vol", what: target || s.text };
    case "bind":
      return { verb: "Auberge", what: target };
    case "hearth":
      return { verb: "Pierre de foyer", what: "" };
    case "deathskip":
      return { verb: "Mort volontaire", what: s.zone ?? "" };
    case "abandon":
      return { verb: "Abandonne", what: s.quest_name ?? "" };
    default:
      return { verb: "Fait", what: s.text };
  }
}

/** Clock time of a guide, "1:02:03", or "2:03" under an hour. */
export function fmtClock(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
}
