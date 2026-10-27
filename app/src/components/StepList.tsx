import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { type Farmed, fmtTime, type QuestInfo, type RouteStep, type Step, type StepReport } from "../api";
import { useT } from "../i18n";
import { REASONS } from "./editing";
import { NumberInput } from "./NumberInput";
import { type Card, fmtClock, isCheckpoint, questOf, type Spans, splitOf, stepStart } from "./guideTimeline";
import type { Segment } from "./segments";
import { KIND_LABELS } from "./segments";

/** Heights (pixels) of a zone's head and of a step's line: fixed, so that only the lines in view are drawn. */
const ZONE_H = 44;
const ROW_H = 26;
/** Lines drawn past the view, above and below (scrolling shows no gap). */
const OVERSCAN = 12;
/** Milliseconds the pointer stays on a line before its tip shows. */
const HOVER_DELAY = 350;

interface Props {
  steps: Step[];
  cards: Card[];
  titles: string[];
  /** The zones (a dungeon run apart), by steps. */
  segments: Segment[];
  /** The timeline of each step (none while the replay loads). */
  spans: Spans;
  quests: Map<number, QuestInfo>;
  /** Card under way. */
  current: number;
  onSelect: (card: number) => void;
  /** Cards checked for a problem report, and checking one (a box on each card when given). */
  checked?: Set<number>;
  onCheck?: (card: number) => void;
  /** Editing: what each line is in the draft, and how to change it. */
  edit?: ListEdit;
}

/** The draft behind the lines of the list being edited, a line per step. */
export interface ListEdit {
  steps: RouteStep[];
  /** The replay of each step (none: not replayed yet), and those changed since the replay shown. */
  reports: (StepReport | undefined)[];
  pending: boolean[];
  /** Move the lines `first..=last` before line `to` (the number of lines: at the end). */
  onMove: (first: number, last: number, to: number) => void;
  onRemove: (i: number) => void;
  onReplace: (i: number, step: RouteStep) => void;
  /** Open the menu to add a step after line `i` (-1: at the start), beside a point of the window. */
  onAdd: (i: number, anchor: { x: number; y: number }) => void;
  /** Grinding added after the last step to reach the target level (seconds), and that level. */
  finalGrind: number;
  toLevel: number;
}

/** The lines dragged ("first-last"). */
const DRAG_TYPE = "text/plain";

/** What the list shows, top to bottom: zone heads, a line per step, the final grind. */
type Item = { kind: "zone"; seg: Segment; next: string | null } | { kind: "step"; i: number } | { kind: "final" };

/**
 * The guide's steps grouped by zone, a line each (more on hover). Only the lines in view are
 * drawn: a guide has thousands of steps.
 */
