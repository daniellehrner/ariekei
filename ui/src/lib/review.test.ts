import assert from "node:assert/strict";
import { test } from "node:test";
import type { ReviewState } from "./api.ts";
import { flatten, nextUnreviewed, progress, rangeOf, scopeOf, sections, type ReviewFile } from "./review.ts";

const f = (path: string, group: ReviewFile["group"], reviewed = false): ReviewFile => ({ path, status: "M", group, reviewed, whitespaceOnly: false });
const files = [f("src/a.rs", "source", true), f("src/b.rs", "source"), f("tests/t.rs", "tests"), f("Cargo.lock", "generated")];

test("review order keeps the backend's groups and folds generated files", () => {
  const s = sections(files, "review");
  assert.deepEqual(s.map((x) => [x.title, x.folded, x.files.length]), [["Source", false, 2], ["Tests", false, 1], ["Generated, lock & vendored", true, 1]]);
});

test("tree order is alphabetical", () => {
  assert.deepEqual(flatten(sections(files, "tree")).map((x) => x.path), ["Cargo.lock", "src/a.rs", "src/b.rs", "tests/t.rs"]);
});

test("progress and the next unreviewed file", () => {
  assert.deepEqual(progress(files), { reviewed: 1, total: 4 });
  assert.equal(nextUnreviewed(files, "src/b.rs"), "tests/t.rs");
  assert.equal(nextUnreviewed(files, "Cargo.lock"), "src/b.rs", "wraps around, skips reviewed files");
  assert.equal(nextUnreviewed([f("x", "source", true)], "x"), null);
});

const state = (over: Partial<ReviewState> = {}): ReviewState => ({
  mode: "ref", dir: "/w", base: "B", baseRef: "main", reviewed: null, sinceReviewFrom: null, rebaseNote: null,
  head: "H", prHead: "H", commits: [{ sha: "C1", subject: "", author: "", date: "", afterReview: false }],
  dirty: false, own: false, ...over,
});

test("the scope shown falls back to what exists", () => {
  assert.equal(scopeOf(null, "review"), "all");
  assert.equal(scopeOf(state(), ""), "all");
  assert.equal(scopeOf(state(), "review"), "all", "no review yet");
  assert.equal(scopeOf(state({ reviewed: "R" }), ""), "review", "since the review by default");
  assert.equal(scopeOf(state({ reviewed: "R" }), "all"), "all");
  assert.equal(scopeOf(state(), "C1"), "C1");
  assert.equal(scopeOf(state(), "GONE"), "all", "a commit no longer on the branch");
});

test("a scope's diff range", () => {
  assert.deepEqual(rangeOf(state(), "all"), ["B", "H"]);
  assert.deepEqual(rangeOf(state({ mode: "worktree" }), "all"), ["B", null], "worktree mode diffs the working tree");
  assert.deepEqual(rangeOf(state({ reviewed: "R" }), "review"), ["R", "H"]);
  assert.deepEqual(rangeOf(state({ reviewed: "R", sinceReviewFrom: "R2" }), "review"), ["R2", "H"], "rebased review");
  assert.deepEqual(rangeOf(state(), "review"), ["B", "H"], "no review: everything");
  assert.deepEqual(rangeOf(state(), "C1"), ["C1^", "C1"]);
});
