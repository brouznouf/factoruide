import type { RequiredQuest } from "./api";

/** The player's ID or exact name, with optional inclusive turn-in levels. */
export function requiredQuest(input: string, min = "", max = ""): RequiredQuest {
  const value = input.trim();
  const quest = /^[1-9]\d*$/.test(value) ? Number(value) : value;
  if (!min && !max) return quest;
  return { quest, ...(min ? { min_level: Number(min) } : {}), ...(max ? { max_level: Number(max) } : {}) };
}

export function questChoice(quest: RequiredQuest): number | string {
  return typeof quest === "object" ? quest.quest : quest;
}

export function requiredQuestLabel(quest: RequiredQuest): string {
  const label = String(questChoice(quest));
  if (typeof quest !== "object") return label;
  if (quest.min_level != null && quest.max_level != null) return `${label} (${quest.min_level}–${quest.max_level})`;
  if (quest.min_level != null) return `${label} (${quest.min_level}+)`;
  if (quest.max_level != null) return `${label} (≤${quest.max_level})`;
  return label;
}
