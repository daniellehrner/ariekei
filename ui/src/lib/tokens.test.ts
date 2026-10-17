// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { test } from "node:test";
import { parseDuration } from "./tokens.ts";

test("durations are read in ms, however the webview writes them", () => {
  assert.equal(parseDuration("1800ms"), 1800);
  assert.equal(parseDuration(" 1.8s"), 1800);
  assert.equal(parseDuration("0ms"), 0);
  assert.equal(parseDuration("0s"), 0);
  assert.equal(parseDuration(""), null);
  assert.equal(parseDuration("fast"), null);
});
