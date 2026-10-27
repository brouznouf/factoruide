import { useState } from "react";
import { api, type CharacterInfo, type Options, type ProfessionGoal, type StartState } from "../api";
import { useGameName, useT } from "../i18n";
import { CharacterPicker } from "./CharacterPicker";
import { Hint } from "./Hint";
import { ProfessionPicker } from "./ProfessionPicker";

/** Players in a group guide, the character included. */
export const GROUP_MAX = 5;
/** Level every player of a group guide must have reached, unless they all start in one place. */
const GROUP_MIN_LEVEL = 10;

/** A player of the guide: a character recorded in game (or pasted), or a new one. */
export interface PlayerSetup {
  /** Stable key of the block. */
  id: number;
  /** The character as its addon recorded it; null: a new character. */
  start: StartState | null;
  /** A new character's name in game ("Name-Realm"), when known: its addon finds it by it. */
  name: string;
  race: string;
  klass: string;
  level: number;
  professions: ProfessionGoal[];
}

/** A new character of the first race and class the game offers. */
export function newPlayer(id: number, options: Options, race?: string): PlayerSetup {
  const r = options.races.find((x) => x.key === race) ?? options.races[0];
  return { id, start: null, name: "", race: r?.key ?? "", klass: r?.classes[0] ?? options.classes[0]?.key ?? "", level: 1, professions: [] };
}

/** The player as a group guide's request has it: its profile, or one made up for a new character
 * (nothing done, at its race's starting place). */
export function asStart(p: PlayerSetup, k: number, options: Options): StartState {
  if (p.start) return p.start;
  const race = options.races.find((r) => r.key === p.race);
  return {
    name: p.name.trim() || `Player ${k + 1}`,
    race: race?.id ?? 0,
    class: p.klass.toUpperCase(),
    faction: race?.faction ?? "",
    level: p.level,
    xp: 0,
    xp_max: 0,
    rested: 0,
    resting: false,
    completed: [],
    log: [],
    position: null,
    instance: false,
    bind: null,
    flights: [],
    professions: [],
    riding: 0,
    gear: [],
    spells: [],
    money: 0,
    time: 0,
  };
}

/** A player of a saved request: a new character when its profile was made up (never recorded). */
export function fromStart(id: number, s: StartState, options: Options, professions: ProfessionGoal[]): PlayerSetup {
  if (s.time > 0) {
    const race = options.races.find((r) => r.id === s.race)?.key ?? "";
    return { id, start: s, name: "", race, klass: s.class.toLowerCase(), level: s.level, professions };
  }
  const race = options.races.find((r) => r.id === s.race)?.key ?? options.races[0]?.key ?? "";
  const made = /^Player \d+$/.test(s.name) ? "" : s.name;
  return { id, start: null, name: made, race, klass: s.class.toLowerCase(), level: s.level, professions };
}

/** Why each player of a group cannot be in it (null: it can), as the planner checks it: one faction,
 * level 10 each unless everyone starts in the same place (where the addon saw them, or their race's
 * starting place for new characters). */
export function groupProblems(players: PlayerSetup[], options: Options, t: ReturnType<typeof useT>): (string | null)[] {
  if (players.length < 2) return players.map(() => null);
  const race = (p: PlayerSetup) => options.races.find((r) => r.key === p.race);
  const faction = (p: PlayerSetup) => (p.start ? p.start.faction : race(p)?.faction);
  const level = (p: PlayerSetup) => p.start?.level ?? p.level;
  const place = (p: PlayerSetup) => (p.start?.position ? `map ${p.start.position.map}` : !p.start ? `start ${race(p)?.start_npc}` : null);
  const together = place(players[0]) !== null && players.every((p) => place(p) === place(players[0]));
  return players.map((p, k) => {
    const who = t("Joueur {n}", { n: k + 1 });
    if (faction(p) !== faction(players[0])) return t("{name} n'est pas de votre faction : tous les joueurs d'un guide de groupe sont de la même faction.", { name: who });
    if (level(p) < GROUP_MIN_LEVEL && !together) return t("{name} est niveau {n} : sous le niveau 10, tous les joueurs doivent partir de la même zone.", { name: who, n: level(p) });
    return null;
  });
}

interface Props {
  index: number;
  player: PlayerSetup;
  options: Options;
  /** Characters recorded or pasted, those of the other players left out (null while loading). */
  characters: CharacterInfo[] | null;
  /** More than one player: a new character can be given its name in game. */
  group: boolean;
  problem: string | null;
  onChange: (player: PlayerSetup) => void;
  /** A character was chosen (null: a new one). */
  onPicked?: (character: CharacterInfo | null) => void;
  /** A character's export was pasted: the list is read again. */
  onImported: (name: string) => Promise<CharacterInfo[]>;
  onRemove?: () => void;
}

/** Flight paths the player ticked or unticked for a character, kept between guides. */
const flightEdits = (character: string): { added: number[]; removed: number[] } => {
  try {
    return { added: [], removed: [], ...JSON.parse(localStorage.getItem(`fg.flights.${character}`) ?? "{}") };
  } catch {
    return { added: [], removed: [] };
  }
};
const mergeFlights = (character: string, recorded: number[], inferred: number[]) => {
  const edits = flightEdits(character);
  const removed = new Set(edits.removed);
  return [...new Set([...recorded, ...inferred, ...edits.added])].filter((id) => !removed.has(id)).sort((a, b) => a - b);
};

