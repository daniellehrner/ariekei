import assert from "node:assert/strict";
import { test } from "node:test";
import type { Row } from "./api.ts";
import { bySection, sectionOf } from "./inboxSections.ts";

function row(fields: Partial<Row>): Row {
  return {
    id: 1, number: 1, kind: "pr", issueNumber: null, cleanupBlockers: null, title: "t", author: "me", role: "other",
    url: "", isDraft: false, open: true, updatedAt: "", ci: null, failingChecks: [], reviewDecision: null,
    unresolvedThreads: 0, newActivity: false, agent: null, worktree: null, unreviewed: false, attention: null,
    taskId: null, agentItem: null, notes: null, outcomesClosed: false, starred: false, removable: false,
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
  // Someone else approved it; the user's review is still asked for.
  assert.equal(sectionOf(row({ role: "requested", reviewDecision: "APPROVED" })), "requested");
  assert.equal(sectionOf(row({ role: "reviewed" })), "reviewed");
  assert.equal(sectionOf(row({ role: "other" })), "open");
  assert.equal(sectionOf(row({ kind: "issue", role: "other" })), "open");
});

test("a task's PRs and issues are listed under it, not in a section", () => {
  const task = row({ id: 1, kind: "task", number: -1 });
  const pr = row({ id: 2, role: "author", taskId: 1 });
  const starred = row({ id: 3, role: "author", taskId: 1, starred: true });
  const mine = row({ id: 4, role: "author" });
  const sections = bySection([task, pr, starred, mine]);
  assert.deepEqual(sections.tasks.map((r) => r.id), [1]);
  assert.deepEqual(sections.mine.map((r) => r.id), [4]);
  assert.deepEqual(sections.starred, []);
});

test("sections keep the Inbox's order", () => {
  const rows = [row({ id: 5, role: "reviewed" }), row({ id: 6 }), row({ id: 7, role: "reviewed" })];
  const sections = bySection(rows);
  assert.deepEqual(sections.reviewed.map((r) => r.id), [5, 7]);
  assert.deepEqual(sections.open.map((r) => r.id), [6]);
});
