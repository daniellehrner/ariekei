// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { test } from "node:test";
import { newestLoad } from "./latest.ts";

test("a slow load that finishes after a newer one is not shown", () => {
  const loads = newestLoad();
  const [slow, newer] = [loads.start(), loads.start()];
  assert.ok(loads.succeeded(newer));
  assert.ok(!loads.succeeded(slow), "it would replace what is newer");
  assert.ok(!loads.failed(slow), "nor does its failure say the newer one is out of date");
});

test("a failure counts until a newer load succeeds", () => {
  const loads = newestLoad();
  const first = loads.start();
  assert.ok(loads.succeeded(first));
  const [failing, next] = [loads.start(), loads.start()];
  assert.ok(loads.failed(failing), "what is shown is out of date");
  assert.ok(loads.succeeded(next));
  const late = loads.start();
  assert.ok(loads.failed(late));
  assert.ok(!loads.failed(next), "an older failure after a newer success");
});

test("an older load that succeeds after a newer one failed is still shown", () => {
  const loads = newestLoad();
  const [older, newer] = [loads.start(), loads.start()];
  assert.ok(loads.failed(newer));
  assert.ok(loads.succeeded(older), "better than what was there before");
});
