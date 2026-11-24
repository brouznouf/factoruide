import { useCallback, useEffect, useMemo } from "react";
import { fmtTime, type GuideVersion, type Route, type RouteStep, type Step } from "../api";
import { useT } from "../i18n";
import { draftLine, useNames } from "./editing";
import type { ViewerEdit } from "./GuideViewer";
import { Delta } from "./ResultView";
import { item, type Draft } from "./useDraft";

/** What the guide viewer shows of a draft: its lines in the draft's order, what is played (the last replay), and the editing. */
export interface EditedView {
  route: Route;
  played: Route;
  keys: { shown: number[]; played: number[] };
  edit: ViewerEdit;
}

/**
 * A draft as the viewer shows it: each step's line from the last replay (a line written from
 * the step itself until it is replayed), and the changes made from the list and the map.
 */
export function useEditedView(version: GuideVersion, draft: Draft): EditedView | null {
  const t = useT();
  const names = useNames(draft.opened?.catalog);
  const { items, view, viewItems, pending, change } = draft;
  const label = useCallback((s: RouteStep) => names.label(s, t), [names, t]);
  // The lines of the last replay, by key; those the replay did not write say what they are.
  const replayed = useMemo(() => {
    const out = new Map<number, Step>();
    viewItems.forEach((x, i) => {
      const line = view?.lines[i];
      if (line) out.set(x.key, line.text ? line : { ...line, text: label(x.step) });
    });
    return out;
  }, [view, viewItems, label]);
  const reports = useMemo(() => new Map(viewItems.map((x, i) => [x.key, view?.report.steps[i]])), [view, viewItems]);

  const onMove = useCallback(
    (first: number, last: number, to: number) => {
      if (to >= first && to <= last + 1) return;
      change((list) => {
        const moved = list.slice(first, last + 1);
        const rest = [...list.slice(0, first), ...list.slice(last + 1)];
        const at = to > last ? to - moved.length : to;
        return { items: [...rest.slice(0, at), ...moved, ...rest.slice(at)], touched: moved.map((x) => x.key) };
      });
    },
    [change],
  );
  const onRemove = useCallback((i: number) => change((list) => ({ items: list.filter((_, k) => k !== i), touched: [] })), [change]);
  const onReplace = useCallback(
    (i: number, step: RouteStep) => change((list) => ({ items: list.map((x, k) => (k === i ? { ...x, step } : x)), touched: [list[i].key] })),
    [change],
  );
  // A dungeon is run once: adding a run the draft has moves it there.
  const onInsert = useCallback(
    (after: number, step: RouteStep) => {
      change((list) => {
        const run = step.kind === "dungeon" ? list.findIndex((x) => x.step.kind === "dungeon" && x.step.dungeon === step.dungeon) : -1;
        const added = run >= 0 ? list[run] : item(step);
        const rest = run >= 0 ? list.filter((_, k) => k !== run) : list;
        const at = run >= 0 && run <= after ? after : after + 1;
        return { items: [...rest.slice(0, at), added, ...rest.slice(at)], touched: [added.key] };
      });
    },
    [change],
  );

  return useMemo(() => {
    const catalog = draft.opened?.catalog;
    if (!view || !catalog) return null;
    const route = version.outcome.route;
    const lines: Step[] = [];
    for (const x of items) lines.push(replayed.get(x.key) ?? draftLine(x.step, label(x.step), lines.at(-1) ?? null));
    const played = viewItems.map((x) => replayed.get(x.key)!);
    const total = view.report.total_time;
    return {
      route: { ...route, steps: lines, total_time: total, total_time_text: view.report.total_time_text },
      played: { ...route, steps: played, total_time: total, total_time_text: view.report.total_time_text },
      keys: { shown: items.map((x) => x.key), played: viewItems.map((x) => x.key) },
      edit: {
        list: {
          steps: items.map((x) => x.step),
          reports: items.map((x) => reports.get(x.key)),
          pending: items.map((x) => pending.has(x.key) || !replayed.has(x.key)),
          onMove,
          onRemove,
          onReplace,
          finalGrind: view.report.final_grind,
          toLevel: route.to_level,
        },
        catalog,
        names,
        suggestions: draft.suggestions,
        onSuggestAt: draft.suggestAfter,
        onInsert,
      },
    };
  }, [draft.opened, draft.suggestions, draft.suggestAfter, view, viewItems, items, replayed, reports, pending, label, names, version, onMove, onRemove, onReplace, onInsert]);
}

/**
 * Undo (Ctrl+Z) and redo (Ctrl+Y, Ctrl+Shift+Z) while editing: not while typing in a field or
 * choosing in the add menu (they keep their own).
 */
