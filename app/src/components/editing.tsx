// What editing a guide shows of its steps: why a step is not possible, the kinds of steps, the
// names of what a step refers to, the mobs killed on the way. Shared by the step list and the
// menu to add steps.
import { useMemo } from "react";
import { fmtTime, type EditCatalog, type Farmed, type RouteStep, type Step } from "../api";
import { useT } from "../i18n";

/** Why a step is not possible or does nothing (`fg_route` editor, English as sent), French key of the catalog. */
export const REASONS: Record<string, string> = {
  "quest not available": "quête indisponible pour ce personnage",
  "unknown flight path": "point de vol inconnu",
  "unknown inn": "auberge inconnue",
  "unknown dungeon": "donjon inconnu",
  "unknown objective": "objectif inconnu",
  "already turned in": "déjà rendue",
  "already in the log": "déjà dans le journal de quêtes",
  "quest log full": "journal de quêtes plein",
  "prerequisite not done": "prérequis pas fait",
  "exclusive quest taken": "une quête exclusive est déjà prise",
  "gray quest leading nowhere": "quête grise sans suite",
  "dungeon quest (dungeons off)": "quête de donjon (donjons désactivés)",
  "quest not accepted": "quête pas prise",
  "objective done in a dungeon run": "objectif fait pendant un donjon",
  "objectives not done": "objectifs pas faits",
  "flight path already known": "point de vol déjà connu",
  "hearthstone already bound there": "pierre de foyer déjà liée ici",
  "no such spell": "sort pas appris",
  "dungeon already run or left out": "donjon déjà fait ou exclu",
  "dungeon quest taken but its dungeon is not run": "quête de donjon prise mais son donjon n'est pas fait",
  "not possible": "impossible",
  unreachable: "inaccessible",
  "done before": "déjà fait plus tôt (sur place ou en chemin)",
  "not ready": "pierre de foyer pas encore prête",
  "target level reached": "niveau cible déjà atteint",
  "no mob to kill": "aucun mob à tuer",
  "same flight path": "même point de vol au départ et à l'arrivée",
  "flight path not known": "point de vol de départ pas encore connu",
  "destination not known": "point de vol d'arrivée pas encore connu",
  "no flight between them": "aucun vol entre ces points",
  "no such crossing": "aucune traversée entre ces deux transports",
  "unknown crossing": "transport inconnu",
  "teleport not learned": "téléportation pas encore apprise",
  "unknown trainer": "entraîneur inconnu",
  "nothing to learn": "rien de nouveau à apprendre",
  "level reached": "niveau déjà atteint",
  "quest not in the log": "quête pas dans le journal",
  "quest time limit passed": "temps limite de la quête dépassé",
};

/** Step type of the guide (its color, see .kind-*) and label of each step kind. */
export const KINDS: Record<RouteStep["kind"], { css: string; label: string }> = {
  camp_grind: { css: "grind", label: "Grind at camp" },
  service: { css: "train", label: "Inventory visit" },
  dungeon_exit: { css: "dungeon", label: "Leave dungeon" },
  accept: { css: "accept", label: "Prendre" },
  objective: { css: "objective", label: "Faire" },
  turn_in: { css: "turnin", label: "Rendre" },
  flight_master: { css: "flight_master", label: "Point de vol" },
  bind: { css: "bind", label: "Auberge" },
  hearth: { css: "hearth", label: "Pierre de foyer" },
  fly: { css: "fly", label: "Vol" },
  dungeon: { css: "dungeon", label: "Donjon" },
  link: { css: "link", label: "Transport" },
  teleport: { css: "teleport", label: "Téléportation" },
  train: { css: "train", label: "Entraîneur" },
  abandon: { css: "abandon", label: "Abandonner" },
  grind: { css: "grind", label: "Farmer" },
  grind_to: { css: "grind", label: "Farmer" },
};

