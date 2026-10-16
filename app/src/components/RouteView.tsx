import { Fragment, useEffect, useMemo, useRef, useState, type ReactNode, type RefObject } from "react";
import { fmtTime, type Route, type Step } from "../api";
import { BreakdownBars } from "./BreakdownBars";
import { LevelChart } from "./LevelChart";
import { useT } from "../i18n";
import { useWidth } from "./useWidth";
import type { WorldPoint } from "./WorldMap";
import { ZoneMap, type NumberedStep } from "./ZoneMap";
import { Money } from "./Money";
import { standingOf } from "./ReputationPanel";

export const KIND_LABELS: Record<string, string> = {
  accept: "Prendre",
  objective: "Faire",
  turnin: "Rendre",
  fly: "Vol",
  flight_master: "Point de vol",
  link: "Transport",
  grind: "Farmer",
  train: "Entraîneur",
  hearth: "Pierre de foyer",
  teleport: "Téléportation",
  bind: "Auberge",
  dungeon: "Donjon",
  profession: "Métier",
  practice: "Pratique",
  deathskip: "Mort volontaire",
  abandon: "Abandonner",
};

/** Why the planner left quests out (`fg_route` model), English as sent, French key of the catalog. */
const SKIP_REASONS: Record<string, string> = {
  "elite objective": "objectif élite",
  "excluded by configuration": "exclue par la configuration",
  "exploration area unknown": "zone d'exploration inconnue",
  "holiday/profession/event category": "catégorie fête, métier ou événement",
  "needs a profession, reputation or spell": "demande un métier, une réputation ou un sort",
  "no reachable quest giver": "aucun donneur accessible",
  "no reachable turn-in": "aucun PNJ de rendu accessible",
  "objective item from elite mobs": "objet d'objectif sur des élites",
  "objective item source unknown": "source de l'objet d'objectif inconnue",
  "objectives and turn-in unknown (catalogue only)": "objectifs et rendu inconnus (catalogue seulement)",
  "objectives unknown (only described)": "objectifs inconnus (seulement décrits)",
  "PvP (battleground) quest": "quête JcJ (champ de bataille)",
  repeatable: "répétable",
  "reputation/spell objective": "objectif de réputation ou de sort",
  "started by a rare drop": "commence par un objet rare",
};

/** What cutting steps into segments needs of a step. */
export interface SegmentStep {
  kind: string;
  zone?: string;
  target?: { name: string };
  inside?: boolean;
  /** Time and level once it is done. */
  time: number;
  level: number;
}

export interface Segment {
  /** Zone, or dungeon name; null: steps done before any place (abandoning quests). */
  zone: string | null;
  dungeon: boolean;
  start: number;
  end: number;
  fromLevel: number;
  toLevel: number;
  /** Indices of the first and last steps. */
  first: number;
  last: number;
}

export const steps = (s: Segment) => s.last - s.first + 1;

/** Consecutive steps in the same zone; a dungeon run is a segment of its own. */
export function segments(list: SegmentStep[]): Segment[] {
  const out: Segment[] = [];
  let previousTime = 0;
  list.forEach((s, i) => {
    const last = out[out.length - 1];
    // The quests done inside a dungeon belong to its run.
    if (s.inside && last?.dungeon) {
      last.end = s.time;
      last.toLevel = s.level;
      last.last = i;
      previousTime = s.time;
      return;
    }
    const dungeon = s.kind === "dungeon";
    // Steps done anywhere (practicing a profession, abandoning a quest) have no place: they
    // belong to the zone the character is in.
    const zone = dungeon ? (s.target?.name ?? s.zone ?? null) : (s.zone ?? (last && !last.dungeon ? last.zone : null));
    if (last && !dungeon && !last.dungeon && last.zone === zone) {
      last.end = s.time;
      last.toLevel = s.level;
      last.last = i;
    } else {
      out.push({ zone, dungeon, start: previousTime, end: s.time, fromLevel: s.level, toLevel: s.level, first: i, last: i });
    }
    previousTime = s.time;
  });
  // Fold short passages (a couple of steps, a few minutes) into the previous segment.
  const merged: Segment[] = [];
  for (const s of out) {
    const last = merged[merged.length - 1];
    const short = steps(s) <= 2 && s.end - s.start < 300;
    if (last && !last.dungeon && !s.dungeon && (short || last.zone === s.zone)) {
      last.end = s.end;
      last.toLevel = s.toLevel;
      last.last = s.last;
    } else {
      merged.push({ ...s });
    }
  }
  return merged;
}

