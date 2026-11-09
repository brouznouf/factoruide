import type { BisSlot } from "../api";
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
                <li key={entry.slot}>
                  <span className="bis-slot">{t(SLOTS[entry.slot] ?? "?")}</span>
                  <strong className="bis-item">{entry.name}</strong>
                  {entry.source.kind === "quest" ? (
                    <small>{t("Quête : {name}", { name: entry.source.quest_name })}</small>
                  ) : (
                    <small>{t("Marchands : {names}", { names: entry.source.npcs.map((v) => v.name).join(", ") })}</small>
                  )}
                </li>
              ))}
            </ul>
          </section>
        ))}
      </div>
    </details>
  );
}