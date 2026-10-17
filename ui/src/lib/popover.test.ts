// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { test } from "node:test";
import { clampTo } from "./popover.ts";

test("a popover opens at the point, inside the window", () => {
  assert.deepEqual(clampTo(100, 100, 200, 80, 1000, 800), { left: 100, top: 100 });
  // Too close to the right edge: moved left; to the bottom: above the point.
  assert.deepEqual(clampTo(900, 760, 200, 80, 1000, 800), { left: 792, top: 680 });
  // Larger than the room above: kept at the top.
  assert.deepEqual(clampTo(0, 50, 200, 900, 1000, 800), { left: 8, top: 8 });
});
