// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { test } from "node:test";
import { newestLoad, reloadsAndChecks } from "./latest.ts";

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

test("a check that overlaps a reload does not count", () => {
  const shown = reloadsAndChecks();
  const before = shown.check();
  const ended = shown.reload();
  const during = shown.check();
  assert.ok(!shown.counts(before), "it began before the reload: compared with the old state");
  ended();
  assert.ok(!shown.counts(during), "it began during the reload: compared with the old state");
  const after = shown.check();
  assert.ok(shown.counts(after));
});

test("a check during a reload does not count, even if it ends before the reload does", () => {
  const shown = reloadsAndChecks();
  const ended = shown.reload();
  assert.ok(!shown.counts(shown.check()));
  ended();
  ended();
  assert.ok(shown.counts(shown.check()), "ending a reload twice counts once");
});

test("checks between reloads count", () => {
  const shown = reloadsAndChecks();
  assert.ok(shown.counts(shown.check()));
  shown.reload()();
  const next = shown.check();
  assert.ok(shown.counts(next));
  shown.reload();
  assert.ok(!shown.counts(next), "a later reload began before it had its answer");
});
