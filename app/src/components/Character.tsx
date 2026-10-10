import type { Options } from "../api";
import { useGameName } from "../i18n";

/** The game's class colors (raid frames, chat). */
const CLASS_COLORS: Record<string, string> = {
  warrior: "#c69b6d",
  paladin: "#f48cba",
  hunter: "#aad372",
  rogue: "#fff468",
  priest: "#ffffff",
  shaman: "#0070dd",
  mage: "#3fc7eb",
  warlock: "#8788ee",
  druid: "#ff7c0a",
};

/** Factions of the races, when the game version's options are not at hand. */
const RACE_FACTIONS: Record<string, string> = {
  human: "Alliance",
  dwarf: "Alliance",
  gnome: "Alliance",
  nightelf: "Alliance",
  draenei: "Alliance",
  skyborne: "Alliance",
  orc: "Horde",
  troll: "Horde",
  undead: "Horde",
  scourge: "Horde",
  tauren: "Horde",
  bloodelf: "Horde",
  skyborne_horde: "Horde",
};

const key = (name: string) => name.toLowerCase().replace(/[^a-z_]/g, "");

export const raceFaction = (race: string) => RACE_FACTIONS[key(race)];

/** A class name in its class color (on a dark chip, so that the priest's white reads in the light theme too). */
export function ClassName({ klass }: { klass: string }) {
  const gameName = useGameName();
  return (
    <span className="class-name" style={{ color: CLASS_COLORS[key(klass)] }}>
      {gameName(klass)}
    </span>
  );
}

/** Alliance (blue shield) or Horde (red banner) emblem, named in its tooltip. */
export function FactionIcon({ faction }: { faction?: string }) {
  const gameName = useGameName();
  if (faction !== "Alliance" && faction !== "Horde") return null;
  const label = gameName(faction);
  return (
    <svg className="faction-icon" viewBox="0 0 16 16" width="16" height="16" role="img" aria-label={label}>
      <title>{label}</title>
      {faction === "Alliance" ? (
        <>
          <path d="M8 1 L14 3 V8 C14 11.5 11.4 13.8 8 15 C4.6 13.8 2 11.5 2 8 V3 Z" fill="#1f4fa8" stroke="#e2b33c" strokeWidth="1.2" />
          <path d="M8 4 L10.5 8.5 H9 V12 H7 V8.5 H5.5 Z" fill="#e2b33c" />
        </>
      ) : (
        <>
          <path d="M2 2 H14 V10 L8 15 L2 10 Z" fill="#a3161b" stroke="#2b0d0d" strokeWidth="1.2" />
          <path d="M4.5 4.5 L8 9 L11.5 4.5 M8 9 V12" fill="none" stroke="#1a0a0a" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
        </>
      )}
    </svg>
  );
}

/** Faction emblem, race and class (and the classes of a group) of a guide's character; the
 * game version's options name the race and its faction. */
export function Character({ race, klass, group, options }: { race: string; klass: string; group?: string[]; options?: Options | null }) {
  const gameName = useGameName();
  const info = options?.races.find((r) => r.key === race);
  return (
    <span className="character">
      <FactionIcon faction={info?.faction ?? raceFaction(race)} />
      <span>{gameName(info?.name ?? race)}</span>
      {[klass, ...(group ?? [])].map((c, i) => (
        <ClassName key={i} klass={c} />
      ))}
    </span>
  );
}
