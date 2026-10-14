import assert from "node:assert/strict";
import { test } from "node:test";
import { flatten, nextUnreviewed, progress, sections, type Guide, type ReviewFile } from "./review.ts";

const f = (path: string, group: ReviewFile["group"], reviewed = false): ReviewFile => ({ path, status: "M", group, reviewed, whitespaceOnly: false });
const files = [f("src/a.rs", "source", true), f("src/b.rs", "source"), f("tests/t.rs", "tests"), f("Cargo.lock", "generated")];

test("review order keeps the backend's groups and folds generated files", () => {
  const s = sections(files, "review", null);
  assert.deepEqual(s.map((x) => [x.title, x.folded, x.files.length]), [["Source", false, 2], ["Tests", false, 1], ["Generated, lock & vendored", true, 1]]);
});

test("tree order is alphabetical", () => {
  assert.deepEqual(flatten(sections(files, "tree", null)).map((x) => x.path), ["Cargo.lock", "src/a.rs", "src/b.rs", "tests/t.rs"]);
});

test("guide order puts the riskiest group first and keeps unmentioned files", () => {
  const guide: Guide = {
    focus: "",
    groups: [
      { title: "Low", intent: "", risk: "low", files: ["src/a.rs"] },
      { title: "High", intent: "", risk: "high", files: ["tests/t.rs", "src/b.rs", "missing.rs"] },
    ],
    claims: [],
    unexplained: [],
  };
  const s = sections(files, "guide", guide);
  assert.deepEqual(s.map((x) => x.title), ["High", "Low", "Not in the guide"]);
  assert.deepEqual(s[0].files.map((x) => x.path), ["tests/t.rs", "src/b.rs"]);
});

test("progress and the next unreviewed file", () => {
  assert.deepEqual(progress(files), { reviewed: 1, total: 4 });
  assert.equal(nextUnreviewed(files, "src/b.rs"), "tests/t.rs");
  assert.equal(nextUnreviewed(files, "Cargo.lock"), "src/b.rs", "wraps around, skips reviewed files");
  assert.equal(nextUnreviewed([f("x", "source", true)], "x"), null);
});
