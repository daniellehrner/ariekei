// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { test } from "node:test";
import { askMessage, classify, feedbackMessage, MAX_ASK_LINES, reviewBody, type CodeRef, type DraftComment } from "./reviewComments.ts";

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

test("feedback for the agent quotes each comment's code and location", () => {
  const msg = feedbackMessage([
    draft({ fromLine: 3, toLine: 4, code: "a\nb", text: "Rename\nthis" }),
    draft({ path: "B.java", fromLine: 7, toLine: 7, code: "x\n".repeat(10), text: " ok " }),
  ]);
  assert.ok(msg.startsWith("Review feedback on your changes:\n\n1. A.java:3-4\n   > a\n   > b\n   Rename\n   this"), msg);
  assert.ok(msg.includes("\n\n2. B.java:7\n"), msg);
  assert.equal(msg.split("\n").filter((l) => l === "   > x").length, 6, "at most 6 quoted lines");
  assert.ok(msg.endsWith("\n   ok\n\nPlease address each point."), msg);
});

const ref = (over: Partial<CodeRef>): CodeRef => ({
  path: "src/Peer.java", fromLine: 12, toLine: 12, code: "reset();\n", side: "new", rev: "1a2b3c4d".padEnd(40, "0"), ...over,
});

test("the agent is told where the code is, the code and the prompt", () => {
  assert.equal(askMessage(ref({}), "  Why here?\n"),
    "About src/Peer.java line 12 (new version, commit 1a2b3c4d):\n\n```java\nreset();\n```\n\nWhy here?");
  const range = askMessage(ref({ fromLine: 3, toLine: 5, code: "a\nb\nc", rev: null }), "What does this do?\nAnd this?");
  assert.ok(range.startsWith("About src/Peer.java lines 3–5 (working tree, uncommitted):\n\n```java\na\nb\nc\n```\n\n"), range);
  assert.ok(range.endsWith("\n\nWhat does this do?\nAnd this?"), range);
});

test("the version is the side's: old, new, unsaved edits", () => {
  const where = (r: CodeRef) => askMessage(r, "?").split("\n")[0];
  assert.equal(where(ref({ side: "old", rev: "ba5e1234".padEnd(40, "9") })), "About src/Peer.java line 12 (old version, before the change, commit ba5e1234):");
  assert.equal(where(ref({ side: "old", rev: `${"c0ffee12".padEnd(40, "3")}^` })), "About src/Peer.java line 12 (old version, before the change, commit c0ffee12^):");
  assert.equal(where(ref({ side: "old", rev: null })), "About src/Peer.java line 12 (old version, before the change):");
  assert.equal(where(ref({ unsaved: true })), "About src/Peer.java line 12 (working tree, with unsaved edits):");
});

test("long code is cut, and a fence outlasts backticks in the code", () => {
  const code = Array.from({ length: MAX_ASK_LINES + 5 }, (_, i) => `l${i + 1}`).join("\n");
  const long = askMessage(ref({ fromLine: 1, toLine: MAX_ASK_LINES + 5, code }), "?");
  assert.ok(long.includes(`\nl${MAX_ASK_LINES}\n\`\`\`\n… 5 more lines, see the file\n\n?`), long);
  assert.ok(!long.includes(`l${MAX_ASK_LINES + 1}`), long);
  const md = askMessage(ref({ path: "README", code: "use ```rust blocks``` and `x`" }), "?");
  assert.ok(md.includes("\n\n````\nuse ```rust blocks``` and `x`\n````\n\n?"), md);
});
