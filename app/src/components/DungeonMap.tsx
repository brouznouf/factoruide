import { useEffect, useMemo, useState, type ReactNode } from "react";
import type { MapInfo, Step } from "../api";
import { useT } from "../i18n";
import { loadImage, loadIndex } from "./WorldMap";

/** The steps of a dungeon in the guide: its entrance, then those done inside it. */
export interface Block {
  first: number;
  last: number;
}

/** The name of the dungeon of a block's steps (null: none is done inside one). */
export function dungeonOf(steps: Step[], block: Block | undefined): string | null {
  if (!block) return null;
  for (let i = block.first; i <= block.last; i++) if (steps[i].inside) return steps[i].zone ?? null;
  return null;
}

/** The floors of a dungeon: the maps its steps are placed on, and those of its name (a dungeon
 * whose steps have no place), by floor. */
export function floorsOf(index: MapInfo[], steps: Step[], block: Block, name: string): MapInfo[] {
  const placed = new Set<number>();
  for (let i = block.first; i <= block.last; i++) {
    const f = steps[i].floor;
    if (f) placed.add(f.ui_map);
  }
  return index.filter((m) => m.kind === 4 && (placed.has(m.id) || m.name === name)).sort((a, b) => (a.floor ?? 0) - (b.floor ?? 0));
}

interface Props {
  steps: Step[];
  /** The dungeon's steps (its entrance, then those done inside). */
  block: Block;
  /** The step under way (index in `steps`). */
  step: number;
  /** A step of the dungeon chosen in its list. */
  onStep: (i: number) => void;
  /** Back to the world's map. */
  onBack: () => void;
  /** The line over the map (what the character is doing). */
  status?: ReactNode;
}

/**
 * The guide's map while it is in a dungeon: the dungeon alone, on the floor of the step under way
 * (or one chosen), its steps numbered in their order, and their list beside it (the only place of
 * the steps of a dungeon none of whose places is known).
 */
export function DungeonMap({ steps, block, step, onStep, onBack, status }: Props) {
  const t = useT();
  const [index, setIndex] = useState<MapInfo[] | null>(null);
  useEffect(() => {
    loadIndex()
      .then(setIndex)
      .catch(() => setIndex([]));
  }, []);
  const name = dungeonOf(steps, block) ?? "";
  const floors = useMemo(() => (index ? floorsOf(index, steps, block, name) : []), [index, steps, block, name]);
  const list = useMemo(() => Array.from({ length: block.last - block.first + 1 }, (_, k) => block.first + k), [block]);

  // The floor shown: the step's under way (else that of the dungeon's first step placed: the
  // entrance has none), or one chosen while the step stays on its floor.
  const stepFloor = steps[step]?.floor?.ui_map ?? list.map((i) => steps[i].floor?.ui_map).find((id) => id != null) ?? null;
  const [chosen, setChosen] = useState<{ id: number; from: number | null } | null>(null);
  const choose = (id: number) => setChosen({ id, from: stepFloor });
  const picked = chosen && chosen.from === stepFloor ? chosen.id : null;
  const info = floors.find((f) => f.id === (picked ?? stepFloor)) ?? floors[0];

  const [image, setImage] = useState<{ id: number; src: string } | null>(null);
  useEffect(() => {
    if (!info) return;
    let live = true;
    loadImage(info.id)
      .then((src) => live && setImage({ id: info.id, src }))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [info]);

  const here = list.filter((i) => info && steps[i].floor?.ui_map === info.id);
  const placed = list.some((i) => steps[i].floor);
  const W = info?.width ?? 1;
  const H = info?.height ?? 1;
  const r = W / 90;
  const at = (i: number) => [(steps[i].floor!.x / 100) * W, (steps[i].floor!.y / 100) * H] as const;
  const number = (i: number) => i - block.first + 1;

  return (
    <div className="guide-map dungeon-map">
      <div className="gm-bar">
        <strong>{name}</strong>
        {floors.length > 1 &&
          floors.map((f, k) => (
            <button key={f.id} className={`gm-toggle${f.id === info?.id ? " on" : ""}`} aria-pressed={f.id === info?.id} onClick={() => choose(f.id)}>
              {t("Étage {n}", { n: f.floor ?? k + 1 })}
            </button>
          ))}
        <span className="gm-bar-fill" />
        <button onClick={onBack}>↩ {t("Retour au monde")}</button>
      </div>
      {status && <div className="gm-status">{status}</div>}
      <div className="dm-body">
        <div className="dm-map">
          {info ? (
            <div className="zone-map-frame" style={{ aspectRatio: `${W} / ${H}` }}>
              {image?.id === info.id && <img src={image.src} alt="" />}
              <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="img" aria-label={name}>
                <polyline className="zone-map-path" points={here.map((i) => at(i).join(",")).join(" ")} strokeWidth={r / 3} />
                {here.map((i) => {
                  const [x, y] = at(i);
                  const on = i === step;
                  return (
                    <g key={i} className={`zone-dot kind-${steps[i].kind}${on ? " active" : ""}`} onClick={() => onStep(i)}>
                      <title>{`${number(i)}. ${steps[i].text}`}</title>
                      <circle cx={x} cy={y} r={on ? r * 1.6 : r} strokeWidth={r / 4} />
                      <text x={x} y={y} fontSize={on ? r * 1.6 : r * 1.1} textAnchor="middle" dominantBaseline="central">
                        {number(i)}
                      </text>
                    </g>
                  );
                })}
              </svg>
            </div>
          ) : (
            <p className="muted">{t("Chargement de la carte…")}</p>
          )}
          {info && !placed && <p className="muted">{t("Les places dans ce donjon ne sont pas connues : ses étapes sont dans la liste.")}</p>}
        </div>
        <ol className="dm-steps">
          {list.map((i) => (
            <li key={i} className={i === step ? "active" : undefined}>
              <button className="link" onClick={() => onStep(i)}>
                <span className="dm-n">{number(i)}</span> {steps[i].text}
              </button>
            </li>
          ))}
        </ol>
      </div>
    </div>
  );
}
