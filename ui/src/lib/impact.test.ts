// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

import assert from "node:assert/strict";
import { test } from "node:test";
import { linksIn, pickedLocation, sectionsOf, splitPath, staleText, type ImpactEntry, type ImpactMap } from "./impact.ts";

const place = (path: string, line: number, inDiff = false) => ({ path, fromLine: line, toLine: line, code: "x", note: "", inDiff });
const entry = (id: number, kind: ImpactEntry["kind"], more: Partial<ImpactEntry> = {}): ImpactEntry => ({
  id, kind, title: `#${id}`, body: "", attention: false, changed: null, locations: [place("a.rs", id)], rev: "r1", ...more,
});
const map = (entries: ImpactEntry[]): ImpactMap => ({ rev: "r1", base: "b", overview: "", entries });

test("sections come in their order, empty ones left out, entries worth attention first", () => {
  const s = sectionsOf([entry(1, "path"), entry(2, "affected"), entry(3, "affected", { attention: true })]);
  assert.deepEqual(s.map((s) => [s.kind, s.entries.map((e) => e.id)]), [["affected", [3, 2]], ["path", [1]]]);
});

test("a picked place is a location, or the changed lines an entry follows from", () => {
  const e = entry(1, "affected", { changed: place("c.rs", 9, true) });
  assert.equal(pickedLocation([e], { entry: 1, place: 0 })?.path, "a.rs");
  assert.equal(pickedLocation([e], { entry: 1, place: -1 })?.path, "c.rs");
  assert.equal(pickedLocation([e], { entry: 1, place: 5 }), null, "gone since");
  assert.equal(pickedLocation([e], { entry: 2, place: 0 }), null);
  assert.equal(pickedLocation([e], null), null);
});

test("the map says how far behind the head it is", () => {
  const m = map([]);
  assert.equal(staleText({ map: m, head: "r1", newer: 0, rewritten: false }), null);
  assert.equal(staleText({ map: m, head: "r2", newer: 1, rewritten: false }), "1 newer commit");
  assert.equal(staleText({ map: m, head: "r2", newer: 3, rewritten: false }), "3 newer commits");
  assert.equal(staleText({ map: m, head: "r2", newer: 0, rewritten: true }), "the branch was rewritten since");
  assert.equal(staleText({ map: null, head: "r2", newer: 0, rewritten: false }), null);
});

test("the diff links to entries at the changed lines they follow from, in the map's version only", () => {
  const m = map([
    entry(1, "affected", { changed: place("c.rs", 9, true) }),
    entry(2, "path", { changed: place("c.rs", 9, true) }),
    entry(3, "similar", { changed: place("c.rs", 4, true) }),
    entry(4, "similar", { changed: place("c.rs", 2, false) }),
    entry(5, "similar", { changed: place("d.rs", 2, true) }),
    entry(6, "similar", { changed: place("c.rs", 1, true), rev: "r0" }),
  ]);
  assert.deepEqual(linksIn(m, "c.rs", "r1"), [{ line: 4, entries: [3] }, { line: 9, entries: [1, 2] }]);
  assert.deepEqual(linksIn(m, "c.rs", "r2"), [], "another version of the file");
  assert.deepEqual(linksIn(m, "c.rs", null), [], "the working tree");
});

test("paths are split for display, long directories shortened", () => {
  assert.deepEqual(splitPath("a.rs"), { dir: "", name: "a.rs" });
  assert.deepEqual(splitPath("src/a.rs"), { dir: "src/", name: "a.rs" });
  assert.deepEqual(splitPath("a/b/c/d/e/F.java"), { dir: "…/c/d/e/", name: "F.java" });
});
