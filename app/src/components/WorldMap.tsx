import { useEffect, useMemo, useRef, useState } from "react";
import { api, type MapInfo } from "../api";
import { useAreaName, useT } from "../i18n";

export type WorldPoint = [number, number, number];
export type ZoneState = "preferred" | "excluded";

export interface MapMarker {
  at: WorldPoint;
  label: string;
  title?: string;
}

export type ContinentView = number | "both";

interface Props {
  /** Game map shown: 0 Eastern Kingdoms, 1 Kalimdor, or both side by side. */
  continent: ContinentView;
  onContinent?: (continent: ContinentView) => void;
  route?: WorldPoint[];
  markers?: MapMarker[];
  /** Zone states by game zone (AreaTable ID). */
  zoneStates?: Record<number, ZoneState>;
  onZoneClick?: (area: number, name: string) => void;
  /** Animate the route (while it is being optimized). */
  live?: boolean;
  /** Maximum height in pixels. */
  height?: number;
}

let indexPromise: Promise<MapInfo[]> | null = null;
const images = new Map<number, Promise<string>>();
const loadIndex = () => (indexPromise ??= api.mapIndex());
const loadImage = (id: number) => {
  let p = images.get(id);
  if (!p) {
    p = api.mapImage(id);
    images.set(id, p);
  }
  return p;
};

/** Forget the maps (another game version shows other maps). */
export function clearMapCache() {
  indexPromise = null;
  images.clear();
}

/** Continents (game maps) shown as tabs, west to east, when the game version has them. */
const CONTINENTS: { map: number; label: string }[] = [
  { map: 1, label: "Kalimdor" },
  { map: 0, label: "Royaumes de l'Est" },
  { map: 530, label: "Outreterre" },
];

/** Projects world coordinates (x north, y west) on a map image of `info`. */
function projector(info: MapInfo) {
  const b = info.bounds!;
  return (x: number, y: number): [number, number] => [
    ((b.max_y - y) / (b.max_y - b.min_y)) * info.width,
    ((b.max_x - x) / (b.max_x - b.min_x)) * info.height,
  ];
}

interface Panel {
  info: MapInfo;
  dx: number;
  project: (x: number, y: number) => [number, number];
}

