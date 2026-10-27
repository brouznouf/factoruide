import { useEffect, useMemo, useRef, useState } from "react";
import { api, type CharacterInfo, type Options, type Params, type PlanRequest, type ProfessionGoal, type StartState } from "../api";
import { MIN_CHOICE_LEVEL, SIMPLE_KEYS, efforts, presets, sections, type Field, type Section } from "../paramSchema";
import { ClassName } from "./Character";
import { CharacterPicker } from "./CharacterPicker";
import { DungeonPanel } from "./DungeonPanel";
import { Hint } from "./Hint";
import { ParamField } from "./ParamField";
import { PowerCurve } from "./PowerCurve";
import { ProfessionPicker } from "./ProfessionPicker";
import { ReputationPanel } from "./ReputationPanel";
import { WorldMap, type ZoneState } from "./WorldMap";
import { ZonePanel } from "./ZonePanel";
import { useGameName, useT } from "../i18n";
import { IS_WEB } from "../web/mode";
import { ProfilePaste } from "../web/components/ProfilePaste";

interface Props {
  options: Options;
  /** Configuration to start from (editing a previous result). */
  initial?: PlanRequest | null;
  /** A recorded character ("Name-Realm") to start from, picked once the characters are read. */
  startCharacter?: string;
  /** Guide language when the configuration has none (the app language). */
  defaultLocale: string;
  onRun: (request: PlanRequest) => void;
  /** Compare the ways the guide can begin before computing it (when there are several), and the switch. */
  compareStarts?: boolean;
  onCompareStarts?: (on: boolean) => void;
  /** A guide of no steps from this configuration, its steps added by hand. */
  onManual?: (request: PlanRequest) => void;
  onExport?: (request: PlanRequest) => void;
  /** The configuration as it is now, after each change. */
  onChange?: (request: PlanRequest) => void;
}

type View = "simple" | "advanced";

/** Progression profiles: how far above its power the character goes. */
const PROGRESSION = [
  { value: "cautious", label: "Prudent", help: "Prudent (survie) : groupes de mobs jamais à plus d'un niveau au-dessus de vous" },
  { value: "normal", label: "Normal", help: "Normal : quêtes jaunes, jamais d'orange (recommandé)" },
  { value: "risky", label: "Risqué", help: "Risqué : quêtes orange, élites limites" },
];

/** One choice among a few, as a row of buttons. */
function Segmented<T>({ label, choices, value, onChange, help }: { label: string; choices: { label: string; value: T }[]; value: T | null; onChange: (v: T) => void; help?: string }) {
  return (
    <div className="field">
      <span>
        {label}
        {help && <Hint text={help} />}
      </span>
      <div className="segmented" role="radiogroup" aria-label={label}>
        {choices.map((c) => (
          <button key={c.label} role="radio" aria-checked={c.value === value} className={c.value === value ? "active" : ""} onClick={() => onChange(c.value)}>
            {/* "Thorough (6 min)": the detail under the name, so that both fit. */}
            {/^(.+) \((.+)\)$/.test(c.label) ? (
              <>
                {c.label.replace(/ \(.+\)$/, "")}
                <small>{c.label.replace(/^.+ \((.+)\)$/, "$1")}</small>
              </>
            ) : (
              c.label
            )}
          </button>
        ))}
      </div>
    </div>
  );
}

/** How crowded the server is, as words for the slider's ends and middle. */
const CROWD_WORDS = ["Vide", "Calme", "Fréquenté", "Chargé", "Lancement"];

/** A level from 0 to 1 with its explanation: a slider in steps of 5 %. */
function Level({ label, help, value, words, onChange }: { label: string; help: string; value: number; words: string[]; onChange: (v: number) => void }) {
  const t = useT();
  const word = words[Math.min(words.length - 1, Math.round(value * (words.length - 1)))];
  return (
    <label className={`toggle level${value > 0 ? " on" : ""}`}>
      <span className="level-value">{Math.round(value * 100)} %</span>
      <span className="toggle-text">
        <strong>
          {label} · {t(word)}
          <Hint text={help} />
        </strong>
        <input type="range" min={0} max={100} step={5} value={Math.round(value * 100)} onChange={(e) => onChange(Number(e.target.value) / 100)} />
      </span>
    </label>
  );
}

