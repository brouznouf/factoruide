import { fmtTime, type Breakdown } from "../api";
import { useT } from "../i18n";

/** Where the game time goes: one bar per activity, single hue, values labelled. */
export function BreakdownBars({ breakdown }: { breakdown: Breakdown }) {
  const t = useT();
  const rows = [
    { label: t("Combat (quêtes)"), value: breakdown.fighting },
    { label: t("Déplacements"), value: breakdown.travel },
    { label: t("Vols"), value: breakdown.flights },
    { label: t("Farm"), value: breakdown.grinding },
    { label: t("Farm en chemin"), value: breakdown.farming ?? 0 },
    { label: t("Donjons"), value: breakdown.dungeons },
    { label: t("Dialogues / loot"), value: breakdown.overhead },
    { label: t("Entraînement"), value: breakdown.training },
  ].filter((r) => r.value > 0);
  const max = Math.max(...rows.map((r) => r.value), 1);
  const total = rows.reduce((s, r) => s + r.value, 0);

  return (
    <figure className="chart">
      <figcaption>{t("Répartition du temps")}</figcaption>
      <div className="bars" role="table">
        {rows.map((r) => (
          <div className="bar-row" role="row" key={r.label} title={`${r.label} : ${fmtTime(r.value)} (${Math.round((r.value / total) * 100)} %)`}>
            <span role="cell" className="bar-label">
              {r.label}
            </span>
            <span role="cell" className="bar-track">
              <span className="bar-fill" style={{ width: `${(r.value / max) * 100}%` }} />
            </span>
            <span role="cell" className="bar-value">
              {fmtTime(r.value)} <small>{Math.round((r.value / total) * 100)} %</small>
            </span>
          </div>
        ))}
      </div>
    </figure>
  );
}
