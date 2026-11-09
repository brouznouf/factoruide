import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  objectivesOf,
  type CharacterInfo,
  type GuideObjective,
  type ObjectiveCounts,
  type ObjectivePreview,
  type Options,
  type PlanRequest,
  type QuestChoice,
} from "../api";
import { ClassName } from "../components/Character";
import { Segmented } from "../components/ConfigForm";
import { CONTINENTS } from "../components/FlightPaths";
import { efforts } from "../paramSchema";
import { ProfilePaste } from "../web/components/ProfilePaste";
import { gameLocale, useAreaName, useLang, useT } from "../i18n";

interface Props {
  options: Options;
  /** An objective guide's configuration to start from (its character and objectives picked). */
  initial?: PlanRequest | null;
  /** A generation is running: no other one. */
  busy: boolean;
  /** Generate the guide (the new guide page shows its progress, then the guide). */
  onGenerate: (request: PlanRequest) => void;
}

type Adding = "dungeon" | "zone" | "quest" | null;

const same = (a: GuideObjective, b: GuideObjective) => JSON.stringify(a) === JSON.stringify(b);

/**
 * An objective guide: from a recorded character, every quest of the objectives chosen (dungeons,
 * zones, quests), with the quests leading to them, in the best order whatever their levels;
 * nothing else. The guide says which level its quests ask when the character is below it.
 */
