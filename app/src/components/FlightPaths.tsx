import type { Options, StartState } from "../api";
import { useT } from "../i18n";

/** Where each flight path stands on each continent (map ID), as the game names them. */
export const CONTINENTS: Record<number, string> = {
  0: "Royaume de l'Est",
  1: "Kalimdor",
  530: "Outreterre",
  571: "Norfendre",
};

interface Props {
  options: Options;
  start: StartState;
  /** The flight paths its addon recorded, and those deduced from the quests it did. */
  recorded: number[];
  inferred: number[];
  onChange: (flights: number[]) => void;
}

/**
 * The flight paths the guide takes as known, by continent: those the addon recorded (it only
 * sees a continent's from a flight master's map there), those deduced from the quests done in
 * their towns, and what is ticked by hand.
 */
export function FlightPaths({ options, start, recorded, inferred, onChange }: Props) {
  const t = useT();
  const side = start.faction === "Horde" ? "horde" : "alliance";
  const nodes = options.flights.filter((f) => f[side]);
  const known = new Set(start.flights);
  const seen = new Set(recorded);
  const guessed = new Set(inferred);
  const continents = [...new Set(nodes.map((n) => n.continent))].sort((a, b) => a - b);
  const set = (ids: number[], on: boolean) => {
    const next = new Set(known);
    for (const id of ids) {
      if (on) next.add(id);
      else next.delete(id);
    }
    onChange([...next].sort((a, b) => a - b));
  };
  return (
    <details className="flight-paths">
      <summary>{t("Points de vol connus ({n})", { n: start.flights.length })}</summary>
      <p className="muted">
        {t("Le jeu ne donne à l'addon que les points de vol du continent où vous ouvrez un maître de vol. Les autres sont déduits des quêtes faites dans leur ville : vérifiez-les et cochez ou décochez à la main.")}
      </p>
      {continents.map((continent) => {
        const here = nodes.filter((n) => n.continent === continent);
        const ids = here.map((n) => n.id);
        const nothingSeen = !here.some((n) => seen.has(n.id));
        return (
          <fieldset key={continent} className="flight-continent">
            <legend>
              {t(CONTINENTS[continent] ?? `Continent ${continent}`)} <span className="muted">{here.filter((n) => known.has(n.id)).length} / {here.length}</span>
            </legend>
            {nothingSeen && (
              <small className="field-error">
                {t("Aucun point de vol relevé en jeu sur ce continent : ouvrez-y un maître de vol pour les enregistrer, ou cochez ceux que vous connaissez.")}
              </small>
            )}
            <div className="flight-actions">
              <button className="link" onClick={() => set(ids, true)}>
                {t("Tout cocher")}
              </button>
              <button className="link" onClick={() => set(ids, false)}>
                {t("Tout décocher")}
              </button>
            </div>
            <ul className="flight-list">
              {here.map((n) => (
                <li key={n.id}>
                  <label>
                    <input type="checkbox" checked={known.has(n.id)} onChange={(e) => set([n.id], e.target.checked)} /> {n.name}
                    {!seen.has(n.id) && guessed.has(n.id) && <small className="muted"> · {t("déduit des quêtes")}</small>}
                  </label>
                </li>
              ))}
            </ul>
          </fieldset>
        );
      })}
    </details>
  );
}