export function StepList({ steps, cards, titles, segments, spans, quests, current, onSelect, checked, onCheck, edit }: Props) {
  const t = useT();
  const scroller = useRef<HTMLDivElement>(null);
  const [drop, setDrop] = useState<number | null>(null);
  const [view, setView] = useState({ top: 0, height: 0 });
  // The line hovered a moment, and where it is in the window (its tip goes beside it).
  const [hover, setHover] = useState<{ i: number; at: DOMRect } | null>(null);
  const hoverTimer = useRef(0);
  const onHover = useCallback((i: number | null, el?: Element) => {
    clearTimeout(hoverTimer.current);
    if (i == null || !el) return setHover(null);
    hoverTimer.current = window.setTimeout(() => setHover({ i, at: el.getBoundingClientRect() }), HOVER_DELAY);
  }, []);
  useEffect(() => () => clearTimeout(hoverTimer.current), []);

  // The card of each step, the items and where each starts.
  const finalGrind = !!edit && edit.finalGrind >= 1;
  const cardOfStep = useMemo(() => {
    const out = new Array<number>(steps.length).fill(0);
    for (const c of cards) for (let i = c.first; i <= c.last; i++) out[i] = c.n;
    return out;
  }, [steps, cards]);
  const { items, tops, height, rowOf } = useMemo(() => {
    const list: Item[] = [];
    segments.forEach((seg, g) => {
      list.push({ kind: "zone", seg, next: segments[g + 1]?.zone ?? null });
      for (let i = seg.first; i <= seg.last; i++) list.push({ kind: "step", i });
    });
    if (finalGrind) list.push({ kind: "final" });
    const starts: number[] = [];
    const rows = new Map<number, number>();
    let y = 0;
    list.forEach((it, k) => {
      starts.push(y);
      if (it.kind === "step") rows.set(it.i, k);
      y += it.kind === "zone" ? ZONE_H : ROW_H;
    });
    return { items: list, tops: starts, height: y, rowOf: rows };
  }, [segments, finalGrind]);

  // The part in view, followed while scrolling and resizing.
  const measure = useCallback(() => {
    const el = scroller.current;
    if (el) setView((v) => (v.top === el.scrollTop && v.height === el.clientHeight ? v : { top: el.scrollTop, height: el.clientHeight }));
  }, []);
  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el) return;
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [measure]);

  // The card under way stays in view (below the zone's head stuck at the top): when it changes
  // only, not when the steps do (an edit leaves the list where it is).
  const shownCurrent = useRef(-1);
  useEffect(() => {
    const el = scroller.current;
    const k = cards[current] && rowOf.get(cards[current].first);
    if (!el || k == null || shownCurrent.current === current) return;
    shownCurrent.current = current;
    if (tops[k] < el.scrollTop + ZONE_H || tops[k] + ROW_H > el.scrollTop + el.clientHeight) el.scrollTop = Math.max(0, tops[k] - ZONE_H - el.clientHeight / 3);
  }, [current, cards, rowOf, tops]);

  // The items in view (binary search of the first), and the zone whose head sticks at the top.
  let lo = 0;
  let hi = items.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (tops[mid] <= view.top) lo = mid;
    else hi = mid - 1;
  }
  const from = Math.max(0, lo - OVERSCAN);
  let to = lo;
  while (to < items.length - 1 && tops[to + 1] < view.top + view.height) to++;
  to = Math.min(items.length - 1, to + OVERSCAN);
  let stuck: Item | undefined;
  for (let k = lo; k >= 0 && !stuck; k--) if (items[k].kind === "zone") stuck = items[k];

  const zoneHead = (it: Extract<Item, { kind: "zone" }>) => (
    <>
      <strong>{it.seg.zone ?? t("Préparation")}</strong>
      <small>
        {fmtClock(it.seg.start)} – {fmtClock(it.seg.end)} · {t("niv. {from} → {to}", { from: it.seg.fromLevel, to: it.seg.toLevel })}
        {it.next && <span className="sl-next"> → {it.next}</span>}
      </small>
    </>
  );

  return (
    <div className="step-list">
      <div className="sl-head">
        <strong>{t("Étapes")}</strong>
        {edit && (
          <button className="link sl-add-first" onClick={(e) => edit.onAdd(-1, anchorOf(e.currentTarget))} title={t("Ajouter une étape au début")}>
            + {t("au début")}
          </button>
        )}
        <span className="muted">{steps.length}</span>
      </div>
      <div
        className={`sl-scroll${edit ? " editing" : ""}`}
        ref={scroller}
        onScroll={() => {
          measure();
          onHover(null);
        }}
        onDragLeave={(e) => {
          if (!scroller.current?.contains(e.relatedTarget as Node)) setDrop(null);
        }}
      >
        {stuck?.kind === "zone" && <header className="sl-zone-head sl-stuck">{zoneHead(stuck)}</header>}
        <div className="sl-rows" style={{ height, marginTop: stuck ? -ZONE_H : 0 }}>
          {items.slice(from, to + 1).map((it, k) => {
            const top = tops[from + k];
            if (it.kind === "zone")
              return (
                <header key={`z${from + k}`} className="sl-zone-head" style={{ top }}>
                  {zoneHead(it)}
                </header>
              );
            if (it.kind === "final")
              return (
                <div key="final" className="sl-row sl-final-grind" style={{ top }} title={t("Les étapes n'amènent pas au niveau {level} : farm ajouté après la dernière.", { level: edit!.toLevel })}>
                  <span className="sl-text">{t("Farm final jusqu'au niveau {level}", { level: edit!.toLevel })}</span>
                  <span className="sl-dur">{fmtClock(edit!.finalGrind)}</span>
                </div>
              );
            const c = cards[cardOfStep[it.i]];
            return (
              <Row
                key={it.i}
                top={top}
                i={it.i}
                steps={steps}
                card={c}
                spans={spans}
                quests={quests}
                active={c.n === current}
                onSelect={onSelect}
                checked={checked?.has(c.n)}
                onCheck={onCheck}
                edit={edit}
                drop={drop}
                onDrop={setDrop}
                onHover={onHover}
              />
            );
          })}
        </div>
      </div>
      {hover && steps[hover.i] && <StepTip i={hover.i} at={hover.at} steps={steps} card={cards[cardOfStep[hover.i]]} title={titles[cardOfStep[hover.i]]} spans={spans} quests={quests} edit={edit} />}
    </div>
  );
}

