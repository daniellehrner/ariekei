import assert from "node:assert/strict";
import { test } from "node:test";
import { isTyping } from "./keys.ts";

// An element stand-in: `closest` finds a selector if it is one of `inside`.
const el = (...inside: string[]) => ({ closest: (sel: string) => (sel.split(", ").some((s) => inside.includes(s)) ? {} : null) });

test("keys belong to fields, the editable diff and the terminal", () => {
  for (const where of ["input", "textarea", "select", "[contenteditable='true']", ".xterm"]) {
    assert.equal(isTyping(el(where) as unknown as EventTarget), true, where);
  }
  assert.equal(isTyping(el("button") as unknown as EventTarget), false);
  assert.equal(isTyping(null), false);
  assert.equal(isTyping({} as EventTarget), false);
});
