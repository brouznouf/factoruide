import type { ReactNode } from "react";
import { fmtTime, type Route } from "../api";
import { BreakdownBars } from "./BreakdownBars";
import { LevelChart } from "./LevelChart";
import { useT } from "../i18n";
import { Money } from "./Money";
import { standingOf } from "./ReputationPanel";
import { mandatoryGoalWarnings, mandatoryGoalWarningText } from "../mandatoryGoalWarnings";
import { reputationFacts } from "../reputationProgram";

/** Why the planner left quests out (`fg_route` model), English as sent, French key of the catalog. */
const SKIP_REASONS: Record<string, string> = {
  "elite objective": "objectif élite",
  "excluded by configuration": "exclue par la configuration",
  "exploration area unknown": "zone d'exploration inconnue",
  "holiday/profession/event category": "catégorie fête, métier ou événement",
  "needs a profession, reputation or spell": "demande un métier, une réputation ou un sort",
  "no reachable quest giver": "aucun donneur accessible",
  "no reachable turn-in": "aucun PNJ de rendu accessible",
  "objective item from elite mobs": "objet d'objectif sur des élites",
  "objective item source unknown": "source de l'objet d'objectif inconnue",
  "objectives and turn-in unknown (catalogue only)": "objectifs et rendu inconnus (catalogue seulement)",
  "objectives unknown (only described)": "objectifs inconnus (seulement décrits)",
  "PvP (battleground) quest": "quête JcJ (champ de bataille)",
  repeatable: "répétable",
  "reputation/spell objective": "objectif de réputation ou de sort",
  "started by a rare drop": "commence par un objet rare",
};

/** "N plannable quests, start …" then "skipped N: reason" lines (`fg_route::job::notes`). */
function Notes({ notes }: { notes: string[] }) {
  const t = useT();
  const skipped = notes.flatMap((n) => {
    const m = /^skipped (\d+): (.+)$/.exec(n);
    return m ? [{ n: Number(m[1]), reason: SKIP_REASONS[m[2]] ? t(SKIP_REASONS[m[2]]) : m[2] }] : [];
  });
  const others = notes.filter((n) => !/^skipped \d+: /.test(n));
  const plannable = others.map((n) => /^(\d+) plannable quests, start (.+)$/.exec(n)).find(Boolean);
  const where = plannable && /^where the character stands \((.+)\)$/.exec(plannable[2]);
  const total = skipped.reduce((s, r) => s + r.n, 0);
  return (
    <details className="notes">
      <summary>
        {plannable
          ? t("{n} quêtes possibles, {skipped} écartées", { n: plannable[1], skipped: total })
          : t("Notes du calcul ({n})", { n: notes.length })}
      </summary>
      {plannable && (
        <p className="muted">{where ? t("Départ : là où se trouve le personnage ({place})", { place: where[1] }) : t("Départ : {place}", { place: plannable[2].replace(/^at /, "") })}</p>
      )}
      {others
        .filter((n) => !/^\d+ plannable quests, /.test(n))
        .map((n, i) => (
          <p key={i} className="muted">
            {n}
          </p>
        ))}
      {skipped.length > 0 && (
        <table className="table compact notes-table">
          <tbody>
            {[...skipped]
              .sort((a, b) => b.n - a.n)
              .map((r) => (
                <tr key={r.reason}>
                  <td className="num">{r.n}</td>
                  <td>{r.reason}</td>
                </tr>
              ))}
          </tbody>
        </table>
      )}
    </details>
  );
}

/** Where the XP comes from, as one stacked bar and its legend. */
function XpTile({ route }: { route: Route }) {
  const t = useT();
  const b = route.breakdown;
  const parts = [
    { label: t("Quêtes"), value: b.quest_xp, color: "var(--cat-1)" },
    { label: t("Mobs"), value: b.mob_xp, color: "var(--cat-3)" },
    { label: t("Farm en chemin"), value: b.farm_xp ?? 0, color: "var(--cat-4)" },
    { label: t("Farm"), value: b.grind_xp, color: "var(--cat-2)" },
    { label: t("Exploration"), value: b.explore_xp ?? 0, color: "var(--cat-7)" },
  ].filter((p) => p.value > 0);
  const total = parts.reduce((s, p) => s + p.value, 0);
  const pct = (v: number) => Math.round((v / Math.max(1, total)) * 100);
  return (
    <div className="tile">
      <span>XP</span>
      <strong>{Math.round(total / 1000)}k</strong>
      <div className="stack" role="img" aria-label={parts.map((p) => `${p.label} ${pct(p.value)} %`).join(", ")}>
        {parts.map((p) => (
          <span key={p.label} style={{ flexGrow: p.value, background: p.color }} title={`${p.label} : ${Math.round(p.value / 1000)}k (${pct(p.value)} %)`} />
        ))}
      </div>
      <small className="stack-legend">
        {parts
          .filter((p) => pct(p.value) >= 1)
          .map((p) => (
          <span key={p.label}>
            <i className="swatch" style={{ background: p.color }} /> {p.label} {pct(p.value)} %
          </span>
        ))}
      </small>
    </div>
  );
}

