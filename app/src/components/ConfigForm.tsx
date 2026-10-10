import { useEffect, useMemo, useRef, useState } from "react";
import { api, type CharacterInfo, type Options, type Params, type PlanRequest, type ProfessionGoal, type StartState } from "../api";
import { SIMPLE_KEYS, budgets, presets, sections } from "../paramSchema";
import { ClassName } from "./Character";
import { CharacterPicker } from "./CharacterPicker";
import { DungeonPicker } from "./DungeonPicker";
import { ParamField } from "./ParamField";
import { ProfessionPicker } from "./ProfessionPicker";
import { WorldMap, type ContinentView, type ZoneState } from "./WorldMap";
import { ZonePicker } from "./ZonePicker";
import { LANGS, useGameName, useT } from "../i18n";

interface Props {
  options: Options;
  /** Configuration to start from (editing a previous result). */
  initial?: PlanRequest | null;
  /** Guide language when the configuration has none (the app language). */
  defaultLocale: string;
  onRun: (request: PlanRequest) => void;
  onExport?: (request: PlanRequest) => void;
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
      <span>{label}</span>
      <div className="segmented" role="radiogroup" aria-label={label}>
        {choices.map((c) => (
          <button key={c.label} role="radio" aria-checked={c.value === value} className={c.value === value ? "active" : ""} onClick={() => onChange(c.value)}>
            {c.label}
          </button>
        ))}
      </div>
      {help && <small>{help}</small>}
    </div>
  );
}

/** An on/off option with its explanation. */
function Toggle({ label, help, checked, onChange }: { label: string; help: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className={`toggle${checked ? " on" : ""}`}>
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      <span className="toggle-switch" aria-hidden />
      <span className="toggle-text">
        <strong>{label}</strong>
        <small>{help}</small>
      </span>
    </label>
  );
}

/**
 * Step 1. The simple view holds the choices that matter (character, pace, dungeons, zones,
 * professions); the advanced view every setting of the planner.
 */
