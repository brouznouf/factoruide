import { useEffect, useMemo, useRef, useState } from "react";
import { api, type CharacterInfo, type Options, type Params, type PlanRequest } from "../api";
import { MIN_CHOICE_LEVEL, SIMPLE_KEYS, efforts, presets, sections, type Field, type Section } from "../paramSchema";
import { DungeonPanel } from "./DungeonPanel";
import { Hint } from "./Hint";
import { ParamField } from "./ParamField";
import { PowerCurve } from "./PowerCurve";
import { GROUP_MAX, PlayerBlock, asStart, fromStart, groupProblems, newPlayer, type PlayerSetup } from "./PlayerBlock";
import { ReputationPanel } from "./ReputationPanel";
import { RequiredQuests } from "./RequiredQuests";
import { WorldMap, type ZoneState } from "./WorldMap";
import { ZonePanel } from "./ZonePanel";
import { useT } from "../i18n";

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

/**
 * Step 1. The simple view holds the choices that matter (character, pace, dungeons, zones,
 * professions); the advanced view every setting of the planner.
 */
export function ConfigForm({ options, initial, startCharacter, defaultLocale, onRun, compareStarts, onCompareStarts, onManual, onExport, onChange }: Props) {
  const t = useT();
  const defaults = options.params;
  const [view, setView] = useState<View>("simple");
  // The players: the guide's own character first, then the others of a group guide.
  const [players, setPlayers] = useState<PlayerSetup[]>(() => {
    const own = initial?.start
      ? fromStart(0, initial.start, options, initial.professions ?? [])
      : { ...newPlayer(0, options, initial?.race), ...(initial ? { klass: initial.class, level: initial.from_level, professions: initial.professions ?? [] } : {}) };
    const mates = (initial?.mates ?? []).map((m, k) => fromStart(k + 1, m, options, initial?.mate_professions?.[k] ?? []));
    return [own, ...mates];
  });
  const nextId = useRef(players.length);
  const own = players[0];
  const { race, klass } = own;
  const fromLevel = own.start?.level ?? own.level;
  const raceInfo = options.races.find((r) => r.key === race);
  const group = players.length > 1;
  const setPlayer = (k: number, p: PlayerSetup) => setPlayers((list) => list.map((x, i) => (i === k ? p : x)));
  const addPlayer = () => {
    // Going from solo to a group: elite quests become worth it.
    if (!group) set("allow_elite", true);
    setPlayers((list) => [...list, newPlayer(nextId.current++, options, list[0].race)]);
  };
  const [triedRun, setTriedRun] = useState(false);
  const nameRef = useRef<HTMLInputElement>(null);
  const [toLevel, setToLevel] = useState(initial?.to_level ?? options.max_level);
  const [name, setName] = useState(initial?.name ?? "");
  const nameMissing = triedRun && !name.trim();
  // The language the guide is first written in (shown and installed in any language later).
  const locale = initial?.locale ?? defaultLocale;
  const [params, setParams] = useState<Params>({ ...defaults, ...(initial?.params ?? {}) });
  const [classQuests, setClassQuests] = useState<string[] | null>(initial?.required_class_quests ?? null);
  const [requiredQuests, setRequiredQuests] = useState<(number | string)[]>(initial?.required_quests ?? []);
  const [newClassQuest, setNewClassQuest] = useState("");
  // The characters the addon recorded, and those pasted.
  const [characters, setCharacters] = useState<CharacterInfo[] | null>(null);
  const readCharacters = () =>
    api.listCharacters().then((list) => {
      setCharacters(list);
      return list;
    });
  useEffect(() => {
    api
      .listCharacters()
      .then(setCharacters)
      .catch(() => setCharacters([]));
  }, []);
  // Opened from a character: it is picked once the characters are read (adjusted while rendering).
  const [picked, setPicked] = useState(false);
  /** The guide's own character chosen: the guide's levels and name follow it. */
  const ownPicked = (c: CharacterInfo | null) => {
    if (!c) return;
    if (toLevel <= c.profile.level) setToLevel(options.max_level);
    if (!name.trim()) setName(`${c.profile.name.split("-")[0]} ${c.profile.level}-${Math.max(toLevel, c.profile.level + 1)}`);
  };
  if (startCharacter && characters && !picked) {
    setPicked(true);
    const found = characters.find((c) => c.profile.name === startCharacter);
    if (found) {
      ownPicked(found);
      setPlayer(0, {
        ...own,
        start: found.profile,
        race: found.race ?? own.race,
        klass: found.class,
        level: found.profile.level,
        professions: found.professions.map(([key]) => ({ key, target: 300 })),
      });
    }
  }
  const problems = groupProblems(players, options, t);

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
    required_quests: requiredQuests,
    professions: own.professions,
    locale,
    start: group ? asStart(own, 0, options) : own.start,
    mates: group ? players.slice(1).map((p, k) => asStart(p, k + 1, options)) : [],
    mate_professions: group ? players.slice(1).map((p) => p.professions) : [],
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
  }, [players, toLevel, name, params, classQuests, requiredQuests, locale, onChange]);

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
          {onCompareStarts && fromLevel >= MIN_CHOICE_LEVEL && (
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
          <section className="panel guide-block">
            <h3>{t("Guide")}</h3>
            <div className="character-grid">
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
                <span>{t("Au niveau")}</span>
                <input type="number" min={2} max={options.max_level} value={toLevel} onChange={(e) => setToLevel(Number(e.target.value))} />
              </label>
            </div>
          </section>

          {players.map((p, k) => (
            <PlayerBlock
              key={p.id}
              index={k}
              player={p}
              options={options}
              characters={characters?.filter((c) => players.every((o) => o === p || o.start?.name !== c.profile.name)) ?? null}
              group={group}
              problem={problems[k]}
              onChange={(next) => setPlayer(k, next)}
              onPicked={k === 0 ? ownPicked : undefined}
              onImported={readCharacters}
              onRemove={k > 0 ? () => setPlayers((list) => list.filter((o) => o !== p)) : undefined}
            />
          ))}
          {players.length < GROUP_MAX && (
            <button className="add-player" onClick={addPlayer} title={t("Un guide pour tout le groupe : la même route pour chacun, l'XP de chacun suivie, les quêtes déjà faites par certains reprises pour les autres. Chaque joueur doit être niveau 10 au moins (ou tous partir de la même zone), tous de la même faction.")}>
              + {t("Ajouter un joueur")}
            </button>
          )}

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
                start={own.start}
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
          <RequiredQuests race={race} klass={klass} locale={locale} quests={requiredQuests} onChange={setRequiredQuests} />
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
