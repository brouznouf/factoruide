import { useMemo, useRef, useState } from "react";
import { fmtTime, type Step } from "../api";
import { useT } from "../i18n";
import { useWidth } from "./useWidth";

const H = 260;
const PAD = { left: 40, right: 16, top: 12, bottom: 28 };

/** Character level over game time, with a crosshair tooltip. */
export function LevelChart({ steps, fromLevel, toLevel }: { steps: Step[]; fromLevel: number; toLevel: number }) {
  const t = useT();
  const svg = useRef<SVGSVGElement>(null);
  const [hover, setHover] = useState<number | null>(null);
  const [frame, W] = useWidth(720);

  const points = useMemo(() => [{ time: 0, level: fromLevel, step: null as Step | null }, ...steps.map((s) => ({ time: s.time, level: s.level, step: s }))], [steps, fromLevel]);
  const maxTime = Math.max(1, points[points.length - 1]?.time ?? 1);
  const x = (time: number) => PAD.left + (time / maxTime) * (W - PAD.left - PAD.right);
  const y = (l: number) => PAD.top + (1 - (l - fromLevel) / Math.max(1, toLevel - fromLevel)) * (H - PAD.top - PAD.bottom);

  // Step line: level only changes at the moment it is reached.
  const path = points.map((p, i) => (i === 0 ? `M${x(p.time)},${y(p.level)}` : `H${x(p.time)}V${y(p.level)}`)).join("");

  const hours = Math.ceil(maxTime / 3600);
  const hourStep = hours > 60 ? 20 : hours > 24 ? 10 : hours > 8 ? 2 : 1;
  const levelStep = toLevel - fromLevel > 30 ? 10 : 5;

  const onMove = (e: React.PointerEvent) => {
    const rect = svg.current!.getBoundingClientRect();
    const at = (((e.clientX - rect.left) / rect.width) * W - PAD.left) / (W - PAD.left - PAD.right) * maxTime;
    let best = 0;
    for (let i = 0; i < points.length; i++) if (points[i].time <= at) best = i;
    setHover(best);
  };
  const h = hover !== null ? points[hover] : null;

  return (
    <figure className="chart">
      <figcaption>{t("Niveau selon le temps de jeu")}</figcaption>
      <div className="chart-plot" ref={frame}>
        <svg ref={svg} width={W} height={H} viewBox={`0 0 ${W} ${H}`} onPointerMove={onMove} onPointerLeave={() => setHover(null)} role="img" aria-label={t("Niveau selon le temps de jeu")}>
          {Array.from({ length: Math.floor(hours / hourStep) + 1 }, (_, i) => i * hourStep).map((hr) => (
            <g key={`x${hr}`}>
              <line className="grid" x1={x(hr * 3600)} x2={x(hr * 3600)} y1={PAD.top} y2={H - PAD.bottom} />
              <text className="axis" x={x(hr * 3600)} y={H - 8} textAnchor="middle">
                {hr}h
              </text>
            </g>
          ))}
          {[fromLevel, ...Array.from({ length: Math.floor(toLevel / levelStep) }, (_, i) => (i + 1) * levelStep).filter((l) => l > fromLevel && l <= toLevel)].map((lvl) => (
            <g key={`y${lvl}`}>
              <line className="grid" x1={PAD.left} x2={W - PAD.right} y1={y(lvl)} y2={y(lvl)} />
              <text className="axis" x={PAD.left - 6} y={y(lvl) + 4} textAnchor="end">
                {lvl}
              </text>
            </g>
          ))}
          <path d={path} className="series-line" />
          {h && (
            <g>
              <line className="crosshair" x1={x(h.time)} x2={x(h.time)} y1={PAD.top} y2={H - PAD.bottom} />
              <circle cx={x(h.time)} cy={y(h.level)} r={5} className="series-dot" />
            </g>
          )}
        </svg>
        {h && (
          <div className="tooltip" style={{ left: `${(x(h.time) / W) * 100}%` }}>
            <strong>
              {t("{time} · niveau {level}", { time: fmtTime(h.time), level: h.level })}
            </strong>
            {h.step && <span>{h.step.text}</span>}
          </div>
        )}
      </div>
    </figure>
  );
}
