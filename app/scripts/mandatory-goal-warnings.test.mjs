import assert from "node:assert/strict";
import { test } from "node:test";
import { mandatoryGoalWarnings, mandatoryGoalWarningText } from "../src/mandatoryGoalWarnings.ts";
import { CATALOG } from "../src/catalog.ts";

test("old guides keep the historical aggregate under an unknown-category warning", () => {
  assert.deepEqual(mandatoryGoalWarnings({ missing_class_quests: 25 }), [{ category: "legacy", count: 25 }]);
  assert.deepEqual(mandatoryGoalWarnings({ missing_class_quests: 25, mandatory_goal_counts: null }), [{ category: "legacy", count: 25 }]);
});

test("authoritative gear-only counts do not become class warnings", () => {
  assert.deepEqual(mandatoryGoalWarnings({
    missing_class_quests: 25,
    mandatory_goal_counts: { class_quests: 0, bis_quests: 25, other_quests: 0 },
  }), [{ category: "gear", count: 25 }]);
});

test("true class counts are authoritative even without a historical aggregate", () => {
  assert.deepEqual(mandatoryGoalWarnings({ mandatory_goal_counts: { class_quests: 3, bis_quests: 0, other_quests: 0 } }), [
    { category: "class", count: 3 },
  ]);
});

test("mixed categories retain overlapping counts separately and in reading order", () => {
  assert.deepEqual(mandatoryGoalWarnings({
    missing_class_quests: 4,
    mandatory_goal_counts: { class_quests: 2, bis_quests: 3, other_quests: 1 },
  }), [{ category: "class", count: 2 }, { category: "gear", count: 3 }, { category: "other", count: 1 }]);
});

test("authoritative zero counts suppress a nonzero historical aggregate", () => {
  assert.deepEqual(mandatoryGoalWarnings({
    missing_class_quests: 25,
    mandatory_goal_counts: { class_quests: 0, bis_quests: 0, other_quests: 0 },
  }), []);
});

test("zero, absent and nonfinite counts never produce warnings", () => {
  for (const missing_class_quests of [undefined, 0, NaN, Infinity]) {
    assert.deepEqual(mandatoryGoalWarnings({ missing_class_quests }), []);
  }
  assert.deepEqual(mandatoryGoalWarnings({}), []);
  assert.deepEqual(mandatoryGoalWarnings({ mandatory_goal_counts: { class_quests: undefined, bis_quests: NaN, other_quests: 0 } }), []);
});

test("visible warning text identifies its category and count in every existing language", () => {
  const expected = {
    class: "Mandatory class quests missed: 25",
    gear: "Mandatory gear quests missed (including prerequisites): 25",
    other: "Other mandatory quests missed: 25",
    legacy: "Mandatory quests missed: 25 (older guide; categories were not saved)",
  };
  for (const [category, english] of Object.entries(expected)) {
    for (const lang of ["fr", "en", "de", "es", "esMX", "pt", "ru", "ko", "zhCN", "zhTW"]) {
      const translate = (key, vars) => {
        const text = lang === "fr" ? key : CATALOG[key]?.[lang];
        assert.equal(typeof text, "string", `${category} translation missing for ${lang}`);
        return text.replace(/\{n\}/g, String(vars.n));
      };
      const text = mandatoryGoalWarningText({ category, count: 25 }, translate);
      assert.match(text, /25/);
      assert.doesNotMatch(text, /undefined|NaN|\{n\}/);
      if (lang === "en") assert.equal(text, english);
    }
  }
});
