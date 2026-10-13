import assert from "node:assert/strict";
import { test } from "node:test";
import { classify, reviewBody, type DraftComment } from "./reviewComments.ts";

const draft = (over: Partial<DraftComment>): DraftComment => ({
  path: "A.java", fromLine: 10, toLine: 10, code: "x", text: "fix", rev: "head", ...over,
});

test("comments on the PR head inside a hunk become line comments", () => {
  const { line, body } = classify([draft({}), draft({ fromLine: 9, toLine: 11 })], "head", { "A.java": [[8, 12]] });
  assert.deepEqual(line, [
    { path: "A.java", line: 10, side: "RIGHT", body: "fix" },
    { path: "A.java", line: 11, start_line: 9, side: "RIGHT", body: "fix" },
  ]);
  assert.equal(body.length, 0);
});

test("other versions, lines outside the diff and unknown files go into the body", () => {
  const ranges = { "A.java": [[8, 12]] as [number, number][] };
  const { line, body } = classify(
    [draft({ rev: null }), draft({ rev: "older" }), draft({ fromLine: 20, toLine: 20 }), draft({ fromLine: 11, toLine: 14 }), draft({ path: "B.java" })],
    "head",
    ranges,
  );
  assert.equal(line.length, 0);
  assert.equal(body.length, 5);
});

test("the review body lists folded comments after the summary", () => {
  assert.equal(reviewBody("Looks good", [draft({ fromLine: 3, toLine: 4, text: "nit" })]), "Looks good\n\n- `A.java:3-4` — nit");
  assert.equal(reviewBody("  ", []), "");
});
