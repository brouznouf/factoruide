// The guide's steps cut into zones (a dungeon run apart), and the names of their kinds: shared
// by the guide viewer, the guide editor and the comparison of versions.

/** Kinds of the guide's steps (French keys of the catalog). */
export const KIND_LABELS: Record<string, string> = {
  vendor: "Vendor",
  mailbox: "Mailbox",
  dungeon_exit: "Leave dungeon",
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
  // Fold short passages (a couple of steps, a few minutes) into the previous segment.
  const merged: Segment[] = [];
  for (const s of out) {
    const last = merged[merged.length - 1];
    const short = steps(s) <= 2 && s.end - s.start < 300;
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

