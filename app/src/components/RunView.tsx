import { useEffect, useRef, useState } from "react";
import { fmtTime } from "../api";
import type { OptimizeState, RaceSeries } from "./OptimizeProgress";
import { RaceChart } from "./RaceChart";
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

/** One line on what the planner is doing. */
function phaseText(state: OptimizeState | null, t: ReturnType<typeof useT>): string {
  if (!state || state.phase === "prepare") {
    return state?.step === "quests" ? t("Chargement des quêtes…") : t("Chargement de la carte…");
  }
  if (state.phase === "construct") return t("Construction des routes de départ…");
  const left = Math.ceil(Math.max(0, (state.budget ?? 0) - (state.elapsed ?? 0)));
  if (state.stage === "qualify") {
    return t("Sélection des meilleurs départs · {done} essayés · encore {s} s", { done: state.qualified ?? 0, s: left });
  }
  if (state.stage === "final") {
    return t("Optimisation des {k} meilleurs départs · encore {s} s", { k: state.finalists ?? 0, s: left });
  }
  return t("Optimisation · encore {n} s", { n: left });
}

interface Props {
  state: OptimizeState | null;
  series: RaceSeries;
  /** Color slot of each followed route. */
  colors: Record<number, number>;
  route: WorldPoint[];
  log: string[];
}

export function RunView({ state, series, colors, route, log }: Props) {
  const t = useT();
  const [continent, setContinent] = useState<ContinentView>("both");
  const [showLog, setShowLog] = useState(false);

  const optimizing = state?.phase === "optimize";
  const ratio = optimizing && state.budget ? Math.min(1, (state.elapsed ?? 0) / state.budget) : 0;
  // The time of the route kept (best score), the best start before the routes are known.
  const kept = state?.routes?.find((r) => r.id === state.selected)?.time ?? state?.best ?? null;

  return (
    <div className="run-view">
      <header className="run-hero">
        <div>
          <div className="run-phase">{phaseText(state, t)}</div>
          <div className="run-best">{kept != null ? <AnimatedTime value={kept} /> : "—"}</div>
          <div className="run-sub">{kept != null ? t("temps du guide retenu") : t("construction en cours")}</div>
        </div>
      </header>
      <div className={`progress big${optimizing ? "" : " indeterminate"}`} role="progressbar" aria-valuenow={Math.round(ratio * 100)}>
        <div style={optimizing ? { width: `${ratio * 100}%` } : undefined} />
      </div>
      <div className="panel">
        <RaceChart state={state} series={series} colors={colors} />
      </div>
      <div className="panel">
        <WorldMap continent={continent} onContinent={setContinent} route={route} live height={520} />
      </div>
      <button className="link" onClick={() => setShowLog(!showLog)}>
        {showLog ? t("Masquer le journal") : t("Afficher le journal")}
      </button>
      {showLog && <pre className="log">{log.join("\n") || t("Démarrage…")}</pre>}
    </div>
  );
}
