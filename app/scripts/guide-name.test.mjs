import assert from "node:assert/strict";
import { test } from "node:test";
import { autoGuideName } from "../src/guideName.ts";

test("the automatic guide name shows the character without its realm and the guide's levels", () => {
  assert.equal(autoGuideName("Besmer-Realm", 25, 40), "Besmer 25-40");
  assert.equal(autoGuideName("Besmer", 25, 60), "Besmer 25-60");
});

test("the automatic guide name never ends at or below its start", () => {
  assert.equal(autoGuideName("Besmer-Realm", 40, 40), "Besmer 40-41");
  assert.equal(autoGuideName("Besmer-Realm", 40, 12), "Besmer 40-41");
});
