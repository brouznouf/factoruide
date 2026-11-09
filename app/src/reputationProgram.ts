import type { RepProgramResult, RouteStep, Step } from "./api";

/** Figures from the replayed programme, never inferred from an ordinary guide's quests. */
export function reputationFacts(steps: Pick<Step, "reputation_program">[]) {
  const programs = steps.flatMap((step) => step.reputation_program ? [step.reputation_program] : []);
  if (!programs.length) return null;
  const facts: Pick<RepProgramResult, "active_play_seconds" | "daily_opportunities" | "reset_transitions" | "calendar_wait_seconds"> & { estimates: boolean; warnings: string[] } = {
    active_play_seconds: 0, daily_opportunities: 0, reset_transitions: 0, calendar_wait_seconds: 0, estimates: false, warnings: [],
  };
  const warnings = new Set<string>();
  for (const program of programs) {
    const result = program.result;
    facts.active_play_seconds += result.active_play_seconds;
    facts.daily_opportunities += result.daily_opportunities;
    facts.reset_transitions += result.reset_transitions;
    facts.calendar_wait_seconds = facts.calendar_wait_seconds == null || result.calendar_wait_seconds == null ? null : facts.calendar_wait_seconds + result.calendar_wait_seconds;
    const used = new Set(program.phases.flatMap((phase) => phase.actions.map((action) => action.source_id)));
    facts.estimates ||= program.sources.some((source) => used.has(source.id) && source.confidence === "classic_estimate");
    // Prefix kills are reported by the combined replay without becoming suffix actions.
    facts.estimates ||= result.warnings.some((warning) => warning.startsWith("Classic-derived reputation rules are forecasts"));
    for (const warning of result.warnings) warnings.add(warning);
  }
  facts.warnings = [...warnings];
  return facts;
}

/** Keep the exact protected suffix while editing ordinary quest and mandatory work. */
export function protectReputationPrograms<T extends { step: RouteStep }>(original: T[], proposed: T[]): T[] {
  return [...proposed.filter((item) => item.step.kind !== "reputation_program"), ...original.filter((item) => item.step.kind === "reputation_program")];
}