interface TipProps {
  i: number;
  /** The line's place in the window. */
  at: DOMRect;
  steps: Step[];
  card: Card;
  title: string;
  spans: Spans;
  quests: Map<number, QuestInfo>;
  edit?: ListEdit;
}

/**
 * What a line does not say, in a bubble beside it: its kind and quest, who or where, when it
 * starts and ends and where its time goes, the mobs killed on the way, why it cannot be done.
 */
function StepTip({ i, at, steps, card, title, spans, quests, edit }: TipProps) {
  const t = useT();
  const box = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  // Right of the line, its top on the line's; kept in the window.
  useLayoutEffect(() => {
    if (!box.current) return;
    const { width, height } = box.current.getBoundingClientRect();
    const left = at.right + 10 + width <= window.innerWidth - 8 ? at.right + 10 : Math.max(8, at.left - width - 10);
    setPos({ left, top: Math.max(8, Math.min(at.top - 6, window.innerHeight - height - 8)) });
  }, [at]);
  const s = steps[i];
  const { kind, label, text } = lineOf(s, quests);
  const span = spans?.[i];
  const split = splitOf(spans, i, i);
  const total = Math.max(1, s.time - stepStart(steps, i));
  const parts = (["travel", "combat", "interact", "wait"] as const).filter((k) => split[k] > 0);
  const r = edit?.reports[i];
  const reason = r?.reason ? t(REASONS[r.reason] ?? r.reason) : null;
  const warning = r?.warning ? t(REASONS[r.warning] ?? r.warning) : null;
  return createPortal(
    <div ref={box} className={`sl-tip kind-${s.kind} obj-${kind}`} role="tooltip" style={pos ?? { left: 0, top: 0, visibility: "hidden" }}>
      <div className="sl-tip-head">
        {!isCheckpoint(s) && (
          <b>
            <span aria-hidden>{GLYPHS[kind] ?? "•"}</span> {t(label)}
          </b>
        )}
        <span className="sl-tip-time">
          {fmtClock(stepStart(steps, i))} → {fmtClock(s.time)}
        </span>
      </div>
      <strong className="sl-tip-title">{isCheckpoint(s) ? s.text : text}</strong>
      {i === card.first && title !== text && <div className="sl-tip-where">{title}</div>}
      {s.text !== text && !isCheckpoint(s) && <div className="sl-tip-text">{s.text}</div>}
      {parts.length > 0 && (
        <>
          <div className="sl-tip-bar" aria-hidden>
            {parts.map((k) => (
              <i key={k} className={`cat-${k}`} style={{ width: `${(split[k] / total) * 100}%` }} />
            ))}
          </div>
          <div className="sl-tip-parts">
            {parts.map((k) => (
              <span key={k}>
                <i className={`cat-${k}`} /> {t(PART_LABELS[k])} {fmtClock(split[k])}
              </span>
            ))}
          </div>
        </>
      )}
      {span?.estimate && <div className="sl-tip-note">{t("Durée estimée : cette étape n'a pas été retrouvée en rejouant le guide")}</div>}
      {s.farm ? <div className="sl-tip-note">{t("× {n} mobs tués en chemin", { n: s.farm })}</div> : null}
      {warning && <div className="sl-tip-why warn">{t("Attention : {warning}", { warning })}</div>}
      {reason && <div className={r?.status === "error" ? "sl-tip-why error" : "sl-tip-why"}>{r?.status === "error" ? t("Impossible : {reason}", { reason }) : t("Rien à faire : {reason}", { reason })}</div>}
    </div>,
    document.body,
  );
}

