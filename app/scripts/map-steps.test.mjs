import assert from "node:assert/strict";
import { test } from "node:test";
import { aheadInZone, aheadPicked, arrowMarks, stepsShown } from "../src/components/mapSteps.ts";

test("the slider's top keeps every step of the next zone", () => {
  const ahead = aheadPicked(12, 12);
  assert.equal(stepsShown(ahead, 12), 12);
  assert.equal(stepsShown(aheadInZone(ahead, 30), 30), 30);
  assert.equal(stepsShown(aheadInZone(ahead, 4), 4), 4);
});

test("a smaller count stays from zone to zone, unless the next zone has fewer steps", () => {
  const ahead = aheadPicked(5, 12);
  assert.equal(stepsShown(aheadInZone(ahead, 30), 30), 5);
  const small = aheadInZone(ahead, 3);
  assert.equal(stepsShown(small, 3), 3);
  // Cut to that zone's count, it is its top: all the steps of the zone after.
  assert.equal(stepsShown(aheadInZone(small, 30), 30), 30);
});

test("arrows follow a line's way, spaced along its length", () => {
  const marks = arrowMarks([[[0, 0], [10, 0], [10, 10]]], 5);
  assert.deepEqual(
    marks.map((m) => [m.x, m.y, Math.round((m.angle * 180) / Math.PI)]),
    [
      [2.5, 0, 0],
      [7.5, 0, 0],
      [10, 2.5, 90],
      [10, 7.5, 90],
    ],
  );
  assert.equal(arrowMarks([[[0, 0], [1000, 0]]], 1, 10).length, 10);
});
