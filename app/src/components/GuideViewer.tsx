import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { EditCatalog, PlanRequest, QuestInfo, Route, RouteStep, Suggestions, Timeline } from "../api";
import { useT } from "../i18n";
import { AddMenu } from "./AddMenu";
import { CharacterPanel } from "./CharacterPanel";
import type { useNames } from "./editing";
import { DEFAULT_LAYERS, GuideMap, type MapLayers } from "./GuideMap";
import { activityAt, buildCards, cardOf, cardTitle, positionAt, stateTracker, stepAt, stepStart, straightMoves, travelEnds } from "./guideTimeline";
import { PlaybackBar } from "./PlaybackBar";
import { Clock, useClock } from "./playback";
import { segments } from "./segments";
import { type ListEdit, StepList } from "./StepList";

const LAYERS_KEY = "factoruide.mapLayers";

/** Editing the guide on its page: the list's changes, what can be added, and adding it. */
export interface ViewerEdit {
  list: Omit<ListEdit, "onAdd">;
  catalog: EditCatalog;
  names: ReturnType<typeof useNames>;
  /** What could be added before the step watched (asked for after the step before it, null: before the first). */
  suggestions: Suggestions | null;
  onSuggestAt: (after: number | null) => void;
  /** Add a step after step `after` (-1: first). */
  onInsert: (after: number, step: RouteStep) => void;
}

interface Props {
  /** The steps the list shows: the guide's, or a draft's lines. */
  route: Route;
  /** What is played (the map, the character, the time): the same, or the last replay of a draft changed since. */
  played?: Route;
  /** The timeline of `played` (null while it loads), and why it is not there. */
  timeline: Timeline | null;
  failed?: string | null;
  request: PlanRequest;
  /** The keys of the lines shown and of those played (a step of a draft keeps its key when it moves). */
  keys?: { shown: number[]; played: number[] };
  /** Edit the guide's steps (read only). */
  onEditSteps?: () => void;
  /** Steps checked for a problem report (indexes), and checking those of a card (a box on each card when given). */
  checked?: Set<number>;
  onCheck?: (steps: number[]) => void;
  /** Editing the steps. */
  edit?: ViewerEdit;
  /** A step to show (its card chosen); `n` changes to show it again. */
  reveal?: { step: number; n: number };
  /** A replay under way (a draft's): a spinner in the status line. */
  computing?: boolean;
}

/** The last step played at or before `p` that the list shows, by keys. */
function shownOf(keys: Props["keys"], shown: Map<number, number> | null, p: number) {
  if (!keys || !shown) return p;
  for (let q = Math.min(p, keys.played.length - 1); q >= 0; q--) {
    const i = shown.get(keys.played[q]);
    if (i != null) return i;
  }
  return 0;
}

/**
 * A guide watched as it is played: its steps by place on the left, the map in the middle with
 * the character moving along the route, the character's state on the right, and the time at
 * the bottom. The steps, the map and the time follow each other. Editing, the steps are moved,
 * removed and added in the list and from the map, the guide replayed in the background.
 */
