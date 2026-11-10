/** How many steps the guide map draws from the current one, and the arrows telling the way. */

/** Steps drawn: all those of the zone under way (`ahead` null), else `ahead`, at most the zone's. */
export function stepsShown(ahead: number | null, max: number): number {
  return ahead == null ? max : Math.max(1, Math.min(ahead, max));
}

/** The slider moved: its top means "all the zone's steps", whatever the next zone's count. */
export function aheadPicked(value: number, max: number): number | null {
  return value >= max ? null : value;
}

/** Another zone under way: a number of steps it cannot draw becomes "all its steps"; a smaller
 * one stays. */
export function aheadInZone(ahead: number | null, max: number): number | null {
  return ahead != null && ahead >= max ? null : ahead;
}

/** An arrow along a line: its place and the way it points (radians). */
export interface Arrow {
  x: number;
  y: number;
  angle: number;
}

/** Arrows every `spacing` along the lines (the first half a spacing from their start), at most
 * `limit` of them. */
export function arrowMarks(lines: [number, number][][], spacing: number, limit = 600): Arrow[] {
  const out: Arrow[] = [];
  if (!(spacing > 0)) return out;
  for (const line of lines) {
    let next = spacing / 2;
    let walked = 0;
    for (let i = 1; i < line.length; i++) {
      const [x0, y0] = line[i - 1];
      const [x1, y1] = line[i];
      const len = Math.hypot(x1 - x0, y1 - y0);
      if (len === 0) continue;
      while (next <= walked + len) {
        const f = (next - walked) / len;
        out.push({ x: x0 + (x1 - x0) * f, y: y0 + (y1 - y0) * f, angle: Math.atan2(y1 - y0, x1 - x0) });
        if (out.length >= limit) return out;
        next += spacing;
      }
      walked += len;
    }
  }
  return out;
}
