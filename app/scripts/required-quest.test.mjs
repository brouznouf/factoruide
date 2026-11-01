import assert from "node:assert/strict";
import { test } from "node:test";
import { requiredQuest, requiredQuestLabel } from "../src/requiredQuest.ts";

test("quest entries preserve IDs and exact names in the plan request", () => {
  assert.deepEqual([" 79976 ", " This Must Be The Place "].map(q => requiredQuest(q)), [79976, "This Must Be The Place"]);
});

test("quest completion windows are saved with their ID or name and displayed", () => {
  const choices = [requiredQuest(" 79976 ", "20", "22"), requiredQuest("This Must Be The Place", "", "25"), requiredQuest("783", "2", "")];
  assert.deepEqual(JSON.parse(JSON.stringify(choices)), [{ quest:79976, min_level:20, max_level:22 }, { quest:"This Must Be The Place", max_level:25 }, { quest:783, min_level:2 }]);
  assert.deepEqual(choices.map(requiredQuestLabel), ["79976 (20–22)", "This Must Be The Place (≤25)", "783 (2+)"]);
});