/** "N plannable quests, start …" then "skipped N: reason" lines (`fg_route::job::notes`). */
function Notes({ notes }: { notes: string[] }) {
  const t = useT();
  const skipped = notes.flatMap((n) => {
    const m = /^skipped (\d+): (.+)$/.exec(n);
    return m ? [{ n: Number(m[1]), reason: SKIP_REASONS[m[2]] ? t(SKIP_REASONS[m[2]]) : m[2] }] : [];
  });
  const others = notes.filter((n) => !/^skipped \d+: /.test(n));
  const plannable = others.map((n) => /^(\d+) plannable quests, start (.+)$/.exec(n)).find(Boolean);
  const where = plannable && /^where the character stands \((.+)\)$/.exec(plannable[2]);
  const total = skipped.reduce((s, r) => s + r.n, 0);
  return (
    <details className="notes">
      <summary>
        {plannable
          ? t("{n} quêtes possibles, {skipped} écartées", { n: plannable[1], skipped: total })
          : t("Notes du calcul ({n})", { n: notes.length })}
      </summary>
      {plannable && (
        <p className="muted">{where ? t("Départ : là où se trouve le personnage ({place})", { place: where[1] }) : t("Départ : {place}", { place: plannable[2].replace(/^at /, "") })}</p>
      )}
      {others
        .filter((n) => !/^\d+ plannable quests, /.test(n))
        .map((n, i) => (
          <p key={i} className="muted">
            {n}
          </p>
        ))}
      {skipped.length > 0 && (
        <table className="table compact notes-table">
          <tbody>
            {[...skipped]
              .sort((a, b) => b.n - a.n)
              .map((r) => (
                <tr key={r.reason}>
                  <td className="num">{r.n}</td>
                  <td>{r.reason}</td>
                </tr>
              ))}
          </tbody>
        </table>
      )}
    </details>
  );
}

/** Where the XP comes from, as one stacked bar and its legend. */
function XpTile({ route }: { route: Route }) {
  const t = useT();
  const b = route.breakdown;
  const parts = [
    { label: t("Quêtes"), value: b.quest_xp, color: "var(--cat-1)" },
    { label: t("Mobs"), value: b.mob_xp, color: "var(--cat-3)" },
    { label: t("Farm en chemin"), value: b.farm_xp ?? 0, color: "var(--cat-4)" },
    { label: t("Farm"), value: b.grind_xp, color: "var(--cat-2)" },
    { label: t("Exploration"), value: b.explore_xp ?? 0, color: "var(--cat-7)" },
  ].filter((p) => p.value > 0);
  const total = parts.reduce((s, p) => s + p.value, 0);
  const pct = (v: number) => Math.round((v / Math.max(1, total)) * 100);
  return (
    <div className="tile">
      <span>XP</span>
      <strong>{Math.round(total / 1000)}k</strong>
      <div className="stack" role="img" aria-label={parts.map((p) => `${p.label} ${pct(p.value)} %`).join(", ")}>
        {parts.map((p) => (
          <span key={p.label} style={{ flexGrow: p.value, background: p.color }} title={`${p.label} : ${Math.round(p.value / 1000)}k (${pct(p.value)} %)`} />
        ))}
      </div>
      <small className="stack-legend">
        {parts
          .filter((p) => pct(p.value) >= 1)
          .map((p) => (
          <span key={p.label}>
            <i className="swatch" style={{ background: p.color }} /> {p.label} {pct(p.value)} %
          </span>
        ))}
      </small>
    </div>
  );
}

/**
 * Before closing a zone from its line stuck at the top (its steps scrolled under it): once
 * closed, its line is brought back at the top instead of leaving the view further down.
 */
export function keepZoneInView(row: HTMLElement) {
  const scroller = scrollParent(row);
  const sticky = parseFloat(getComputedStyle(row).scrollMarginTop) || 0;
  const top = (scroller?.getBoundingClientRect().top ?? 0) + sticky;
  if (row.getBoundingClientRect().top <= top + 1) requestAnimationFrame(() => row.scrollIntoView({ block: "start" }));
}

