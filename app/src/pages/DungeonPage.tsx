import { useCallback, useEffect, useMemo, useState } from "react";
import { api, type CharacterInfo, type Options, type PlanRequest } from "../api";
import { dungeonLevels } from "../components/DungeonPanel";
import { ClassName } from "../components/Character";
import { ProfilePaste } from "../web/components/ProfilePaste";
import { gameLocale, useAreaName, useLang, useT } from "../i18n";

interface Props {
  options: Options;
  /** A dungeon guide's configuration to start from (its character and dungeon picked). */
  initial?: PlanRequest | null;
  /** A generation is running: no other one. */
  busy: boolean;
  /** Generate the guide (the new guide page shows its progress, then the guide). */
  onGenerate: (request: PlanRequest) => void;
}

/**
 * A dungeon guide: from a recorded character, every quest of the chosen dungeon, with the quests
 * leading to them, in the best order; nothing else. Planned at the level its quests need: the
 * guide says so when the character is below it.
 */
export function DungeonPage({ options, initial, busy, onGenerate }: Props) {
  const t = useT();
  const lang = useLang();
  const areaName = useAreaName();
  const [characters, setCharacters] = useState<CharacterInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [picked, setPicked] = useState<string | null>(initial?.start?.name ?? null);
  const [dungeon, setDungeon] = useState<number | null>(initial?.dungeon ?? null);
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
  const paste = pasting ? (
    <ProfilePaste
      compact
      onImported={(name) => {
        setPasting(false);
        setPicked(name);
        load();
      }}
    />
  ) : (
    <button className="link" onClick={() => setPasting(true)}>
      {t("Coller l'export d'un personnage (en jeu : /fg, « Exporter mon personnage »)…")}
    </button>
  );
  const character = characters?.find((c) => c.profile.name === picked) ?? characters?.[0] ?? null;
  const level = character?.profile.level ?? 1;
  const { dungeon_from: from, dungeon_to: to } = options.params;
  // By level, those whose levels hold the character's first.
  const dungeons = useMemo(() => [...options.dungeons].filter((d) => d.level != null).sort((a, b) => (a.level ?? 0) - (b.level ?? 0)), [options.dungeons]);
  const near = (at: number, d: { level: number | null }) => {
    const range = d.level == null ? null : dungeonLevels(d.level, from, to, options.max_level);
    return range != null && at >= range[0] && at <= range[1];
  };
  const chosen = dungeons.find((d) => d.id === dungeon) ?? null;
  const firstLevel = chosen?.level != null ? dungeonLevels(chosen.level, from, to, options.max_level)?.[0] ?? chosen.level : null;

  const generate = () => {
    if (!character?.race || !chosen) return;
    onGenerate({
      race: character.race,
      class: character.class,
      from_level: level,
      to_level: level,
      name: `${(character.full_name ?? character.profile.name).split("-")[0]} — ${areaName(chosen)}`,
      params: options.params,
      required_class_quests: [],
      required_quests: [],
      professions: [],
      locale: gameLocale(lang),
      start: character.profile,
      mates: [],
      mate_professions: [],
      dungeon: chosen.id,
    });
  };

  return (
    <div className="dungeon-page">
      <div className="page-head">
        <h2>{t("Guide de donjon")}</h2>
      </div>
      <p className="muted">
        {t("Toutes les quêtes d'un donjon, avec celles qui y mènent, dans le meilleur ordre depuis là où en est votre personnage : rien d'autre. Le guide finit une fois les quêtes rendues.")}
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
          <h3>{t("Donjon")}</h3>
          <div className="dungeon-grid" role="radiogroup" aria-label={t("Donjon")}>
            {dungeons.map((d) => (
              <button
                key={d.id}
                role="radio"
                aria-checked={d.id === dungeon}
                className={`dungeon${d.id === dungeon ? " forced" : ""}${near(level, d) ? " near" : ""}`}
                title={near(level, d) ? t("À votre niveau") : undefined}
                onClick={() => setDungeon(d.id)}
              >
                <span>{areaName(d)}</span>
                <small>{d.level}</small>
              </button>
            ))}
          </div>
        </section>
      )}
      {character && chosen && firstLevel != null && level < firstLevel && (
        <div className="notice">
          {t("{dungeon} se fait plutôt à partir du niveau {n} (votre personnage est niveau {level}). Le guide est calculé au niveau que demandent ses quêtes : à vous de voir.", {
            dungeon: areaName(chosen),
            n: firstLevel,
            level,
          })}
        </div>
      )}
      {character && (
        <div className="actions">
          <button className="primary" disabled={!chosen || busy} title={busy ? t("Génération en cours") : undefined} onClick={generate}>
            {t("Générer le guide")}
          </button>
        </div>
      )}
    </div>
  );
}