export function GuideViewer({ route, played, timeline, failed, request, keys, onEditSteps, checked, onCheck, edit, reveal, computing }: Props) {
  const t = useT();
  const play = played ?? route;
  const steps = route.steps;

  // Steps shown and played by key (the same steps but while a draft replays).
  const [shownByKey, playedByKey] = useMemo(
    () => (keys ? [new Map(keys.shown.map((k, i) => [k, i])), new Map(keys.played.map((k, i) => [k, i]))] : [null, null]),
    [keys],
  );
  const toPlay = useCallback((i: number) => (keys && playedByKey ? playedByKey.get(keys.shown[i]) : i), [keys, playedByKey]);
  const toShown = useCallback((p: number) => shownOf(keys, shownByKey, p), [keys, shownByKey]);

  const spans = useMemo(() => (timeline ? steps.map((_, i) => timeline.steps[toPlay(i) ?? -1]) : null), [timeline, steps, toPlay]);
  const cards = useMemo(() => buildCards(steps, route.from_level), [steps, route.from_level]);
  const titles = useMemo(() => cards.map((c) => cardTitle(c, steps, spans)), [cards, steps, spans]);
  const segs = useMemo(() => segments(steps), [steps]);
  const quests = useMemo(() => new Map((timeline?.quests ?? []).map((q) => [q.id, q])), [timeline]);
  const moves = useMemo(() => (timeline ? { start: timeline.start.at, legs: timeline.legs } : straightMoves(play.steps)), [timeline, play]);
  const tracker = useMemo(() => (timeline ? stateTracker(timeline) : null), [timeline]);
  // One clock for the page: the time stays where it is when a draft's replay comes.
  const [clock] = useState(() => new Clock(play.total_time));
  useEffect(() => {
    clock.total = play.total_time;
    if (clock.time > clock.total) clock.seek(clock.total);
  }, [clock, play.total_time]);
  useEffect(() => () => clock.dispose(), [clock]);

  // The step played: the one of the time (a flight or a boat still the step that takes it),
  // within the steps of the card chosen while the time is in it (steps of no time at its start
  // belong to it, not to the card before).
  const pinned = useRef<{ first: number; last: number; start: number; end: number } | null>(null);
  const travels = useMemo(() => travelEnds(play.steps, moves.legs), [play, moves]);
  const stepOf = useCallback(
    (time: number) => {
      const i = stepAt(play.steps, time);
      const p = pinned.current;
      if (p && time >= p.start && time <= p.end) return Math.min(Math.max(i, p.first), p.last);
      return i > 0 && (travels.get(i - 1) ?? -1) > time ? i - 1 : i;
    },
    [play, travels],
  );
  const step = useClock(
    clock,
    useCallback((c: Clock) => stepOf(c.time), [stepOf]),
  );
  const shownStep = toShown(step);
  const card = cardOf(cards, shownStep);
  const seg = Math.max(
    0,
    segs.findIndex((s) => s.first <= shownStep && shownStep <= s.last),
  );
  const zone = useMemo<[number, number]>(() => (segs[seg] ? [cardOf(cards, segs[seg].first), cardOf(cards, segs[seg].last)] : [0, 0]), [segs, seg, cards]);
  const state = useMemo(() => (tracker ? tracker(step - 1) : null), [tracker, step]);
  const moving = useClock(
    clock,
    useCallback(
      (c: Clock) => {
        const p = positionAt(moves, c.time);
        return p.moving ? moves.legs[p.leg].mode : null;
      },
      [moves],
    ),
  );

  const select = useCallback(
    (n: number) => {
      const c = cards[Math.min(cards.length - 1, Math.max(0, n))];
      if (!c) return;
      const at: number[] = [];
      for (let i = c.first; i <= c.last; i++) {
        const p = toPlay(i);
        if (p != null) at.push(p);
      }
      if (!at.length) return;
      const [first, last] = [Math.min(...at), Math.max(...at)];
      const start = stepStart(play.steps, first);
      pinned.current = { first, last, start, end: play.steps[last].time };
      clock.seek(start);
    },
    [cards, clock, play, toPlay],
  );
  // Shown on demand (the first impossible step).
  useEffect(() => {
    if (reveal) select(cardOf(cards, reveal.step));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reveal?.n]);
  // Editing: what could be added where the guide is watched.
  const suggestAt = edit?.onSuggestAt;
  useEffect(() => suggestAt?.(shownStep > 0 ? shownStep - 1 : null), [suggestAt, shownStep]);

  const [follow, setFollow] = useState(true);
  // A card chosen in the list, with the keys or the playback bar's steps: shown on the map.
  const [focus, setFocus] = useState<{ card: number; seq: number } | undefined>(undefined);
  const show = useCallback(
    (n: number) => {
      select(n);
      setFocus((f) => ({ card: Math.min(cards.length - 1, Math.max(0, n)), seq: (f?.seq ?? 0) + 1 }));
    },
    [select, cards.length],
  );
  const [layers, setLayers] = useState<MapLayers>(() => {
    try {
      return { ...DEFAULT_LAYERS, ...JSON.parse(localStorage.getItem(LAYERS_KEY) ?? "{}") };
    } catch {
      return DEFAULT_LAYERS;
    }
  });
  const changeLayers = (l: MapLayers) => {
    setLayers(l);
    localStorage.setItem(LAYERS_KEY, JSON.stringify(l));
  };

  // The page's height: from the top of the viewer to the bottom of the window (measured again
  // when what is above it changes, as the problem report's form).
  const root = useRef<HTMLDivElement>(null);
  const [height, setHeight] = useState(600);
  useLayoutEffect(() => {
    const measure = () => {
      const top = root.current?.getBoundingClientRect().top ?? 0;
      // The content area's padding under it.
      setHeight(Math.max(480, Math.floor(window.innerHeight - top - 21)));
    };
    measure();
    const above = root.current?.parentElement;
    const ro = new ResizeObserver(measure);
    if (above) ro.observe(above);
    window.addEventListener("resize", measure);
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, []);

  // Space plays or pauses, the arrows go from card to card (not while typing): the viewer has
  // the keyboard once open.
  useEffect(() => root.current?.focus({ preventScroll: true }), []);
  const onKeyDown = (e: React.KeyboardEvent) => {
    const target = e.target as HTMLElement;
    if (/^(INPUT|SELECT|TEXTAREA)$/.test(target.tagName) || target.getAttribute("role") === "slider") return;
    if (e.key === " ") clock.toggle();
    else if (e.key === "ArrowDown" || e.key === "ArrowRight") show(card + 1);
    else if (e.key === "ArrowUp" || e.key === "ArrowLeft") show(card - 1);
    else return;
    e.preventDefault();
  };

  const checkedCards = useMemo(() => {
    if (!checked) return undefined;
    return new Set(cards.filter((c) => checked.has(c.first)).map((c) => c.n));
  }, [checked, cards]);

  // Adding a step: the menu beside the line it comes after.
  const [adding, setAdding] = useState<{ at: number; anchor: { x: number; y: number } } | null>(null);
  const closeAdding = useCallback(() => setAdding(null), []);
  const openAdding = useCallback((at: number, anchor: { x: number; y: number }) => setAdding({ at, anchor }), []);
  const listEdit = useMemo<ListEdit | undefined>(() => edit && { ...edit.list, onAdd: openAdding }, [edit, openAdding]);
  const titleOf = useCallback((p: number) => titles[cardOf(cards, toShown(p))] ?? "", [titles, cards, toShown]);

  const status = (
    <>
      {computing && <span className="spinner" role="status" aria-label={t("Le guide est rejoué…")} title={t("Le guide est rejoué…")} />}
      {timeline ? (
        <Activity clock={clock} stepOf={stepOf} route={play} timeline={timeline} quests={quests} titleOf={titleOf} />
      ) : (
        <span className="muted">{failed ? t("Replay indisponible : {error}", { error: failed }) : t("Chargement du replay…")}</span>
      )}
      {onEditSteps && (
        <button className="link gm-edit" onClick={onEditSteps}>
          ✎ {t("Modifier les étapes")}
        </button>
      )}
    </>
  );

  return (
    <div className={`guide-viewer${edit ? " editing" : ""}`} ref={root} style={{ height }} onKeyDown={onKeyDown} tabIndex={-1}>
      <StepList
        steps={steps}
        cards={cards}
        titles={titles}
        segments={segs}
        spans={spans}
        quests={quests}
        current={card}
        onSelect={show}
        checked={checkedCards}
        onCheck={onCheck && ((n) => onCheck(Array.from({ length: cards[n].last - cards[n].first + 1 }, (_, k) => cards[n].first + k)))}
        edit={listEdit}
      />
      <GuideMap
        steps={steps}
        moves={moves}
        cards={cards}
        titles={titles}
        current={card}
        currentEnd={Math.max(cards[card]?.end ?? 0, travels.get(step) ?? 0)}
        zone={zone}
        clock={clock}
        follow={follow}
        onFollow={setFollow}
        layers={layers}
        onLayers={changeLayers}
        onSelect={select}
        focus={focus}
        status={status}
        suggestions={edit?.suggestions ?? null}
        onPick={edit && ((s) => edit.onInsert(shownStep - 1, s))}
      />
      <CharacterPanel
        route={play}
        request={request}
        timeline={timeline}
        quests={quests}
        state={state}
        clock={clock}
        step={step}
        card={card}
        cards={cards.length}
        moving={moving}
      />
      <PlaybackBar clock={clock} segments={segs} current={seg} onStep={(d) => show(card + d)} />
      {edit && adding && (
        <AddMenu
          at={adding.at}
          anchor={adding.anchor}
          level={steps[adding.at]?.level ?? route.from_level}
          steps={edit.list.steps}
          catalog={edit.catalog}
          names={edit.names}
          onAdd={(s) => {
            edit.onInsert(adding.at, s);
            setAdding(null);
          }}
          onShow={(i) => {
            setAdding(null);
            show(cardOf(cards, i));
          }}
          onClose={closeAdding}
        />
      )}
    </div>
  );
}

/** What the character is doing now, over the map ("Killing Young Wolf"). */
function Activity({
  clock,
  stepOf,
  route,
  timeline,
  quests,
  titleOf,
}: {
  clock: Clock;
  stepOf: (time: number) => number;
  route: Route;
  timeline: Timeline;
  quests: Map<number, QuestInfo>;
  /** The title of the card of a step played. */
  titleOf: (step: number) => string;
}) {
  const t = useT();
  const moves = useMemo(() => ({ start: timeline.start.at, legs: timeline.legs }), [timeline]);
  const text = useClock(
    clock,
    useCallback(
      (c: Clock) => {
        const p = positionAt(moves, c.time);
        const i = stepOf(c.time);
        const a = activityAt(route, timeline, quests, i, titleOf(i), c.time, p.leg, p.moving);
        return `${a.verb}\u0000${a.what}`;
      },
      [moves, route, timeline, quests, stepOf, titleOf],
    ),
  );
  const [verb, what] = text.split("\u0000");
  return (
    <span className="gm-activity">
      <b>{t(verb)}</b> {what}
    </span>
  );
}
