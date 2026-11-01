import type { RequiredQuest } from "./api";

/** The player's ID or exact name. */
export function requiredQuest(input: string): number | string {
  const value = input.trim();
  return /^[1-9]\d*$/.test(value) ? Number(value) : value;
}

export function questChoice(quest: RequiredQuest): number | string {
  return typeof quest === "object" ? quest.quest : quest;
}

/** Edit one bound, keeping the other; clearing both restores the unbounded request. */
export function withBound(entry: RequiredQuest, bound: "min_level" | "max_level", value: string): RequiredQuest {
  if (value && !Number.isInteger(Number(value))) return entry;
  const window = typeof entry === "object" ? { ...entry } : { quest: entry };
  if (value) window[bound] = Number(value);
  else delete window[bound];
  return window.min_level == null && window.max_level == null ? window.quest : window;
}
