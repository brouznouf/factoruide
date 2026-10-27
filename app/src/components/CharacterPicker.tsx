import type { CharacterInfo, Options, StartState } from "../api";
import { FlightPaths } from "./FlightPaths";
import { useDate, useGameName, useT } from "../i18n";
import { Hint } from "./Hint";
import { ProfilePaste } from "../web/components/ProfilePaste";
import { useState } from "react";

interface Props {
  /** Characters the addon recorded (null while loading). */
  characters: CharacterInfo[] | null;
  /** The character the guide starts from (null: a new character). */
  start: StartState | null;
  onPick: (character: CharacterInfo | null) => void;
  /** The game version's flight paths, those its addon recorded for the character and those deduced from its quests, and the list edited. */
  options?: Options;
  recorded?: number[];
  inferred?: number[];
  onFlights?: (flights: number[]) => void;
  /** A character's export was pasted (its "Name-Realm"). */
  onImported?: (name: string) => void;
}

/** Start the guide from a character recorded in game ("Save the profile now", or at each logout): its
 * level, quests done and in the log, position, hearthstone and flight paths. */
export function CharacterPicker({ characters, start, onPick, options, recorded, inferred, onFlights, onImported }: Props) {
  const t = useT();
  const date = useDate();
  const gameName = useGameName();
  const latest = start ? characters?.find((c) => c.profile.name === start.name) : undefined;
  const pick = (name: string) => onPick(characters?.find((c) => c.profile.name === name) ?? null);
  // The export of a character, pasted here (a friend's, or on the web build).
  const [pasting, setPasting] = useState(false);
  const xpShare = start && start.xp_max > 0 ? Math.floor((100 * start.xp) / start.xp_max) : 0;

  return (
    <div className="field character-picker">
      <span>
        {t("Partir d'un personnage")}
        <Hint
          text={
            start
              ? t("Le guide part de ce personnage : ses quêtes faites sont ignorées ; celles de son journal sont reprises si elles servent, sinon abandonnées en première étape. En jeu, /fg puis « Enregistrer le profil maintenant » (ou une déconnexion) met le profil à jour.")
              : characters?.length === 0
                ? t("Aucun personnage enregistré : connectez-vous en jeu avec l'addon installé, puis /fg et « Enregistrer le profil maintenant » (ou déconnectez-vous).")
                : t("Choisissez un personnage enregistré en jeu pour reprendre là où il en est.")
          }
        />
      </span>
      <select aria-label={t("Partir d'un personnage")} value={start?.name ?? ""} onChange={(e) => pick(e.target.value)}>
        <option value="">{t("Nouveau personnage")}</option>
        {start && !latest && <option value={start.name}>{t("{name} (instantané de ce guide)", { name: start.name })}</option>}
        {characters?.map((c) => (
          <option key={c.profile.name} value={c.profile.name}>
            {c.profile.name} · {c.race ? gameName(c.race) : ""} {gameName(c.class)} {c.profile.level}
          </option>
        ))}
      </select>
      {!pasting && (
        <button className="link" onClick={() => setPasting(true)}>
          {t("Coller l'export d'un personnage (en jeu : /fg, « Exporter mon personnage »)…")}
        </button>
      )}
      {pasting && (
        <ProfilePaste
          compact
          onImported={(name) => {
            setPasting(false);
            onImported?.(name);
          }}
        />
      )}
      {start && (
        <div className="character-summary">
          <span>{t("Niveau {level} ({pct} %)", { level: start.level, pct: xpShare })}</span>
          {start.rested > 0 && <span>{t("{n} XP de repos", { n: start.rested })}</span>}
          <span>{t("{done} quêtes faites, {log} dans le journal", { done: start.completed.length, log: start.log.length })}</span>
          {start.flights.length > 0 && <span>{t("{n} trajets aériens connus", { n: start.flights.length })}</span>}
          {start.bind && <span>{t("Pierre de foyer : {place}", { place: start.bind.name })}</span>}
          <span className="muted">{t("Enregistré le {date}", { date: date(start.time) })}</span>
        </div>
      )}
      {start && latest && latest.profile.time > start.time && (
        <small>
          {t("Un profil plus récent existe.")}{" "}
          <button className="link" onClick={() => onPick(latest)}>
            {t("Utiliser le plus récent")}
          </button>
        </small>
      )}
      {start && options && onFlights && (
        <FlightPaths options={options} start={start} recorded={recorded ?? []} inferred={inferred ?? []} onChange={onFlights} />
      )}
      {start && start.flights.length === 0 && (
        <small className="field-error">{t("Aucun trajet aérien connu : ouvrez la carte d'un maître de vol en jeu pour les enregistrer.")}</small>
      )}
    </div>
  );
}