export function ObjectivesPage({ options, initial, busy, onGenerate }: Props) {
  const t = useT();
  const lang = useLang();
  const areaName = useAreaName();
  const [characters, setCharacters] = useState<CharacterInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [picked, setPicked] = useState<string | null>(initial?.start?.name ?? null);
  const [objectives, setObjectives] = useState<GuideObjective[]>(() => objectivesOf(initial));
  // The name typed by the player (null: made from the objectives).
  const [name, setName] = useState<string | null>(initial?.name ?? null);
  const [effort, setEffort] = useState<number>(initial?.params?.effort ?? options.params.effort);
  const [adding, setAdding] = useState<Adding>(null);
  // The counts and the preview, with the character (and objectives) they are of.
  const [countsOf, setCountsOf] = useState<{ key: string; counts: ObjectiveCounts } | null>(null);
  const [previewOf, setPreviewOf] = useState<{ key: string; preview?: ObjectivePreview; error?: string } | null>(null);
  // Quest names by ID: those found by the search, and those of a guide edited again.
  const [questNames, setQuestNames] = useState<Record<number, QuestChoice>>({});
  // A character's export pasted here (a friend's, or on the web build): read again, then picked.
  const [pasting, setPasting] = useState(false);
  const load = useCallback(
    () =>
      api
        .listCharacters()
        // Another player's profile is for group guides; a race of another game version cannot play here.
        .then((list) => setCharacters(list.filter((c) => !c.shared && c.race && options.races.some((r) => r.key === c.race))))
        .catch((e) => {
          setCharacters([]);
          setError(String(e));
        }),
    [options.races],
  );
  useEffect(() => {
    load();
  }, [load]);
  const character = characters?.find((c) => c.profile.name === picked) ?? characters?.[0] ?? null;
  const level = character?.profile.level ?? 1;
  const completed = useMemo(() => new Set(character?.profile.completed ?? []), [character]);

  // The quests the character has left in each zone and dungeon.
  const countsKey = character?.profile.name ?? "";
  const counts = countsOf?.key === countsKey ? countsOf.counts : null;
  useEffect(() => {
    if (!character) return;
    let live = true;
    api
      .objectiveCounts(character.profile)
      .then((c) => live && setCountsOf({ key: character.profile.name, counts: c }))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [character]);

  // The names of the quests of a guide edited again.
  useEffect(() => {
    if (!character?.race) return;
    for (const o of objectives) {
      if (!("quest" in o) || questNames[o.quest]) continue;
      api
        .questChoices(character.race, character.class, String(o.quest), gameLocale(lang))
        .then((found) => {
          const q = found.find((f) => f.id === o.quest);
          if (q) setQuestNames((names) => ({ ...names, [q.id]: q }));
        })
        .catch(() => {});
    }
  }, [character, objectives, questNames, lang]);

  const request = useCallback(
    (title: string): PlanRequest | null =>
      character?.race
        ? {
            race: character.race,
            class: character.class,
            from_level: level,
            to_level: level,
            name: title,
            params: { ...(initial?.params ?? options.params), effort },
            required_class_quests: [],
            required_quests: [],
            professions: [],
            locale: gameLocale(lang),
            start: character.profile,
            mates: [],
            mate_professions: [],
            objectives,
            dungeon: null,
          }
        : null,
    [character, level, initial, options.params, effort, lang, objectives],
  );

  // What the objectives plan for the character, once they stop changing (the effort changes
  // nothing to the quests).
  const previewKey = JSON.stringify([countsKey, objectives]);
  const current = previewOf?.key === previewKey ? previewOf : null;
  const preview = current?.preview ?? null;
  const previewError = current?.error ?? null;
  const previewing = objectives.length > 0 && !current;
  const previewRequest = useRef(request);
  useEffect(() => {
    previewRequest.current = request;
  }, [request]);
  useEffect(() => {
    const req = previewRequest.current("");
    if (!req || req.objectives?.length === 0) return;
    let live = true;
    const timer = setTimeout(() => {
      api
        .objectivePreview(req)
        .then((p) => live && setPreviewOf({ key: previewKey, preview: p }))
        .catch((e) => live && setPreviewOf({ key: previewKey, error: String(e) }));
    }, 500);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [previewKey]);

  const dungeonOf = (id: number) => options.dungeons.find((d) => d.id === id);
  const zoneOf = (id: number) => options.zones.find((z) => z.id === id);
  const label = (o: GuideObjective) => {
    if ("dungeon" in o) {
      const d = dungeonOf(o.dungeon);
      return d ? areaName(d) : `#${o.dungeon}`;
    }
    if ("zone" in o) {
      const z = zoneOf(o.zone);
      return z ? areaName(z) : `#${o.zone}`;
    }
    return questNames[o.quest]?.name ?? `#${o.quest}`;
  };
  const left = (o: GuideObjective): string => {
    if ("dungeon" in o) return t("{n} quêtes restantes", { n: counts?.dungeons[o.dungeon] ?? "…" });
    if ("zone" in o) return t("{n} quêtes restantes", { n: counts?.zones[o.zone] ?? "…" });
    return completed.has(o.quest) ? t("déjà faite") : t("avec les quêtes qui y mènent");
  };
  // "Deadmines + Westfall + 3 quests", after the character's name.
  const autoName = () => {
    const places = objectives.filter((o) => !("quest" in o)).map(label);
    const quests = objectives.filter((o) => "quest" in o);
    const parts =
      quests.length === 1 && places.length === 0
        ? [label(quests[0])]
        : [...places, ...(quests.length ? [t("{n} quêtes", { n: quests.length })] : [])];
    const who = character ? (character.full_name ?? character.profile.name).split("-")[0] : "";
    return `${who} — ${parts.join(" + ")}`;
  };
  const toggle = (o: GuideObjective) => setObjectives((list) => (list.some((x) => same(x, o)) ? list.filter((x) => !same(x, o)) : [...list, o]));
  const chosen = (o: GuideObjective) => objectives.some((x) => same(x, o));

  const generate = () => {
    const req = request(name?.trim() || autoName());
    if (req && objectives.length) onGenerate(req);
  };

  const paste = pasting ? (
    <ProfilePaste
      compact
      onImported={(n) => {
        setPasting(false);
        setPicked(n);
        load();
      }}
    />
  ) : (
    <button className="link" onClick={() => setPasting(true)}>
      {t("Coller l'export d'un personnage (en jeu : /fg, « Exporter mon personnage »)…")}
    </button>
  );

  return (
    <div className="dungeon-page objectives-page">
      <div className="page-head">
        <h2>{t("Guide d'objectifs")}</h2>
      </div>
      <p className="muted">
        {t("Les quêtes de donjons, de zones ou des quêtes choisies, avec celles qui y mènent, dans le meilleur ordre depuis là où en est votre personnage : rien d'autre, quel que soit leur niveau. Le guide finit une fois les quêtes rendues.")}
      </p>
      {error && <div className="error">{error}</div>}
      {characters === null && <p className="muted">{t("Chargement…")}</p>}
      {characters?.length === 0 && (
        <section className="panel">
          <div className="notice">{t("Aucun personnage enregistré : connectez-vous en jeu avec l'addon Factoruide, puis revenez ici (le profil est enregistré à chaque déconnexion).")}</div>
          {paste}
        </section>
      )}
      {character && (
        <section className="panel">
          <h3>{t("Personnage")}</h3>
          <div className="dungeon-characters" role="radiogroup" aria-label={t("Personnage")}>
            {characters!.map((c) => (
              <button key={c.profile.name} role="radio" aria-checked={c === character} className={c === character ? "active" : ""} onClick={() => setPicked(c.profile.name)}>
                <strong>{c.full_name ?? c.profile.name.split("-")[0]}</strong> <ClassName klass={c.class} /> <span className="muted">{t("niveau {n}", { n: c.profile.level })}</span>
              </button>
            ))}
          </div>
          <div className="dungeon-paste">{paste}</div>
        </section>
      )}
      {character && (
        <section className="panel">
          <h3>{t("Objectifs")}</h3>
          {objectives.length === 0 && <p className="muted">{t("Aucun objectif : ajoutez un donjon, une zone ou une quête.")}</p>}
          {objectives.length > 0 && (
            <ul className="objective-list">
              {objectives.map((o) => (
                <li key={JSON.stringify(o)}>
                  <span className="kind">{"dungeon" in o ? t("Donjon") : "zone" in o ? t("Zone") : t("Quête")}</span>
                  <strong>{label(o)}</strong>
                  <span className="muted">{left(o)}</span>
                  <button className="link" aria-label={t("Retirer")} title={t("Retirer")} onClick={() => toggle(o)}>
                    ✕
                  </button>
                </li>
              ))}
            </ul>
          )}
          <div className="objective-add" role="tablist" aria-label={t("Ajouter")}>
            <span className="muted">{t("Ajouter :")}</span>
            {(
              [
                ["dungeon", "Donjon"],
                ["zone", "Zone"],
                ["quest", "Quête"],
              ] as const
            ).map(([kind, text]) => (
              <button key={kind} role="tab" aria-selected={adding === kind} className={adding === kind ? "active" : ""} onClick={() => setAdding(adding === kind ? null : kind)}>
                {t(text)}
              </button>
            ))}
          </div>
          {adding === "dungeon" && <DungeonPicker options={options} counts={counts} chosen={(id) => chosen({ dungeon: id })} onToggle={(id) => toggle({ dungeon: id })} />}
          {adding === "zone" && <ZonePicker options={options} counts={counts} chosen={(id) => chosen({ zone: id })} onToggle={(id) => toggle({ zone: id })} />}
          {adding === "quest" && character.race && (
            <QuestPicker
              race={character.race}
              klass={character.class}
              completed={completed}
              chosen={(id) => chosen({ quest: id })}
              onAdd={(q) => {
                setQuestNames((names) => ({ ...names, [q.id]: q }));
                toggle({ quest: q.id });
              }}
            />
          )}
        </section>
      )}
      {character && objectives.length > 0 && (
        <section className="panel">
          <h3>{t("Résumé")}</h3>
          {previewing && !preview && (
            <p className="muted">
              <span className="spinner inline" role="status" aria-hidden /> {t("Recherche des quêtes…")}
            </p>
          )}
          {previewError && <div className="error">{previewError}</div>}
          {preview && (
            <>
              <p>
                {preview.excluded.length
                  ? t("{n} quêtes au programme, {m} exclues.", { n: preview.quests, m: preview.excluded.length })
                  : t("{n} quêtes au programme.", { n: preview.quests })}
              </p>
              {preview.excluded.length > 0 && (
                <details className="objective-excluded">
                  <summary>{t("Quêtes exclues ({n})", { n: preview.excluded.length })}</summary>
                  <ul>
                    {preview.excluded.map((q) => (
                      <li key={q.id}>
                        {q.name} <span className="muted">#{q.id} · {excludedReason(t, q.reason)}</span>
                      </li>
                    ))}
                  </ul>
                </details>
              )}
              {preview.recommended != null && preview.recommended > level && (
                <div className="notice">
                  {t("Niveau {n} recommandé pour certaines de ces quêtes (votre personnage est niveau {level}). Le guide les fait quand même : à vous de voir.", {
                    n: preview.recommended,
                    level,
                  })}
                </div>
              )}
            </>
          )}
          <label className="objective-name">
            {t("Nom du guide")}
            <input type="text" value={name ?? ""} placeholder={autoName()} onChange={(e) => setName(e.target.value || null)} />
          </label>
          <div className="run-box" title={t("Plus le calcul est long, plus le guide est court. Sa durée dépend du processeur, le guide non.")}>
            <Segmented label={t("Temps de calcul")} choices={efforts.map((e) => ({ label: t(e.label), value: e.effort }))} value={effort} onChange={setEffort} />
          </div>
        </section>
      )}
      {character && (
        <div className="actions">
          <button className="primary" disabled={objectives.length === 0 || busy || preview?.quests === 0} title={busy ? t("Génération en cours") : undefined} onClick={generate}>
            {t("Générer le guide")}
          </button>
        </div>
      )}
    </div>
  );
}

/** Why the planner leaves a quest out, in the app's language when it is a known reason. */
function excludedReason(t: ReturnType<typeof useT>, reason: string): string {
  const known: Record<string, string> = {
    "a quest it needs first cannot be planned": "une quête qui la précède ne peut pas être planifiée",
    "not available to this character": "pas disponible pour ce personnage",
  };
  return known[reason] ? t(known[reason]) : reason;
}

/** The dungeons, by level, with the quests the character has left in each. */
function DungeonPicker({ options, counts, chosen, onToggle }: { options: Options; counts: ObjectiveCounts | null; chosen: (id: number) => boolean; onToggle: (id: number) => void }) {
  const t = useT();
  const areaName = useAreaName();
  const dungeons = useMemo(() => [...options.dungeons].filter((d) => d.level != null).sort((a, b) => (a.level ?? 0) - (b.level ?? 0)), [options.dungeons]);
  return (
    <div className="dungeon-grid" role="group" aria-label={t("Donjon")}>
      {dungeons.map((d) => {
        const n = counts?.dungeons[d.id];
        return (
          <button key={d.id} aria-pressed={chosen(d.id)} className={`dungeon${chosen(d.id) ? " forced" : ""}`} title={n != null ? t("{n} quêtes restantes", { n }) : undefined} onClick={() => onToggle(d.id)}>
            <span>{areaName(d)}</span>
            <small>
              {d.level}
              {n != null && <> · {n}</>}
            </small>
          </button>
        );
      })}
    </div>
  );
}

/** The zones with quests left for the character, by continent, found by name. */
function ZonePicker({ options, counts, chosen, onToggle }: { options: Options; counts: ObjectiveCounts | null; chosen: (id: number) => boolean; onToggle: (id: number) => void }) {
  const t = useT();
  const areaName = useAreaName();
  const [query, setQuery] = useState("");
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const zones = options.zones
    .filter((z) => (counts?.zones[z.id] ?? 0) > 0)
    .filter((z) => words.every((w) => areaName(z).toLowerCase().includes(w) || z.name.toLowerCase().includes(w)))
    .sort((a, b) => areaName(a).localeCompare(areaName(b)));
  const continents = [...new Set(zones.map((z) => z.continent))].sort((a, b) => a - b);
  return (
    <div className="zone-picker">
      <input type="search" value={query} placeholder={t("Chercher une zone")} onChange={(e) => setQuery(e.target.value)} autoFocus />
      {counts === null && <p className="muted">{t("Chargement…")}</p>}
      {counts !== null && zones.length === 0 && <p className="muted">{t("Aucune zone trouvée.")}</p>}
      {continents.map((c) => (
        <fieldset key={c}>
          <legend>{t(CONTINENTS[c] ?? `Continent ${c}`)}</legend>
          <div className="dungeon-grid">
            {zones
              .filter((z) => z.continent === c)
              .map((z) => (
                <button key={z.id} aria-pressed={chosen(z.id)} className={`dungeon${chosen(z.id) ? " forced" : ""}`} onClick={() => onToggle(z.id)}>
                  <span>{areaName(z)}</span>
                  <small title={t("{n} quêtes restantes", { n: counts?.zones[z.id] ?? 0 })}>{counts?.zones[z.id]}</small>
                </button>
              ))}
          </div>
        </fieldset>
      ))}
    </div>
  );
}

/** A quest found by name or ID among those the character can take and has not done. */
function QuestPicker({ race, klass, completed, chosen, onAdd }: { race: string; klass: string; completed: Set<number>; chosen: (id: number) => boolean; onAdd: (q: QuestChoice) => void }) {
  const t = useT();
  const lang = useLang();
  const [query, setQuery] = useState("");
  const [found, setFound] = useState<QuestChoice[]>([]);
  useEffect(() => {
    if (!query.trim()) return;
    let live = true;
    const timer = setTimeout(() => {
      api
        .questChoices(race, klass, query, gameLocale(lang))
        .then((list) => live && setFound(list.filter((q) => !completed.has(q.id))))
        .catch(() => {});
    }, 250);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [query, race, klass, completed, lang]);
  const shown = query.trim() ? found : [];
  return (
    <div className="quest-picker">
      <input type="search" value={query} placeholder={t("Nom ou numéro de la quête")} onChange={(e) => setQuery(e.target.value)} autoFocus />
      {query.trim() && shown.length === 0 && <p className="muted">{t("Aucune quête trouvée.")}</p>}
      <ul>
        {shown.map((q) => (
          <li key={q.id}>
            <button className={chosen(q.id) ? "active" : ""} disabled={chosen(q.id)} onClick={() => onAdd(q)}>
              <strong>{q.name}</strong> <span className="muted">[{q.level}]{q.zone ? ` · ${q.zone}` : ""} · #{q.id}</span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
