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
  elapsed?: number;
  budget?: number;
  start?: number | null;
  best?: number | null;
}

export interface Gain {
  /** Seconds saved by this improvement. */
  saved: number;
  /** When it happened (seconds into the optimization). */
  at: number;
}

/** "−45 s", "−3 min", "−1h02". */
export function fmtGain(seconds: number): string {
  if (seconds < 60) return `−${Math.max(1, Math.round(seconds))} s`;
  if (seconds < 3600) return `−${Math.round(seconds / 60)} min`;
  const m = Math.round(seconds / 60);
  return `−${Math.floor(m / 60)}h${String(m % 60).padStart(2, "0")}`;
}

