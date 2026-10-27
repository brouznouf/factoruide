import { type DungeonOption } from "../api";
import { useAreaName, useT } from "../i18n";
import { Hint } from "./Hint";

interface Props {
  dungeons: DungeonOption[];
  forced: number[];
  excluded: number[];
  onChange: (forced: number[], excluded: number[]) => void;
  /** Levels dungeons are run at, around their level, and the game's highest level. */
  from: number;
  to: number;
  maxLevel: number;
  onLevels: (from: number, to: number) => void;
}

/** The levels a dungeon of level `level` is run at: from `from` to `to` around it, within the
 * game's levels (null: none). */
export function dungeonLevels(level: number, from: number, to: number, maxLevel: number): [number, number] | null {
  const [low, high] = [Math.max(1, level + from), Math.min(maxLevel, level + to)];
  return low <= high ? [low, high] : null;
}

/** The dungeons whose levels the choice leaves none for (empty: the choice works). */
export function dungeonsWithoutLevels(dungeons: DungeonOption[], from: number, to: number, maxLevel: number): DungeonOption[] {
  return dungeons.filter((d) => d.level != null && !dungeonLevels(d.level, from, to, maxLevel));
}

const signed = (n: number) => (n > 0 ? `+${n}` : String(n));

/** The dungeons by level, a click cycling one: automatic → forced → excluded. Above them, the
 * levels they are run at, which the guide keeps to. */
export function DungeonPanel({ dungeons, forced, excluded, onChange, from, to, maxLevel, onLevels }: Props) {
  const t = useT();
  const areaName = useAreaName();
  const without = (ids: number[], id: number) => ids.filter((x) => x !== id);
  const force = (id: number) => onChange([...without(forced, id), id], without(excluded, id));
  const exclude = (id: number) => onChange(without(forced, id), [...without(excluded, id), id]);
  const remove = (id: number) => onChange(without(forced, id), without(excluded, id));
  const cycle = (id: number) => (forced.includes(id) ? exclude(id) : excluded.includes(id) ? remove(id) : force(id));
  const levels = (d: DungeonOption) => {
    const range = d.level == null ? null : dungeonLevels(d.level, from, to, maxLevel);
    return range ? (range[0] === range[1] ? String(range[0]) : `${range[0]}–${range[1]}`) : "";
  };
  const empty = dungeonsWithoutLevels(dungeons, from, to, maxLevel);
  const offset = (value: number, onValue: (n: number) => void, label: string) => (
    <label className="field dungeon-offset">
      <span>{label}</span>
      <select aria-label={label} value={value} onChange={(e) => onValue(Number(e.target.value))}>
        {Array.from({ length: 21 }, (_, k) => k - 10).map((n) => (
          <option key={n} value={n}>
            {signed(n)}
          </option>
        ))}
      </select>
    </label>
  );

  return (
    <section className="panel">
      <h3>
        {t("Donjons")}
        <Hint text={`${t("Cliquez un donjon : forcé → exclu → automatique.")} ${t("Forcé : fait même s'il n'est pas rentable.")}`} />
      </h3>
      <div className="dungeon-levels">
        <span>
          {t("Niveau des donjons, par rapport à leurs monstres")}
          <Hint text={t("Le guide fait chaque donjon entre ces niveaux, jamais en dehors : les niveaux affichés sous chaque donjon. Le profil de progression les remet à sa valeur.")} />
        </span>
        {offset(from, (n) => onLevels(n, to), t("Minimum"))}
        {offset(to, (n) => onLevels(from, n), t("Maximum"))}
      </div>
      {empty.length > 0 && (
        <p className="error">{t("Aucun niveau possible pour : {names}. Changez le minimum ou le maximum.", { names: empty.map(areaName).join(", ") })}</p>
      )}
      <div className="dungeon-grid">
        {dungeons.map((d) => {
          const state = forced.includes(d.id) ? "forced" : excluded.includes(d.id) ? "excluded" : "";
          return (
            <button
              key={d.id}
              className={`dungeon ${state}`}
              onClick={() => cycle(d.id)}
              title={state === "forced" ? t("Forcé") : state === "excluded" ? t("Exclu") : t("Automatique")}
              aria-label={`${areaName(d)} ${levels(d)} : ${state === "forced" ? t("Forcé") : state === "excluded" ? t("Exclu") : t("Automatique")}`}
            >
              <span>
                {state === "forced" && <b aria-hidden>★ </b>}
                {state === "excluded" && <b aria-hidden>✕ </b>}
                {areaName(d)}
              </span>
              <small>{levels(d)}</small>
            </button>
          );
        })}
      </div>
    </section>
  );
}