/** After closing every zone: back to the top of the table when it is above the view. */
export function keepTableInView(table: HTMLElement | null) {
  if (!table) return;
  const scroller = scrollParent(table);
  if (table.getBoundingClientRect().top < (scroller?.getBoundingClientRect().top ?? 0)) requestAnimationFrame(() => table.scrollIntoView({ block: "start" }));
}

/** The element that scrolls `el` (the app's content area). */
export function scrollParent(el: HTMLElement): HTMLElement | null {
  for (let p = el.parentElement; p; p = p.parentElement) {
    if (/(auto|scroll)/.test(getComputedStyle(p).overflowY)) return p;
  }
  return null;
}

interface Props {
  route: Route;
  notes?: string[];
  /** Shown under the play time (the difference with the previous version). */
  compare?: ReactNode;
  /** Steps of the zone row opened (their positions), null when closed. */
  onHighlight?: (points: WorldPoint[] | null) => void;
  /** Steps checked (a problem report), and checking one: a box on each step when given. */
  selected?: Set<number>;
  onSelect?: (index: number) => void;
}

export function RouteView({ route, notes, compare, onHighlight, selected, onSelect }: Props) {
  const t = useT();
  const [filter, setFilter] = useState("");
  // Zones opened, the one shown on the maps (the opened zone whose steps are in view), and the
  // step pointed at.
  const [open, setOpen] = useState<Set<number>>(new Set());
  const [focus, setFocus] = useState<number | null>(null);
  const [active, setActive] = useState<number | null>(null);
  const [frame, width] = useWidth(1000);
  const table = useRef<HTMLTableElement>(null);
  // Wide enough: the zone map beside the table.
  const wide = width >= 1150;
  const b = route.breakdown;
  const segs = useMemo(() => segments(route.steps), [route]);
  const kindLabel = (s: Step) => t(KIND_LABELS[s.kind] ?? s.kind);
  const numbered = (i: number): NumberedStep => ({ ...route.steps[i], n: i + 1 });
  const stepsOf = (s: Segment) => Array.from({ length: steps(s) }, (_, k) => s.first + k);
  // Filtered: the zones with matching steps, opened on them (all their steps when the zone matches).
  const q = filter.trim().toLowerCase();
  const rows = segs
    .map((s, i) => {
      const all = stepsOf(s);
      if (!q || (s.zone ?? "").toLowerCase().includes(q)) return { s, i, shown: all };
      const x = (k: number) => route.steps[k];
      return { s, i, shown: all.filter((k) => x(k).text.toLowerCase().includes(q) || kindLabel(x(k)).toLowerCase().includes(q)) };
    })
    .filter((r) => !q || r.shown.length > 0);
  const isOpen = (i: number) => (q ? true : open.has(i));
  const flights = route.steps.filter((s) => s.kind === "fly").length;

  // The zone shown is drawn over the guide map too.
  useEffect(() => {
    const s = focus !== null ? segs[focus] : null;
    onHighlight?.(s ? route.steps.slice(s.first, s.last + 1).flatMap((x) => (x.world ? [x.world as WorldPoint] : [])) : null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focus, segs, route]);
  // Beside the table, the map shows the zone under the pointer, else the first opened one in view.
  useFollowPointer(table, wide, "tr.sub-row", setFocus, [open, q]);
  const show = (i: number | null) => setFocus(i);
  const toggle = (i: number) => {
    const next = new Set(open);
    if (next.delete(i)) {
      if (focus === i) show(null);
    } else {
      next.add(i);
      show(i);
    }
    setOpen(next);
  };
  const duration = (seconds: number) => (seconds >= 30 ? fmtTime(seconds) : "");
  const focused = focus !== null ? segs[focus] : null;

  return (
    <div className="route-view">
      <div className="tiles">
        <div className="tile tile-hero">
          <span>{t("Temps de jeu estimé")}</span>
          <strong>{route.total_time_text}</strong>
          <small>
            {t("niveau {from} → {to}", { from: route.from_level, to: route.to_level })}
            {compare && <> · {compare}</>}
          </small>
        </div>
        <div className="tile">
          <span>{t("Quêtes")}</span>
          <strong>{route.quests}</strong>
          <small>{t("{n} étapes", { n: route.steps.length })}</small>
        </div>
        <div className="tile">
          <span>{t("Donjons")}</span>
          <strong>{b.dungeon_runs}</strong>
          <small>{b.dungeon_runs > 0 ? fmtTime(b.dungeons) : "—"}</small>
        </div>
        <div className="tile">
          <span>{t("Pierres de foyer")}</span>
          <strong>{b.hearths}</strong>
          <small>
            {t("{n} vols", { n: flights })}
            {flights > 0 && b.flight_cost != null && b.flight_cost > 0 && (
              <span title={t("Or dépensé en vols")}>
                {" · "}
                <Money copper={b.flight_cost} />
              </span>
            )}
          </small>
        </div>
        <XpTile route={route} />
      </div>
      {b.missing_class_quests > 0 && (
        <div className="notice warn">{t("Quêtes de classe obligatoires manquées : {n}", { n: b.missing_class_quests })}</div>
      )}
      {(b.missing_zone_quests ?? 0) > 0 && <div className="notice warn">{t("Quêtes des zones forcées manquées : {n}", { n: b.missing_zone_quests ?? 0 })}</div>}
      {(route.reputation ?? []).map((r) => {
        const standing = (points: number) => {
          const s = standingOf(points);
          return `${t(s.name)} ${s.into}/${s.of}`;
        };
        const missing = r.target - r.reached;
        return (
          <div key={r.faction} className={`notice${missing > 0 ? " warn" : ""}`}>
            {t("Réputation {faction} : {start} → {reached} (objectif : {target})", {
              faction: r.name,
              start: standing(r.start),
              reached: standing(r.reached),
              target: t(standingOf(r.target).name),
            })}
            {missing > 0 && ` — ${t("{n} points manquants : le guide n'a pas assez de quêtes de cette faction", { n: Math.round(missing) })}`}
          </div>
        );
      })}

      <div className="charts">
        <LevelChart steps={route.steps} fromLevel={route.from_level} toLevel={route.to_level} />
        <BreakdownBars breakdown={b} />
      </div>

      {notes && notes.length > 0 && <Notes notes={notes} />}

      <div className="route-toolbar">
        <input className="search" placeholder={t("Filtrer (quête, zone, type…)")} value={filter} onChange={(e) => setFilter(e.target.value)} />
        <button onClick={() => setOpen(new Set(segs.map((_, i) => i)))} disabled={!!q}>
          {t("Tout déplier")}
        </button>
        <button
          onClick={() => {
            setOpen(new Set());
            show(null);
            keepTableInView(table.current);
          }}
          disabled={!!q}
        >
          {t("Tout replier")}
        </button>
      </div>

      <div className={`route-layout${wide ? " wide" : ""}`} ref={frame}>
        {/* Fixed columns: opening a zone does not move them. */}
        <table className="table route-table" ref={table}>
          <colgroup>
            <col className="c-time" />
            <col className="c-time" />
            <col className="c-level" />
            <col />
            <col className="c-last" />
          </colgroup>
          <thead>
            <tr>
              <th className="num">{t("Début")}</th>
              <th className="num">{t("Durée")}</th>
              <th>{t("Niveaux")}</th>
              <th>{t("Zone")}</th>
              <th className="num">{t("Étapes")}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map(({ s, i, shown }) => (
              <Fragment key={i}>
                <tr
                  data-seg={i}
                  className={`clickable zone-line${focus === i ? " selected" : ""}`}
                  onClick={(e) => {
                    if (q) return;
                    if (isOpen(i)) keepZoneInView(e.currentTarget);
                    toggle(i);
                  }}
                >
                  <td className="num">{fmtTime(s.start)}</td>
                  <td className="num">{duration(s.end - s.start)}</td>
                  <td>{s.fromLevel === s.toLevel ? s.fromLevel : `${s.fromLevel} → ${s.toLevel}`}</td>
                  <td>
                    {/* Its click reaches the row, which opens or closes. */}
                    <button className="disclosure" aria-expanded={isOpen(i)} aria-label={isOpen(i) ? t("Replier") : t("Déplier")} disabled={!!q}>
                      {isOpen(i) ? "▾" : "▸"}
                    </button>{" "}
                    {s.dungeon && <span className="kind kind-dungeon">{t("Donjon")}</span>} {s.zone ?? <em className="muted">{t("Préparation")}</em>}
                  </td>
                  <td className="num">{steps(s)}</td>
                </tr>
                {isOpen(i) &&
                  shown.map((k) => {
                    const x = route.steps[k];
                    const begin = k > 0 ? route.steps[k - 1].time : 0;
                    return (
                      <tr
                        key={k}
                        data-seg={i}
                        className={`sub-row${active === k + 1 ? " active" : ""}`}
                        onMouseEnter={() => setActive(k + 1)}
                        onMouseLeave={() => setActive(null)}
                      >
                        <td className="num">{fmtTime(begin)}</td>
                        <td className={`num${x.time - begin >= 1800 ? " long" : ""}`}>{duration(x.time - begin)}</td>
                        <td>{x.level}</td>
                        <td>
                          {onSelect && (
                            <input
                              type="checkbox"
                              className="step-check"
                              checked={selected?.has(k) ?? false}
                              onChange={() => onSelect(k)}
                              aria-label={t("Joindre l'étape {n} au signalement", { n: k + 1 })}
                            />
                          )}
                          <span className="step-n">{k + 1}</span> <span className={`kind kind-${x.kind}`}>{kindLabel(x)}</span> {x.text}
                          {x.bg && <span className="along"> {t("en chemin")}</span>}
                          {x.farm ? <span className="along"> {t("(tuer ~{n} mobs en chemin)", { n: x.farm })}</span> : null}
                        </td>
                        <td className="num coords">{x.map ? `${x.map.x.toFixed(1)}, ${x.map.y.toFixed(1)}` : ""}</td>
                      </tr>
                    );
                  })}
              </Fragment>
            ))}
          </tbody>
        </table>
        {wide && (
          <aside className="zone-panel">
            {focused ? (
              <ZoneMap title={focused.zone ?? t("Préparation")} steps={stepsOf(focused).map(numbered)} active={active} onActive={setActive} />
            ) : (
              <p className="muted">{t("Ouvrez une zone pour voir ses étapes sur sa carte.")}</p>
            )}
          </aside>
        )}
      </div>
    </div>
  );
}

