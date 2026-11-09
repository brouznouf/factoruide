// A guide version edited on its page: its steps as a draft, replayed in the background after
// each change (the replay under way gives up for the newer one), and what could be added at a
// moment of it.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type EditOpened, type EditView, type GuideVersion, type RouteStep, type Suggestions, type VersionMeta } from "../api";
import { protectReputationPrograms } from "../reputationProgram";
import { gameLocale, useLang } from "../i18n";

/** A step of the draft, with a key kept while it moves (its replay is found by it). */
export interface Item {
  key: number;
  step: RouteStep;
}

let nextKey = 0;
export const item = (step: RouteStep): Item => ({ key: nextKey++, step });

const sameStep = (a: RouteStep, b: RouteStep) => JSON.stringify(a) === JSON.stringify(b);
/** What a replay given up for a newer one answers. */
const isSuperseded = (e: unknown) => String(e).includes("superseded");
/** Wait after the step under way changes before asking what could be added there. */
const SUGGEST_MS = 250;

/** A draft changed, and the keys of the steps changed (shown pending until replayed). */
export interface Changed {
  items: Item[];
  touched: number[];
}

export interface Draft {
  /** Opened in the editor (its catalog, the replay of the version as it is), null meanwhile. */
  opened: EditOpened | null;
  error: string | null;
  items: Item[];
  /** The last replay, and the draft it is of (an older one while the current one replays). */
  view: EditView | null;
  viewItems: Item[];
  /** A replay is under way. */
  computing: boolean;
  /** Keys of the steps changed since the replay shown. */
  pending: Set<number>;
  /** The draft differs from the version. */
  changed: boolean;
  canUndo: boolean;
  canRedo: boolean;
  /** A change of the draft as it is then. */
  change: (edit: (draft: Item[]) => Changed) => void;
  undo: () => void;
  /** Puts back the change undone last. */
  redo: () => void;
  save: () => Promise<VersionMeta>;
  /** What could be added after step `after` of the draft (null: before the first), and asking for it. */
  suggestions: Suggestions | null;
  suggestAfter: (after: number | null) => void;
}

/** The draft of `version`, opened in the editor while `active` (closed when it is not). */
export function useDraft(version: GuideVersion, active: boolean): Draft {
  const locale = gameLocale(useLang());
  const [opened, setOpened] = useState<EditOpened | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [items, setItems] = useState<Item[]>([]);
  // The drafts before each change, and those undone (the latest last).
  const [history, setHistory] = useState<Item[][]>([]);
  const [undone, setUndone] = useState<Item[][]>([]);
  const [replayed, setReplayed] = useState<{ items: Item[]; view: EditView } | null>(null);
  const [touched, setTouched] = useState<Set<number>>(new Set());
  const [suggestions, setSuggestions] = useState<Suggestions | null>(null);
  const [after, setAfter] = useState<number | null | undefined>(undefined);
  // The latest request: the answers of older ones are dropped.
  const asked = useRef(0);
  const saved = useRef(false);

  useEffect(() => {
    if (!active) return;
    let live = true;
    saved.current = false;
    setError(null);
    api
      .editOpen(version.meta.guide, version.meta.version, locale)
      .then((o) => {
        if (!live) return;
        const first = o.steps.map(item);
        setOpened(o);
        setItems(first);
        setHistory([]);
        setUndone([]);
        setTouched(new Set());
        setReplayed({ items: first, view: o.view });
      })
      .catch((e) => live && setError(String(e)));
    return () => {
      live = false;
      setOpened(null);
      setReplayed(null);
      setSuggestions(null);
      if (!saved.current) api.editClose().catch(() => {});
    };
    // Opened once per version and edit (the language is kept from the opening).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, version.meta.guide, version.meta.version]);

  const steps = useMemo(() => items.map((x) => x.step), [items]);
  const computing = !!opened && replayed?.items !== items;

  // Replay the draft after each change, at once: the replay under way gives up for it.
  useEffect(() => {
    if (!opened || replayed?.items === items) return;
    const n = ++asked.current;
    api
      .editSimulate(steps, locale)
      .then((view) => {
        if (n !== asked.current) return;
        setReplayed({ items, view });
        setTouched(new Set());
        setError(null);
      })
      .catch((e) => {
        if (n === asked.current && !isSuperseded(e)) setError(String(e));
      });
  }, [items, steps, opened, replayed, locale]);

  // What could be added where the guide is watched, once the replay is there (asking meanwhile
  // would make it give up).
  useEffect(() => {
    if (!opened || computing || after === undefined) return;
    const timer = setTimeout(() => {
      const n = ++asked.current;
      api
        .editSuggest(steps, after, locale)
        .then((s) => n === asked.current && setSuggestions(s))
        .catch(() => {});
    }, SUGGEST_MS);
    return () => clearTimeout(timer);
  }, [opened, computing, after, steps, locale]);

  // The draft as it is now, for the changes (kept the same function: the list's cards are only
  // drawn again when their own lines change).
  const now = useRef(items);
  now.current = items;
  const change = useCallback((edit: (draft: Item[]) => Changed) => {
    const before = now.current;
    const { items: proposed, touched: keys } = edit(before);
    const next = protectReputationPrograms(before, proposed);
    now.current = next;
    setHistory((h) => [...h.slice(-99), before]);
    setUndone([]);
    setItems(next);
    setTouched((t) => new Set([...t, ...keys]));
    setSuggestions(null);
  }, []);
  const undo = useCallback(() => {
    const previous = history[history.length - 1];
    if (!previous) return;
    setHistory(history.slice(0, -1));
    setUndone([...undone, now.current]);
    now.current = previous;
    setItems(previous);
    setSuggestions(null);
  }, [history, undone]);
  const redo = useCallback(() => {
    const next = undone[undone.length - 1];
    if (!next) return;
    setUndone(undone.slice(0, -1));
    setHistory([...history, now.current]);
    now.current = next;
    setItems(next);
    setSuggestions(null);
  }, [history, undone]);
  const save = useCallback(async () => {
    const meta = await api.editSave(steps);
    saved.current = true;
    return meta;
  }, [steps]);
  const changed = !!opened && !(steps.length === opened.steps.length && steps.every((s, i) => sameStep(s, opened.steps[i])));

  return {
    opened,
    error,
    items,
    view: replayed?.view ?? null,
    viewItems: replayed?.items ?? [],
    computing,
    pending: touched,
    changed,
    canUndo: history.length > 0,
    canRedo: undone.length > 0,
    change,
    undo,
    redo,
    save,
    suggestions,
    suggestAfter: setAfter,
  };
}