export function WorldMap({ continent, onContinent, route, markers, zoneStates, onZoneClick, live, height = 520 }: Props) {
  const t = useT();
  const areaName = useAreaName();
  const [index, setIndex] = useState<MapInfo[] | null>(null);
  const [loaded, setLoaded] = useState<Record<number, string>>({});
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState({ scale: 1, x: 0, y: 0 });
  const [hover, setHover] = useState<{ id: number; name: string } | null>(null);
  const drag = useRef<{ x: number; y: number; vx: number; vy: number; moved: boolean } | null>(null);
  const svgRef = useRef<SVGSVGElement>(null);
  const frameRef = useRef<HTMLDivElement>(null);

  // Ctrl + wheel zooms (the wheel alone scrolls the page). A native, non-passive listener is
  // needed to stop the page scroll and the webview zoom.
  useEffect(() => {
    const frame = frameRef.current;
    if (!frame) return;
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey) return;
      e.preventDefault();
      const rect = frame.getBoundingClientRect();
      const cx = e.clientX - rect.left;
      const cy = e.clientY - rect.top;
      setView((v) => {
        const scale = Math.min(8, Math.max(1, v.scale * (e.deltaY < 0 ? 1.2 : 1 / 1.2)));
        const f = scale / v.scale;
        return { scale, x: scale === 1 ? 0 : cx - (cx - v.x) * f, y: scale === 1 ? 0 : cy - (cy - v.y) * f };
      });
    };
    frame.addEventListener("wheel", onWheel, { passive: false });
    return () => frame.removeEventListener("wheel", onWheel);
  });

  useEffect(() => {
    loadIndex().then(setIndex).catch((e) => setError(String(e)));
  }, []);

  // Continent maps shown, Kalimdor on the left (west) when both.
  const panels = useMemo<Panel[]>(() => {
    if (!index) return [];
    const base = (map: number) =>
      index.filter((m) => m.kind === 2 && m.bounds?.map === map).sort((a, b) => b.width - a.width)[0];
    const maps = (continent === "both" ? CONTINENTS.map((c) => base(c.map)) : [base(continent)]).filter(Boolean);
    let dx = 0;
    return maps.map((info) => {
      const p = { info, dx, project: projector(info) };
      dx += info.width;
      return p;
    });
  }, [index, continent]);
  const key = panels.map((p) => p.info.id).join("+");

  useEffect(() => {
    setView({ scale: 1, x: 0, y: 0 });
    for (const p of panels) {
      loadImage(p.info.id)
        .then((src) => setLoaded((l) => ({ ...l, [p.info.id]: src })))
        .catch((e) => setError(String(e)));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  const zones = useMemo(
    () =>
      panels.flatMap((panel) =>
        (index ?? [])
          .filter((m) => m.kind === 3 && m.parent === panel.info.id && m.bounds && m.area)
          .map((z) => {
            const b = z.bounds!;
            const [x1, y1] = panel.project(b.max_x, b.max_y);
            const [x2, y2] = panel.project(b.min_x, b.min_y);
            const rect = { x: panel.dx + Math.min(x1, x2), y: Math.min(y1, y2), w: Math.abs(x2 - x1), h: Math.abs(y2 - y1) };
            // The zone's outline, else its rectangle.
            const rings: [number, number][][] = z.shape?.length
              ? z.shape.map((r) => r.map(([x, y]) => [panel.dx + x, y] as [number, number]))
              : [[[rect.x, rect.y], [rect.x + rect.w, rect.y], [rect.x + rect.w, rect.y + rect.h], [rect.x, rect.y + rect.h]]];
            const d = rings.map((r) => `M${r.map(([x, y]) => `${x},${y}`).join("L")}Z`).join("");
            return { z, rings, d, label: labelPoint(rings), area: rings.reduce((s, r) => s + Math.abs(ringArea(r)), 0) };
          }),
      ),
    [index, panels],
  );

  if (error) return <div className="error">{t("Carte indisponible : {error}", { error })}</div>;
  if (!panels.length) return <div className="map-frame" style={{ aspectRatio: "3 / 2" }} />;

  const W = panels.reduce((s, p) => s + p.info.width, 0);
  const H = Math.max(...panels.map((p) => p.info.height));
  const panelOf = (map: number) => panels.find((p) => p.info.bounds!.map === map);
  const project = (p: WorldPoint): [number, number] | null => {
    const panel = panelOf(p[0]);
    if (!panel) return null;
    const [x, y] = panel.project(p[1], p[2]);
    return [panel.dx + x, y];
  };
  // Smallest zone whose outline holds the point (zones without an outline overlap).
  const zoneAt = (px: number, py: number) =>
    zones.filter((r) => r.rings.reduce((inside, ring) => inside !== inRing(ring, px, py), false)).sort((a, b) => a.area - b.area)[0];
  const toMap = (e: React.MouseEvent) => {
    const ctm = svgRef.current?.getScreenCTM();
    if (!ctm) return null;
    const p = new DOMPoint(e.clientX, e.clientY).matrixTransform(ctm.inverse());
    return [p.x, p.y] as const;
  };

  // Local moves as a solid line; long jumps (flights, hearthstone) as faint dashes; trips
  // between continents (boats, zeppelins) drawn from one map to the other.
  let path = "";
  let jumps = "";
  let crossings = "";
  let prev: WorldPoint | null = null;
  let prevXY: [number, number] | null = null;
  const fmt = (q: [number, number]) => `${q[0].toFixed(1)},${q[1].toFixed(1)}`;
  for (const p of route ?? []) {
    const q = project(p);
    if (!q) {
      prev = null;
      prevXY = null;
      continue;
    }
    if (!prev || !prevXY) path += `M${fmt(q)}`;
    else if (prev[0] !== p[0]) {
      crossings += `M${fmt(prevXY)}L${fmt(q)}`;
      path += `M${fmt(q)}`;
    } else if (Math.hypot(p[1] - prev[1], p[2] - prev[2]) > 1200) {
      jumps += `M${fmt(prevXY)}L${fmt(q)}`;
      path += `M${fmt(q)}`;
    } else path += `L${fmt(q)}`;
    prev = p;
    prevXY = q;
  }
  // Tabs: the continents of this game version, then all of them side by side.
  const available = CONTINENTS.filter((c) => (index ?? []).some((m) => m.kind === 2 && m.bounds?.map === c.map));
  const TABS: { view: ContinentView; label: string }[] = [
    ...available.map((c) => ({ view: c.map as ContinentView, label: c.label })),
    { view: "both", label: available.length > 2 ? "Tous" : "Les deux" },
  ];
  const k = 1 / view.scale; // keep strokes and labels readable when zoomed
  const s = panels.length > 2 ? 2.2 : panels.length > 1 ? 1.6 : 1; // maps side by side are drawn smaller

  return (
    <div className="world-map">
      <div className="map-tabs" role="tablist">
        {onContinent &&
          TABS.map((tab) => (
            <button key={String(tab.view)} role="tab" aria-selected={tab.view === continent} className={tab.view === continent ? "active" : ""} onClick={() => onContinent(tab.view)}>
              {t(tab.label)}
            </button>
          ))}
        <span className="map-hover">{hover?.name ?? (onZoneClick ? t("Cliquez une zone : privilégiée → exclue → neutre · Ctrl + molette : zoom") : t("Ctrl + molette : zoom · glisser : déplacer · double-clic : vue entière"))}</span>
      </div>
      <div
        className="map-frame"
        style={{ aspectRatio: `${W} / ${H}`, maxWidth: height ? (height * W) / H : undefined }}
        ref={frameRef}
        onMouseDown={(e) => (drag.current = { x: e.clientX, y: e.clientY, vx: view.x, vy: view.y, moved: false })}
        onMouseMove={(e) => {
          const d = drag.current;
          if (d && e.buttons === 1) {
            const dx = e.clientX - d.x;
            const dy = e.clientY - d.y;
            if (Math.abs(dx) + Math.abs(dy) > 4) d.moved = true;
            if (d.moved) setView((v) => ({ ...v, x: d.vx + dx, y: d.vy + dy }));
          } else {
            const p = toMap(e);
            const hit = p && zoneAt(p[0], p[1]);
            setHover(hit ? { id: hit.z.id, name: areaName(hit.z) } : null);
          }
        }}
        onMouseUp={() => setTimeout(() => (drag.current = null), 0)}
        onMouseLeave={() => {
          drag.current = null;
          setHover(null);
        }}
        onDoubleClick={() => setView({ scale: 1, x: 0, y: 0 })}
      >
        <div className="map-inner" style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.scale})` }}>
          <svg
            ref={svgRef}
            viewBox={`0 0 ${W} ${H}`}
            preserveAspectRatio="none"
            onClick={(e) => {
              if (!onZoneClick || drag.current?.moved) return;
              const p = toMap(e);
              const hit = p && zoneAt(p[0], p[1]);
              if (hit) onZoneClick(hit.z.area!, hit.z.name);
            }}
          >
            {panels.map((p) =>
              loaded[p.info.id] ? (
                <image key={p.info.id} href={loaded[p.info.id]} x={p.dx} y={0} width={p.info.width} height={p.info.height} preserveAspectRatio="none" />
              ) : (
                <text key={p.info.id} x={p.dx + p.info.width / 2} y={H / 2} textAnchor="middle" className="map-loading-text">
                  {t("Chargement de la carte…")}
                </text>
              ),
            )}
            {onZoneClick && hover && (
              <path d={zones.find((r) => r.z.id === hover.id)?.d} className="zone-hover" fillRule="evenodd" strokeWidth={2.5 * k * s} strokeLinejoin="round" />
            )}
            {zones
              .filter((r) => zoneStates?.[r.z.area!])
              .map((r) => (
                <g key={r.z.id} className={`zone-mark ${zoneStates![r.z.area!]}`}>
                  <path d={r.d} fillRule="evenodd" strokeWidth={3 * k * s} strokeLinejoin="round" />
                  <text x={r.label[0]} y={r.label[1]} fontSize={24 * k * s} textAnchor="middle" dominantBaseline="middle">
                    {zoneStates![r.z.area!] === "preferred" ? "★ " : "✕ "}
                    {areaName(r.z)}
                  </text>
                </g>
              ))}
            {jumps && <path d={jumps} className="route-jump" strokeWidth={1.5 * k * s} />}
            {crossings && <path d={crossings} className="route-crossing" strokeWidth={1.6 * k * s} />}
            {path && (
              <>
                <path d={path} className="route-halo" strokeWidth={8 * k * s} />
                <path key={live ? path.length : "static"} d={path} className={`route-line${live ? " live" : ""}`} strokeWidth={3.5 * k * s} />
              </>
            )}
            {(markers ?? []).map((m, i) => {
              const q = project(m.at);
              if (!q) return null;
              return (
                <g key={i} className="map-marker" transform={`translate(${q[0]},${q[1]}) scale(${k * s})`}>
                  <title>{m.title ?? m.label}</title>
                  <circle r={15} />
                  <text textAnchor="middle" dominantBaseline="central" fontSize={15}>
                    {m.label}
                  </text>
                </g>
              );
            })}
          </svg>
        </div>
      </div>
    </div>
  );
}

/** The big stages of a route: a marker each time it settles in another zone (level shown). */
export function routeMarkers(
  steps: { zone?: string; level: number; time: number; world?: WorldPoint }[],
  title: (zone: string, level: number) => string = (zone, level) => `${zone} · niveau ${level}`,
  minStops = 12,
): MapMarker[] {
  const out: MapMarker[] = [];
  let i = 0;
  while (i < steps.length) {
    const zone = steps[i].zone;
    let j = i;
    while (j < steps.length && (steps[j].zone === zone || !steps[j].zone)) j++;
    const first = steps.slice(i, j).find((s) => s.world);
    if (zone && first?.world && j - i >= minStops && out[out.length - 1]?.title?.split(" · ")[0] !== zone) {
      out.push({ at: first.world, label: String(first.level), title: title(zone, first.level) });
    }
    i = Math.max(j, i + 1);
  }
  return out;
}

/** Ray casting: whether a point is inside a ring. */
function inRing(ring: [number, number][], x: number, y: number): boolean {
  let inside = false;
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    const [xi, yi] = ring[i];
    const [xj, yj] = ring[j];
    if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}

/** Signed area (shoelace). */
function ringArea(ring: [number, number][]): number {
  let a = 0;
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) a += ring[j][0] * ring[i][1] - ring[i][0] * ring[j][1];
  return a / 2;
}

/** Where to write a zone's name: the centroid of its largest ring, or the inner point closest to it. */
function labelPoint(rings: [number, number][][]): [number, number] {
  const ring = rings.reduce((a, b) => (Math.abs(ringArea(b)) > Math.abs(ringArea(a)) ? b : a));
  const area = ringArea(ring);
  let cx = 0;
  let cy = 0;
  for (let i = 0, j = ring.length - 1; i < ring.length; j = i++) {
    const f = ring[j][0] * ring[i][1] - ring[i][0] * ring[j][1];
    cx += (ring[j][0] + ring[i][0]) * f;
    cy += (ring[j][1] + ring[i][1]) * f;
  }
  const c: [number, number] = area ? [cx / (6 * area), cy / (6 * area)] : ring[0];
  if (inRing(ring, c[0], c[1])) return c;
  // Concave shape: scan the ring's box for the inside point nearest the centroid.
  const xs = ring.map((p) => p[0]);
  const ys = ring.map((p) => p[1]);
  let best: [number, number] = ring[0];
  let bestD = Infinity;
  for (let x = Math.min(...xs); x <= Math.max(...xs); x += 4)
    for (let y = Math.min(...ys); y <= Math.max(...ys); y += 4) {
      const d = (x - c[0]) ** 2 + (y - c[1]) ** 2;
      if (d < bestD && inRing(ring, x, y)) [best, bestD] = [[x, y], d];
    }
  return best;
}