interface RowProps {
  top: number;
  i: number;
  steps: Step[];
  card: Card;
  spans: Spans;
  quests: Map<number, QuestInfo>;
  active: boolean;
  onSelect: (card: number) => void;
  checked?: boolean;
  onCheck?: (card: number) => void;
  edit?: ListEdit;
  /** Where what is dragged lands: before line `drop`. */
  drop: number | null;
  onDrop: (drop: number | null) => void;
  /** The pointer over this line (its element), or off it. */
  onHover: (i: number | null, el?: Element) => void;
}

/** A point of the window beside an element (where a menu opens): its right edge, at its top. */
const anchorOf = (el: Element) => {
  const r = el.getBoundingClientRect();
  return { x: r.right + 8, y: r.top };
};

/** A drag starts: no text stays selected (it would be left greyed over the lines). */
const clearSelection = () => window.getSelection()?.removeAllRanges();

/** Where a drag over line `i` lands: before it in its upper half, after it in the lower. */
function landing(e: React.DragEvent, i: number) {
  const box = e.currentTarget.getBoundingClientRect();
  return e.clientY < box.top + box.height / 2 ? i : i + 1;
}

/** The lines dragged, read back from a drag. */
function dragged(e: React.DragEvent): [number, number] | null {
  const m = /^(\d+)-(\d+)$/.exec(e.dataTransfer.getData(DRAG_TYPE));
  return m ? [Number(m[1]), Number(m[2])] : null;
}

/**
 * A step on one line: its card's number (on its first step), its kind (a colored glyph) and what it is about,
 * the level reached, its time; the rest on hover (`StepTip`). Editing:
 * dragged to move it, killing the mobs on the way, a grind's mobs or level, and buttons to add
 * a step after it or remove it.
 */
function Row({ top, i, steps, card, spans, quests, active, onSelect, checked, onCheck, edit, drop, onDrop, onHover }: RowProps) {
  const t = useT();
  const s = steps[i];
  const head = i === card.first;
  const { kind, label, text } = lineOf(s, quests);
  const span = spans?.[i];
  const dur = s.time - stepStart(steps, i);
  const ding = head && card.ding ? card.ding : !head && i > 0 && s.level > steps[i - 1].level ? s.level : null;
  const r = edit?.reports[i];
  const step = edit?.steps[i];
  const farmed = (f: Farmed) => t("{n} mobs tués en chemin : {xp} xp, {time}", { n: f.kills, xp: f.xp.toLocaleString(), time: fmtTime(f.seconds) });

  const classes = [
    "sl-row",
    `kind-${s.kind}`,
    `obj-${kind}`,
    head ? "head" : "attached",
    active && "active",
    card.travel && "travel",
    isCheckpoint(s) && "checkpoint",
    edit && "edit",
    r?.status === "error" && "invalid",
    r?.status === "skipped" && "skipped",
    r?.warning && "warned",
    edit?.pending[i] && "pending",
    drop === i && "drop-before",
    drop === steps.length && i === steps.length - 1 && "drop-after",
  ];
  return (
    <div
      className={classes.filter(Boolean).join(" ")}
      style={{ top }}
      role="button"
      tabIndex={0}
      aria-current={active && head ? "step" : undefined}
      onMouseEnter={(e) => onHover(i, e.currentTarget)}
      onMouseLeave={() => onHover(null)}
      draggable={!!edit}
      onClick={() => onSelect(card.n)}
      onKeyDown={(e) => {
        if (e.key === "Enter") onSelect(card.n);
      }}
      onDragStart={(e) => {
        onHover(null);
        clearSelection();
        e.dataTransfer.effectAllowed = "move";
        e.dataTransfer.setData(DRAG_TYPE, `${i}-${i}`);
      }}
      onDragOver={(e) => {
        if (!edit) return;
        e.preventDefault();
        const at = landing(e, i);
        if (drop !== at) onDrop(at);
      }}
      onDrop={(e) => {
        e.preventDefault();
        const d = dragged(e);
        if (edit && d) edit.onMove(d[0], d[1], landing(e, i));
        onDrop(null);
      }}
      onDragEnd={() => onDrop(null)}
    >
      {onCheck && head ? (
        <input
          type="checkbox"
          className="step-check"
          checked={checked ?? false}
          onClick={(e) => e.stopPropagation()}
          onChange={() => onCheck(card.n)}
          aria-label={t("Joindre l'étape {n} au signalement", { n: card.n + 1 })}
        />
      ) : (
        onCheck && <span className="step-check" />
      )}
      <span className="sl-n">{head ? card.n + 1 : ""}</span>
      {isCheckpoint(s) ? (
        <span className="sl-text">{s.text}</span>
      ) : (
        <>
          <b aria-label={t(label)}>{GLYPHS[kind] ?? "•"}</b>
          <span className="sl-text">{text}</span>
        </>
      )}
      {edit && step?.kind === "objective" && (
        <label className={`sl-farm-toggle${step.farm ? " on" : ""}`} onClick={(e) => e.stopPropagation()} title={`${t("Tuer tous les mobs croisés en chemin")}${step.farm ? (r?.farm ? `\n${farmed(r.farm)}` : "") : r?.farm_all ? `\n${t("si coché : ~{n} mobs, ~{xp} xp, {time}", { n: r.farm_all.kills, xp: r.farm_all.xp.toLocaleString(), time: fmtTime(r.farm_all.seconds) })}` : ""}`}>
          <input type="checkbox" checked={!!step.farm} onChange={(e) => edit.onReplace(i, { ...step, farm: e.target.checked || undefined })} aria-label={t("Tuer tous les mobs croisés en chemin")} />✕
        </label>
      )}
      {edit && step?.kind === "grind" && (
        <span className="sl-num" onClick={(e) => e.stopPropagation()} title={t("Mobs à tuer")}>
          <NumberInput value={step.mobs} min={1} max={2000} onChange={(mobs) => mobs >= 1 && edit.onReplace(i, { kind: "grind", mobs: Math.round(mobs) })} aria-label={t("Mobs à tuer")} />
        </span>
      )}
      {edit && step?.kind === "grind_to" && (
        <span className="sl-num" onClick={(e) => e.stopPropagation()} title={t("Niveau à atteindre")}>
          <NumberInput value={step.level} min={2} max={70} onChange={(level) => level >= 2 && edit.onReplace(i, { kind: "grind_to", level: Math.round(level) })} aria-label={t("Niveau à atteindre")} />
        </span>
      )}
      {ding && <span className="sl-ding">{t("NIV. {n}", { n: ding })}</span>}
      {edit ? (
        <span className="sl-tools">
          <button
            className="icon"
            onClick={(e) => {
              e.stopPropagation();
              edit.onAdd(i, anchorOf(e.currentTarget.closest(".sl-row") ?? e.currentTarget));
            }}
            title={t("Ajouter une étape après")}
            aria-label={t("Ajouter une étape après")}
          >
            +
          </button>
          <button
            className="icon danger"
            onClick={(e) => {
              e.stopPropagation();
              edit.onRemove(i);
            }}
            title={t("Supprimer l'étape")}
            aria-label={t("Supprimer l'étape")}
          >
            ×
          </button>
        </span>
      ) : null}
      <span className="sl-dur">
        {span?.estimate && "≈ "}
        {fmtClock(dur)}
      </span>
    </div>
  );
}

