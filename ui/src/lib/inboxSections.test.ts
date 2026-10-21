// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { test } from "node:test";
import type { Row } from "./api.ts";
import { bySection, readyToMerge, sectionOf } from "./inboxSections.ts";

function row(fields: Partial<Row>): Row {
  return {
    id: 1, number: 1, kind: "pr", issueNumber: null, cleanupBlockers: null, title: "t", author: "me", role: "other",
    url: "", isDraft: false, open: true, updatedAt: "", ci: null, failingChecks: [], reviewDecision: null,
    unresolvedThreads: 0, newActivity: false, agent: null, worktree: null, unreviewed: false, attention: null,
    notes: null, starred: false, removable: false,
    ...fields,
  };
}

test("each row is in the first section that fits", () => {
  assert.equal(sectionOf(row({ role: "assignee", starred: true })), "starred");
  assert.equal(sectionOf(row({ kind: "task", role: "author", starred: true })), "starred");
  assert.equal(sectionOf(row({ kind: "task", role: "author" })), "tasks");
  assert.equal(sectionOf(row({ role: "assignee", reviewDecision: "APPROVED" })), "assigned");
  assert.equal(sectionOf(row({ kind: "issue", role: "assignee" })), "assigned");
  assert.equal(sectionOf(row({ role: "author", reviewDecision: "APPROVED" })), "approved");
  assert.equal(sectionOf(row({ role: "author", reviewDecision: "CHANGES_REQUESTED" })), "mine");
  // Merged, its cleanup blocked: nothing left to merge.
  assert.equal(sectionOf(row({ role: "author", reviewDecision: "APPROVED", open: false })), "mine");
  // Someone else approved it; the user's review is still asked for.
  assert.equal(sectionOf(row({ role: "requested", reviewDecision: "APPROVED" })), "requested");
  assert.equal(sectionOf(row({ role: "reviewed" })), "reviewed");
  assert.equal(sectionOf(row({ role: "other" })), "open");
  assert.equal(sectionOf(row({ kind: "issue", role: "other" })), "open");
});

test("sections keep the Inbox's order", () => {
  const rows = [row({ id: 5, role: "reviewed" }), row({ id: 6 }), row({ id: 7, role: "reviewed" })];
  const sections = bySection(rows);
  assert.deepEqual(sections.reviewed.map((r) => r.id), [5, 7]);
  assert.deepEqual(sections.open.map((r) => r.id), [6]);
});

test("an approved PR is ready to merge only when nothing else holds it up", () => {
  const approved = { role: "author", reviewDecision: "APPROVED" } as const;
  assert.ok(readyToMerge(row({ ...approved, ci: "SUCCESS" })));
  assert.ok(readyToMerge(row({ ...approved })), "no checks");
  assert.ok(!readyToMerge(row({ ...approved, isDraft: true })));
  assert.ok(!readyToMerge(row({ ...approved, ci: "FAILURE" })));
  assert.ok(!readyToMerge(row({ ...approved, ci: "PENDING" })));
  assert.ok(!readyToMerge(row({ ...approved, open: false })));
  assert.ok(!readyToMerge(row({ role: "author", reviewDecision: "CHANGES_REQUESTED" })));
});
