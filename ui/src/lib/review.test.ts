// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { test } from "node:test";
import type { ReviewState } from "./api.ts";
import { flatten, neighbourFile, nextChange, rangeOf, scopeOf, sections, whatMoved, type ReviewFile } from "./review.ts";

const f = (path: string, group: ReviewFile["group"]): ReviewFile => ({ path, status: "M", group, whitespaceOnly: false });
const files = [f("src/a.rs", "source"), f("src/b.rs", "source"), f("tests/t.rs", "tests"), f("Cargo.lock", "generated")];

test("review order keeps the backend's groups and folds generated files", () => {
  const s = sections(files, "review");
  assert.deepEqual(s.map((x) => [x.title, x.folded, x.files.length]), [["Source", false, 2], ["Tests", false, 1], ["Generated, lock & vendored", true, 1]]);
});

test("tree order is alphabetical", () => {
  assert.deepEqual(flatten(sections(files, "tree")).map((x) => x.path), ["Cargo.lock", "src/a.rs", "src/b.rs", "tests/t.rs"]);
});

test("n and p move from change to change, and say when the file has no more", () => {
  // Changes at 10–20, 30–30 (a deletion on this side) and 50–60.
  const changes = [{ from: 10, to: 20 }, { from: 30, to: 30 }, { from: 50, to: 60 }];
  assert.equal(nextChange(changes, 0, 1), 0);
  assert.equal(nextChange(changes, 10, 1), 1, "from inside a change, past it");
  assert.equal(nextChange(changes, 30, 1), 2);
  assert.equal(nextChange(changes, 50, 1), null, "the last change");
  assert.equal(nextChange(changes, 55, -1), 1);
  assert.equal(nextChange(changes, 30, -1), 0);
  assert.equal(nextChange(changes, 10, -1), null, "the first change");
  assert.equal(nextChange([], 0, 1), null);
});

test("the next file for n and p does not wrap around", () => {
  const paths = ["a", "b", "c"];
  assert.equal(neighbourFile(paths, "a", 1), "b");
  assert.equal(neighbourFile(paths, "c", 1), null);
  assert.equal(neighbourFile(paths, "a", -1), null);
  assert.equal(neighbourFile(paths, "c", -1), "b");
  assert.equal(neighbourFile(paths, null, -1), "c");
});

const state = (over: Partial<ReviewState> = {}): ReviewState => ({
  mode: "ref", dir: "/w", base: "B", baseRef: "main",
  head: "H", prHead: "H", commits: [{ sha: "C1", subject: "", author: "", date: "" }],
  dirty: false, own: false, ...over,
});

test("the scope shown falls back to what exists", () => {
  assert.equal(scopeOf(null, "review"), "all");
  assert.equal(scopeOf(state(), ""), "all");
  assert.equal(scopeOf(state(), "review"), "all", "the old \"since your review\" scope");
  assert.equal(scopeOf(state(), "C1"), "C1");
  assert.equal(scopeOf(state(), "GONE"), "all", "a commit no longer on the branch");
});

test("a scope's diff range", () => {
  assert.deepEqual(rangeOf(state(), "all"), ["B", "H"]);
  assert.deepEqual(rangeOf(state({ mode: "worktree" }), "all"), ["B", null], "worktree mode diffs the working tree");
  assert.deepEqual(rangeOf(state(), "C1"), ["C1^", "C1"]);
});

test("what a fetch from GitHub changed in the review shown", () => {
  assert.equal(whatMoved(state(), state()), null);
  assert.equal(whatMoved(state(), state({ head: "H2" })), "head", "the PR has new commits");
  assert.equal(whatMoved(state(), state({ head: "H2", base: "B2" })), "head");
  assert.equal(whatMoved(state(), state({ base: "B2" })), "base");
  assert.equal(whatMoved(state(), state({ baseRef: "next" })), "base", "retargeted");
  const wt = state({ mode: "worktree" });
  assert.equal(whatMoved(wt, state({ mode: "worktree", head: "H2" })), null, "the agent's own commit");
  assert.equal(whatMoved(wt, state({ mode: "worktree", base: "B2" })), "base");
});
