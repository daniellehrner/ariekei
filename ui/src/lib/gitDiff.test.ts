import assert from "node:assert/strict";
import { test } from "node:test";
import { gitChanges } from "./gitDiff.ts";

const spans = (o: string, n: string, hunks: Parameters<typeof gitChanges>[2]) =>
  gitChanges(o, n, hunks).map((c) => [o.slice(c.fromA, c.toA), n.slice(c.fromB, c.toB)]);

test("modified line: character-level change inside the hunk", () => {
  assert.deepEqual(spans("a\nreturn x;\nc\n", "a\nreturn y;\nc\n", [{ oldStart: 2, oldLines: 1, newStart: 2, newLines: 1 }]), [["x", "y"]]);
});

test("pure insertion after a line", () => {
  assert.deepEqual(spans("a\nb\n", "a\nnew\nb\n", [{ oldStart: 1, oldLines: 0, newStart: 2, newLines: 1 }]), [["", "new\n"]]);
});

test("deletion and a new file", () => {
  assert.deepEqual(spans("a\ngone\nb\n", "a\nb\n", [{ oldStart: 2, oldLines: 1, newStart: 1, newLines: 0 }]), [["gone\n", ""]]);
  assert.deepEqual(spans("", "x\ny\n", [{ oldStart: 0, oldLines: 0, newStart: 1, newLines: 2 }]), [["", "x\ny\n"]]);
});

test("identical text elsewhere is not marked when git says it is unchanged", () => {
  // A naive diff could align the second "dup" differently; git's hunk pins the change.
  const o = "dup\nx\ndup\n";
  const n = "dup\nx\ndup\ndup\n";
  const changes = gitChanges(o, n, [{ oldStart: 3, oldLines: 0, newStart: 4, newLines: 1 }]);
  assert.deepEqual(changes.map((c) => [c.fromB, c.toB]), [[10, 14]]);
});
