import { useEffect, useState } from "react";
import { api, fmtTime, type Edition, type GuideSummary, type GuideVersion, type Options, type Timeline, type VersionMeta } from "../api";
import { Character } from "./Character";
import { EditBar, useEditedView, useUndoKeys } from "./EditMode";
import { GuideViewer } from "./GuideViewer";
import { useDraft } from "./useDraft";
import { fmtGain } from "./OptimizeProgress";
import { RouteSummary } from "./RouteSummary";
import { StepTable } from "./StepTable";
import { gameLocale, useDate, useLang, useT } from "../i18n";
import { Icon } from "./Icons";
import { IssueForm } from "./IssueForm";
import { ExportMenu } from "./ExportMenu";

interface Props {
  version: GuideVersion;
  /** The game version the app works on (Polestar exists on WoW Forever only). */
  edition: Edition | null;
  /** Compute the same configuration again (new data, new algorithm): a new version. */
  onRerun: (version: GuideVersion) => void;
  /** Start a new configuration from this one. */
  onEdit: (version: GuideVersion) => void;
  /** A new guide from the same configuration (under another name). */
  onClone?: (version: GuideVersion) => void;
  /** The guide's steps edited by hand were saved: this new version. */
  onSaved?: (meta: VersionMeta) => void;
  /** The steps are edited and not saved: leaving the page asks first. */
  onUnsaved?: (unsaved: boolean) => void;
  /** Show another version of the guide. */
  onOpenVersion?: (n: number) => void;
  /** Compare two versions of the guide (`a` the older one). */
  onCompare?: (a: number, b: number) => void;
  /** Options of the game version (race names and factions), when loaded. */
  options?: Options | null;
  /** Open with its steps edited (not read only). */
  editing?: boolean;
}

/** Read a guide and its versions into `set` (null when it cannot be read). */
function fetchGuide(id: string, set: (guide: GuideSummary | null) => void) {
  api
    .getGuide(id)
    .then(set)
    .catch(() => set(null));
}

/** Time difference as "identique", "−3 min" (faster) or "+1h02" (slower). */
export function Delta({ seconds }: { seconds: number }) {
  const t = useT();
  if (Math.abs(seconds) < 60) return <span>{t("identique")}</span>;
  return <span className={seconds < 0 ? "better" : "worse"}>{seconds < 0 ? fmtGain(-seconds) : `+${fmtTime(seconds)}`}</span>;
}

