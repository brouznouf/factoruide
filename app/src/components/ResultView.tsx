import { useCallback, useEffect, useMemo, useState } from "react";
import { api, fmtTime, type GuideSummary, type GuideVersion } from "../api";
import { fmtGain } from "./OptimizeProgress";
import { RouteView } from "./RouteView";
import { useDate, useGameName, useT } from "../i18n";
import { WorldMap, routeMarkers, type ContinentView, type WorldPoint } from "./WorldMap";

interface Props {
  version: GuideVersion;
  /** Compute the same configuration again (new data, new algorithm): a new version. */
  onRerun: (version: GuideVersion) => void;
  /** Start a new configuration from this one. */
  onEdit: (version: GuideVersion) => void;
  /** Show another version of the guide. */
  onOpenVersion?: (n: number) => void;
}

/** Time difference as "identique", "−3 min" (faster) or "+1h02" (slower). */
function Delta({ seconds }: { seconds: number }) {
  const t = useT();
  if (Math.abs(seconds) < 60) return <span>{t("identique")}</span>;
  return <span className={seconds < 0 ? "better" : "worse"}>{seconds < 0 ? fmtGain(-seconds) : `+${fmtTime(seconds)}`}</span>;
}

/** A guide version: its figures, its versions, its map and steps, and what to do with it. */
export function ResultView({ version, onRerun, onEdit, onOpenVersion }: Props) {
  const t = useT();
  const date = useDate();
  const gameName = useGameName();
  const { meta, outcome } = version;
  const route = outcome.route;
  const [guide, setGuide] = useState<GuideSummary | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [continent, setContinent] = useState<ContinentView>("both");

  const loadGuide = useCallback(() => {
    api.getGuide(meta.guide).then(setGuide).catch(() => setGuide(null));
  }, [meta.guide]);
  useEffect(loadGuide, [loadGuide]);

  const line = useMemo(() => route.steps.filter((s) => s.world).map((s) => s.world as WorldPoint), [route]);
  const markers = useMemo(() => routeMarkers(route.steps, (zone, level) => t("{zone} · niveau {level}", { zone, level })), [route, t]);

  const act = async (action: () => Promise<string | void>) => {
    setError(null);
    setMessage(null);
    try {
      const m = await action();
      if (m) setMessage(m);
      loadGuide();
    } catch (e) {
      setError(String(e));
    }
  };
  const install = (n: number | null) =>
    act(async () => t("Guides dans l'addon : {n} — faites /reload en jeu", { n: await api.setInstalled(meta.guide, n) }));

  const versions = guide?.versions ?? [];
  const previous = versions.find((v) => v.version < meta.version);
  const installed = guide?.installed === meta.version;
  return (
    <div className="result-view">
      <header className="result-head">
        <div>
          <h2>
            {route.name} <span className="version-tag">v{meta.version}</span>
            {installed && <span className="tag installed">{t("installée")}</span>}
          </h2>
          <div className="muted">
            {t("Calculé le {date} · niveau {from} → {to}", { date: date(meta.created), from: route.from_level, to: route.to_level })}
            {meta.data && <> · {meta.data.split(":")[0]}</>}
            {meta.imported && <> · {t("importé")}</>}
            {(version.request.group?.length ?? 0) > 0 && (
              <> · {t("groupe : {classes}", { classes: [version.request.class, ...(version.request.group ?? [])].map(gameName).join(", ") })}</>
            )}
          </div>
        </div>
        <div className="result-figures">
          <div>
            <strong>{route.total_time_text}</strong>
            <small>{t("temps de jeu estimé")}</small>
          </div>
          <div>
            <strong>{route.quests}</strong>
            <small>{t("quêtes")}</small>
          </div>
          {previous && (
            <div>
              <strong>
                <Delta seconds={meta.total_time - previous.total_time} />
              </strong>
              <small>{t("vs version {n}", { n: previous.version })}</small>
            </div>
          )}
        </div>
      </header>
      <div className="actions">
        {installed ? (
          <button onClick={() => install(null)}>{t("Retirer de l'addon")}</button>
        ) : (
          <button className="primary" onClick={() => install(meta.version)}>
            {guide?.installed ? t("Installer cette version (remplace la v{n})", { n: guide.installed }) : t("Installer dans l'addon")}
          </button>
        )}
        <button onClick={() => onRerun(version)} title={t("Même configuration, données et algorithme actuels : nouvelle version")}>
          {t("Relancer le calcul")}
        </button>
        <button onClick={() => onEdit(version)}>{t("Modifier la configuration")}</button>
        <button
          onClick={() => act(async () => t("Fichier à partager enregistré : {path}", { path: await api.exportVersion(meta.guide, meta.version) }))}
          title={t("Enregistre un fichier .fgguide (configuration et guide) à envoyer aux autres joueurs")}
        >
          {t("Partager")}
        </button>
      </div>
      {message && <div className="success">{message}</div>}
      {error && <div className="error">{error}</div>}

      {versions.length > 1 && (
        <section className="panel">
          <h3>{t("Versions")}</h3>
          <table className="table versions">
            <tbody>
              {versions.map((v, i) => {
                const before = versions[i + 1];
                return (
                  <tr key={v.version} className={v.version === meta.version ? "selected" : ""}>
                    <td>
                      {onOpenVersion && v.version !== meta.version ? (
                        <button className="link" onClick={() => onOpenVersion(v.version)}>
                          v{v.version}
                        </button>
                      ) : (
                        <strong>v{v.version}</strong>
                      )}
                      {guide?.installed === v.version && <span className="tag installed">{t("installée")}</span>}
                      {v.imported && <span className="tag">{t("importé")}</span>}
                    </td>
                    <td>{date(v.created)}</td>
                    <td>{v.total_time_text}</td>
                    <td>{before ? <Delta seconds={v.total_time - before.total_time} /> : ""}</td>
                    <td className="row-actions">
                      {guide?.installed !== v.version && (
                        <button className="link" onClick={() => install(v.version)}>
                          {t("Installer")}
                        </button>
                      )}
                      {v.version !== meta.version && (
                        <button
                          className="link danger"
                          onClick={() => {
                            if (confirm(t("Supprimer la version {n} ?", { n: v.version }))) act(() => api.deleteVersion(meta.guide, v.version));
                          }}
                        >
                          {t("Supprimer")}
                        </button>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </section>
      )}

      <section className="panel">
        <h3>{t("Carte du guide")}</h3>
        <p className="muted">{t("Les pastilles indiquent le niveau en arrivant dans chaque grande étape (survolez pour la zone).")}</p>
        <WorldMap continent={continent} onContinent={setContinent} route={line} markers={markers} height={560} />
      </section>
      <RouteView route={route} notes={outcome.notes} />
    </div>
  );
}
