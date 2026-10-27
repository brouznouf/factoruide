import { useEffect, useRef, useState, type ReactNode } from "react";
import type { EditCatalog, RouteStep } from "../api";
import { useT } from "../i18n";
import { NodeSelect, type useNames } from "./editing";
import { NumberInput } from "./NumberInput";

type AddKind = "accept" | "objective" | "turn_in" | "bind" | "hearth" | "fly" | "flight_master" | "train" | "abandon" | "grind" | "dungeon" | "link";

const ADD_KINDS: { kind: AddKind; label: string }[] = [
  { kind: "accept", label: "Prendre une quête" },
  { kind: "objective", label: "Faire une quête" },
  { kind: "turn_in", label: "Rendre une quête" },
  { kind: "bind", label: "Lier la pierre de foyer" },
  { kind: "hearth", label: "Pierre de foyer" },
  { kind: "fly", label: "Prendre un vol" },
  { kind: "flight_master", label: "Apprendre un point de vol" },
  { kind: "train", label: "S'entraîner" },
  { kind: "abandon", label: "Abandonner une quête" },
  { kind: "grind", label: "Farmer" },
  { kind: "dungeon", label: "Donjon" },
  { kind: "link", label: "Prendre un transport" },
];

/** Quests listed at once (search to find the others). */
const LIST_MAX = 80;

interface Props {
  /** Inserted after this step (-1: first), where the character has this level. */
  at: number;
  level: number;
  steps: RouteStep[];
  catalog: EditCatalog;
  names: ReturnType<typeof useNames>;
  onAdd: (step: RouteStep) => void;
  /** Shows step `i` of the list (where a quest is already taken). */
  onShow: (i: number) => void;
  onClose: () => void;
  /** Where it opens: beside this point of the window (the button that opened it). */
  anchor: { x: number; y: number };
}

/**
 * What can be added after a step, in a small menu beside it: a quest to take, do or turn in,
 * the hearthstone, a flight, a flight path, training, a dungeon, mobs to kill, a quest to give
 * up, a boat.
 */