/** A guide version: watched on its map (steps, character, time), its figures and versions, and what to do with it. */
export function ResultView({ version, edition, onRerun, onEdit, onClone, onSaved, onUnsaved, onOpenVersion, onCompare, options, editing }: Props) {
  const t = useT();
  const date = useDate();
  const lang = useLang();
  const { meta, outcome } = version;
  const route = outcome.route;
  // Read only, or its steps edited on the page (a draft, saved as a new version).
  const [readOnly, setReadOnly] = useState(!editing);
  // The guide played back: its replay comes a little after it (straight lines meanwhile). Asked
  // for once shown read only, not while the editor opens: the editor replays the draft itself.
  const [timeline, setTimeline] = useState<Timeline | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  useEffect(() => {
    if (!readOnly) return;
    let live = true;
    api
      .guideTimeline(meta.guide, meta.version, gameLocale(lang))
      .then((tl) => live && setTimeline(tl))
      .catch((e) => live && setFailed(String(e)));
    return () => {
      live = false;
    };
  }, [meta.guide, meta.version, lang, readOnly]);
  const draft = useDraft(version, !readOnly);
  const edited = useEditedView(version, draft);
  useUndoKeys(draft, !readOnly);
  const [saving, setSaving] = useState(false);
  const [reveal, setReveal] = useState<{ step: number; n: number } | undefined>(undefined);
  const unsaved = !readOnly && draft.changed;
  useEffect(() => onUnsaved?.(unsaved), [unsaved, onUnsaved]);
  useEffect(() => () => onUnsaved?.(false), [onUnsaved]);
  const startEditing = () => {
    setTab("map");
    setReporting(false);
    setReadOnly(false);
  };
  const stopEditing = () => {
    if (draft.changed && !confirm(t("Quitter l'édition sans enregistrer ? Les modifications seront perdues."))) return;
    setReadOnly(true);
  };
  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      const saved = await draft.save();
      onUnsaved?.(false);
      onSaved?.(saved);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };
  const [guide, setGuide] = useState<GuideSummary | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // The map and steps, or the figures and versions.
  const [tab, setTab] = useState<"map" | "summary">("map");
  // A problem report open, and the steps checked for it.
  const [reporting, setReporting] = useState(false);
  const [reported, setReported] = useState<Set<number>>(new Set());
  const toggleReported = (k: number) =>
    setReported((r) => {
      const next = new Set(r);
      if (!next.delete(k)) next.add(k);
      return next;
    });
  // The steps of a card, all checked or none.
  const toggleCard = (steps: number[]) =>
    setReported((r) => {
      const next = new Set(r);
      const on = !steps.every((k) => r.has(k));
      for (const k of steps) {
        if (on) next.add(k);
        else next.delete(k);
      }
      return next;
    });

  // The guide and its versions, read again after each action.
  useEffect(() => fetchGuide(meta.guide, setGuide), [meta.guide]);
  const loadGuide = () => fetchGuide(meta.guide, setGuide);

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
  // The version the global export sends to the game (the latest unless one is chosen).
  const follow = (n: number) =>
    act(async () => {
      await api.setInstalled(meta.guide, n);
      return t("La version {n} est celle de l'export global.", { n });
    });

  const versions = guide?.versions ?? [];
  const previous = versions.find((v) => v.version < meta.version);
  const followed = guide?.installed ?? versions[0]?.version;
  const installed = followed === meta.version;
  return (
    <div className="result-view">
      <header className="result-head compact">
        <div className="rh-title">
          <h2>
            {route.name} <span className="version-tag">v{meta.version}</span>
            {installed && versions.length > 1 && <span className="tag installed" title={t("Version envoyée au jeu par l'export global")}>{t("exportée")}</span>}
          </h2>
          <div className="muted result-meta">
            <Character race={version.request.race} klass={version.request.class} group={version.request.mates?.map((m) => m.class.toLowerCase())} options={options} /> ·{" "}
            {t("niveau {from} → {to}", { from: route.from_level, to: route.to_level })} · {route.total_time_text} ·{" "}
            <span title={meta.data ? t("Données : {data}", { data: meta.data }) : undefined}>{t("Calculé le {date}", { date: date(meta.created) })}</span>
            {meta.imported && <> · {t("importé")}</>}
            {meta.edited && <> · {t("modifié à la main")}</>}
            {meta.character && <> · {t("depuis {name}", { name: meta.character })}</>}
          </div>
          {route.recommended && version.request.start && (
            <div className="notice">
              {t("Niveau {n} recommandé pour ce guide : votre personnage était niveau {level} quand il a été calculé. Le guide fait quand même ces quêtes, à vous de voir.", {
                n: route.recommended.level,
                level: version.request.start.level,
              })}
              <details>
                <summary>{t("{n} quêtes au-dessus de son niveau", { n: route.recommended.quests.length })}</summary>
                <ul>
                  {route.recommended.quests.map((q) => (
                    <li key={q.id}>
                      {q.name} <span className="muted">{t("niveau {n}", { n: q.level })}</span>
                    </li>
                  ))}
                </ul>
              </details>
            </div>
          )}
          {/* A dungeon guide made before the objective guides: planned at the level its quests asked. */}
          {!route.recommended && version.request.dungeon != null && version.request.start && route.from_level > version.request.start.level && (
            <div className="notice">
              {t("Niveau {n} recommandé pour ce guide : votre personnage était niveau {level} quand il a été calculé. Il est fait comme au niveau {n}, à vous de voir.", {
                n: route.from_level,
                level: version.request.start.level,
              })}
            </div>
          )}
        </div>
        <div className="actions compact">
          <ExportMenu
            guide={meta.guide}
            version={meta.version}
            edition={edition}
            prominent
            onDone={(r) => {
              setError(r.error ?? null);
              setMessage(r.message || null);
            }}
          />
          {!installed && (
            <button onClick={() => follow(meta.version)} title={t("L'export global enverra cette version au jeu (sinon la plus récente).")}>
              {t("Utiliser cette version")}
            </button>
          )}
          <button className="icon-only" onClick={() => onRerun(version)} title={`${t("Relancer le calcul")} : ${t("Même configuration, données et algorithme actuels : nouvelle version")}`} aria-label={t("Relancer le calcul")}>
            <Icon name="rerun" />
          </button>
          <button className="icon-only" onClick={() => onEdit(version)} title={t("Modifier la configuration")} aria-label={t("Modifier la configuration")}>
            <Icon name="config" />
          </button>
          <button
            className={`icon-only${readOnly ? "" : " on"}`}
            onClick={startEditing}
            disabled={!readOnly}
            title={`${t("Modifier les étapes")} : ${t("Déplacer, supprimer ou ajouter des étapes ; le guide est rejoué à chaque modification")}`}
            aria-label={t("Modifier les étapes")}
          >
            <Icon name="steps" />
          </button>
          {onClone && (
            <button className="icon-only" onClick={() => onClone(version)} title={`${t("Cloner")} : ${t("Nouveau guide à partir de la même configuration")}`} aria-label={t("Cloner")}>
              <Icon name="clone" />
            </button>
          )}
          <button
            className="icon-only"
            onClick={() => setReporting(true)}
            disabled={reporting || !readOnly}
            title={`${t("Signaler un problème")} : ${t("Envoyer un signalement sur ce guide, avec les étapes concernées")}`}
            aria-label={t("Signaler un problème")}
          >
            <Icon name="report" />
          </button>
        </div>
        <label className="read-only" title={t("Désactivé : les étapes se déplacent, s'ajoutent et se suppriment sur la carte et dans la liste")}>
          <span className="switch">
            <input type="checkbox" checked={readOnly} onChange={(e) => (e.target.checked ? stopEditing() : startEditing())} />
            <span />
          </span>
          {t("Lecture seule")}
        </label>
        <div className="view-tabs" role="tablist">
          <button role="tab" aria-selected={tab === "map"} className={tab === "map" ? "active" : ""} onClick={() => setTab("map")}>
            {t("Carte et étapes")}
          </button>
          <button role="tab" aria-selected={tab === "summary"} className={tab === "summary" ? "active" : ""} onClick={() => setTab("summary")}>
            {t("Résumé")}
            {versions.length > 1 && <span className="badge">{versions.length}</span>}
          </button>
        </div>
      </header>
      {reporting && (
        <IssueForm
          guide={{ guide: meta.guide, version: meta.version, steps: route.steps }}
          selected={[...reported]}
          onUnselect={toggleReported}
          onSent={() => setReported(new Set())}
          onClose={() => {
            setReporting(false);
            setReported(new Set());
          }}
        />
      )}
      {message && <div className="success">{message}</div>}
      {error && <div className="error">{error}</div>}

      {!readOnly && (
        <EditBar
          version={version}
          draft={draft}
          saving={saving}
          onSave={save}
          onDiscard={stopEditing}
          onReveal={(step) => setReveal((r) => ({ step, n: (r?.n ?? 0) + 1 }))}
        />
      )}
      {tab === "map" ? (
        !readOnly && edited && draft.view ? (
          <GuideViewer
            key={`${meta.guide}-${meta.version}`}
            route={edited.route}
            played={edited.played}
            keys={edited.keys}
            timeline={draft.view.timeline}
            request={version.request}
            edit={edited.edit}
            computing={draft.computing}
            reveal={reveal}
          />
        ) : (
          <GuideViewer
            key={`${meta.guide}-${meta.version}`}
            route={route}
            timeline={timeline}
            failed={failed}
            request={version.request}
            onEditSteps={readOnly ? startEditing : undefined}
            checked={reporting ? reported : undefined}
            onCheck={reporting ? toggleCard : undefined}
          />
        )
      ) : (
        <>
          <RouteSummary
            route={route}
            notes={outcome.notes}
            compare={
              previous && (
                <>
                  <Delta seconds={meta.total_time - previous.total_time} /> {t("vs version {n}", { n: previous.version })}
                  {onCompare && (
                    <>
                      {" · "}
                      <button className="link" onClick={() => onCompare(previous.version, meta.version)}>
                        {t("Comparer")}
                      </button>
                    </>
                  )}
                </>
              )
            }
          />
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
                          {followed === v.version && <span className="tag installed" title={t("Version envoyée au jeu par l'export global")}>{t("exportée")}</span>}
                          {v.imported && <span className="tag">{t("importé")}</span>}
                          {v.edited && <span className="tag">{t("modifiée")}</span>}
                        </td>
                        <td>{date(v.created)}</td>
                        <td>{v.total_time_text}</td>
                        <td>{before ? <Delta seconds={v.total_time - before.total_time} /> : ""}</td>
                        <td className="row-actions">
                          {onCompare && v.version !== meta.version && (
                            <button
                              className="link"
                              title={t("Les étapes des deux versions côte à côte, leurs différences marquées")}
                              onClick={() => onCompare(Math.min(v.version, meta.version), Math.max(v.version, meta.version))}
                            >
                              {t("Comparer")}
                            </button>
                          )}
                          {followed !== v.version && (
                            <button className="link" onClick={() => follow(v.version)} title={t("L'export global enverra cette version au jeu (sinon la plus récente).")}>
                              {t("Utiliser")}
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
          <StepTable route={route} />
        </>
      )}
    </div>
  );
}
