import { useMemo, useState } from "react";
import { fmtTime, type Route } from "../api";
import { BreakdownBars } from "./BreakdownBars";
import { LevelChart } from "./LevelChart";
import { useT } from "../i18n";

const KIND_LABELS: Record<string, string> = {
  accept: "Prendre",
  objective: "Faire",
  turnin: "Rendre",
  fly: "Vol",
  flight_master: "Point de vol",
  link: "Transport",
  grind: "Farmer",
  train: "Entraîneur",
  hearth: "Pierre de foyer",
  bind: "Auberge",
  dungeon: "Donjon",
  profession: "Métier",
  practice: "Pratique",
};

interface Segment {
  zone: string;
  start: number;
  end: number;
  fromLevel: number;
  toLevel: number;
  steps: number;
}

/** Consecutive steps in the same zone. */
function segments(route: Route): Segment[] {
  const out: Segment[] = [];
  let previousTime = 0;
  for (const s of route.steps) {
    const last = out[out.length - 1];
    // Steps done anywhere (practicing a profession, abandoning a quest) have no place: they
    // belong to the zone the character is in.
    const zone = s.zone ?? last?.zone ?? "?";
    if (last && last.zone === zone) {
      last.end = s.time;
      last.toLevel = s.level;
      last.steps++;
    } else {
      out.push({ zone, start: previousTime, end: s.time, fromLevel: s.level, toLevel: s.level, steps: 1 });
    }
    previousTime = s.time;
  }
  // Fold short passages (a couple of steps, a few minutes) into the previous segment.
  const merged: Segment[] = [];
  for (const s of out) {
    const last = merged[merged.length - 1];
    if (last && s.steps <= 2 && s.end - s.start < 300) {
      last.end = s.end;
      last.toLevel = s.toLevel;
      last.steps += s.steps;
    } else if (last && last.zone === s.zone) {
      last.end = s.end;
      last.toLevel = s.toLevel;
      last.steps += s.steps;
    } else {
      merged.push({ ...s });
    }
  }
  return merged;
}

export function RouteView({ route, notes }: { route: Route; notes?: string[] }) {
  const t = useT();
  const [filter, setFilter] = useState("");
  const [tab, setTab] = useState<"zones" | "steps">("zones");
  const b = route.breakdown;
  const segs = useMemo(() => segments(route), [route]);
  const steps = useMemo(() => {
    const q = filter.trim().toLowerCase();
    return route.steps
      .map((s, i) => ({ ...s, n: i + 1, start: i > 0 ? route.steps[i - 1].time : 0 }))
      .filter((s) => !q || s.text.toLowerCase().includes(q) || (s.zone ?? "").toLowerCase().includes(q) || t(KIND_LABELS[s.kind] ?? s.kind).toLowerCase().includes(q));
  }, [route, filter, t]);

  return (
    <div className="route-view">
      <div className="tiles">
        <div className="tile tile-hero">
          <span>{t("Temps de jeu estimé")}</span>
          <strong>{route.total_time_text}</strong>
          <small>
            {t("niveau {from} → {to}", { from: route.from_level, to: route.to_level })}
          </small>
        </div>
        <div className="tile">
          <span>{t("Quêtes")}</span>
          <strong>{route.quests}</strong>
          <small>{t("{n} étapes", { n: route.steps.length })}</small>
        </div>
        <div className="tile">
          <span>{t("Donjons")}</span>
          <strong>{b.dungeon_runs}</strong>
          <small>{fmtTime(b.dungeons)}</small>
        </div>
        <div className="tile">
          <span>{t("Pierres de foyer")}</span>
          <strong>{b.hearths}</strong>
          <small>{t("quêtes de classe manquées : {n}", { n: b.missing_class_quests })}</small>
        </div>
        <div className="tile">
          <span>XP</span>
          <strong>{Math.round((b.quest_xp + b.mob_xp + b.grind_xp + (b.farm_xp ?? 0) + (b.explore_xp ?? 0)) / 1000)}k</strong>
          <small>
            {t("quêtes {q}k · mobs {m}k · farm {g}k · exploration {e}k", { q: Math.round(b.quest_xp / 1000), m: Math.round(b.mob_xp / 1000), g: Math.round(b.grind_xp / 1000), e: Math.round((b.explore_xp ?? 0) / 1000) })}
            {(b.farm_xp ?? 0) > 0 && t(" · farm en chemin {f}k", { f: Math.round((b.farm_xp ?? 0) / 1000) })}
          </small>
        </div>
      </div>

      <div className="charts">
        <LevelChart steps={route.steps} fromLevel={route.from_level} toLevel={route.to_level} />
        <BreakdownBars breakdown={b} />
      </div>

      {notes && notes.length > 0 && (
        <details className="notes">
          <summary>{t("Notes du calcul ({n})", { n: notes.length })}</summary>
          <ul>
            {notes.map((n, i) => (
              <li key={i}>{n}</li>
            ))}
          </ul>
        </details>
      )}

      <div className="tabs">
        <button className={tab === "zones" ? "active" : ""} onClick={() => setTab("zones")}>
          {t("Parcours par zone")}
        </button>
        <button className={tab === "steps" ? "active" : ""} onClick={() => setTab("steps")}>
          {t("Toutes les étapes")}
        </button>
      </div>

      {tab === "zones" ? (
        <table className="table">
          <thead>
            <tr>
              <th>{t("Début")}</th>
              <th>{t("Durée")}</th>
              <th>{t("Niveaux")}</th>
              <th>{t("Zone")}</th>
              <th>{t("Étapes")}</th>
            </tr>
          </thead>
          <tbody>
            {segs.map((s, i) => (
              <tr key={i}>
                <td>{fmtTime(s.start)}</td>
                <td>{fmtTime(s.end - s.start)}</td>
                <td>
                  {s.fromLevel === s.toLevel ? s.fromLevel : `${s.fromLevel} → ${s.toLevel}`}
                </td>
                <td>{s.zone}</td>
                <td>{s.steps}</td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : (
        <>
          <input className="search" placeholder={t("Filtrer (quête, zone, type…)")} value={filter} onChange={(e) => setFilter(e.target.value)} />
          <table className="table">
            <thead>
              <tr>
                <th>#</th>
                <th>{t("Début")}</th>
                <th>{t("Durée")}</th>
                <th>{t("Niv.")}</th>
                <th>{t("Type")}</th>
                <th>{t("Zone")}</th>
                <th>{t("Étape")}</th>
                <th>{t("Coords")}</th>
              </tr>
            </thead>
            <tbody>
              {steps.slice(0, 2000).map((s) => (
                <tr key={s.n} className={`kind-${s.kind}`}>
                  <td>{s.n}</td>
                  <td>{fmtTime(s.start)}</td>
                  <td className={s.time - s.start >= 1800 ? "long" : ""}>{fmtTime(s.time - s.start)}</td>
                  <td>{s.level}</td>
                  <td>
                    <span className="kind">{t(KIND_LABELS[s.kind] ?? s.kind)}</span>
                  </td>
                  <td>{s.zone}</td>
                  <td>
                    {s.text}
                    {s.bg && <span className="along"> {t("en chemin")}</span>}
                  </td>
                  <td>{s.map ? `${s.map.x.toFixed(1)}, ${s.map.y.toFixed(1)}` : ""}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
    </div>
  );
}