/** The parts of a step's time, as the hover says them (French keys of the catalog). */
const PART_LABELS = { travel: "Trajet", combat: "Combat", interact: "Interaction", wait: "Attente" } as const;

/** Objective kinds as the line says them (French keys of the catalog). */
const OBJECTIVE_LABELS: Record<string, string> = { kill: "Tuer", collect: "Récupérer", use: "Utiliser", talk: "Parler", other: "Faire" };
/** The glyph before each kind of line. */
const GLYPHS: Record<string, string> = { accept: "!", turnin: "?", kill: "✕", collect: "◆", use: "◆", talk: "…", fly: "➜", link: "➜", hearth: "⌂", teleport: "➜", deathskip: "✝" };

/** What a line says: its kind (French key), the kind of its objective, its text. */
function lineOf(s: Step, quests: Map<number, QuestInfo>) {
  const { quest, objective } = questOf(quests, s);
  let kind = s.kind;
  let label = KIND_LABELS[s.kind] ?? s.kind;
  let text = s.text;
  if (s.kind === "accept" || s.kind === "turnin" || s.kind === "abandon") text = quest?.name ?? s.quest_name ?? s.text;
  else if (s.kind === "objective" && objective) {
    kind = objective.kind;
    label = OBJECTIVE_LABELS[objective.kind];
    text = objective.kind === "other" || !s.target ? objective.text : `${s.target.name}${objective.count > 1 ? ` ×${objective.count}` : ""}`;
  }
  return { kind, label, text };
}
