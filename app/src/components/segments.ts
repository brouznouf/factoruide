// The guide's steps cut into zones (a dungeon run apart), and the names of their kinds: shared
// by the guide viewer, the guide editor and the comparison of versions.

/** Kinds of the guide's steps (French keys of the catalog). */
export const KIND_LABELS: Record<string, string> = {
  accept: "Prendre",
  objective: "Faire",
  turnin: "Rendre",
  fly: "Vol",
  flight_master: "Point de vol",
  link: "Transport",
  grind: "Farmer",
  train: "Entraîneur",
  hearth: "Pierre de foyer",
  teleport: "Téléportation",
  bind: "Auberge",
  dungeon: "Donjon",
  profession: "Métier",
  practice: "Pratique",
  deathskip: "Mort volontaire",
  abandon: "Abandonner",
};

/** What cutting steps into segments needs of a step. */
export interface SegmentStep {
  kind: string;
  zone?: string;
  target?: { name: string };
  inside?: boolean;
  /** Time and level once it is done. */
  time: number;
  level: number;
}

export interface Segment {
  /** Zone, or dungeon name; null: steps done before any place (abandoning quests). */
  zone: string | null;
  dungeon: boolean;
  start: number;
  end: number;
  fromLevel: number;
  toLevel: number;
  /** Indices of the first and last steps. */
  first: number;
  last: number;
}

export const steps = (s: Segment) => s.last - s.first + 1;

/** Steps that take the character elsewhere: the hearthstone, teleports and death skips land in
 * their step's zone, flights and boats leave from it (the next step is where they land). */
const ARRIVING = new Set(["hearth", "teleport", "deathskip"]);
const LEAVING = new Set(["fly", "link"]);

/** Consecutive steps in the same zone; a dungeon run is a segment of its own. */
export function segments(list: SegmentStep[]): Segment[] {
  const out: Segment[] = [];
  let previousTime = 0;
  list.forEach((s, i) => {
    const last = out[out.length - 1];
    // The quests done inside a dungeon belong to its run.
    if (s.inside && last?.dungeon) {
      last.end = s.time;
      last.toLevel = s.level;
      last.last = i;
      previousTime = s.time;
      return;
    }
    const dungeon = s.kind === "dungeon";
    // Steps done anywhere (practicing a profession, abandoning a quest) have no place: they
    // belong to the zone the character is in.
    const zone = dungeon ? (s.target?.name ?? s.zone ?? null) : (s.zone ?? (last && !last.dungeon ? last.zone : null));
    if (last && !dungeon && !last.dungeon && last.zone === zone) {
      last.end = s.time;
      last.toLevel = s.level;
      last.last = i;
    } else {
      out.push({ zone, dungeon, start: previousTime, end: s.time, fromLevel: s.level, toLevel: s.level, first: i, last: i });
    }
    previousTime = s.time;
  });
  // Fold short passages (a couple of steps, a few minutes) into the previous segment, unless
  // the character travelled there (the hearthstone, a flight, a boat): a new zone starts.
  const merged: Segment[] = [];
  for (const s of out) {
    const last = merged[merged.length - 1];
    const travelled = ARRIVING.has(list[s.first].kind) || (s.first > 0 && LEAVING.has(list[s.first - 1].kind));
    const short = steps(s) <= 2 && s.end - s.start < 300 && !travelled;
    if (last && !last.dungeon && !s.dungeon && (short || last.zone === s.zone)) {
      last.end = s.end;
      last.toLevel = s.toLevel;
      last.last = s.last;
    } else {
      merged.push({ ...s });
    }
  }
  return merged;
}

