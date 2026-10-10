import { useCallback, useEffect, useRef, useState } from "react";
import { api, type GuideSummary, type GuideVersion } from "../api";
import { ResultView } from "../components/ResultView";
import { useDate, useGameName, useT } from "../i18n";

interface Props {
  onRerun: (version: GuideVersion) => void;
  onEdit: (version: GuideVersion) => void;
}

/** Every guide with its versions; a checkbox installs (the latest version of) a guide in the addon. */
export function GuidesPage({ onRerun, onEdit }: Props) {
  const t = useT();
  const date = useDate();
  const gameName = useGameName();
  const [guides, setGuides] = useState<GuideSummary[] | null>(null);
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
        <ResultView key={`${open.meta.guide}-${open.meta.version}`} version={open} onRerun={onRerun} onEdit={onEdit} onOpenVersion={(n) => openVersion(open.meta.guide, n)} />
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
  if (!guides) return error ? notices : <div className="empty">{t("Chargement…")}</div>;
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
            <th title={t("Installé dans l'addon")}>{t("Addon")}</th>
            <th>{t("Guide")}</th>
            <th>{t("Personnage")}</th>
            <th>{t("Niveaux")}</th>
            <th>{t("Temps estimé")}</th>
            <th>{t("Versions")}</th>
            <th>{t("Calculé le")}</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {guides.map((g) => {
            const latest = g.versions[0];
            const shown = g.versions.find((v) => v.version === g.installed) ?? latest;
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
                <td>{[latest.class, ...(latest.group ?? [])].map(gameName).join(", ")}</td>
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
