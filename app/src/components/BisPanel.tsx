import { useEffect, useState } from "react";
import { api, type BisSlot } from "../api";
import { useT } from "../i18n";

/** The equipment slots of the power model (`fg_route::power`), by index. */
const SLOTS: Record<number, string> = {
  0: "Tête",
  1: "Cou",
  2: "Épaules",
  3: "Dos",
  4: "Torse",
  5: "Poignets",
  6: "Mains",
  7: "Ceinture",
  8: "Jambes",
  9: "Pieds",
  10: "Doigt",
  12: "Bibi",
  14: "Main droite",
  15: "Main gauche",
  16: "Deux mains",
  17: "À distance",
};

/** An item's icon (an object URL), fetched once: null without one. */
function useIcon(item: number): string | null {
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    let made: string | null = null;
    api.itemIcon(item).then((fresh) => {
      if (!alive) {
        if (fresh) URL.revokeObjectURL(fresh);
        return;
      }
      made = fresh;
      setUrl(fresh);
    });
    return () => {
      alive = false;
      if (made) URL.revokeObjectURL(made);
    };
  }, [item]);
  return url;
}

/** The best item of each slot at each level band (`Route::bis`), grouped by band: what to pick
 * up or buy and when, the drops left out (never guaranteed). */
export function BisPanel({ bis }: { bis: BisSlot[] }) {
  const t = useT();
  const bands = new Map<number, BisSlot[]>();
  for (const entry of bis) {
    const list = bands.get(entry.level) ?? [];
    list.push(entry);
    bands.set(entry.level, list);
  }
  return (
    <details className="bis-panel">
      <summary>
        {t("Meilleur équipement par tranche de niveaux")} <span className="muted">{bis.length}</span>
      </summary>
      <div className="bis-bands">
        {[...bands.entries()].map(([level, entries]) => (
          <section key={level} className="bis-band">
            <h4>
              {t("Niveaux {from} – {to}", { from: level - 4, to: level })}
            </h4>
            <ul>
              {entries.map((entry) => (
                <BisRow key={entry.slot} entry={entry} />
              ))}
            </ul>
          </section>
        ))}
      </div>
    </details>
  );
}

/** One best item: its slot, its icon, its name and where it comes from. */
function BisRow({ entry }: { entry: BisSlot }) {
  const t = useT();
  const icon = useIcon(entry.item);
  return (
    <li>
      <span className="bis-slot">{t(SLOTS[entry.slot] ?? "?")}</span>
      {icon && (
        <img
          className="bis-icon"
          src={icon}
          alt=""
          width={20}
          height={20}
          loading="lazy"
          title={entry.name}
        />
      )}
      <strong className="bis-item">{entry.name}</strong>
      {entry.source.kind === "quest" ? (
        <small>{t("Quête : {name}", { name: entry.source.quest_name })}</small>
      ) : (
        <small>{t("Marchands : {names}", { names: entry.source.npcs.map((v) => v.name).join(", ") })}</small>
      )}
    </li>
  );
}