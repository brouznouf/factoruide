/** A route the optimization follows: its play time and its score (time plus what makes it
 * tedious: grinding, zone changes, gold spent on flights). The route kept has the best score. */
export interface RaceRoute {
  id: number;
  time: number;
  score: number;
}

/** Live state of the optimization, from the planner's `@progress` messages. */
export interface OptimizeState {
  phase: "prepare" | "construct" | "optimize";
  /** While preparing: what is loading. */
  step?: "world" | "quests";
  /** Race (several candidate routes): qualification, then the final of the best ones. */
  stage?: "qualify" | "final";
  candidates?: number;
  qualified?: number;
  finalists?: number;
  /** Seconds of qualification, at the start of the budget. */
  qualify?: number;
  elapsed?: number;
  budget?: number;
  start?: number | null;
  best?: number | null;
  /** While qualifying the best candidates so far, then the finalists. */
  routes?: RaceRoute[];
  /** Route kept (best score) among `routes`. */
  selected?: number | null;
}

/** Times of each followed route, as [seconds into the optimization, play time] points. */
export type RaceSeries = Record<number, [number, number][]>;

/** Categorical colors (see index.css `--cat-*`); beyond them routes are drawn in gray. */
export const RACE_COLORS = 8;

/** "−45 s", "−3 min", "−1h02". */
export function fmtGain(seconds: number): string {
  if (seconds < 60) return `−${Math.max(1, Math.round(seconds))} s`;
  if (seconds < 3600) return `−${Math.round(seconds / 60)} min`;
  const m = Math.round(seconds / 60);
  return `−${Math.floor(m / 60)}h${String(m % 60).padStart(2, "0")}`;
}