export function ConfigForm({ options, initial, defaultLocale, onRun, onExport }: Props) {
  const t = useT();
  const gameName = useGameName();
  const defaults = options.params;
  const [view, setView] = useState<View>("simple");
  const [race, setRace] = useState(initial?.race ?? options.races[0]?.key ?? "");
  const [klass, setKlass] = useState(initial?.class ?? options.races.find((r) => r.key === race)?.classes[0] ?? options.classes[0]?.key ?? "");
  // Classes the race can play, and those of its faction (for the other players of a group).
  const raceInfo = options.races.find((r) => r.key === race);
  const playable = (keys: string[] | undefined) => options.classes.filter((c) => !keys?.length || keys.includes(c.key));
  const raceClasses = playable(raceInfo?.classes);
  const factionClasses = playable(options.races.filter((r) => r.faction === raceInfo?.faction).flatMap((r) => r.classes));
  // Adjusted while rendering (not in an effect) when the race can't play the class.
  if (raceClasses.length && !raceClasses.some((c) => c.key === klass)) setKlass(raceClasses[0].key);
  const [triedRun, setTriedRun] = useState(false);
  const nameRef = useRef<HTMLInputElement>(null);
  const [fromLevel, setFromLevel] = useState(initial?.from_level ?? 1);
  const [toLevel, setToLevel] = useState(initial?.to_level ?? options.max_level);
  const [name, setName] = useState(initial?.name ?? "");
  const nameMissing = triedRun && !name.trim();
  const [locale, setLocale] = useState(initial?.locale ?? defaultLocale);
  const [params, setParams] = useState<Params>({ ...defaults, ...(initial?.params ?? {}) });
  const [classQuests, setClassQuests] = useState<string[] | null>(initial?.required_class_quests ?? null);
  const [professions, setProfessions] = useState<ProfessionGoal[]>(initial?.professions ?? []);
  // Classes of the players leveling with the character (up to 4).
  const [group, setGroup] = useState<string[]>(initial?.group ?? []);
  const addCompanion = () => {
    // Going from solo to a group: elite quests become worth it.
    if (group.length === 0) set("allow_elite", true);
    setGroup([...group, factionClasses.find((c) => c.key !== klass)?.key ?? klass]);
  };
  const [newClassQuest, setNewClassQuest] = useState("");
  // The character met in game the guide starts from, and those the addon recorded.
  const [start, setStart] = useState<StartState | null>(initial?.start ?? null);
  const [characters, setCharacters] = useState<CharacterInfo[] | null>(null);
  useEffect(() => {
    api
      .listCharacters()
      .then(setCharacters)
      .catch(() => setCharacters([]));
  }, []);
  const pickCharacter = (c: CharacterInfo | null) => {
    setStart(c?.profile ?? null);
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

  const className = options.classes.find((c) => c.key === klass)?.name.toLowerCase() ?? klass;
  // Class quests follow the class unless they come from the configuration being edited (adjusted
  // while rendering, when one of their inputs changes).
  const classQuestsInputs = [className, klass, options.class_quests, initial] as const;
  const [prevClassQuestsInputs, setPrevClassQuestsInputs] = useState<typeof classQuestsInputs | null>(null);
  if (!prevClassQuestsInputs || classQuestsInputs.some((v, i) => v !== prevClassQuestsInputs[i])) {
    setPrevClassQuestsInputs(classQuestsInputs);
    if (!initial?.required_class_quests || initial.class !== klass) setClassQuests(options.class_quests[className] ?? []);
  }
  const faction = options.races.find((r) => r.key === race)?.faction;
  const factionContinent: ContinentView = faction === "Horde" ? 1 : 0;
  const [continent, setContinent] = useState<ContinentView>(factionContinent);
  const [prevFaction, setPrevFaction] = useState(faction);
  if (faction !== prevFaction) {
    setPrevFaction(faction);
    setContinent(factionContinent);
  }

  const set = (key: keyof Params, value: Params[keyof Params]) => setParams((p) => ({ ...p, [key]: value }));
  const factions = useMemo(() => [...new Set(options.races.map((r) => r.faction))], [options.races]);

  const zoneStates = useMemo(() => {
    const s: Record<number, ZoneState> = {};
    for (const z of params.preferred_zones) s[z] = "preferred";
    for (const z of params.excluded_zones) s[z] = "excluded";
    return s;
  }, [params.preferred_zones, params.excluded_zones]);
  const cycleZone = (area: number) => {
    const state = zoneStates[area];
    const preferred = params.preferred_zones.filter((z) => z !== area);
    const excluded = params.excluded_zones.filter((z) => z !== area);
    if (!state) preferred.push(area);
    else if (state === "preferred") excluded.push(area);
    setParams((p) => ({ ...p, preferred_zones: preferred, excluded_zones: excluded }));
  };

  // The pace preset whose values are all in use, if any.
  const pace = presets.find((p) => Object.entries(p.values).every(([k, v]) => params[k as keyof Params] === v))?.label ?? null;
  // Settings of the advanced view that differ from the defaults.
  const changed = sections
    .flatMap((s) => s.fields)
    .filter((f) => !SIMPLE_KEYS.includes(f.key) && JSON.stringify(params[f.key]) !== JSON.stringify(defaults[f.key])).length;

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
    group,
    start,
  });

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
        <div className="actions">
          {onExport && (
            <button className="link" onClick={() => onExport(request())} title={t("Enregistre la requête dans debug/ pour la rejouer en ligne de commande")}>
              {t("Exporter (debug)")}
            </button>
          )}
          <button
            className="primary big"
            onClick={() => {
              if (name.trim()) return onRun(request());
              // The guide needs a name: show where to type it.
              setTriedRun(true);
              setView("simple");
              setTimeout(() => nameRef.current?.focus(), 0);
            }}
          >
            {t("Lancer la génération →")}
          </button>
        </div>
      </div>

      {view === "simple" ? (
        <div className="config-simple">
          <div className="config-col">
            <section className="panel">
              <h3>{t("Personnage")}</h3>
              <CharacterPicker characters={characters} start={start} onPick={pickCharacter} />
              <label className={`field${nameMissing ? " invalid" : ""}`}>
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
              <div className="row">
                <label className="field grow">
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
                <label className="field grow">
                  <span>{t("Classe")}</span>
                  <select value={klass} disabled={!!start} onChange={(e) => setKlass(e.target.value)}>
                    {raceClasses.map((c) => (
                      <option key={c.key} value={c.key}>
                        {gameName(c.name)}
                      </option>
                    ))}
                  </select>
                </label>
              </div>
              <div className="row">
                <label className="field grow">
                  <span>{t("Du niveau")}</span>
                  <input type="number" min={1} max={options.max_level - 1} value={fromLevel} disabled={!!start} onChange={(e) => setFromLevel(Number(e.target.value))} />
                </label>
                <label className="field grow">
                  <span>{t("Au niveau")}</span>
                  <input type="number" min={2} max={options.max_level} value={toLevel} onChange={(e) => setToLevel(Number(e.target.value))} />
                </label>
              </div>
              <label className="field">
                <span>{t("Langue du guide")}</span>
                <select value={locale} onChange={(e) => setLocale(e.target.value)}>
                  {LANGS.map((l) => (
                    <option key={l.locale} value={l.locale}>
                      {l.label}
                    </option>
                  ))}
                </select>
                <small>{t("Noms des quêtes, PNJ et objets traduits ; anglais quand une traduction manque. Phrases en français ou en anglais.")}</small>
              </label>
            </section>

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
                help={t("Les mobs croisés à pied sont tués au passage. Ce farm remplace le farm pur et reste limité à {n} % de chaque niveau (réglable dans les réglages avancés).", { n: Math.round(params.grind_cap * 100) })}
                checked={params.farm_on_way}
                onChange={(v) => set("farm_on_way", v)}
              />
              <Toggle
                label={t("Serveur blindé (lancement)")}
                help={t("Les escortes, les événements et les mobs ou objets peu nombreux sont disputés : le guide compte l'attente et les évite quand d'autres quêtes rapportent plus.")}
                checked={params.crowded}
                onChange={(v) => set("crowded", v)}
              />
              <Toggle
                label={t("Faire les donjons rentables en groupe")}
                help={t("Un donjon est fait, avec ses quêtes, quand il fait gagner du temps.")}
                checked={params.dungeons}
                onChange={(v) => set("dungeons", v)}
              />
              <DungeonPicker
                dungeons={options.dungeons}
                forced={params.forced_dungeons}
                excluded={params.excluded_dungeons}
                onChange={(forced, excluded) => {
                  set("forced_dungeons", forced);
                  set("excluded_dungeons", excluded);
                }}
              />
              <Toggle
                label={t("Quêtes élites (groupe)")}
                help={t("Quêtes dont les objectifs sont des mobs élites hors donjon.")}
                checked={params.allow_elite}
                onChange={(v) => set("allow_elite", v)}
              />
            </section>

            <section className="panel">
              <h3>{t("Groupe")}</h3>
              <p className="muted">
                {group.length === 0
                  ? t("Seul. Ajoutez les joueurs qui montent avec vous : combats plus rapides, XP des mobs partagée, quêtes de classe de chacun.")
                  : t("{n} joueurs : combats plus rapides, XP des mobs partagée, quêtes de classe de chacun.", { n: group.length + 1 })}
              </p>
              <div className="companions">
                <div className="companion me">
                  <span className="muted">{t("Vous")}</span>
                  <ClassName klass={klass} />
                </div>
                {group.map((c, i) => (
                  <div className="companion" key={i}>
                    <select aria-label={t("Classe du joueur {n}", { n: i + 2 })} value={c} onChange={(e) => setGroup(group.map((g, j) => (j === i ? e.target.value : g)))}>
                      {factionClasses.map((o) => (
                        <option key={o.key} value={o.key}>
                          {gameName(o.name)}
                        </option>
                      ))}
                    </select>
                    <button className="link" aria-label={t("Retirer")} title={t("Retirer")} onClick={() => setGroup(group.filter((_, j) => j !== i))}>
                      ✕
                    </button>
                  </div>
                ))}
                {group.length < 4 && (
                  <button className="companion add" onClick={addCompanion}>
                    + {t("Ajouter un joueur")}
                  </button>
                )}
              </div>
            </section>

            <section className="panel">
              <h3>{t("Calcul")}</h3>
              <Segmented
                label={t("Temps de calcul")}
                choices={budgets.map((b) => ({ label: t(b.label), value: b.ms }))}
                value={params.time_limit_ms}
                onChange={(ms) => set("time_limit_ms", ms)}
              />
              <small className="muted">{t("Plus le calcul est long, plus le guide est court.")}</small>
            </section>
          </div>

          <div className="config-col">
            <section className="panel">
              <h3>{t("Zones")}</h3>
              <WorldMap continent={continent} onContinent={setContinent} zoneStates={zoneStates} onZoneClick={cycleZone} height={560} />
              <div className="row zone-pickers">
                <ZonePicker label={t("★ Privilégiées")} zones={options.zones} selected={params.preferred_zones} onChange={(ids) => set("preferred_zones", ids)} />
                <ZonePicker label={t("✕ Exclues")} zones={options.zones} selected={params.excluded_zones} onChange={(ids) => set("excluded_zones", ids)} />
              </div>
            </section>
            <section className="panel">
              <h3>{t("Métiers")}</h3>
              <p className="muted">{t("Pas de temps ni d'étapes dans le guide : l'addon indique si vous êtes en avance ou en retard sur la courbe, et les quêtes du métier sont proposées (facultatives).")}</p>
              <ProfessionPicker goals={professions} onChange={setProfessions} />
            </section>
          </div>
        </div>
      ) : (
        <div className="config-advanced">
          <section className="panel">
            <h3>{t("Quêtes de classe obligatoires")}</h3>
            <p className="muted">{t("Les autres quêtes de classe sont faites seulement si elles sont rentables.")}</p>
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

          {sections.map((s) => (
            <section className="panel" key={s.title}>
              <h3>{t(s.title)}</h3>
              {s.help && <p className="muted">{t(s.help)}</p>}
              {s.fields
                .filter((f) => !SIMPLE_KEYS.includes(f.key))
                .map((f) => (
                  <ParamField key={f.key} field={f} params={params} defaults={defaults} onChange={set} />
                ))}
            </section>
          ))}

          <section className="panel">
            <h3>{t("Métiers")}</h3>
            <ProfessionPicker goals={professions} onChange={setProfessions} />
          </section>

          <div className="config-reset">
            <button onClick={() => setParams(defaults)}>{t("Tout remettre par défaut")}</button>
          </div>
        </div>
      )}
    </div>
  );
}
