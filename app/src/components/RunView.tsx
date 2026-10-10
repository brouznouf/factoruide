import { useEffect, useRef, useState } from "react";
import { fmtTime } from "../api";
import { fmtGain, type Gain, type OptimizeState } from "./OptimizeProgress";
import { useT } from "../i18n";
import { WorldMap, type ContinentView, type WorldPoint } from "./WorldMap";

/** A number of seconds shown as h:mm, sliding smoothly to new values. */
function AnimatedTime({ value }: { value: number }) {
  const [shown, setShown] = useState(value);
  const from = useRef(value);
  useEffect(() => {
    const reduce = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    const start = performance.now();
    const a = from.current;
    if (reduce || !Number.isFinite(a)) {
      setShown(value);
      from.current = value;
      return;
    }
    let frame = 0;
    const tick = (now: number) => {
      const t = Math.min(1, (now - start) / 600);
      const v = a + (value - a) * (1 - (1 - t) ** 3);
      setShown(v);
      from.current = v;
      if (t < 1) frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [value]);
  return <>{fmtTime(shown)}</>;
}

/** Best route time over the optimization (one series: no legend, the title names it). */
function BestChart({ history, budget }: { history: [number, number][]; budget: number }) {
  const t = useT();
  if (history.length < 2) return <div className="best-chart empty-chart">{t("La courbe apparaît dès les premières améliorations…")}</div>;
  const W = 360;
  const H = 190;
  const pad = { l: 50, r: 10, t: 10, b: 22 };
  const ys = history.map((h) => h[1]);
  const [lo, hi] = [Math.min(...ys), Math.max(...ys)];
  const span = Math.max(hi - lo, 600);
  const x = (s: number) => pad.l + (s / Math.max(budget, history[history.length - 1][0], 1)) * (W - pad.l - pad.r);
  const y = (v: number) => pad.t + ((hi - v) / span) * (H - pad.t - pad.b);
  const line = history.map((h, i) => `${i ? "L" : "M"}${x(h[0]).toFixed(1)},${y(h[1]).toFixed(1)}`).join("");
  const last = history[history.length - 1];
  const area = `${line}L${x(last[0]).toFixed(1)},${H - pad.b}L${x(history[0][0]).toFixed(1)},${H - pad.b}Z`;
  const ticks = [hi, (hi + lo) / 2, lo];
  return (
    <figure className="best-chart">
      <figcaption>{t("Meilleur temps du guide au fil de l'optimisation")}</figcaption>
      <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label={t("De {from} à {to}", { from: fmtTime(history[0][1]), to: fmtTime(last[1]) })}>
        {ticks.map((tick) => (
          <g key={tick}>
            <line x1={pad.l} x2={W - pad.r} y1={y(tick)} y2={y(tick)} className="grid" />
            <text x={pad.l - 6} y={y(tick)} textAnchor="end" dominantBaseline="middle" className="axis">
              {fmtTime(tick)}
            </text>
          </g>
        ))}
        <text x={pad.l} y={H - 6} className="axis">0 s</text>
        <text x={W - pad.r} y={H - 6} textAnchor="end" className="axis">
          {Math.round(Math.max(budget, last[0]))} s
        </text>
        <path d={area} className="series-area" />
        <path d={line} className="series-line" />
        <circle cx={x(last[0])} cy={y(last[1])} r={4} className="series-dot" />
      </svg>
    </figure>
  );
}

/** One line on what the planner is doing. */
function phaseText(state: OptimizeState | null, t: ReturnType<typeof useT>): string {
  if (!state || state.phase === "prepare") {
    return state?.step === "quests" ? t("Chargement des quêtes…") : t("Chargement de la carte…");
  }
  if (state.phase === "construct") return t("Construction des routes de départ…");
  const left = Math.ceil(Math.max(0, (state.budget ?? 0) - (state.elapsed ?? 0)));
  if (state.stage === "qualify") {
    return t("Sélection des meilleurs départs · {done}/{n} essayés · encore {s} s", {
      done: state.qualified ?? 0,
      n: state.candidates ?? 0,
      s: left,
    });
  }
  if (state.stage === "final") {
    return t("Optimisation des {k} meilleurs départs · encore {s} s", { k: state.finalists ?? 0, s: left });
  }
  return t("Optimisation · encore {n} s", { n: left });
}

interface Props {
  state: OptimizeState | null;
  gains: Gain[];
  history: [number, number][];
  route: WorldPoint[];
  log: string[];
}

export function RunView({ state, gains, history, route, log }: Props) {
  const t = useT();
  const [continent, setContinent] = useState<ContinentView>("both");
  const [showLog, setShowLog] = useState(false);

  const optimizing = state?.phase === "optimize";
  const ratio = optimizing && state.budget ? Math.min(1, (state.elapsed ?? 0) / state.budget) : 0;
  const best = state?.best ?? null;
  const saved = state?.start != null && best != null ? state.start - best : 0;
  const latest = gains[gains.length - 1];

  return (
    <div className="run-view">
      <header className="run-hero">
        <div>
          <div className="run-phase">{phaseText(state, t)}</div>
          <div className="run-best">{best != null ? <AnimatedTime value={best} /> : "—"}</div>
          <div className="run-sub">
            {best != null && state?.start != null ? (
              <>
                {t("meilleur guide · départ {time}", { time: fmtTime(state.start) })}
                {saved >= 1 && <span className="run-saved"> · {t("gagné {time}", { time: fmtGain(saved).slice(1) })}</span>}
              </>
            ) : (
              t("construction en cours")
            )}
          </div>
        </div>
        {latest && (
          <div className="run-toast" key={gains.length} aria-live="polite">
            {t("Temps gagné")} <strong>{fmtGain(latest.saved)}</strong>
          </div>
        )}
      </header>
      <div className={`progress big${optimizing ? "" : " indeterminate"}`} role="progressbar" aria-valuenow={Math.round(ratio * 100)}>
        <div style={optimizing ? { width: `${ratio * 100}%` } : undefined} />
      </div>
      <div className="run-grid">
        <div className="panel">
          <WorldMap continent={continent} onContinent={setContinent} route={route} live height={560} />
        </div>
        <div className="run-side">
          <div className="panel">
            <BestChart history={history} budget={state?.budget ?? 60} />
          </div>
          <div className="panel">
            <h3>{t("Améliorations")}</h3>
            <ul className="gains">
              {gains.length === 0 && <li>{t("En attente des premières améliorations…")}</li>}
              {gains
                .slice(-8)
                .reverse()
                .map((g, i) => (
                  <li key={`${g.at}-${gains.length - i}`} className={i === 0 ? "latest" : undefined}>
                    {t("Temps gagné")} <strong>{fmtGain(g.saved)}</strong> <small>{t("à {n} s", { n: Math.round(g.at) })}</small>
                  </li>
                ))}
            </ul>
          </div>
        </div>
      </div>
      <button className="link" onClick={() => setShowLog(!showLog)}>
        {showLog ? t("Masquer le journal") : t("Afficher le journal")}
      </button>
      {showLog && <pre className="log">{log.join("\n") || t("Démarrage…")}</pre>}
    </div>
  );
}
