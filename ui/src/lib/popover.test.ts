// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { test } from "node:test";
import { clampTo, nearLines } from "./popover.ts";

test("a popover opens at the point, inside the window", () => {
  assert.deepEqual(clampTo(100, 100, 200, 80, 1000, 800), { left: 100, top: 100 });
  // Too close to the right edge: moved left; to the bottom: above the point.
  assert.deepEqual(clampTo(900, 760, 200, 80, 1000, 800), { left: 792, top: 680 });
  // Larger than the room above: kept at the top.
  assert.deepEqual(clampTo(0, 50, 200, 900, 1000, 800), { left: 8, top: 8 });
});

test("the Ask agent box goes below the lines, else above them, else docked over the dock", () => {
  const area = { left: 500, right: 1000, top: 100, bottom: 800 };
  // Room below.
  assert.deepEqual(nearLines(400, 150, 600, { top: 300, bottom: 340 }, area), { left: 592, top: 344, docked: false });
  // Not below: above the first line.
  assert.deepEqual(nearLines(400, 150, 520, { top: 600, bottom: 700 }, area), { left: 520, top: 446, docked: false });
  // Neither: docked at the bottom of the pane.
  assert.deepEqual(nearLines(400, 150, 600, { top: 200, bottom: 700 }, area), { left: 508, top: 642, docked: true });
});
