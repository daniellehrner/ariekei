// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { test } from "node:test";
import { diff } from "@codemirror/merge";
import { docText, gitChanges, gitOverride, lineSeparatorOf } from "./gitDiff.ts";

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

test("a CRLF file is split into git's lines and keeps its line endings", () => {
  assert.equal(lineSeparatorOf("a\r\nb\r\n"), "\r\n");
  assert.equal(lineSeparatorOf("a\r\nb"), "\r\n");
  // LF, mixed, and lone CRs (which git does not count as line breaks): split at "\n" only.
  for (const text of ["a\nb\n", "a\r\nb\n", "\na\r\n", "a\rb\n", ""]) assert.equal(lineSeparatorOf(text), "\n", JSON.stringify(text));
  assert.equal(docText("a\r\nb\rc\r\n"), "a\nb\rc\n");
  const text = "one\r\ntwo\r\n";
  assert.equal(docText(text).split("\n").join(lineSeparatorOf(text)), text, "joined again, it is the file");
});

test("git's hunks are used for CRLF files too", () => {
  // With whitespace ignored git reports only line 2; CodeMirror's own diff also marks line 1.
  const o = "a  \r\nb\r\n";
  const n = "a\r\nc\r\n";
  const hunks = [{ oldStart: 2, oldLines: 1, newStart: 2, newLines: 1 }];
  const [a, b] = [docText(o), docText(n)];
  assert.ok(diff(a, b).length > 1);
  assert.deepEqual(gitOverride(o, n, hunks)(a, b).map((c) => [a.slice(c.fromA, c.toA), b.slice(c.fromB, c.toB)]), [["b", "c"]]);
});
