import type { ProfessionGoal } from "../api";
import { useT } from "../i18n";

/**
 * Mirror of `fg_route::profession::PROFESSIONS`: default skill at the levels of `MILESTONE_LEVELS`
 * (from classic leveling guides: taken around level 5, slow start, catching up by level 20;
 * gathering ahead, crafting behind its materials, fishing fast then slow).
 */
export const PROFESSIONS: { key: string; label: string; secondary: boolean; curve: number[] }[] = [
  { key: "alchemy", label: "Alchimie", secondary: false, curve: [25, 70, 115, 145, 170, 215, 265, 300] },
  { key: "blacksmithing", label: "Forge", secondary: false, curve: [20, 55, 100, 125, 150, 205, 260, 300] },
  { key: "enchanting", label: "Enchantement", secondary: false, curve: [5, 35, 75, 110, 140, 195, 250, 300] },
  { key: "engineering", label: "Ingénierie", secondary: false, curve: [20, 60, 100, 125, 150, 200, 255, 300] },
  { key: "herbalism", label: "Herboristerie", secondary: false, curve: [40, 90, 135, 165, 190, 240, 285, 300] },
  { key: "leatherworking", label: "Travail du cuir", secondary: false, curve: [20, 60, 105, 130, 150, 200, 250, 300] },
  { key: "mining", label: "Minage", secondary: false, curve: [30, 75, 115, 145, 170, 225, 275, 300] },
  { key: "skinning", label: "Dépeçage", secondary: false, curve: [35, 75, 110, 135, 160, 210, 260, 300] },
  { key: "tailoring", label: "Couture", secondary: false, curve: [20, 60, 105, 135, 160, 215, 265, 300] },
  { key: "cooking", label: "Cuisine", secondary: true, curve: [35, 80, 125, 150, 175, 225, 265, 300] },
  { key: "fishing", label: "Pêche", secondary: true, curve: [45, 95, 140, 165, 190, 230, 265, 300] },
  { key: "first_aid", label: "Secourisme", secondary: true, curve: [30, 75, 120, 150, 180, 240, 285, 300] },
];

const MILESTONE_LEVELS = [10, 15, 20, 25, 30, 40, 50, 60];

/** Mirror of `fg_route::profession::START_LEVEL`. */
const START_LEVEL = 5;

function defaultMilestones(key: string): [number, number][] {
  const curve = PROFESSIONS.find((p) => p.key === key)?.curve ?? [];
  return curve.map((s, i) => [MILESTONE_LEVELS[i], s]);
}

/** Mirror of `fg_route::profession::target_at`: skill to have at a level (0 until the profession is taken). */
function skillAt(goal: ProfessionGoal, level: number): number {
  const start = goal.start_level ?? START_LEVEL;
  if (level <= start) return 0;
  const sorted = [...(goal.milestones ?? [])].sort((a, b) => a[0] - b[0]);
  if (sorted.length === 0) sorted.push(...defaultMilestones(goal.key));
  const points = sorted.filter(([l]) => l > start);
  let curve = 5 * (level - start);
  if (sorted.length > 0) {
    const i = points.findIndex(([l]) => l >= level);
    if (i === 0)
      curve = Math.trunc((points[0][1] * (level - start)) / Math.max(1, points[0][0] - start));
    else if (i > 0) {
      const [[l0, s0], [l1, s1]] = [points[i - 1], points[i]];
      curve =
        s0 + Math.trunc(((s1 - s0) * (level - l0)) / Math.max(1, l1 - l0));
    } else curve = sorted[sorted.length - 1][1];
  }
  return Math.max(0, Math.min(curve, goal.target, 300));
}

interface Props {
  goals: ProfessionGoal[];
  onChange: (goals: ProfessionGoal[]) => void;
}

export function ProfessionPicker({ goals, onChange }: Props) {
  const t = useT();
  const primaries = goals.filter(
    (g) => !PROFESSIONS.find((p) => p.key === g.key)?.secondary,
  ).length;
  const toggle = (key: string, on: boolean) =>
    onChange(
      on
        ? [...goals, { key, target: 300 }]
        : goals.filter((g) => g.key !== key),
    );
  const update = (key: string, patch: Partial<ProfessionGoal>) =>
    onChange(goals.map((g) => (g.key === key ? { ...g, ...patch } : g)));

  const row = (p: (typeof PROFESSIONS)[number]) => {
    const goal = goals.find((g) => g.key === p.key);
    const blocked = !goal && !p.secondary && primaries >= 2;
    return (
      <div className="profession" key={p.key}>
        <label
          className="field-bool"
          title={blocked ? t("Deux métiers principaux au maximum") : undefined}
        >
          <input
            type="checkbox"
            checked={Boolean(goal)}
            disabled={blocked}
            onChange={(e) => toggle(p.key, e.target.checked)}
          />
          <span>{t(p.label)}</span>
        </label>
        {goal && (
          <>
            <span className="profession-goal">
              {t("jusqu'à")}
              <input
                type="number"
                min={1}
                max={300}
                step={5}
                value={goal.target}
                onChange={(e) =>
                  update(p.key, { target: Number(e.target.value) })
                }
              />
              {t("dès le niveau")}
              <input
                type="number"
                min={1}
                max={60}
                step={1}
                value={goal.start_level ?? START_LEVEL}
                title={t("Niveau où vous prenez le métier : la courbe part de 0 à ce niveau")}
                onChange={(e) => update(p.key, { start_level: Number(e.target.value) })}
              />
            </span>
            <details className="milestones">
              <summary title={t("Compétence à atteindre selon le niveau du personnage")}>
                {t("Paliers")}{" "}
                {goal.milestones?.length ? t("(personnalisés)") : t("(guides classiques)")}
              </summary>
              <div className="milestone-grid">
                {MILESTONE_LEVELS.map((level) => (
                  <label key={level}>
                    <small>{t("niv. {level}", { level })}</small>
                    <input
                      type="number"
                      min={0}
                      max={300}
                      step={5}
                      value={skillAt(goal, level)}
                      onChange={(e) =>
                        update(p.key, {
                          milestones: MILESTONE_LEVELS.map((l) => [
                            l,
                            l === level
                              ? Number(e.target.value)
                              : skillAt(goal, l),
                          ]),
                        })
                      }
                    />
                  </label>
                ))}
              </div>
              {Boolean(goal.milestones?.length) && (
                <button
                  type="button"
                  className="link"
                  onClick={() => update(p.key, { milestones: [] })}
                >
                  {t("Revenir aux valeurs des guides")}
                </button>
              )}
            </details>
          </>
        )}
      </div>
    );
  };

  return (
    <div className="professions">
      <small className="muted">{t("Principaux (2 max)")}</small>
      {PROFESSIONS.filter((p) => !p.secondary).map(row)}
      <small className="muted">{t("Secondaires")}</small>
      {PROFESSIONS.filter((p) => p.secondary).map(row)}
    </div>
  );
}