/**
 * Beside a table of steps grouped in segments (rows with `data-seg`), the segment shown on the
 * map: the one under the pointer, else (pointer outside the table) the one whose first opened row
 * (`rows`) is at the top of the view. `onFocus` follows them only when `active`.
 */
export function useFollowPointer(
  table: RefObject<HTMLTableElement | null>,
  active: boolean,
  rows: string,
  onFocus: (segment: number) => void,
  deps: unknown[],
) {
  useEffect(() => {
    const el = table.current;
    if (!active || !el) return;
    const scroller = scrollParent(el);
    let frameId = 0;
    // Last pointer position over the table, null once it left.
    let pointer: { x: number; y: number } | null = null;
    const segAt = (target: Element | null) => {
      const seg = target?.closest<HTMLElement>("tr[data-seg]")?.dataset.seg;
      return seg != null && el.contains(target) ? Number(seg) : null;
    };
    const update = () => {
      frameId = 0;
      if (pointer) {
        // Scrolling under a still pointer: the row now beneath it.
        const seg = segAt(document.elementFromPoint(pointer.x, pointer.y));
        if (seg != null) return onFocus(seg);
      }
      const lines = el.querySelectorAll<HTMLTableRowElement>(rows);
      if (!lines.length) return;
      const top = (scroller?.getBoundingClientRect().top ?? 0) + 40;
      // Rows are in order: the first one ending below the top of the view.
      let lo = 0;
      let hi = lines.length - 1;
      while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if (lines[mid].getBoundingClientRect().bottom > top) hi = mid;
        else lo = mid + 1;
      }
      onFocus(Number(lines[lo].dataset.seg));
    };
    const onScroll = () => {
      if (!frameId) frameId = requestAnimationFrame(update);
    };
    const onOver = (e: PointerEvent) => {
      pointer = { x: e.clientX, y: e.clientY };
      const seg = segAt(e.target as Element);
      if (seg != null) onFocus(seg);
    };
    const onMove = (e: PointerEvent) => {
      pointer = { x: e.clientX, y: e.clientY };
    };
    const onLeave = () => {
      pointer = null;
    };
    update();
    scroller?.addEventListener("scroll", onScroll, { passive: true });
    el.addEventListener("pointerover", onOver);
    el.addEventListener("pointermove", onMove, { passive: true });
    el.addEventListener("pointerleave", onLeave);
    return () => {
      scroller?.removeEventListener("scroll", onScroll);
      el.removeEventListener("pointerover", onOver);
      el.removeEventListener("pointermove", onMove);
      el.removeEventListener("pointerleave", onLeave);
      cancelAnimationFrame(frameId);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, rows, ...deps]);
}