/** An on/off option with its explanation. */
function Toggle({ label, help, checked, onChange }: { label: string; help: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className={`toggle${checked ? " on" : ""}`}>
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      <span className="toggle-switch" aria-hidden />
      <span className="toggle-text">
        <strong>
          {label}
          <Hint text={help} />
        </strong>
      </span>
    </label>
  );
}

/** Players in a group guide, the character included. */
const GROUP_MAX = 5;
/** Level every player of a group guide must have reached, unless they all start in one zone. */
const GROUP_MIN_LEVEL = 10;

/** "Name" of a "Name-Realm". */
const shortName = (name: string) => name.split("-")[0];

/** Whether these players can have a group guide: level 10 each, or all in the same zone (new characters). */
const groupAllows = (players: StartState[]) =>
  players.every((p) => p.level >= GROUP_MIN_LEVEL) ||
  (players[0]?.position != null && players.every((p) => p.position?.map === players[0].position?.map));

/**
 * The other players of a group guide, under the character: those the addon recorded on this
 * computer, or each player's export pasted (in game: /fg, "Export my character").
 */
function GroupRow({
  start,
  mates,
  characters,
  note,
  onAdd,
  onRemove,
  onPasted,
}: {
  start: StartState | null;
  mates: StartState[];
  characters: CharacterInfo[] | null;
  /** Why the character just pasted was not added to the group. */
  note: string | null;
  onAdd: (mate: StartState) => void;
  onRemove: (name: string) => void;
  onPasted: (name: string) => void;
}) {
  const t = useT();
  const [adding, setAdding] = useState(false);
  const candidates = (characters ?? []).filter(
    (c) =>
      start &&
      c.profile.name !== start.name &&
      c.profile.faction === start.faction &&
      !mates.some((m) => m.name === c.profile.name) &&
      groupAllows([start, ...mates, c.profile]),
  );
  const player = (p: StartState) => (
    <>
      <strong>{shortName(p.name)}</strong> <ClassName klass={p.class.toLowerCase()} /> <span className="muted">{t("niveau {n}", { n: p.level })}</span>
    </>
  );
  return (
    <div className="group-row">
      <div className="group-players">
        <span className="group-label">
          {t("Groupe")}
          <Hint text={t("Un guide pour tout le groupe : la même route pour chacun, l'XP de chacun suivie, les quêtes déjà faites par certains reprises pour les autres. Chaque joueur doit être niveau 10 au moins (ou tous partir de la même zone), tous de la même faction.")} />
        </span>
        {start ? (
          <>
            <span className="companion me">
              <span className="muted">{t("Vous")}</span> {player(start)}
            </span>
            {mates.map((m) => (
              <span className="companion" key={m.name}>
                {player(m)}
                <button className="link" aria-label={t("Retirer")} title={t("Retirer")} onClick={() => onRemove(m.name)}>
                  ✕
                </button>
              </span>
            ))}
            {mates.length < GROUP_MAX - 1 && (
              <button className="companion add" aria-expanded={adding} onClick={() => setAdding(!adding)}>
                + {t("Ajouter un joueur")}
              </button>
            )}
          </>
        ) : (
          <span className="muted">{t("Choisissez d'abord votre personnage.")}</span>
        )}
      </div>
      {start && adding && (
        <div className="group-add">
          {candidates.length > 0 && (
            <select
              aria-label={t("Ajouter un joueur")}
              value=""
              onChange={(e) => {
                const c = candidates.find((x) => x.profile.name === e.target.value);
                if (c) {
                  onAdd(c.profile);
                  setAdding(false);
                }
              }}
            >
              <option value="">{IS_WEB ? t("Un personnage déjà importé…") : t("Un personnage enregistré par l'addon…")}</option>
              {candidates.map((c) => (
                <option key={c.profile.name} value={c.profile.name}>
                  {shortName(c.profile.name)} · {t("niveau {n}", { n: c.profile.level })}
                </option>
              ))}
            </select>
          )}
          <ProfilePaste
            compact
            onImported={(name) => {
              onPasted(name);
              setAdding(false);
            }}
          />
        </div>
      )}
      {note && <p className="error">{note}</p>}
    </div>
  );
}

