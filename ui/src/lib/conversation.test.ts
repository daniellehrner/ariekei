// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { test } from "node:test";
import type { Thread } from "./api.ts";
import { byLatest, isNew, latestAt } from "./conversation.ts";

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
