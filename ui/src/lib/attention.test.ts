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
  const all = { terminal: true };
  assert.equal(landingTab("agent needs you", all), "terminal");
  assert.equal(landingTab("agent needs you", { terminal: false }), null);
  assert.equal(landingTab("new reviews or comments", all), "comments");
  assert.equal(landingTab("CI failing", all), "overview");
  assert.equal(landingTab("agent working", all), null);
  assert.equal(landingTab(null, all), null);
});

test("every attention reason of the Inbox (inbox.rs) has a pip kind", async () => {
  const { readFile } = await import("node:fs/promises");
  const rust = await readFile(new URL("../../../src-tauri/src/inbox.rs", import.meta.url), "utf8");
  const body = rust.slice(rust.indexOf("fn attention("), rust.indexOf("pub fn order("));
  const reasons = [...body.matchAll(/Some\("([^"]+)"\)/g)].map((m) => m[1]);
  assert.ok(reasons.length >= 6, `found ${reasons.length} reasons`);
  for (const reason of reasons) assert.notEqual(attentionKind(reason), "none", reason);
});

test("a task whose PRs and issues are all closed opens on its Overview", () => {
  assert.equal(attentionKind("all its PRs and issues are closed"), "all-closed");
  assert.equal(landingTab("all its PRs and issues are closed", { terminal: true, review: true }), "overview");
});
