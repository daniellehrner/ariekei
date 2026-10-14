import assert from "node:assert/strict";
import { test } from "node:test";
import { attentionKind, landingTab } from "./attention.ts";

test("attention reasons map to pip kinds", () => {
  assert.equal(attentionKind("agent needs you"), "needs-you");
  assert.equal(attentionKind("finished, cleanup blocked"), "blocked");
  assert.equal(attentionKind(null), "none");
  assert.equal(attentionKind("something new"), "none");
});

test("an item opens on the tab that resolves its attention reason", () => {
  const all = { terminal: true, review: true };
  assert.equal(landingTab("agent needs you", all), "terminal");
  assert.equal(landingTab("agent needs you", { terminal: false, review: true }), null);
  assert.equal(landingTab("new reviews or comments", all), "comments");
  assert.equal(landingTab("changes to review", all), "review");
  assert.equal(landingTab("CI failing", all), "overview");
  assert.equal(landingTab("agent working", all), null);
  assert.equal(landingTab(null, all), null);
});
