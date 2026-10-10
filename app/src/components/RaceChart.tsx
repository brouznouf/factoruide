import { useRef, useState } from "react";
import { fmtTime } from "../api";
import { useT } from "../i18n";
import type { OptimizeState, RaceRoute, RaceSeries } from "./OptimizeProgress";
import { useWidth } from "./useWidth";

/** Color of a route: its categorical slot, gray without one. */
const color = (slot: number | undefined) => (slot == null ? "var(--text-muted)" : `var(--cat-${slot + 1})`);

/** Value of a step series at time `x` (its last point before), or undefined before it starts. */
function valueAt(points: [number, number][], x: number): number | undefined {
  let v: number | undefined;
  for (const [px, py] of points) {
    if (px > x) break;
    v = py;
  }
  return v;
}

/** About four round tick values between `lo` and `hi` (seconds of play time). */
function ticks(lo: number, hi: number): number[] {
  const steps = [60, 120, 300, 600, 900, 1800, 3600, 7200, 18000];
  const step = steps.find((s) => (hi - lo) / s <= 4) ?? 36000;
  const out = [];
  for (let v = Math.ceil(lo / step) * step; v <= hi; v += step) out.push(v);
  return out;
}

interface Props {
  state: OptimizeState | null;
  series: RaceSeries;
  /** Color slot of each route. */
  colors: Record<number, number>;
}

/**
 * The routes the optimization follows. While the best starts are picked, their times are only
 * points on the left edge (they change as better candidates come); once picked, each route's
 * time is drawn until the end, the kept route in bold.
 */
