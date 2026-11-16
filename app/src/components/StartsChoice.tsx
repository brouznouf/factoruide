import { useEffect, useState } from "react";
import { fmtTime, type Options, type StartCandidate, type StartOption } from "../api";
import { useAreaName, useT } from "../i18n";
import { loadImage } from "./WorldMap";

interface Props {
  /** The zones a start can be forced to, known at once, and the starts computed so far. */
  candidates: StartCandidate[];
  results: StartOption[];
  /** Every start is computed. */
  finished: boolean;
  /** How many starts were computed (those beginning in the same zone are shown once). */
  tried: number;
  /** The start chosen when the guide was computed before (a rerun): chosen again at first. */
  previous?: number;
  /** What was tried, shown on demand. */
  details: string[];
  zones: Options["zones"];
  /** The start chosen, as the zone to force (`params.start_zone`). */
  onPick: (startZone: number) => void;
  onBack: () => void;
}

/** The map of a start's zone, as a picture for its card. */
function ZonePicture({ map }: { map: number }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    if (!map) return;
    let live = true;
    loadImage(map)
      .then((s) => live && setSrc(s))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [map]);
  return <div className="start-picture">{src && <img src={src} alt="" />}</div>;
}

/**
 * The ways the guide can begin: the zones at once, each start filling in as it is computed (the
 * time of the guide it leads to and how much slower than the fastest, the score the planner ranks
 * by in a tooltip, its steps and quests). None is hidden for being slow: these guides are barely
 * improved, their times only roughly ranked. One can be chosen before the others are done; the
 * fastest is chosen until then.
 */
export function StartsChoice({ candidates, results, finished, tried, previous, details, zones, onPick, onBack }: Props) {
  const t = useT();
  const areaName = useAreaName();
  const [chosen, setChosen] = useState<number | null>(null);
  const name = (zone: number) => {
    const z = zones.find((x) => x.id === zone);
    return z ? areaName(z) : `#${zone}`;
  };
  // The starts computed, the shortest play time first, one per zone they begin in.
  const sorted = [...results].sort((a, b) => a.time - b.time || a.score - b.score);
  const fastest = sorted[0]?.time ?? 0;
  const seen = new Set<number>();
  const done: StartOption[] = [];
  for (const o of sorted) {
    if (seen.has(o.zone)) continue;
    seen.add(o.zone);
    done.push(o);
  }
  const computed = new Set(results.map((r) => r.start_zone));
  const pending = finished ? [] : candidates.filter((c) => !computed.has(c.start_zone));
  const wanted = chosen ?? (previous && done.some((o) => o.start_zone === previous) ? previous : (done[0]?.start_zone ?? null));
  return (
    <section className="panel starts">
      <h2>{t("Par où commencer ?")}</h2>
      <p className="muted">
        {t("Plusieurs départs sont possibles : chacun est un guide calculé rapidement, avant d'être optimisé à fond. Le plus rapide est choisi d'office.")}{" "}
        {finished
          ? t("{tried} départs calculés, {shown} réellement différents : ceux qui commencent dans la même zone sont regroupés.", { tried, shown: done.length })
          : t("Calcul en cours : choisissez dès maintenant, ou attendez les autres.")}
      </p>
      <div className="starts-grid" role="radiogroup" aria-label={t("Départs possibles")}>
        {done.map((s, i) => (
          <button key={s.start_zone} role="radio" aria-checked={wanted === s.start_zone} className={`start-card${wanted === s.start_zone ? " chosen" : ""}`} onClick={() => setChosen(s.start_zone)} onDoubleClick={() => onPick(s.start_zone)}>
            <ZonePicture map={s.ui_map} />
            <div className="start-body">
              <div className="start-zone">
                {name(s.zone)}
                {i === 0 && <span className="start-best">{finished ? t("Le plus rapide") : t("Le plus rapide jusqu'ici")}</span>}
                {previous ? s.start_zone === previous && <span className="start-previous">{t("Départ précédent")}</span> : null}
              </div>
              <div className="start-time">
                {fmtTime(s.time)}
                {i > 0 && fastest > 0 && <span className="start-slower"> {t("+{n} %", { n: Math.round((s.time / fastest - 1) * 100) })}</span>}
              </div>
              <div
                className="start-score"
                title={t("Temps noté : le temps sur lequel le moteur classe les guides. Il compte en plus les vols, le grind et les changements de zone, c'est pourquoi il peut différer du temps de jeu estimé.")}
              >
                {t("noté {time}", { time: fmtTime(s.score) })}
              </div>
              <div className="start-counts">{t("{steps} étapes · {quests} quêtes", { steps: s.steps.toLocaleString(), quests: s.quests.toLocaleString() })}</div>
              <ol className="start-first">
                {s.first.slice(0, 3).map((line, k) => (
                  <li key={k}>{line}</li>
                ))}
              </ol>
            </div>
          </button>
        ))}
        {pending.map((c) => (
          <button key={c.start_zone} role="radio" aria-checked={wanted === c.start_zone} className={`start-card pending${wanted === c.start_zone ? " chosen" : ""}`} onClick={() => setChosen(c.start_zone)} onDoubleClick={() => onPick(c.start_zone)}>
            <ZonePicture map={c.ui_map} />
            <div className="start-body">
              <div className="start-zone">{c.start_zone ? name(c.start_zone) : t("Choix du planificateur")}</div>
              {c.quests > 0 && <div className="start-counts">{t("{n} quêtes à faire sur place", { n: c.quests })}</div>}
              <div className="start-counts">
                <span className="spinner inline" role="status" aria-hidden /> {t("calcul en cours…")}
              </div>
            </div>
          </button>
        ))}
      </div>
      {finished && details.length > 0 && (
        <details className="starts-details">
          <summary>{t("Détail du calcul")}</summary>
          <ul>
            {details.map((line, i) => (
              <li key={i}>{line}</li>
            ))}
          </ul>
        </details>
      )}
      <div className="starts-actions">
        <button onClick={onBack}>{t("Retour")}</button>
        <button className="primary big" disabled={wanted == null} onClick={() => wanted != null && onPick(wanted)}>
          {t("Calculer le guide avec ce départ")}
        </button>
      </div>
    </section>
  );
}
