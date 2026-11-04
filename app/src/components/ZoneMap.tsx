import { useEffect, useMemo, useState } from "react";
import type { MapInfo, Step } from "../api";
import { useAreaName, useT } from "../i18n";
import { loadImage, loadIndex } from "./WorldMap";

/** A step of the guide, numbered as in the route table. */
export interface NumberedStep extends Step {
  n: number;
}

interface Props {
  /** The zone (or dungeon) of the steps. */
  title: string;
  steps: NumberedStep[];
  /** Step pointed at (in the table or on the map). */
  active: number | null;
  onActive: (n: number | null) => void;
}

/** Where a step is shown: on its dungeon floor's map when inside one, else on its zone's. */
const placeOf = (s: Step) => s.floor ?? s.map;

/** The map of a zone with its steps in order: the zone the most steps are on (cities, caves and
 * dungeons have maps of their own, a dungeon one per floor). Each dot is a step, colored by its
 * type. */
export function ZoneMap({ title, steps, active, onActive }: Props) {
  const t = useT();
  const areaName = useAreaName();
  const [index, setIndex] = useState<MapInfo[] | null>(null);
  const [image, setImage] = useState<{ id: number; src: string } | null>(null);
  useEffect(() => {
    loadIndex()
      .then(setIndex)
      .catch(() => setIndex([]));
  }, []);
  const mapId = useMemo(() => {
    const count = new Map<number, number>();
    for (const s of steps) {
      const p = placeOf(s);
      if (p) count.set(p.ui_map, (count.get(p.ui_map) ?? 0) + 1);
    }
    return [...count].sort((a, b) => b[1] - a[1])[0]?.[0] ?? null;
  }, [steps]);
  useEffect(() => {
    if (mapId == null) return;
    let live = true;
    loadImage(mapId)
      .then((src) => live && setImage({ id: mapId, src }))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [mapId]);

  const info = index?.find((m) => m.id === mapId);
  const here = steps.filter((s) => placeOf(s)?.ui_map === mapId);
  const elsewhere = steps.filter((s) => placeOf(s) && placeOf(s)!.ui_map !== mapId).length;
  if (mapId == null || !info) {
    return (
      <div className="zone-map">
        <div className="zone-map-title">{title}</div>
        <p className="muted">{mapId == null ? t("Pas de position pour ces étapes.") : t("Chargement de la carte…")}</p>
      </div>
    );
  }
  const W = info.width;
  const H = info.height;
  const at = (s: NumberedStep) => [(placeOf(s)!.x / 100) * W, (placeOf(s)!.y / 100) * H] as const;
  const r = W / 90;
  // Numbers on every dot when there are few, else only on the step pointed at.
  const numbered = here.length <= 20;
  return (
    <div className="zone-map">
      <div className="zone-map-title">
        {title}
        {areaName(info) !== title && <small className="muted"> · {areaName(info)}</small>}
      </div>
      <div className="zone-map-frame" style={{ aspectRatio: `${W} / ${H}` }}>
        {image?.id === mapId && <img src={image.src} alt="" />}
        <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" onMouseLeave={() => onActive(null)}>
          <polyline className="zone-map-path" points={here.map((s) => at(s).join(",")).join(" ")} strokeWidth={r / 3} />
          {here.map((s, k) => {
            const [x, y] = at(s);
            const on = s.n === active;
            return (
              // A step may have several dots (the guide editor's: the lines of a step).
              <g key={k} className={`zone-dot kind-${s.kind}${on ? " active" : ""}`} onMouseEnter={() => onActive(s.n)}>
                <title>{`${s.n}. ${s.text}`}</title>
                <circle cx={x} cy={y} r={on ? r * 1.6 : r} strokeWidth={r / 4} />
                {(numbered || on) && (
                  <text x={x} y={y} fontSize={on ? r * 1.6 : r * 1.1} textAnchor="middle" dominantBaseline="central">
                    {s.n}
                  </text>
                )}
              </g>
            );
          })}
        </svg>
      </div>
      {elsewhere > 0 && <small className="muted">{t("{n} étapes sur une autre carte (ville, grotte, donjon)", { n: elsewhere })}</small>}
    </div>
  );
}