export function RaceChart({ state, series, colors }: Props) {
  const t = useT();
  const plot = useRef<SVGSVGElement>(null);
  const [hover, setHover] = useState<number | null>(null);
  const [frame, W] = useWidth(900);
  const routes: RaceRoute[] = state?.routes ?? [];
  const selected = state?.selected ?? null;
  const qualifying = state?.stage === "qualify";
  const byTime = [...routes].sort((a, b) => a.time - b.time);

  const legend = (
    <ul className="race-times">
      {byTime.map((r) => (
        <li key={r.id} className={r.id === selected ? "selected" : undefined}>
          <span className="swatch" style={{ background: color(colors[r.id]) }} />
          {r.id === selected ? <strong>{fmtTime(r.time)}</strong> : fmtTime(r.time)}
          {r.id === selected && <span className="tag">{t("retenu")}</span>}
        </li>
      ))}
    </ul>
  );
  const explanation = (
    <p className="race-note">
      {t(
        "Le temps en gras est le guide retenu. Ce n'est pas toujours le plus court : chaque chemin est noté sur son temps de jeu, plus un malus pour ce qui rend la montée pénible (farm, allers-retours entre zones, or dépensé en vols). Le guide retenu a la meilleure note : c'est le plus confortable à jouer.",
      )}
    </p>
  );

  if (routes.length === 0) {
    return (
      <figure className="race-chart">
        <div className="empty-chart">{t("Les temps apparaissent dès que les premiers chemins sont construits…")}</div>
      </figure>
    );
  }

  const H = 260;
  const pad = { l: 56, r: 16, t: 12, b: 24 };
  const elapsed = state?.elapsed ?? 0;
  const x0 = state?.stage ? (state.qualify ?? 0) : 0;
  const x1 = Math.max(state?.budget ?? 60, elapsed, x0 + 1);
  const values = qualifying ? routes.map((r) => r.time) : [...Object.values(series).flatMap((s) => s.map((p) => p[1])), ...routes.map((r) => r.time)];
  const [lo, hi] = [Math.min(...values), Math.max(...values)];
  const margin = Math.max((hi - lo) * 0.1, 300);
  const [ylo, yhi] = [lo - margin, hi + margin];
  const x = (s: number) => pad.l + ((Math.min(s, x1) - x0) / (x1 - x0)) * (W - pad.l - pad.r);
  const y = (v: number) => pad.t + ((yhi - v) / (yhi - ylo)) * (H - pad.t - pad.b);
  const now = Math.max(x0, Math.min(elapsed, x1));
  // The kept route last, on top of the others.
  const drawn = [...routes].sort((a, b) => Number(a.id === selected) - Number(b.id === selected));
  const path = (points: [number, number][]) =>
    points.map(([px, py], i) => `${i ? `H${x(px).toFixed(1)}V` : `M${x(px).toFixed(1)},`}${y(py).toFixed(1)}`).join("") + `H${x(now).toFixed(1)}`;

  const onMove = (e: React.MouseEvent<SVGSVGElement>) => {
    const box = plot.current?.getBoundingClientRect();
    if (!box || qualifying) return;
    const sx = ((e.clientX - box.left) / box.width) * W;
    const s = x0 + ((sx - pad.l) / (W - pad.l - pad.r)) * (x1 - x0);
    setHover(s >= x0 && s <= now ? s : null);
  };
  const tip =
    hover == null
      ? []
      : routes
          .map((r) => ({ r, v: valueAt(series[r.id] ?? [], hover) }))
          .filter((e): e is { r: RaceRoute; v: number } => e.v != null)
          .sort((a, b) => a.v - b.v);

  return (
    <figure className="race-chart">
      <figcaption>
        {qualifying
          ? t("Départ des {n} meilleurs chemins", { n: routes.length })
          : t("Temps des {n} chemins au fil de l'optimisation", { n: routes.length })}
      </figcaption>
      {legend}
      <div className="chart-plot" ref={frame}>
        <svg ref={plot} width={W} height={H} viewBox={`0 0 ${W} ${H}`} role="img" aria-label={byTime.map((r) => fmtTime(r.time)).join(", ")} onMouseMove={onMove} onMouseLeave={() => setHover(null)}>
          {ticks(ylo, yhi).map((v) => (
            <g key={v}>
              <line x1={pad.l} x2={W - pad.r} y1={y(v)} y2={y(v)} className="grid" />
              <text x={pad.l - 6} y={y(v)} textAnchor="end" dominantBaseline="middle" className="axis">
                {fmtTime(v)}
              </text>
            </g>
          ))}
          <text x={pad.l} y={H - 6} className="axis">
            {Math.round(x0)} s
          </text>
          <text x={W - pad.r} y={H - 6} textAnchor="end" className="axis">
            {Math.round(x1)} s
          </text>
          {qualifying && (
            <text x={(W + pad.l) / 2} y={H / 2} textAnchor="middle" className="axis race-wait">
              {t("Les courbes démarrent quand les {n} meilleurs chemins sont choisis", { n: routes.length })}
            </text>
          )}
          {!qualifying &&
            drawn.map((r) => {
              const points = series[r.id];
              if (!points?.length) return null;
              return <path key={r.id} d={path(points)} className={`race-line${r.id === selected ? " selected" : ""}`} style={{ stroke: color(colors[r.id]) }} />;
            })}
          {drawn.map((r) => (
            <circle key={r.id} cx={x(qualifying ? x0 : now)} cy={y(r.time)} r={r.id === selected ? 5 : 4} className="race-dot" style={{ fill: color(colors[r.id]) }}>
              <title>{fmtTime(r.time)}</title>
            </circle>
          ))}
          {hover != null && <line x1={x(hover)} x2={x(hover)} y1={pad.t} y2={H - pad.b} className="crosshair" />}
        </svg>
        {tip.length > 0 && hover != null && (
          <div className="tooltip" style={{ left: `${(x(hover) / W) * 100}%` }}>
            <div className="muted">{t("à {n} s", { n: Math.round(hover) })}</div>
            {tip.map(({ r, v }) => (
              <div key={r.id}>
                <span className="swatch" style={{ background: color(colors[r.id]) }} /> {r.id === selected ? <strong>{fmtTime(v)}</strong> : fmtTime(v)}
              </div>
            ))}
          </div>
        )}
      </div>
      {explanation}
    </figure>
  );
}