/**
 * Step 1. The simple view holds the choices that matter (character, pace, dungeons, zones,
 * professions); the advanced view every setting of the planner.
 */
export function ConfigForm({ options, initial, startCharacter, defaultLocale, onRun, compareStarts, onCompareStarts, onManual, onExport, onChange }: Props) {
  const t = useT();
  const gameName = useGameName();
  const defaults = options.params;
  const [view, setView] = useState<View>("simple");
  const [race, setRace] = useState(initial?.race ?? options.races[0]?.key ?? "");
  const [klass, setKlass] = useState(initial?.class ?? options.races.find((r) => r.key === race)?.classes[0] ?? options.classes[0]?.key ?? "");
  // Classes the race can play.
  const raceInfo = options.races.find((r) => r.key === race);
  const playable = (keys: string[] | undefined) => options.classes.filter((c) => !keys?.length || keys.includes(c.key));
  const raceClasses = playable(raceInfo?.classes);
  // Adjusted while rendering (not in an effect) when the race can't play the class.
  if (raceClasses.length && !raceClasses.some((c) => c.key === klass)) setKlass(raceClasses[0].key);
  const [triedRun, setTriedRun] = useState(false);
  const nameRef = useRef<HTMLInputElement>(null);
  const [fromLevel, setFromLevel] = useState(initial?.from_level ?? 1);
  const [toLevel, setToLevel] = useState(initial?.to_level ?? options.max_level);
  const [name, setName] = useState(initial?.name ?? "");
  const nameMissing = triedRun && !name.trim();
  // The language the guide is first written in (shown and installed in any language later).
  const locale = initial?.locale ?? defaultLocale;
  const [params, setParams] = useState<Params>({ ...defaults, ...(initial?.params ?? {}) });
  const [classQuests, setClassQuests] = useState<string[] | null>(initial?.required_class_quests ?? null);
  const [professions, setProfessions] = useState<ProfessionGoal[]>(initial?.professions ?? []);
  // The other players of a group guide (up to 4), as their addon recorded them.
  const [mates, setMates] = useState<StartState[]>(initial?.mates ?? []);
  const addMate = (mate: StartState) => {
    // Going from solo to a group: elite quests become worth it.
    if (mates.length === 0) set("allow_elite", true);
    setMates([...mates, mate]);
  };
  // Why the character just pasted for the group was left out of it.
  const [groupNote, setGroupNote] = useState<string | null>(null);
  const [newClassQuest, setNewClassQuest] = useState("");
  // The character met in game the guide starts from, and those the addon recorded.
  const [start, setStart] = useState<StartState | null>(initial?.start ?? null);
  const [characters, setCharacters] = useState<CharacterInfo[] | null>(null);
  /** A player's export just pasted: in the group when it can be, else why not. */
  const addPasted = (pasted: string) =>
    api
      .listCharacters()
      .then((list) => {
        setCharacters(list);
        const p = list.find((c) => c.profile.name === pasted)?.profile;
        if (!p || !start) return;
        const who = shortName(p.name);
        if (p.name === start.name || mates.some((m) => m.name === p.name)) setGroupNote(t("{name} est déjà dans le groupe.", { name: who }));
        else if (mates.length >= GROUP_MAX - 1) setGroupNote(t("Le groupe est complet ({n} joueurs).", { n: GROUP_MAX }));
        else if (p.faction !== start.faction) setGroupNote(t("{name} n'est pas de votre faction : tous les joueurs d'un guide de groupe sont de la même faction.", { name: who }));
        else if (!groupAllows([start, ...mates, p])) setGroupNote(t("{name} est niveau {n} : sous le niveau 10, tous les joueurs doivent partir de la même zone.", { name: who, n: p.level }));
        else {
          setGroupNote(null);
          addMate(p);
        }
      })
      .catch(() => {});
  useEffect(() => {
    api
      .listCharacters()
      .then(setCharacters)
      .catch(() => setCharacters([]));
  }, []);
  // Opened from a character: it is picked once the characters are read (adjusted while rendering).
  const [picked, setPicked] = useState(false);
  // Flight paths: what the addon recorded, what the quests done deduce; the player's ticks and unticks
  // are kept for the character (`fg.flights.<name>`).
  const [flightInfo, setFlightInfo] = useState<{ recorded: number[]; inferred: number[] }>({ recorded: initial?.start?.flights ?? [], inferred: [] });
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
  const editFlights = (flights: number[]) => {
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
    setStart({ ...start, flights, flights_final: true });
  };
  const pickCharacter = (c: CharacterInfo | null) => {
    setStart(c ? { ...c.profile, flights: mergeFlights(c.profile.name, c.profile.flights, []) } : null);
    if (c) {
      const recorded = c.profile.flights;
      setFlightInfo({ recorded, inferred: [] });
      api
        .inferredFlights(c.profile)
        .then((inferred) => {
          setFlightInfo({ recorded, inferred });
          setStart((s) => (s && s.name === c.profile.name ? { ...s, flights: mergeFlights(c.profile.name, recorded, inferred), flights_final: true } : s));
        })
        .catch(() => {});
    }
    // A group starts from the characters as the addon saw them, of one faction (level 10, or one zone).
    setMates((list) => {
      const kept = c ? list.filter((m) => m.name !== c.profile.name && m.faction === c.profile.faction) : [];
      return c && groupAllows([c.profile, ...kept]) ? kept : [];
    });
    if (!c) {
      setFromLevel(1);
      return;
    }
    if (c.race) setRace(c.race);
    setKlass(c.class);
    setFromLevel(c.profile.level);
    if (toLevel <= c.profile.level) setToLevel(options.max_level);
    if (!name.trim()) setName(`${c.profile.name.split("-")[0]} ${c.profile.level}-${Math.max(toLevel, c.profile.level + 1)}`);
    // The professions it has are leveled on (their curve goes through its skill).
    const added = c.professions.filter(([key]) => !professions.some((g) => g.key === key));
    if (added.length) setProfessions([...professions, ...added.map(([key]) => ({ key, target: 300 }))]);
  };
  if (startCharacter && characters && !picked) {
    setPicked(true);
    const found = characters.find((c) => c.profile.name === startCharacter);
    if (found) pickCharacter(found);
  }

  const className = options.classes.find((c) => c.key === klass)?.name.toLowerCase() ?? klass;
  // Class quests follow the class unless they come from the configuration being edited (adjusted
  // while rendering, when one of their inputs changes).
  const classQuestsInputs = [className, klass, options.class_quests, initial] as const;
  const [prevClassQuestsInputs, setPrevClassQuestsInputs] = useState<typeof classQuestsInputs | null>(null);
  if (!prevClassQuestsInputs || classQuestsInputs.some((v, i) => v !== prevClassQuestsInputs[i])) {
    setPrevClassQuestsInputs(classQuestsInputs);
    if (!initial?.required_class_quests || initial.class !== klass) setClassQuests(options.class_quests[className] ?? []);
    // An adjusted power curve belongs to the class it was made for.
    if (prevClassQuestsInputs && prevClassQuestsInputs[1] !== klass && (params.power_curve?.length || params.power_gear != null)) {
      setParams((p) => ({ ...p, power_curve: [], power_gear: null }));
    }
  }
  const set = (key: keyof Params, value: Params[keyof Params]) => setParams((p) => ({ ...p, [key]: value }));
  const factions = useMemo(() => [...new Set(options.races.map((r) => r.faction))], [options.races]);

  const zoneStates = useMemo(() => {
    const s: Record<number, ZoneState> = {};
    for (const z of params.preferred_zones) s[z] = "preferred";
    for (const z of params.forced_zones) s[z] = "forced";
    for (const z of params.excluded_zones) s[z] = "excluded";
    return s;
  }, [params.preferred_zones, params.forced_zones, params.excluded_zones]);
  // A click goes neutral → preferred → forced → excluded → neutral.
  const cycleZone = (area: number) => {
    const state = zoneStates[area];
    const preferred = params.preferred_zones.filter((z) => z !== area);
    const forced = params.forced_zones.filter((z) => z !== area);
    const excluded = params.excluded_zones.filter((z) => z !== area);
    if (!state) preferred.push(area);
    else if (state === "preferred") forced.push(area);
    else if (state === "forced") excluded.push(area);
    setParams((p) => ({ ...p, preferred_zones: preferred, forced_zones: forced, excluded_zones: excluded }));
  };

  // The pace preset whose values are all in use, if any.
  const pace = presets.find((p) => Object.entries(p.values).every(([k, v]) => params[k as keyof Params] === v))?.label ?? null;
  // Settings of the advanced view, and those that differ from the defaults.
  const advancedKeys = sections.flatMap((s) => s.fields).map((f) => f.key).filter((k) => !SIMPLE_KEYS.includes(k));
  const isChanged = (k: keyof Params) => JSON.stringify(params[k]) !== JSON.stringify(defaults[k]);
  // The class power curve (in "Puissance du personnage"), adjusted or not.
  const powerChanged = Boolean(params.power_curve?.length) || params.power_gear != null;
  const changed = advancedKeys.filter(isChanged).length + (powerChanged ? 1 : 0);
  // Advanced view: a search over the settings' names and explanations; the sections for experts
  // folded until opened (all open while searching).
  const [query, setQuery] = useState("");
  const [openSections, setOpenSections] = useState(() => new Set(sections.filter((s) => !s.expert).map((s) => s.title)));
  const plain = (text: string) => text.normalize("NFD").replace(/\p{Diacritic}/gu, "").toLowerCase();
  const matches = (f: Field) => !query.trim() || plain(`${t(f.label)} ${f.help ? t(f.help) : ""}`).includes(plain(query.trim()));

  const request = (): PlanRequest => ({
    race,
    class: klass,
    from_level: fromLevel,
    to_level: toLevel,
    name: name.trim(),
    params,
    required_class_quests: classQuests ?? [],
    professions,
    locale,
    start,
    mates: start ? mates : [],
  });

  /** Generate the guide, or create it by hand: the request, once the guide has a name. */
  const launch = (action: (request: PlanRequest) => void) => {
    if (name.trim()) return action(request());
    // The guide needs a name: show where to type it.
    setTriedRun(true);
    setView("simple");
    setTimeout(() => nameRef.current?.focus(), 0);
  };

  useEffect(() => {
    onChange?.(request());
    // The request is made of these.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [race, klass, fromLevel, toLevel, name, params, classQuests, professions, locale, mates, start, onChange]);

  /** A section of the advanced settings (null when the search leaves none of its fields). */
  const settingsSection = (s: Section) => {
    const fields = s.fields.filter((f) => !SIMPLE_KEYS.includes(f.key) && matches(f));
    // The class power curve goes with the character's power.
    const power = s.title === "Puissance du personnage" && (!query.trim() || plain(`${t("Courbe de puissance de la classe")} ${t("Dépendance à l'équipement")}`).includes(plain(query.trim())));
    if (!fields.length && !power) return null;
    const open = openSections.has(s.title) || Boolean(query.trim());
    const count = s.fields.filter((f) => !SIMPLE_KEYS.includes(f.key) && isChanged(f.key)).length + (power && powerChanged ? 1 : 0);
    return (
      <section className={`panel settings-section${open ? " open" : ""}`} key={s.title}>
        <button
          className="section-head"
          aria-expanded={open}
          onClick={() =>
            setOpenSections((o) => {
              const next = new Set(o);
              if (next.has(s.title)) next.delete(s.title);
              else next.add(s.title);
              return next;
            })
          }
        >
          <h3>
            {t(s.title)}
            {s.help && <Hint text={t(s.help)} />}
          </h3>
          {s.expert && <span className="tag">{t("Pour experts")}</span>}
          {count > 0 && (
            <span className="badge" title={t("Réglages modifiés : {n}", { n: count })}>
              {count}
            </span>
          )}
          <span className="chevron" aria-hidden>
            ▾
          </span>
        </button>
        {open && fields.map((f) => <ParamField key={f.key} field={f} params={params} defaults={defaults} onChange={set} />)}
        {open && power && (
          <PowerCurve
            params={params}
            options={options}
            className={className}
            maxLevel={options.max_level}
            onChange={(curve, gear) => setParams((p) => ({ ...p, power_curve: curve, power_gear: gear }))}
          />
        )}
      </section>
    );
  };

  return (
    <div className="config">
      <div className="config-bar">
        <div>
          <h2>{t("Configuration")}</h2>
          <div className="view-tabs" role="tablist">
            <button role="tab" aria-selected={view === "simple"} className={view === "simple" ? "active" : ""} onClick={() => setView("simple")}>
              {t("Essentiel")}
            </button>
            <button role="tab" aria-selected={view === "advanced"} className={view === "advanced" ? "active" : ""} onClick={() => setView("advanced")}>
              {t("Avancé")}
              {changed > 0 && <span className="badge">{changed}</span>}
            </button>
          </div>
        </div>
        {/* The last choice before launching: how much to compute. */}
        <div className="run-box" title={t("Plus le calcul est long, plus le guide est court. Sa durée dépend du processeur, le guide non.")}>
          <Segmented
            label={t("Temps de calcul")}
            choices={efforts.map((e) => ({ label: t(e.label), value: e.effort }))}
            value={params.effort}
            onChange={(effort) => set("effort", effort)}
          />
          {onCompareStarts && (start || fromLevel > 1) && (start?.level ?? fromLevel) >= MIN_CHOICE_LEVEL && (
            <label className="compare-starts" title={t("Avant de calculer le guide, calcule rapidement quelques départs différents (zone, vol, pierre de foyer) et vous les présente pour que vous choisissiez. N'apparaît que s'il y a vraiment plusieurs départs possibles.")}>
              <input type="checkbox" checked={compareStarts ?? true} onChange={(e) => onCompareStarts(e.target.checked)} /> {t("Comparer les départs")}
            </label>
          )}
          {onManual && (
            <button onClick={() => launch(onManual)} title={t("Un guide sans étape, qui part du même personnage avec les mêmes réglages : vous ajoutez ses étapes une à une dans l'éditeur.")}>
              {t("Création manuelle")}
            </button>
          )}
          <button className="primary big" onClick={() => launch(onRun)}>
            {t("Lancer la génération →")}
          </button>
        </div>
      </div>

      {view === "simple" ? (
        <div className="config-simple">
          <section className="panel">
            <h3>{t("Personnage")}</h3>
            <div className="character-grid">
              <CharacterPicker
                characters={characters?.filter((c) => !c.shared) ?? null}
                start={start}
                onPick={pickCharacter}
                options={options}
                recorded={flightInfo.recorded}
                inferred={flightInfo.inferred}
                onFlights={editFlights}
                onImported={(character) =>
                  api
                    .listCharacters()
                    .then((list) => {
                      setCharacters(list);
                      const found = list.find((c) => c.profile.name === character);
                      if (found) pickCharacter(found);
                    })
                    .catch(() => {})
                }
              />
              <label className={`field guide-name${nameMissing ? " invalid" : ""}`}>
                <span>
                  {t("Nom du guide")} <em className="required">*</em>
                </span>
                <input
                  ref={nameRef}
                  required
                  aria-invalid={nameMissing}
                  placeholder={t("ex. Duo nain guerrier et prêtre")}
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                />
                {nameMissing && <small className="field-error">{t("Donnez un nom au guide pour le retrouver et le partager.")}</small>}
              </label>
              <label className="field">
                <span>{t("Race")}</span>
                <select value={race} disabled={!!start} onChange={(e) => setRace(e.target.value)}>
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
                <select value={klass} disabled={!!start} onChange={(e) => setKlass(e.target.value)}>
                  {raceClasses.map((c) => (
                    <option key={c.key} value={c.key}>
                      {gameName(c.name)}
                    </option>
                  ))}
                </select>
              </label>
              <label className="field">
                <span>{t("Du niveau")}</span>
                <input type="number" min={1} max={options.max_level - 1} value={fromLevel} disabled={!!start} onChange={(e) => setFromLevel(Number(e.target.value))} />
              </label>
              <label className="field">
                <span>{t("Au niveau")}</span>
                <input type="number" min={2} max={options.max_level} value={toLevel} onChange={(e) => setToLevel(Number(e.target.value))} />
              </label>
            </div>

            <GroupRow
              start={start}
              mates={mates}
              characters={characters}
              note={groupNote}
              onAdd={(mate) => {
                setGroupNote(null);
                addMate(mate);
              }}
              onRemove={(gone) => setMates(mates.filter((m) => m.name !== gone))}
              onPasted={addPasted}
            />

            <div className="character-extras">
              <div>
                <h3>
                  {t("Métiers")}
                  <Hint text={t("Pas de temps ni d'étapes dans le guide : l'addon indique si vous êtes en avance ou en retard sur la courbe, et les quêtes du métier sont proposées (facultatives).")} />
                </h3>
                <ProfessionPicker goals={professions} onChange={setProfessions} />
              </div>
            </div>
          </section>

          <div className="config-columns">
            {/* How to play and the group in a column; beside it, the dungeons over the zones. */}
            <div className="config-col">
              <section className="panel">
                <h3>{t("Façon de jouer")}</h3>
                <Segmented
                  label={t("Rythme de jeu")}
                  choices={presets.map((p) => ({ label: t(p.label), value: p.label }))}
                  value={pace}
                  onChange={(label) => setParams((cur) => ({ ...cur, ...presets.find((p) => p.label === label)!.values }))}
                />
                <Segmented
                  label={t("Profil de progression")}
                  choices={PROGRESSION.map((p) => ({ label: t(p.label), value: p.value }))}
                  value={params.progression}
                  onChange={(v) => set("progression", v)}
                  help={PROGRESSION.filter((p) => p.value === params.progression).map((p) => t(p.help))[0]}
                />
                <Toggle
                  label={t("Farm autorisé")}
                  help={t("Sans farm, le guide va chercher plus de quêtes, quitte à être plus long. Il ne farme que s'il ne reste aucune quête.")}
                  checked={params.allow_grind}
                  onChange={(v) => set("allow_grind", v)}
                />
                <Toggle
                  label={t("Farmer en chemin")}
                  help={t("Les mobs croisés à pied sont tués au passage. Ce farm remplace le farm pur et reste limité à {n} % de chaque niveau (réglable dans les réglages avancés).", { n: Math.round(params.farm_cap * 100) })}
                  checked={params.farm_on_way}
                  onChange={(v) => set("farm_on_way", v)}
                />
                <Toggle
                  label={t("Faire les donjons rentables en groupe")}
                  help={t("Un donjon est fait, avec ses quêtes, quand il fait gagner du temps.")}
                  checked={params.dungeons}
                  onChange={(v) => set("dungeons", v)}
                />
                <Toggle
                  label={t("Quêtes élites (groupe)")}
                  help={t("Quêtes dont les objectifs sont des mobs élites hors donjon.")}
                  checked={params.allow_elite}
                  onChange={(v) => set("allow_elite", v)}
                />
                <Level
                  label={t("Monde sur le serveur")}
                  help={t("D'autres joueurs farment les mêmes mobs et objets, et attendent leur tour pour les escortes et les événements : le guide compte l'attente aux petits spots et les évite quand d'autres quêtes rapportent plus.")}
                  value={params.crowded}
                  words={CROWD_WORDS}
                  onChange={(v) => set("crowded", v)}
                />
              </section>

              <ReputationPanel
                factions={options.factions}
                raceId={raceInfo?.id}
                side={raceInfo?.faction}
                start={start}
                goals={params.reputation_goals}
                onChange={(reputation_goals) => setParams((p) => ({ ...p, reputation_goals }))}
              />

            </div>

            <div className="config-col config-side">
              <DungeonPanel
                dungeons={options.dungeons}
                forced={params.forced_dungeons}
                excluded={params.excluded_dungeons}
                onChange={(forced_dungeons, excluded_dungeons) => setParams((p) => ({ ...p, forced_dungeons, excluded_dungeons }))}
              />
              <section className="panel zones-layout">
                <WorldMap zoneStates={zoneStates} onZoneClick={cycleZone} height={420} />
                <div>
                  <h3>
                    {t("Zones")}
                    <Hint text={t("Cliquez une zone sur la carte ou cherchez-la ici.")} />
                  </h3>
                  <ZonePanel
                    zones={options.zones}
                    preferred={params.preferred_zones}
                    forced={params.forced_zones}
                    excluded={params.excluded_zones}
                    onChange={(preferred_zones, forced_zones, excluded_zones) => setParams((p) => ({ ...p, preferred_zones, forced_zones, excluded_zones }))}
                  />
                </div>
              </section>
            </div>
          </div>
        </div>
      ) : (
        <>
        <div className="advanced-bar">
          <input type="search" className="settings-search" placeholder={t("Rechercher un réglage…")} value={query} onChange={(e) => setQuery(e.target.value)} />
          <span className="muted">{changed > 0 ? t("Réglages modifiés : {n}", { n: changed }) : t("Tous les réglages sont aux valeurs par défaut.")}</span>
          <button disabled={changed === 0} onClick={() => setParams((p) => ({ ...p, ...Object.fromEntries(advancedKeys.map((k) => [k, defaults[k]])) }))}>
            {t("Tout remettre par défaut")}
          </button>
          {onExport && (
            <button className="link" onClick={() => onExport(request())} title={t("Enregistre la requête dans debug/ pour la rejouer en ligne de commande")}>
              {t("Exporter (debug)")}
            </button>
          )}
        </div>
        {/* Two fixed columns: folding a section moves only those under it, never the other column. */}
        <div className="config-advanced">
          <div className="config-col">
          {!query.trim() && (
          <section className="panel">
            <h3>
              {t("Quêtes de classe obligatoires")}
              <Hint text={t("Les autres quêtes de classe sont faites seulement si elles sont rentables.")} />
            </h3>
            <div className="chips">
              {(classQuests ?? []).map((q) => (
                <button key={q} className="chip" onClick={() => setClassQuests((classQuests ?? []).filter((c) => c !== q))}>
                  {q} ×
                </button>
              ))}
            </div>
            <div className="row">
              <input placeholder={t("Nom exact de la quête")} value={newClassQuest} onChange={(e) => setNewClassQuest(e.target.value)} />
              <button
                disabled={!newClassQuest.trim()}
                onClick={() => {
                  setClassQuests([...(classQuests ?? []), newClassQuest.trim()]);
                  setNewClassQuest("");
                }}
              >
                {t("Ajouter")}
              </button>
            </div>
            <label className="field">
              <span>{t("Quêtes exclues (IDs)")}</span>
              <input
                value={params.excluded_quests.join(", ")}
                onChange={(e) =>
                  set(
                    "excluded_quests",
                    e.target.value
                      .split(/[ ,;]+/)
                      .map(Number)
                      .filter((n) => n > 0),
                  )
                }
              />
            </label>
          </section>
          )}

          {sections.slice(0, 3).map(settingsSection)}
          </div>
          <div className="config-col">{sections.slice(3).map(settingsSection)}</div>
          {query.trim() && !sections.some((s) => s.fields.some((f) => !SIMPLE_KEYS.includes(f.key) && matches(f))) && <p className="muted settings-none">{t("Aucun réglage trouvé.")}</p>}
        </div>
        </>
      )}
    </div>
  );
}
