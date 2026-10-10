import { type NamedId } from "../api";
import { useAreaName, useT } from "../i18n";

interface Props {
  dungeons: NamedId[];
  forced: number[];
  excluded: number[];
  onChange: (forced: number[], excluded: number[]) => void;
}

/** Every dungeon as a chip; a click cycles it: automatic → forced (★) → excluded (✕). */
export function DungeonPicker({ dungeons, forced, excluded, onChange }: Props) {
  const t = useT();
  const areaName = useAreaName();
  const cycle = (id: number) => {
    const others = (ids: number[]) => ids.filter((d) => d !== id);
    if (forced.includes(id)) onChange(others(forced), [...others(excluded), id]);
    else if (excluded.includes(id)) onChange(others(forced), others(excluded));
    else onChange([...forced, id], others(excluded));
  };
  return (
    <div className="field">
      <span>{t("Donjons à forcer ou exclure")}</span>
      <div className="chips">
        {dungeons.map((d) => {
          const state = forced.includes(d.id) ? "forced" : excluded.includes(d.id) ? "excluded" : "";
          return (
            <button
              key={d.id}
              className={`chip ${state}`}
              onClick={() => cycle(d.id)}
              title={state === "forced" ? t("Forcé") : state === "excluded" ? t("Exclu") : t("Automatique")}
            >
              {state === "forced" ? "★ " : state === "excluded" ? "✕ " : ""}
              {areaName(d)}
            </button>
          );
        })}
      </div>
      <small>{t("Un clic : forcé (fait même s'il n'est pas rentable), deux : exclu, trois : automatique.")}</small>
    </div>
  );
}
