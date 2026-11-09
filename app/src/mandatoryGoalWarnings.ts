import type { Breakdown } from "./api";

type WarningCategory = "class" | "gear" | "other" | "legacy";
export interface MandatoryGoalWarning {
  category: WarningCategory;
  count: number;
}

type WarningCounts = Pick<Breakdown, "mandatory_goal_counts"> & Partial<Pick<Breakdown, "missing_class_quests">>;

/** Select missed-goal warnings without consulting request modes or gear recommendations. */
export function mandatoryGoalWarnings(breakdown: WarningCounts): MandatoryGoalWarning[] {
  const counts = breakdown.mandatory_goal_counts;
  const warnings: MandatoryGoalWarning[] = counts != null
    ? [
        { category: "class", count: counts.class_quests },
        { category: "gear", count: counts.bis_quests },
        { category: "other", count: counts.other_quests },
      ]
    : [{ category: "legacy", count: breakdown.missing_class_quests ?? 0 }];
  return warnings.filter(({ count }) => Number.isFinite(count) && count > 0);
}

/** Keep category and count together in the existing visible warning text. */
export function mandatoryGoalWarningText(warning: MandatoryGoalWarning, t: (key: string, vars: { n: number }) => string): string {
  const vars = { n: warning.count };
  switch (warning.category) {
    case "class": return t("Quêtes de classe obligatoires manquées : {n}", vars);
    case "gear": return t("Quêtes d'équipement obligatoires manquées (prérequis inclus) : {n}", vars);
    case "other": return t("Autres quêtes obligatoires manquées : {n}", vars);
    case "legacy": return t("Quêtes obligatoires manquées : {n} (ancien guide ; catégories non enregistrées)", vars);
  }
}