/** Mobs killed on the way: count, XP and time (`estimate`: what checking the box would give). */
export function FarmText({ farm, estimate }: { farm: Farmed; estimate?: boolean }) {
  const t = useT();
  const vars = { n: farm.kills, xp: farm.xp.toLocaleString(), time: fmtTime(farm.seconds) };
  return <>{estimate ? t("si coché : ~{n} mobs, ~{xp} xp, {time}", vars) : t("{n} mobs tués en chemin : {xp} xp, {time}", vars)}</>;
}

/** Names of the catalog by ID, and the label of a step that has no line of the guide (not replayed). */
export function useNames(catalog: EditCatalog | undefined) {
  return useMemo(() => {
    const quests = new Map(catalog?.quests.map((q) => [q.id, q]));
    const nodes = new Map(catalog?.nodes.map((n) => [n.id, n]));
    const inns = new Map(catalog?.inns.map((n) => [n.id, n]));
    const trainers = new Map(catalog?.trainers.map((n) => [n.id, n]));
    const dungeons = new Map(catalog?.dungeons.map((d) => [d.id, d]));
    const quest = (id: number) => quests.get(id)?.name ?? `#${id}`;
    const node = (id: number) => nodes.get(id)?.name ?? `#${id}`;
    const label = (s: RouteStep, t: (fr: string, vars?: Record<string, string | number>) => string): string => {
      switch (s.kind) {
        case "camp_grind":
          return `Kill ${s.mobs} mobs at camp`;
        case "service":
          return `${s.service === "vendor" ? "Vendor" : "Mailbox"} #${s.id}`;
        case "dungeon_exit":
          return `Leave ${dungeons.get(s.dungeon)?.name ?? `#${s.dungeon}`}`;
        case "accept":
          return quest(s.quest);
        case "objective":
          return `${quest(s.quest)} — ${quests.get(s.quest)?.objectives[s.objective] ?? `#${s.objective + 1}`}`;
        case "turn_in":
          return quest(s.quest);
        case "flight_master":
          return t("Apprendre le point de vol {name}", { name: node(s.node) });
        case "bind": {
          const inn = inns.get(s.inn);
          return t("Lier la pierre de foyer : {name} ({zone})", { name: inn?.name ?? `#${s.inn}`, zone: inn?.zone ?? "?" });
        }
        case "hearth":
          return s.recall ? t("Rappel astral") : t("Pierre de foyer");
        case "fly":
          return t("Vol {from} → {to}", { from: node(s.from), to: node(s.to) });
        case "dungeon":
          return dungeons.get(s.dungeon)?.name ?? `#${s.dungeon}`;
        case "link":
          return s.first.name === s.last.name && s.first.to === s.last.to ? s.first.name : t("{first}, puis {last}", { first: s.first.name, last: s.last.name });
        case "teleport":
          return s.spell;
        case "train": {
          const trainer = trainers.get(s.trainer);
          return t("S'entraîner auprès de {name} ({zone})", { name: trainer?.name ?? `#${s.trainer}`, zone: trainer?.zone ?? "?" });
        }
        case "abandon":
          return t("Abandonner {quest}", { quest: quest(s.quest) });
        case "grind":
          return t("Tuer {n} mobs dans le coin", { n: s.mobs });
        case "grind_to":
          return t("Farmer jusqu'au niveau {level}", { level: s.level });
      }
    };
    return { label, quests, nodes, dungeons };
  }, [catalog]);
}

/** The line a step of the draft shows before it is replayed (`at`: the line before it). */
export function draftLine(step: RouteStep, label: string, at: Step | null): Step {
  const quest = "quest" in step ? step.quest : undefined;
  return {
    kind: KINDS[step.kind].css,
    text: label,
    quest,
    objective: step.kind === "objective" ? step.objective : undefined,
    level: at?.level ?? 1,
    time: at?.time ?? 0,
  };
}

/** The flight paths to choose from. */
export function NodeSelect({ nodes, value, onChange }: { nodes: EditCatalog["nodes"]; value: number; onChange: (id: number) => void }) {
  return (
    <select value={value} onChange={(e) => onChange(Number(e.target.value))}>
      {nodes.map((n) => (
        <option key={n.id} value={n.id}>
          {n.name}
        </option>
      ))}
    </select>
  );
}