export function useUndoKeys(draft: Draft, active: boolean) {
  const { undo, redo } = draft;
  useEffect(() => {
    if (!active) return;
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
      const el = e.target instanceof Element ? e.target : null;
      if (el?.closest("input, textarea, select, [contenteditable], .add-menu")) return;
      const key = e.key.toLowerCase();
      const action = key === "z" ? (e.shiftKey ? redo : undo) : key === "y" && !e.shiftKey ? redo : null;
      if (!action) return;
      e.preventDefault();
      action();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [active, undo, redo]);
}

interface BarProps {
  version: GuideVersion;
  draft: Draft;
  saving: boolean;
  onSave: () => void;
  onDiscard: () => void;
  /** Show a step (the first impossible one). */
  onReveal: (step: number) => void;
}

/**
 * Editing the steps: the time of the draft against the version's, a spinner while it replays,
 * the impossible steps (saving is refused while there are), a guide left incomplete, and undo,
 * save (a new version) or discard.
 */
export function EditBar({ version, draft, saving, onSave, onDiscard, onReveal }: BarProps) {
  const t = useT();
  const { opened, view, computing } = draft;
  if (!opened) {
    return (
      <div className="edit-bar">
        {draft.error ? <span className="error-text">{draft.error}</span> : <span className="spinner" role="status" aria-label={t("Ouverture du guide dans l'éditeur…")} />}
        <span className="muted">{draft.error ? "" : t("Ouverture du guide dans l'éditeur…")}</span>
        <button onClick={onDiscard}>{t("Annuler")}</button>
      </div>
    );
  }
  const baseline = opened.view.report;
  const report = view?.report ?? baseline;
  const current = !computing;
  const errors = current ? report.errors : 0;
  const firstError = current ? report.steps.findIndex((r) => r.status === "error") : -1;
  // Quests left unfinished by the edit (the version's own do not count), the grind added at the end
  // (all of it for a guide created by hand: its version has no step, only that grind).
  const before = new Set(baseline.unfinished.map((u) => u.id));
  const unfinished = report.unfinished.filter((u) => !before.has(u.id));
  const shortfall = report.final_grind - (opened.steps.length ? baseline.final_grind : 0);
  const canSave = current && errors === 0 && draft.changed && !saving;
  return (
    <div className="edit-bar">
      <span className="tag draft">{t("brouillon")}</span>
      <span className={computing ? "edit-time pending" : "edit-time"}>
        <strong>{report.total_time_text}</strong> <Delta seconds={report.total_time - baseline.total_time} /> {t("vs version {n}", { n: version.meta.version })}
      </span>
      {computing && <span className="spinner" role="status" aria-label={t("Le guide est rejoué…")} title={t("Le guide est rejoué…")} />}
      {errors > 0 && (
        <span className="error-text">
          {t("{n} étape(s) impossible(s) : le guide ne peut pas être enregistré.", { n: errors })}{" "}
          {firstError >= 0 && (
            <button className="link" onClick={() => onReveal(firstError)}>
              {t("Voir la première")}
            </button>
          )}
        </span>
      )}
      {current && (unfinished.length > 0 || shortfall >= 60) && (
        <span
          className="warn-text"
          title={[
            unfinished.length > 0 ? t("Quêtes prises jamais rendues : {list}", { list: unfinished.map((u) => u.name).join(", ") }) : "",
            shortfall >= 60 ? t("Les étapes n'amènent plus au niveau {level} : {time} de farm ajoutés à la fin.", { level: version.outcome.route.to_level, time: fmtTime(shortfall) }) : "",
          ]
            .filter(Boolean)
            .join("\n")}
        >
          {t("Guide incomplet")} — {t("il peut tout de même être enregistré.")}
        </span>
      )}
      {draft.error && <span className="error-text">{draft.error}</span>}
      {opened.rebuilt && <span className="muted" title={t("Ce guide a été calculé avant l'éditeur : ses étapes ont été relues depuis le guide, son temps peut un peu différer.")}>ⓘ</span>}
      <span className="edit-bar-fill" />
      <button onClick={draft.undo} disabled={!draft.canUndo} title="Ctrl+Z">
        {t("Annuler la dernière modification")}
      </button>
      <button onClick={draft.redo} disabled={!draft.canRedo} title="Ctrl+Y / Ctrl+Shift+Z">
        {t("Rétablir")}
      </button>
      <button className="primary" disabled={!canSave} onClick={onSave} title={errors > 0 ? t("Corrigez les étapes impossibles pour enregistrer") : undefined}>
        {saving ? t("Enregistrement…") : t("Enregistrer (nouvelle version)")}
      </button>
      <button onClick={onDiscard}>{draft.changed ? t("Abandonner les modifications") : t("Fermer")}</button>
    </div>
  );
}
