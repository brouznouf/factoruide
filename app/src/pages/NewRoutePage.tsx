import { useEffect, useRef, useState } from "react";
import { api, type Options, type PlanRequest, type StartCandidate, type StartOption, type VersionMeta } from "../api";
import { ConfigForm } from "../components/ConfigForm";
import { RACE_COLORS, type OptimizeState, type RaceMark, type RaceSeries } from "../components/OptimizeProgress";
import { RunView } from "../components/RunView";
import { MIN_CHOICE_LEVEL } from "../paramSchema";
import { StartsChoice } from "../components/StartsChoice";
import type { WorldPoint } from "../components/WorldMap";
import { gameLocale, useLang, useT } from "../i18n";

export interface WizardStart {
  /** Configuration to start from. */
  initial: PlanRequest | null;
  /** Compute `initial` again at once (a new version of its guide). */
  rerun: boolean;
  /** The recorded character ("Name-Realm") a new guide starts from. */
  character?: string;
}

type Step = "config" | "preview" | "starts" | "run";

const COMPARE_KEY = "fg.compareStarts";
/** Whether the starts are compared before a guide is computed (on unless turned off). */
const compareStartsSaved = () => localStorage.getItem(COMPARE_KEY) !== "off";

const STEPS: { id: Step; label: string }[] = [
  { id: "config", label: "Configuration" },
  { id: "starts", label: "Départ" },
  { id: "run", label: "Génération" },
];

interface Props {
  options: Options;
  start: WizardStart;
  onBusy?: (busy: boolean) => void;
  /** The configuration as it is now, after each change (kept when switching game versions). */
  onDraft?: (request: PlanRequest) => void;
  /** The guide is saved: show it (with a notice when the optimization was stopped early), in the editor when created by hand. */
  onDone: (meta: VersionMeta, notice: string | null, edit?: boolean) => void;
  /** Back to the dungeon guide page with a dungeon guide's configuration (cancelled or refused). */
  onDungeon?: (request: PlanRequest) => void;
}

/**
 * New route in two steps: configure, then watch the generation live; the saved guide opens at the end.
 * Created by hand instead, the guide is saved with no step and opens in the editor.
 */
