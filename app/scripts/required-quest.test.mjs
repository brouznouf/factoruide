import assert from "node:assert/strict";
import { test } from "node:test";
import { requiredQuest, questChoice, withBound } from "../src/requiredQuest.ts";

test("quest entries preserve IDs and exact names", () => {
  assert.deepEqual([" 79976 ", " This Must Be The Place "].map(requiredQuest), [79976, "This Must Be The Place"]);
});

test("editing and clearing completion bounds preserves the selected quest and the other bound", () => {
  for (const quest of [79976, "This Must Be The Place"]) {
    const min = withBound(quest, "min_level", "20");
    const both = withBound(min, "max_level", "22");
    assert.deepEqual(JSON.parse(JSON.stringify(both)), { quest, min_level:20, max_level:22 });
    assert.equal(questChoice(both), quest);
    const max = withBound(both, "min_level", "");
    assert.deepEqual(max, { quest, max_level:22 });
    assert.equal(withBound(max, "max_level", ""), quest);
    assert.equal(withBound(quest, "min_level", "3.5"), quest);
  }
});