export function AddMenu({ at, level, steps, catalog, names, onAdd, onShow, onClose, anchor }: Props) {
  const t = useT();
  const [kind, setKind] = useState<AddKind>("accept");
  const [search, setSearch] = useState("");
  const [farm, setFarm] = useState(false);
  const [mobs, setMobs] = useState(10);
  const [toLevel, setToLevel] = useState(level + 1);
  const [from, setFrom] = useState<number>(catalog.nodes[0]?.id ?? 0);
  const [to, setTo] = useState<number>(catalog.nodes[1]?.id ?? 0);
  const q = search.trim().toLowerCase();
  const match = (...texts: (string | number)[]) => !q || texts.some((x) => String(x).toLowerCase().includes(q));

  // Quests the draft takes, does and turns in.
  const takenAt = new Map<number, number[]>();
  steps.forEach((s, i) => {
    if (s.kind === "accept") takenAt.set(s.quest, [...(takenAt.get(s.quest) ?? []), i]);
  });
  const taken = new Set(takenAt.keys());
  const turned = new Set(steps.flatMap((s) => (s.kind === "turn_in" ? [s.quest] : [])));
  const done = new Set(steps.flatMap((s) => (s.kind === "objective" ? [`${s.quest}/${s.objective}`] : [])));

  // "already in the guide · step 313, 325": each number shows its step.
  const takenNote = (id: number) => (
    <span className="tag">
      {t("déjà dans le guide")} · {t("étape")}{" "}
      {(takenAt.get(id) ?? []).map((i, k) => (
        <span key={i}>
          {k > 0 && ", "}
          <button className="link" title={t("Voir l'étape")} onClick={() => onShow(i)}>
            {i + 1}
          </button>
        </span>
      ))}
    </span>
  );

  const questRow = (id: number, extra: string | null, add: () => void, disabled = false, note?: ReactNode) => {
    const quest = names.quests.get(id);
    return (
      <li key={`${id}/${extra ?? ""}`}>
        <button className="link" onClick={add} disabled={disabled}>
          {quest?.name ?? `#${id}`}
        </button>
        {extra && <span> — {extra}</span>}
        {quest && (
          <span className="muted">
            {" "}
            · {t("niveau {level}", { level: quest.level })} · {quest.zone}
          </span>
        )}
        {note}
      </li>
    );
  };

  let body;
  switch (kind) {
    case "accept": {
      // Those of the character's level there first, those the guide already takes last.
      const list = catalog.quests
        .filter((x) => match(x.name, x.zone, x.id))
        .sort((a, b) => Number(taken.has(a.id)) - Number(taken.has(b.id)) || Math.abs(a.level - level) - Math.abs(b.level - level) || a.level - b.level)
        .slice(0, LIST_MAX);
      body = <ul className="pick-list">{list.map((x) => questRow(x.id, null, () => onAdd({ kind: "accept", quest: x.id }), taken.has(x.id), taken.has(x.id) ? takenNote(x.id) : undefined))}</ul>;
      break;
    }
    case "objective": {
      const list = [...taken].flatMap((id) =>
        (names.quests.get(id)?.objectives ?? []).flatMap((text, k) => (done.has(`${id}/${k}`) ? [] : [{ id, k, text }])),
      );
      const shown = list.filter((x) => match(names.quests.get(x.id)?.name ?? "", x.text, x.id));
      body = (
        <>
          <label className="farm-toggle">
            <input type="checkbox" checked={farm} onChange={(e) => setFarm(e.target.checked)} /> {t("Tuer tous les mobs croisés en chemin")}{" "}
            <span className="muted">{t("(l'XP estimée s'affiche sur l'étape)")}</span>
          </label>
          {shown.length === 0 ? (
            <p className="muted">{t("Aucun objectif à ajouter : chaque quête prise par le guide a déjà ses objectifs.")}</p>
          ) : (
            <ul className="pick-list">{shown.slice(0, LIST_MAX).map((x) => questRow(x.id, x.text, () => onAdd({ kind: "objective", quest: x.id, objective: x.k, farm: farm || undefined })))}</ul>
          )}
        </>
      );
      break;
    }
    case "turn_in": {
      const list = [...taken].filter((id) => !turned.has(id) && match(names.quests.get(id)?.name ?? "", id));
      body =
        list.length === 0 ? (
          <p className="muted">{t("Aucune quête à rendre : chaque quête prise par le guide est déjà rendue.")}</p>
        ) : (
          <ul className="pick-list">{list.slice(0, LIST_MAX).map((id) => questRow(id, null, () => onAdd({ kind: "turn_in", quest: id })))}</ul>
        );
      break;
    }
    case "bind": {
      const list = catalog.inns.filter((x) => match(x.name, x.zone));
      body = (
        <ul className="pick-list">
          {list.slice(0, LIST_MAX).map((x) => (
            <li key={x.id}>
              <button className="link" onClick={() => onAdd({ kind: "bind", inn: x.id })}>
                {x.name}
              </button>{" "}
              <span className="muted">· {x.zone}</span>
            </li>
          ))}
        </ul>
      );
      break;
    }
    case "hearth":
      body = (
        <div className="actions">
          <button onClick={() => onAdd({ kind: "hearth" })}>{t("Utiliser la pierre de foyer")}</button>
          {catalog.recall && <button onClick={() => onAdd({ kind: "hearth", recall: true })}>{t("Utiliser le Rappel astral")}</button>}
        </div>
      );
      break;
    case "fly":
      body = (
        <div className="fly-pick">
          <label>
            {t("Départ")}
            <NodeSelect nodes={catalog.nodes} value={from} onChange={setFrom} />
          </label>
          <label>
            {t("Arrivée")}
            <NodeSelect nodes={catalog.nodes} value={to} onChange={setTo} />
          </label>
          <button onClick={() => onAdd({ kind: "fly", from, to })} disabled={from === to}>
            {t("Ajouter le vol")}
          </button>
          <p className="muted">{t("Les deux points de vol doivent être connus à ce moment du guide.")}</p>
        </div>
      );
      break;
    case "flight_master": {
      const list = catalog.nodes.filter((x) => match(x.name, x.zone));
      body = (
        <ul className="pick-list">
          {list.map((x) => (
            <li key={x.id}>
              <button className="link" onClick={() => onAdd({ kind: "flight_master", node: x.id })}>
                {x.name}
              </button>
            </li>
          ))}
        </ul>
      );
      break;
    }
    case "train": {
      const list = catalog.trainers.filter((x) => match(x.name, x.zone));
      body = (
        <ul className="pick-list">
          {list.map((x) => (
            <li key={x.id}>
              <button className="link" onClick={() => onAdd({ kind: "train", trainer: x.id })}>
                {x.name}
              </button>{" "}
              <span className="muted">· {x.zone}</span>
            </li>
          ))}
        </ul>
      );
      break;
    }
    case "abandon": {
      const list = [...taken].filter((id) => !turned.has(id) && match(names.quests.get(id)?.name ?? "", id));
      body =
        list.length === 0 ? (
          <p className="muted">{t("Aucune quête à abandonner : chaque quête prise par le guide est déjà rendue.")}</p>
        ) : (
          <ul className="pick-list">{list.slice(0, LIST_MAX).map((id) => questRow(id, null, () => onAdd({ kind: "abandon", quest: id })))}</ul>
        );
      break;
    }
    case "grind":
      body = (
        <div className="fly-pick">
          <label>
            {t("Mobs à tuer")}
            <NumberInput value={mobs} min={1} max={2000} onChange={(n) => setMobs(Math.max(1, Math.round(n)))} aria-label={t("Mobs à tuer")} />
          </label>
          <button onClick={() => onAdd({ kind: "grind", mobs })}>{t("Ajouter")}</button>
          <label>
            {t("Niveau à atteindre")}
            <NumberInput value={toLevel} min={2} max={70} onChange={(n) => setToLevel(Math.max(2, Math.round(n)))} aria-label={t("Niveau à atteindre")} />
          </label>
          <button onClick={() => onAdd({ kind: "grind_to", level: toLevel })}>{t("Ajouter")}</button>
          <p className="muted">{t("Des mobs du niveau du personnage, là où il se trouve : l'XP gagnée s'affiche sur l'étape.")}</p>
        </div>
      );
      break;
    case "link": {
      const list = catalog.links.filter((l) => match(l.link.name, l.from, l.to));
      body = (
        <ul className="pick-list">
          {list.slice(0, LIST_MAX).map((l) => (
            <li key={`${l.link.name}/${l.link.to}`}>
              <button className="link" onClick={() => onAdd({ kind: "link", first: l.link, last: l.link })}>
                {l.link.name}
              </button>{" "}
              <span className="muted">
                · {l.from} → {l.to}
              </span>
            </li>
          ))}
        </ul>
      );
      break;
    }
    case "dungeon": {
      const list = catalog.dungeons.filter((d) => match(d.name));
      body = (
        <ul className="pick-list">
          {list.map((d) => (
            <li key={d.id}>
              <button className="link" onClick={() => onAdd({ kind: "dungeon", dungeon: d.id })}>
                {d.name}
              </button>{" "}
              <span className="muted">
                · {d.min_level}–{d.max_level}
              </span>
            </li>
          ))}
        </ul>
      );
      break;
    }
  }
  // Closed by a click elsewhere or Escape.
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const down = (e: PointerEvent) => {
      if (!box.current?.contains(e.target as Node)) onClose();
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("pointerdown", down);
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("pointerdown", down);
      document.removeEventListener("keydown", key);
    };
  }, [onClose]);
  const searchable = kind !== "hearth" && kind !== "fly" && kind !== "grind";
  // Beside the button, kept in the window.
  const style = { left: Math.min(anchor.x, window.innerWidth - 380), top: Math.max(8, Math.min(anchor.y, window.innerHeight - 440)) };
  return (
    <div className="add-menu" ref={box} style={style} role="dialog" aria-label={at < 0 ? t("Ajouter une étape au début") : t("Ajouter une étape après l'étape {n}", { n: at + 1 })}>
      <div className="add-head">
        <strong>{at < 0 ? t("Ajouter une étape au début") : t("Ajouter une étape après l'étape {n}", { n: at + 1 })}</strong>
        <button className="link" onClick={onClose} aria-label={t("Fermer")}>
          ×
        </button>
      </div>
      <div className="add-kinds" role="tablist">
        {ADD_KINDS.filter((k) => (k.kind !== "dungeon" || catalog.dungeons.length > 0) && (k.kind !== "link" || catalog.links.length > 0)).map((k) => (
          <button key={k.kind} role="tab" aria-selected={kind === k.kind} className={kind === k.kind ? "active" : ""} onClick={() => setKind(k.kind)}>
            {t(k.label)}
          </button>
        ))}
      </div>
      {searchable && <input className="search" placeholder={t("Rechercher…")} value={search} onChange={(e) => setSearch(e.target.value)} autoFocus />}
      <div className="add-body">{body}</div>
    </div>
  );
}
