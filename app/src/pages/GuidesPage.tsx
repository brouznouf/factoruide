import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type GuideSummary, type GuideVersion, type Options, type VersionMeta } from "../api";
import { Character } from "../components/Character";
import { ResultView } from "../components/ResultView";
import { useDate, useGameName, useT } from "../i18n";

interface Props {
  /** Options of the game version (race names and factions), when loaded. */
  options: Options | null;
  onRerun: (version: GuideVersion) => void;
  onEdit: (version: GuideVersion) => void;
}

type SortKey = "addon" | "name" | "character" | "levels" | "time" | "versions" | "created";

/** The version a row shows: the one installed, else the latest. */
const shownVersion = (g: GuideSummary): VersionMeta => g.versions.find((v) => v.version === g.installed) ?? g.versions[0];

/** Every guide with its versions; a checkbox installs (the latest version of) a guide in the addon. */
export function GuidesPage({ options, onRerun, onEdit }: Props) {
  const t = useT();
  const date = useDate();
  const gameName = useGameName();
  const [guides, setGuides] = useState<GuideSummary[] | null>(null);
  // Column sorted by (none: the saved order), ascending or not.
  const [sort, setSort] = useState<{ key: SortKey; asc: boolean } | null>(null);
  const [open, setOpen] = useState<GuideVersion | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);

  const load = useCallback(() => {
    api.listGuides().then(setGuides).catch((e) => setError(String(e)));
  }, []);
  useEffect(load, [load]);

  const act = async (action: () => Promise<string | void>) => {
    setError(null);
    setMessage(null);
    try {
      const m = await action();
      if (m) setMessage(m);
      load();
    } catch (e) {
      setError(String(e));
    }
  };
  const openVersion = (guide: string, version: number) => api.getVersion(guide, version).then(setOpen).catch((e) => setError(String(e)));

  const sorted = useMemo(() => {
    if (!guides || !sort) return guides;
    const character = (v: VersionMeta) => `${gameName(options?.races.find((r) => r.key === v.race)?.name ?? v.race)} ${gameName(v.class)}`;
    const compare: Record<SortKey, (a: GuideSummary, b: GuideSummary) => number> = {
      addon: (a, b) => Number(a.installed != null) - Number(b.installed != null),
      name: (a, b) => a.name.localeCompare(b.name),
      character: (a, b) => character(a.versions[0]).localeCompare(character(b.versions[0])),
      levels: (a, b) => shownVersion(a).from_level - shownVersion(b).from_level || shownVersion(a).to_level - shownVersion(b).to_level,
      time: (a, b) => shownVersion(a).total_time - shownVersion(b).total_time,
      versions: (a, b) => a.versions[0].version - b.versions[0].version,
      created: (a, b) => a.versions[0].created - b.versions[0].created,
    };
    return [...guides].sort((a, b) => (sort.asc ? 1 : -1) * compare[sort.key](a, b) || a.name.localeCompare(b.name));
  }, [guides, sort, gameName, options]);

  /** A column header sorting the table: ascending first, then descending. */
  const header = (key: SortKey, label: string, title?: string) => (
    <th aria-sort={sort?.key === key ? (sort.asc ? "ascending" : "descending") : "none"} title={title}>
      <button className="sort" onClick={() => setSort(sort?.key === key ? { key, asc: !sort.asc } : { key, asc: true })}>
        {label}
        <span className="sort-arrow" aria-hidden="true">
          {sort?.key === key ? (sort.asc ? "▲" : "▼") : "↕"}
        </span>
      </button>
    </th>
  );

  if (open) {
    return (
      <div>
        <button
          className="link"
          onClick={() => {
            setOpen(null);
            load();
          }}
        >
          {t("← Tous les guides")}
        </button>
        <ResultView
          key={`${open.meta.guide}-${open.meta.version}`}
          version={open}
          options={options}
          onRerun={onRerun}
          onEdit={onEdit}
          onOpenVersion={(n) => openVersion(open.meta.guide, n)}
        />
      </div>
    );
  }

  const bar = (
    <div className="results-bar">
      <h2>{t("Mes guides")}</h2>
      <input
        ref={fileRef}
        type="file"
        accept=".fgguide,.bqguide,.bqroute,application/json"
        hidden
        onChange={(e) => {
          const file = e.target.files?.[0];
          if (file) act(async () => t("Guide « {name} » importé", { name: (await api.importGuide(await file.text())).name }));
          e.target.value = "";
        }}
      />
      <button onClick={() => fileRef.current?.click()} title={t("Ajoute un guide reçu d'un autre joueur (fichier .fgguide)")}>
        {t("Importer un guide…")}
      </button>
    </div>
  );
  const notices = (
    <>
      {message && <div className="success">{message}</div>}
      {error && <div className="error">{error}</div>}
    </>
  );
  if (!guides || !sorted) return error ? notices : <div className="empty">{t("Chargement…")}</div>;
  if (guides.length === 0)
    return (
      <div className="results">
        {bar}
        {notices}
        <div className="empty">{t("Aucun résultat pour l'instant : lancez une génération depuis « Nouveau guide ».")}</div>
      </div>
    );

  return (
    <div className="results">
      {bar}
      {notices}
      <table className="table">
        <thead>
          <tr>
            {header("addon", t("Addon"), t("Installé dans l'addon"))}
            {header("name", t("Guide"))}
            {header("character", t("Personnage"))}
            {header("levels", t("Niveaux"))}
            {header("time", t("Temps estimé"))}
            {header("versions", t("Versions"))}
            {header("created", t("Calculé le"))}
            <th />
          </tr>
        </thead>
        <tbody>
          {sorted.map((g) => {
            const latest = g.versions[0];
            const shown = shownVersion(g);
            return (
              <tr key={g.id}>
                <td>
                  <input
                    type="checkbox"
                    aria-label={t("Installer « {name} » dans l'addon", { name: g.name })}
                    checked={g.installed != null}
                    onChange={(e) =>
                      act(async () => t("Guides dans l'addon : {n} — faites /reload en jeu", { n: await api.setInstalled(g.id, e.target.checked ? latest.version : null) }))
                    }
                  />
                </td>
                <td>
                  <button className="link" onClick={() => openVersion(g.id, shown.version)}>
                    {g.name}
                  </button>
                  {g.installed != null && g.installed !== latest.version && (
                    <span className="tag" title={t("Une version plus récente existe")}>
                      {t("v{n} installée", { n: g.installed })}
                    </span>
                  )}
                  {latest.imported && <span className="tag">{t("importé")}</span>}
                </td>
                <td>
                  <Character race={latest.race} klass={latest.class} group={latest.group} options={options} />
                </td>
                <td>
                  {shown.from_level} → {shown.to_level}
                </td>
                <td>{shown.total_time_text}</td>
                <td>v{latest.version}</td>
                <td>{date(latest.created)}</td>
                <td>
                  <button
                    className="link danger"
                    onClick={() => {
                      if (confirm(t("Supprimer le guide « {name} » et ses {n} versions ?", { name: g.name, n: g.versions.length }))) act(() => api.deleteGuide(g.id));
                    }}
                  >
                    {t("Supprimer")}
                  </button>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