interface Props {
  route: Route;
  notes?: string[];
  /** Shown under the play time (the difference with the previous version). */
  compare?: ReactNode;
}

/** A guide's figures: play time, quests, dungeons, hearthstones, XP, warnings, level over time, where the time goes, the gold of the quests, the planner's notes (the best gear has its own tab). */
export function RouteSummary({ route, notes, compare }: Props) {
  const t = useT();
  const b = route.breakdown;
  const flights = route.steps.filter((s) => s.kind === "fly").length;
  const reputation = reputationFacts(route.steps);
  return (
    <div className="route-summary">
      <div className="tiles">
        <div className="tile tile-hero">
          <span>{t("Temps de jeu estimé")}</span>
          <strong>{route.total_time_text}</strong>
          <small>
            {t("niveau {from} → {to}", { from: route.from_level, to: route.to_level })}
            {compare && <> · {compare}</>}
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
          <small>{b.dungeon_runs > 0 ? fmtTime(b.dungeons) : "—"}</small>
        </div>
        <div className="tile">
          <span>{t("Pierres de foyer")}</span>
          <strong>{b.hearths}</strong>
          <small>
            {t("{n} vols", { n: flights })}
            {flights > 0 && b.flight_cost != null && b.flight_cost > 0 && (
              <span title={t("Or dépensé en vols")}>
                {" · "}
                <Money copper={b.flight_cost} />
              </span>
            )}
          </small>
        </div>
        <XpTile route={route} />
      </div>
      {(b.gold_earned ?? 0) > 0 && (
        <div className="route-gold">
          <span>{t("Or des quêtes")}</span> <Money copper={b.gold_earned ?? 0} />
          {(b.flight_cost ?? 0) > 0 && (
            <>
              {" · "}
              {t("Vols")} <Money copper={b.flight_cost ?? 0} />
            </>
          )}
          {(b.training_cost ?? 0) > 0 && (
            <>
              {" · "}
              {t("Entraînement")} <Money copper={b.training_cost ?? 0} />
            </>
          )}
          {(b.vendor_cost ?? 0) > 0 && (
            <>
              {" · "}
              {t("Marchands")} <Money copper={b.vendor_cost ?? 0} />
            </>
          )}
        </div>
      )}
      {mandatoryGoalWarnings(b).map((warning) => (
        <div key={warning.category} className="notice warn">{mandatoryGoalWarningText(warning, t)}</div>
      ))}
      {(b.missing_zone_quests ?? 0) > 0 && <div className="notice warn">{t("Quêtes des zones forcées manquées : {n}", { n: b.missing_zone_quests ?? 0 })}</div>}
      {(route.reputation ?? []).map((r) => {
        const standing = (points: number) => {
          const s = standingOf(points);
          return `${t(s.name)} ${s.into}/${s.of}`;
        };
        const missing = r.target - r.reached;
        return (
          <div key={r.faction} className={`notice${missing > 0 ? " warn" : ""}`}>
            {t("Réputation {faction} : {start} → {reached} (objectif : {target})", {
              faction: r.name,
              start: standing(r.start),
              reached: standing(r.reached),
              target: t(standingOf(r.target).name),
            })}
            {missing > 0 && ` — ${t("{n} points manquants dans ce guide : l'objectif n'est pas atteint", { n: missing })}`}
          </div>
        );
      })}
      {reputation && (
        <section aria-label={t("Programme de réputation")}>
          <h3>{t("Programme de réputation")}</h3>
          <p>{t("Jeu actif estimé pour la réputation : {time}", { time: fmtTime(reputation.active_play_seconds) })}</p>
          {reputation.daily_opportunities > 0 && (
            <>
              <p>{t("Au moins {days} jours de quêtes journalières, avec {resets} réinitialisations du serveur.", { days: reputation.daily_opportunities, resets: reputation.reset_transitions })}</p>
              <p className="muted">{t("Chaque circuit fait toutes ses sources avant d'attendre le prochain reset. Ces fenêtres minimales ne donnent pas de date de fin.")}</p>
              <p className="muted">{reputation.calendar_wait_seconds == null ? t("Attente calendaire inconnue : le premier jour disponible et l'heure du reset restent à vérifier.") : t("Attente calendaire estimée, hors jeu actif : {time}", { time: fmtTime(reputation.calendar_wait_seconds) })}</p>
            </>
          )}
          {reputation.estimates && <p className="notice warn">{t("Estimation Classic : les gains prévus ne sont pas garantis. L'addon vérifie vos sacs et vos réputations réelles.")}</p>}
          {reputation.warnings.length > 0 && (
            <details className="notes">
              <summary>{t("Notes du calcul ({n})", { n: reputation.warnings.length })}</summary>
              {reputation.warnings.map((warning) => <p className="muted" key={warning}>{warning}</p>)}
            </details>
          )}
        </section>
      )}

      <div className="charts">
        <LevelChart steps={route.steps} fromLevel={route.from_level} toLevel={route.to_level} />
        <BreakdownBars breakdown={b} />
      </div>

      {notes && notes.length > 0 && <Notes notes={notes} />}
    </div>
  );
}