/** One player of the guide: who (a recorded character or a new one), and the professions it levels. */
export function PlayerBlock({ index, player, options, characters, group, problem, onChange, onPicked, onImported, onRemove }: Props) {
  const t = useT();
  const gameName = useGameName();
  // Flight paths: what the addon recorded, what the quests done deduce.
  const [flightInfo, setFlightInfo] = useState<{ recorded: number[]; inferred: number[] }>({ recorded: player.start?.flights ?? [], inferred: [] });
  const raceInfo = options.races.find((r) => r.key === player.race);
  const classes = options.classes.filter((c) => !raceInfo?.classes.length || raceInfo.classes.includes(c.key));
  const factions = [...new Set(options.races.map((r) => r.faction))];

  const pick = (c: CharacterInfo | null) => {
    onPicked?.(c);
    if (!c) {
      onChange({ ...player, start: null, level: 1 });
      return;
    }
    const recorded = c.profile.flights;
    setFlightInfo({ recorded, inferred: [] });
    // The professions it has are leveled on (their curve goes through its skill).
    const added = c.professions.filter(([key]) => !player.professions.some((g) => g.key === key)).map(([key]) => ({ key, target: 300 }));
    const picked: PlayerSetup = {
      ...player,
      start: { ...c.profile, flights: mergeFlights(c.profile.name, recorded, []) },
      race: c.race ?? player.race,
      klass: c.class,
      level: c.profile.level,
      professions: [...player.professions, ...added],
    };
    onChange(picked);
    api
      .inferredFlights(c.profile)
      .then((inferred) => {
        setFlightInfo({ recorded, inferred });
        onChange({ ...picked, start: { ...picked.start!, flights: mergeFlights(c.profile.name, recorded, inferred), flights_final: true } });
      })
      .catch(() => {});
  };

  const editFlights = (flights: number[]) => {
    const start = player.start;
    if (!start) return;
    const before = new Set(start.flights);
    const after = new Set(flights);
    const edits = flightEdits(start.name);
    const added = new Set(edits.added);
    const removed = new Set(edits.removed);
    for (const id of after) {
      if (before.has(id)) continue;
      added.add(id);
      removed.delete(id);
    }
    for (const id of before) {
      if (after.has(id)) continue;
      removed.add(id);
      added.delete(id);
    }
    localStorage.setItem(`fg.flights.${start.name}`, JSON.stringify({ added: [...added], removed: [...removed] }));
    onChange({ ...player, start: { ...start, flights, flights_final: true } });
  };

  return (
    <section className="panel player-block">
      <div className="player-head">
        <h3>{t("Joueur {n}", { n: index + 1 })}</h3>
        {onRemove && (
          <button className="link" onClick={onRemove}>
            ✕ {t("Retirer")}
          </button>
        )}
      </div>
      <div className="character-grid">
        <CharacterPicker
          characters={characters}
          start={player.start}
          onPick={pick}
          options={options}
          recorded={flightInfo.recorded}
          inferred={flightInfo.inferred}
          onFlights={editFlights}
          onImported={(name) =>
            onImported(name)
              .then((list) => {
                const found = list.find((c) => c.profile.name === name);
                if (found) pick(found);
              })
              .catch(() => {})
          }
        />
        {!player.start && group && (
          <label className="field">
            <span>{t("Nom du personnage en jeu (facultatif)")}</span>
            <input placeholder={t("ex. Brouz-Realm")} value={player.name} onChange={(e) => onChange({ ...player, name: e.target.value })} />
          </label>
        )}
        <label className="field">
          <span>{t("Race")}</span>
          <select
            value={player.race}
            disabled={!!player.start}
            onChange={(e) => {
              const race = options.races.find((r) => r.key === e.target.value);
              const klass = race && race.classes.length && !race.classes.includes(player.klass) ? race.classes[0] : player.klass;
              onChange({ ...player, race: e.target.value, klass });
            }}
          >
            {factions.map((f) => (
              <optgroup key={f} label={gameName(f)}>
                {options.races
                  .filter((r) => r.faction === f)
                  .map((r) => (
                    <option key={r.key} value={r.key}>
                      {gameName(r.name)}
                    </option>
                  ))}
              </optgroup>
            ))}
          </select>
        </label>
        <label className="field">
          <span>{t("Classe")}</span>
          <select value={player.klass} disabled={!!player.start} onChange={(e) => onChange({ ...player, klass: e.target.value })}>
            {classes.map((c) => (
              <option key={c.key} value={c.key}>
                {gameName(c.name)}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span>{t("Niveau")}</span>
          <input
            type="number"
            min={1}
            max={options.max_level - 1}
            value={player.start?.level ?? player.level}
            disabled={!!player.start}
            onChange={(e) => onChange({ ...player, level: Number(e.target.value) })}
          />
        </label>
      </div>
      <div className="character-extras">
        <div>
          <h3>
            {t("Métiers")}
            <Hint text={t("Pas de temps ni d'étapes dans le guide : l'addon indique si vous êtes en avance ou en retard sur la courbe, et les quêtes du métier sont proposées (facultatives).")} />
          </h3>
          <ProfessionPicker goals={player.professions} onChange={(professions) => onChange({ ...player, professions })} />
        </div>
      </div>
      {problem && <p className="error">{problem}</p>}
    </section>
  );
}
