// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { test } from "node:test";
import type { Thread } from "./api.ts";
import { byLatest, isNew, latestAt, threadHasNew } from "./conversation.ts";

const thread = (id: string, ...times: string[]): Thread => ({
  id, path: "A.java", line: 1, resolved: false, outdated: false, diffHunk: "",
  comments: times.map((createdAt, i) => ({ id: i, author: "carol", bodyHtml: "", createdAt, url: "" })),
});

test("a reply to an old thread puts it before newer threads", () => {
  const old = thread("old", "2026-10-09T06:00:00Z", "2026-10-20T06:00:00Z");
  const newer = thread("newer", "2026-10-19T23:00:00Z");
  const quiet = thread("quiet", "2026-10-10T08:00:00Z");
  assert.equal(latestAt(old), "2026-10-20T06:00:00Z");
  assert.deepEqual(byLatest([quiet, old, newer]).map((t) => t.id), ["old", "newer", "quiet"]);
});

test("new: by someone else, after the user last saw the item", () => {
  const since = "2026-10-19T23:17:18Z";
  assert.ok(isNew({ author: "siladu", at: "2026-10-20T06:04:38Z" }, since, "me"));
  assert.ok(!isNew({ author: "me", at: "2026-10-20T06:04:38Z" }, since, "me"), "the user's own");
  assert.ok(!isNew({ author: "siladu", at: since }, since, "me"), "what they saw");
  assert.ok(!isNew({ author: "siladu", at: "2026-10-20T06:04:38Z" }, null, "me"), "never seen");
});

test("a thread without comments goes last, and the threads given stay as they were", () => {
  const empty = thread("empty");
  const a = thread("a", "2026-10-10T08:00:00Z");
  const b = thread("b", "2026-10-10T08:00:00Z");
  const given = [empty, a, b];
  assert.deepEqual(byLatest(given).map((t) => t.id), ["a", "b", "empty"], "ties keep their order");
  assert.deepEqual(given.map((t) => t.id), ["empty", "a", "b"]);
});

test("seen with no activity yet: everything by others is new", () => {
  assert.ok(isNew({ author: "carol", at: "2026-10-10T08:00:00Z" }, "0", "me"));
  assert.ok(!isNew({ author: "me", at: "2026-10-10T08:00:00Z" }, "0", "me"));
});

test("a thread is new for any new comment in it, the first or a reply", () => {
  const since = "2026-10-15T00:00:00Z";
  const reply = { ...thread("t", "2026-10-09T06:00:00Z", "2026-10-20T06:00:00Z"), resolved: true };
  assert.ok(threadHasNew(reply, since, "me"), "a reply in a resolved thread");
  assert.ok(threadHasNew(thread("first", "2026-10-20T06:00:00Z"), since, "me"));
  assert.ok(!threadHasNew(thread("old", "2026-10-09T06:00:00Z"), since, "me"));
  assert.ok(!threadHasNew(reply, since, "carol"), "the user's own");
  assert.ok(!threadHasNew(reply, null, "me"));
});
