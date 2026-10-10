import { useEffect, useRef, useState } from "react";
import { api, type GuideVersion, type Options, type PlanRequest } from "../api";
import { ConfigForm } from "../components/ConfigForm";
import type { Gain, OptimizeState } from "../components/OptimizeProgress";
import { ResultView } from "../components/ResultView";
import { RunView } from "../components/RunView";
import type { WorldPoint } from "../components/WorldMap";
import { gameLocale, useLang, useT } from "../i18n";

export interface WizardStart {
  /** Configuration to start from. */
  initial: PlanRequest | null;
  /** Compute `initial` again at once (a new version of its guide). */
  rerun: boolean;
}

type Step = "config" | "run" | "result";

const STEPS: { id: Step; label: string }[] = [
  { id: "config", label: "1. Configuration" },
  { id: "run", label: "2. Génération" },
  { id: "result", label: "3. Résultat" },
];

/** New route in three steps: configure, watch the generation live, then the saved result. */
export function NewRoutePage({ options, start, onBusy }: { options: Options; start: WizardStart; onBusy?: (busy: boolean) => void }) {
  const t = useT();
  const lang = useLang();
  const [step, setStep] = useState<Step>("config");
  const [initial, setInitial] = useState<PlanRequest | null>(start.initial);
  const [optimize, setOptimize] = useState<OptimizeState | null>(null);
  const [gains, setGains] = useState<Gain[]>([]);
  const [history, setHistory] = useState<[number, number][]>([]);
  const [route, setRoute] = useState<WorldPoint[]>([]);
  const [log, setLog] = useState<string[]>([]);
  const [result, setResult] = useState<GuideVersion | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const lastBest = useRef<number | null>(null);
  const running = useRef(false);

  useEffect(() => {
    const off = api.onPlanProgress((m) => {
      if (m.startsWith("@progress ")) {
        const next = JSON.parse(m.slice(10)) as OptimizeState;
        const prev = lastBest.current;
        if (next.best != null) {
          if (prev != null && next.best < prev - 0.5) setGains((g) => [...g.slice(-50), { saved: prev - next.best!, at: next.elapsed ?? 0 }]);
          if (prev == null || next.best < prev - 0.5) setHistory((h) => [...h, [next.elapsed ?? 0, next.best!]]);
          lastBest.current = next.best;
        }
        setOptimize(next);
      } else if (m.startsWith("@route ")) {
        setRoute((JSON.parse(m.slice(7)) as { points: WorldPoint[] }).points);
      } else {
        setLog((l) => [...l.slice(-300), m]);
      }
    });
    return () => {
      off.then((f) => f());
    };
  }, []);

  const run = async (request: PlanRequest) => {
    if (running.current) return;
    running.current = true;
    onBusy?.(true);
    setInitial(request);
    setStep("run");
    setError(null);
    setMessage(null);
    setOptimize(null);
    setGains([]);
    setHistory([]);
    setRoute([]);
    setLog([]);
    lastBest.current = null;
    try {
      const meta = await api.planRoute(request);
      setResult(await api.getVersion(meta.guide, meta.version));
      setStep("result");
    } catch (e) {
      setError(String(e));
      setStep("config");
    } finally {
      running.current = false;
      onBusy?.(false);
    }
  };

  // Recomputing a saved result starts right away.
  useEffect(() => {
    if (start.rerun && start.initial) run(start.initial);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="wizard">
      <ol className="wizard-steps">
        {STEPS.map((s) => (
          <li key={s.id} className={s.id === step ? "active" : STEPS.findIndex((x) => x.id === step) > STEPS.findIndex((x) => x.id === s.id) ? "done" : ""}>
            {s.id === "config" && step === "result" ? (
              <button className="link" onClick={() => setStep("config")}>
                {t(s.label)}
              </button>
            ) : (
              t(s.label)
            )}
          </li>
        ))}
      </ol>
      {error && <div className="error">{error}</div>}
      {message && <div className="success">{message}</div>}
      {step === "config" && (
        <ConfigForm
          key={JSON.stringify(initial)}
          options={options}
          initial={initial}
          defaultLocale={gameLocale(lang)}
          onRun={(r) => run(r)}
          onExport={async (r) => {
            try {
              setMessage(t("Configuration exportée : {path}", { path: await api.exportRequest(r) }));
            } catch (e) {
              setError(String(e));
            }
          }}
        />
      )}
      {step === "run" && <RunView state={optimize} gains={gains} history={history} route={route} log={log} />}
      {step === "result" && result && (
        <ResultView
          key={`${result.meta.guide}-${result.meta.version}`}
          version={result}
          onOpenVersion={(n) => api.getVersion(result.meta.guide, n).then(setResult)}
          onRerun={(v) => run(v.request)}
          onEdit={(r) => {
            setInitial(r.request);
            setStep("config");
          }}
        />
      )}
    </div>
  );
}
