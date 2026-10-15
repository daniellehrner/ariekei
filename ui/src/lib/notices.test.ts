import assert from "node:assert/strict";
import { test } from "node:test";
import { MAX_NOTICES, fades, stillCurrent, withNotice, type Notice } from "./notices.ts";

const notice = (id: number, item: number | null, kind: Notice["kind"] = "finished"): Notice => ({ id, item, kind, title: "", body: "" });

test("a notice about the item on screen is not shown", () => {
  const none = () => undefined;
  assert.equal(stillCurrent(notice(1, 7), 7, none), false);
  assert.equal(stillCurrent(notice(1, 7), 8, none), true);
  assert.equal(stillCurrent(notice(1, null, "lifecycle"), 8, none), true);
});

test("a permission request is shown only while its agent still waits, and does not fade", () => {
  const ask = notice(1, 7, "needs-you");
  assert.equal(stillCurrent(ask, null, () => "waitingForPermission"), true);
  assert.equal(stillCurrent(ask, null, () => "working"), false);
  assert.equal(fades(ask), false);
  assert.equal(fades(notice(2, 7, "ci-failing")), true);
});

test("a newer notice about the same item replaces the older one, and only the newest stay", () => {
  let list: Notice[] = [];
  list = withNotice(list, notice(1, 7));
  list = withNotice(list, notice(2, 8));
  list = withNotice(list, notice(3, 7, "error"));
  assert.deepEqual(list.map((n) => [n.id, n.item]), [[2, 8], [3, 7]]);
  // Notices about no item never replace each other.
  list = withNotice(withNotice(list, notice(4, null)), notice(5, null));
  assert.deepEqual(list.map((n) => n.id), [2, 3, 4, 5]);
  list = withNotice(list, notice(6, 9));
  assert.equal(list.length, MAX_NOTICES);
  assert.deepEqual(list.map((n) => n.id), [3, 4, 5, 6]);
});
