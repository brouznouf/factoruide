import assert from "node:assert/strict";
import { test } from "node:test";
import { protectReputationPrograms, reputationFacts } from "../src/reputationProgram.ts";

const result = (extra = {}) => ({ active_play_seconds: 120, daily_opportunities: 3, reset_transitions: 2, calendar_wait_seconds: null, warnings: ["Known source cap"], ...extra });
const program = (extra = {}) => ({ sources: [{ id: "estimated", confidence: "classic_estimate" }], phases: [{ actions: [{ source_id: "estimated" }] }], result: result(), ...extra });

test("older guides have no invented daily or reputation activity figures", () => {
  assert.equal(reputationFacts([{ kind: "turnin" }]), null);
});

test("programme facts keep active work separate from unknown calendar waiting", () => {
  assert.deepEqual(reputationFacts([{ reputation_program: program() }]), {
    active_play_seconds: 120, daily_opportunities: 3, reset_transitions: 2, calendar_wait_seconds: null,
    estimates: true, warnings: ["Known source cap"],
  });
});

test("only used estimate sources warn and duplicate warnings appear once", () => {
  const p = program({ phases: [{ actions: [{ source_id: "confirmed" }] }], result: result({ daily_opportunities: 0, reset_transitions: 0, calendar_wait_seconds: 0 }) });
  const facts = reputationFacts([{ reputation_program: p }, { reputation_program: p }]);
  assert.equal(facts.estimates, false);
  assert.equal(facts.active_play_seconds, 240);
  assert.equal(facts.calendar_wait_seconds, 0);
  assert.deepEqual(facts.warnings, ["Known source cap"]);
});

test("identified prefix kill forecasts retain the visible Classic estimate notice", () => {
  const p = program({ phases: [], result: result({ warnings: ["Classic-derived reputation rules are forecasts; gray-level scaling, per-kill rounding and live server behavior may differ."] }) });
  assert.equal(reputationFacts([{ reputation_program: p }]).estimates, true);
});

test("editing cannot delete or move the reputation suffix into mandatory prefix work", () => {
  const a = { key: 1, step: { kind: "accept", quest: 1 } };
  const b = { key: 2, step: { kind: "turn_in", quest: 1 } };
  const rep = { key: 3, step: { kind: "reputation_program", program: program() } };
  const added = { key: 4, step: { kind: "train", trainer: 1 } };
  assert.deepEqual(protectReputationPrograms([a, b, rep], [rep, b, a]), [b, a, rep]);
  assert.deepEqual(protectReputationPrograms([a, b, rep], [a, b]), [a, b, rep]);
  assert.deepEqual(protectReputationPrograms([a, b, rep], [a, b, rep, added]), [a, b, added, rep]);
  assert.equal(protectReputationPrograms([a, b, rep], [a, b])[2], rep, "preserves the exact payload and stable editor key");
});

test("ordinary guides retain normal editing without new protected work", () => {
  const a = { step: { kind: "accept", quest: 1 } };
  assert.deepEqual(protectReputationPrograms([a], []), []);
});