export function NewRoutePage({ options, start, onBusy, onDraft, onDone, onDungeon }: Props) {
  const t = useT();
  const lang = useLang();
  const [step, setStep] = useState<Step>("config");
  const [compareStarts, setCompareStarts] = useState(compareStartsSaved);
  // The starts to choose from, and how far their comparison is.
  const [starts, setStarts] = useState<StartOption[]>([]);
  const [candidates, setCandidates] = useState<StartCandidate[]>([]);
  // Every start is computed; the start chosen before they all were.
  const [startsDone, setStartsDone] = useState(false);
  const pickedEarly = useRef<number | null>(null);
  const [tried, setTried] = useState(0);
  // The start chosen when the guide was computed before (a rerun), marked on the choice.
  const [previousStart, setPreviousStart] = useState(0);
  const [startDetails, setStartDetails] = useState<string[]>([]);
  const [initial, setInitial] = useState<PlanRequest | null>(start.initial);
  const [optimize, setOptimize] = useState<OptimizeState | null>(null);
  // Time of each followed route over the optimization, and its color slot.
  const [series, setSeries] = useState<RaceSeries>({});
  const [marks, setMarks] = useState<RaceMark[]>([]);
  const [colors, setColors] = useState<Record<number, number>>({});
  const [route, setRoute] = useState<WorldPoint[]>([]);
  const [log, setLog] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  // Stop (keep the best guide so far) or cancel asked, until the generation ends.
  const [interrupting, setInterrupting] = useState<"stop" | "cancel" | null>(null);
  const interrupted = useRef<"stop" | "cancel" | null>(null);
  const slots = useRef(new Map<number, number>());
  const running = useRef(false);

  useEffect(() => {
    const off = api.onPlanProgress((m) => {
      if (m.startsWith("@progress ")) {
        const next = JSON.parse(m.slice(10)) as OptimizeState;
        const routes = next.routes ?? [];
        // Color follows the route: a route keeps its slot while shown, a new one takes a free slot.
        const shown = new Set(routes.map((r) => r.id));
        for (const id of [...slots.current.keys()]) if (!shown.has(id)) slots.current.delete(id);
        for (const r of routes) {
          if (slots.current.has(r.id)) continue;
          const used = new Set(slots.current.values());
          const free = [...Array(RACE_COLORS).keys()].find((k) => !used.has(k));
          if (free != null) slots.current.set(r.id, free);
        }
        setColors(Object.fromEntries(slots.current));
        // Each route's time is drawn against the share of the tries done, from where it is first
        // followed (a route of the rounds from the end of the race); each round marked.
        if (next.phase === "optimize" && routes.length) {
          const at = next.progress ?? 0;
          const begin = next.stage === "qualify" ? at : (next.race_end ?? 0);
          setSeries((prev) => {
            const out = { ...prev };
            for (const r of routes) {
              const points = out[r.id] ?? [];
              const last = points[points.length - 1];
              if (!last) out[r.id] = [[begin, r.time]];
              else if (Math.abs(last[1] - r.time) > 0.5) out[r.id] = [...points, [at, r.time]];
            }
            return out;
          });
        }
        if (next.phase === "optimize" && next.stage !== "qualify" && routes.length) {
          const at = next.progress ?? 0;
          const begin = next.race_end ?? 0;
          const round = next.round ?? 1;
          setMarks((prev) => (prev.some((mark) => mark.round === round) ? prev : [...prev, { round, at: prev.length ? at : begin, width: next.width ?? routes.length }]));
        }
        setOptimize(next);
      } else if (m.startsWith("@starts ")) {
        // The zones a start can be forced to: the choice is shown, each start filling in.
        setCandidates(JSON.parse(m.slice(8)) as StartCandidate[]);
        setStarts([]);
        setStartsDone(false);
        setStep((s) => (s === "preview" ? "starts" : s));
      } else if (m.startsWith("@start ")) {
        const option = JSON.parse(m.slice(7)) as StartOption;
        setStarts((prev) => [...prev.filter((o) => o.start_zone !== option.start_zone), option]);
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

  /** Compare the starts first when the character has several (a choice to make), else compute. */
  const begin = async (given: PlanRequest) => {
    if (!compareStarts || running.current || (given.start?.level ?? given.from_level) < MIN_CHOICE_LEVEL) return run(given);
    // A start chosen before is offered again among the others, not kept.
    const before = given.params?.start_zone ?? 0;
    const request: PlanRequest = given.params ? { ...given, params: { ...given.params, start_zone: 0 } } : given;
    setPreviousStart(before);
    running.current = true;
    onBusy?.(true);
    setInitial(request);
    setStep("preview");
    setError(null);
    setMessage(null);
    setNotice(null);
    setCandidates([]);
    setStarts([]);
    setStartsDone(false);
    pickedEarly.current = null;
    interrupted.current = null;
    const withStart = (zone: number): PlanRequest => (request.params ? { ...request, params: { ...request.params, start_zone: zone } } : request);
    let found: StartOption[] = [];
    let count = 0;
    let lines: string[] = [];
    try {
      const preview = await api.previewStarts(request);
      found = preview.options;
      count = preview.tried;
      lines = preview.details;
    } catch (e) {
      // Cancelled: a start chosen before the end goes on with it; else back to the configuration.
      // Any other failure: the guide is computed as usual.
      if (String(e) === "cancelled" && pickedEarly.current != null) {
        running.current = false;
        onBusy?.(false);
        return run(withStart(pickedEarly.current));
      }
      if (String(e) === "cancelled") {
        setNotice(t("Comparaison annulée : la configuration est conservée."));
        setStep("config");
        running.current = false;
        onBusy?.(false);
        return;
      }
    }
    running.current = false;
    if (pickedEarly.current != null) return run(withStart(pickedEarly.current));
    if (found.length < 2) return run(request);
    onBusy?.(false);
    setStarts((prev) => (prev.length ? prev : found));
    setTried(count);
    setStartDetails(lines);
    setStartsDone(true);
    setStep("starts");
  };

  const run = async (request: PlanRequest) => {
    if (running.current) return;
    running.current = true;
    onBusy?.(true);
    setInitial(request);
    setStep("run");
    setError(null);
    setMessage(null);
    setNotice(null);
    setInterrupting(null);
    interrupted.current = null;
    setOptimize(null);
    setSeries({});
    setMarks([]);
    setColors({});
    slots.current.clear();
    setRoute([]);
    setLog([]);
    try {
      const meta = await api.planRoute(request);
      setStep("config");
      onDone(
        meta,
        interrupted.current === "stop" ? t("Optimisation arrêtée avant la fin : voici le meilleur guide trouvé jusque-là. « Relancer le calcul » le cherche jusqu'au bout.") : null,
      );
    } catch (e) {
      // Cancelled: back to the configuration (the same one), nothing saved.
      if (String(e) === "cancelled") setNotice(t("Génération annulée : la configuration est conservée."));
      else setError(String(e));
      setStep("config");
    } finally {
      setInterrupting(null);
      running.current = false;
      onBusy?.(false);
    }
  };

  // A guide created by hand: saved with no step, it opens in the editor.
  const [creating, setCreating] = useState(false);
  const manual = async (request: PlanRequest) => {
    if (running.current) return;
    running.current = true;
    onBusy?.(true);
    setInitial(request);
    setCreating(true);
    setError(null);
    setMessage(null);
    setNotice(null);
    try {
      onDone(await api.manualRoute(request), null, true);
    } catch (e) {
      setError(String(e));
    } finally {
      setCreating(false);
      running.current = false;
      onBusy?.(false);
    }
  };

  const interrupt = (how: "stop" | "cancel") => {
    if (!running.current || interrupted.current === "cancel" || interrupted.current === how) return;
    interrupted.current = how;
    setInterrupting(how);
    (how === "stop" ? api.stopPlan() : api.cancelPlan()).catch((e) => setError(String(e)));
  };
  const cancel = () => interrupt("cancel");

  // Recomputing a saved result starts right away.
  useEffect(() => {
    if (start.rerun && start.initial) begin(start.initial);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="wizard">
      <ol className="wizard-steps">
        {STEPS.filter((s) => s.id !== "starts" || step === "preview" || step === "starts").map((s, i, shown) => {
          const current = step === "preview" ? "starts" : step;
          const done = shown.findIndex((x) => x.id === current) > i;
          return (
            <li key={s.id} className={s.id === current ? "active" : done ? "done" : ""} aria-current={s.id === current ? "step" : undefined}>
              <span className="step-num" aria-hidden>
                {done ? "✓" : i + 1}
              </span>
              {s.id === "config" && (step === "run" || step === "preview") ? (
                <button className="link" onClick={cancel} disabled={interrupting === "cancel"} title={t("Abandonne la génération et revient à la configuration")}>
                  {t(s.label)}
                </button>
              ) : (
                <span>{t(s.label)}</span>
              )}
            </li>
          );
        })}
      </ol>
      {error && <div className="error">{error}</div>}
      {message && <div className="success">{message}</div>}
      {notice && step !== "run" && <div className="notice">{notice}</div>}
      {creating && (
        <div className="notice">
          <span className="spinner" role="status" aria-hidden /> {t("Création du guide…")}
        </div>
      )}
      {/* A dungeon guide is configured on its own page. */}
      {step === "config" && initial?.dungeon != null && (
        <div className="actions">
          <button className="primary" onClick={() => onDungeon?.(initial)}>
            {t("Retour au guide de donjon")}
          </button>
        </div>
      )}
      {step === "config" && initial?.dungeon == null && (
        <ConfigForm
          key={JSON.stringify(initial)}
          options={options}
          initial={initial}
          startCharacter={initial ? undefined : start.character}
          defaultLocale={gameLocale(lang)}
          onRun={(r) => begin(r)}
          compareStarts={compareStarts}
          onCompareStarts={(on) => {
            setCompareStarts(on);
            localStorage.setItem(COMPARE_KEY, on ? "on" : "off");
          }}
          onManual={(r) => manual(r)}
          onChange={onDraft}
          onExport={async (r) => {
            try {
              setMessage(t("Configuration exportée : {path}", { path: await api.exportRequest(r) }));
            } catch (e) {
              setError(String(e));
            }
          }}
        />
      )}
      {step === "preview" && (
        <div className="notice">
          <span className="spinner" role="status" aria-hidden /> {t("Comparaison des départs…")}{" "}
          <button className="link" onClick={cancel}>
            {t("Annuler")}
          </button>
        </div>
      )}
      {step === "starts" && initial && (
        <StartsChoice
          candidates={candidates}
          results={starts}
          finished={startsDone}
          tried={tried}
          previous={previousStart}
          details={startDetails}
          zones={options.zones}
          onBack={() => (startsDone ? setStep("config") : cancel())}
          onPick={(zone) => {
            const request: PlanRequest = { ...initial, params: initial.params ? { ...initial.params, start_zone: zone } : initial.params };
            if (startsDone) return run(request);
            // Chosen while the others are still computed: they are given up, then the guide goes on.
            pickedEarly.current = zone;
            interrupt("cancel");
          }}
        />
      )}
      {step === "run" && <RunView state={optimize} series={series} marks={marks} colors={colors} route={route} log={log} onStop={() => interrupt("stop")} onCancel={cancel} interrupting={interrupting} />}
    </div>
  );
}
