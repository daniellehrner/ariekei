// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { test } from "node:test";
import { hunkTail } from "./hunk.ts";

test("the end of a hunk keeps old and new line numbers", () => {
  const hunk = "@@ -9,3 +9,3 @@\n line9\n-line10\n+line10 changed\n line11";
  assert.deepEqual(hunkTail(hunk, 3), [
    { kind: "del", number: 10, text: "line10" },
    { kind: "add", number: 10, text: "line10 changed" },
    { kind: "ctx", number: 11, text: "line11" },
  ]);
  assert.deepEqual(hunkTail("", 4), []);
});
