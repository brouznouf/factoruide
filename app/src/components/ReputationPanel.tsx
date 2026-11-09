import { useId } from "react";
import { type FactionOption, type ReputationGoal, type StartState } from "../api";
import { useT } from "../i18n";
import { Hint } from "./Hint";

/** The game's standings, by the points they start at (counted from the bottom of Neutral). */
export const STANDINGS: [number, string][] = [
  [-42000, "Haï"],
  [-6000, "Hostile"],
  [-3000, "Inamical"],
  [0, "Neutre"],
  [3000, "Amical"],
  [9000, "Honoré"],
  [21000, "Révéré"],
  [42000, "Exalté"],
];

/** The standings a goal can aim at. */
const TARGETS = STANDINGS.slice(4);

/** A standing and the points into it ("Amical 1000/6000"); Exalted has 999 points. */
export function standingOf(points: number): { name: string; into: number; of: number } {
  let k = 0;
  while (k + 1 < STANDINGS.length && points >= STANDINGS[k + 1][0]) k++;
  const [from, name] = STANDINGS[k];
  const to = k + 1 < STANDINGS.length ? STANDINGS[k + 1][0] : from + 999;
  return { name, into: points - from, of: to - from };
}

interface Props {
  factions: FactionOption[];
  raceId: number | undefined;
  /** The race's side ("Alliance", "Horde"): the factions of the other side are not offered. */
  side: string | undefined;
  /** The character met in game the guide starts from. */
  start: StartState | null;
  goals: ReputationGoal[];
  onChange: (goals: ReputationGoal[]) => void;
  estimates: boolean;
  onEstimatesChange: (allow: boolean) => void;
}

/** Reputation goals: factions the guide does quests for until the standing aimed at (none by default). */
export function ReputationPanel({ factions, raceId, side, start, goals, onChange, estimates, onEstimatesChange }: Props) {
  const t = useT();
  const estimateHelp = useId();
  const offered = factions.filter((f) => !f.side || f.side === side);
  const startOf = (id: number) => start?.reputation?.find((r) => r.id === id)?.value ?? (raceId != null ? offered.find((f) => f.id === id)?.starts[raceId] : undefined) ?? 0;
  const standingText = (points: number) => {
    const s = standingOf(points);
    return `${t(s.name)} ${s.into}/${s.of}`;
  };
  const add = () => {
    const faction = offered.find((f) => !goals.some((g) => g.faction === f.id));
    if (!faction) return;
    // The next standing above where the character starts.
    const target = TARGETS.find(([points]) => points > startOf(faction.id))?.[0] ?? 42000;
    onChange([...goals, { faction: faction.id, target }]);
  };
  const update = (i: number, goal: ReputationGoal) => onChange(goals.map((g, j) => (j === i ? goal : g)));

  return (
    <section className="panel">
      <h3>
        {t("Réputation")}
        <Hint text={t("Le guide cherche à améliorer les réputations choisies, même après le niveau visé. Un objectif inaccessible ne bloque pas le guide : le résultat indique les points manquants et les limites des sources. Aucun par défaut.")} />
      </h3>
      <label className={`toggle${estimates ? " on" : ""}`}>
        <input type="checkbox" checked={estimates} aria-describedby={estimateHelp} onChange={(e) => onEstimatesChange(e.target.checked)} />
        <span className="toggle-switch" aria-hidden />
        <span className="toggle-text"><strong>{t("Autoriser les estimations de réputation de Classic")}</strong></span>
      </label>
      <p className="muted" id={estimateHelp}>{t("Utilise les règles de Classic quand cette édition n'a pas de règle confirmée. Ces estimations sont signalées ; les désactiver conserve vos objectifs.")}</p>
      {goals.map((goal, i) => {
        const begin = startOf(goal.faction);
        const recorded = start?.reputation?.some((r) => r.id === goal.faction) ?? false;
        return (
          <div key={i}>
            <div className="companion">
              <select aria-label={t("Faction")} value={goal.faction} onChange={(e) => update(i, { ...goal, faction: Number(e.target.value) })}>
                {offered
                  .filter((f) => f.id === goal.faction || !goals.some((g) => g.faction === f.id))
                  .map((f) => (
                    <option key={f.id} value={f.id}>
                      {f.name}
                    </option>
                  ))}
              </select>
              <select aria-label={t("Rang visé")} value={goal.target} onChange={(e) => update(i, { ...goal, target: Number(e.target.value) })}>
                {TARGETS.map(([points, name]) => (
                  <option key={points} value={points}>
                    {t(name)}
                  </option>
                ))}
              </select>
              <button className="link" aria-label={t("Retirer")} title={t("Retirer")} onClick={() => onChange(goals.filter((_, j) => j !== i))}>
                ✕
              </button>
            </div>
            <p className="muted">
              {t("Départ : {standing}", { standing: standingText(begin) })}
              {begin >= goal.target && ` — ${t("déjà atteint")}`}
              {start && !recorded && ` — ${t("valeur de race estimée : aucun rang enregistré pour cette faction")}`}
            </p>
          </div>
        );
      })}
      {offered.length > goals.length && (
        <button className="companion add" onClick={add}>
          + {t("Ajouter un objectif")}
        </button>
      )}
    </section>
  );
}
