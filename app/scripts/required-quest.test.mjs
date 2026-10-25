import assert from "node:assert/strict";
import { test } from "node:test";
import { requiredQuest } from "../src/requiredQuest.ts";

test("quest entries preserve IDs and exact names in the plan request", () => {
  assert.deepEqual([" 79976 ", " This Must Be The Place "].map(requiredQuest), [79976, "This Must Be The Place"]);
});
