import assert from "node:assert/strict";
import { test } from "node:test";
import { externalLinks } from "./html.ts";

// A container stand-in that hands out its click listener, and a click on a link (or not).
function container() {
  let listener: ((e: MouseEvent) => void) | null = null;
  const node = { addEventListener: (_: string, f: (e: MouseEvent) => void) => (listener = f), removeEventListener: () => (listener = null) };
  return { node: node as unknown as HTMLElement, click: (href: string | null) => {
    let prevented = false;
    const target = { closest: () => (href === null ? null : { href }) };
    listener?.({ target, preventDefault: () => (prevented = true) } as unknown as MouseEvent);
    return prevented;
  } };
}

test("links in GitHub's HTML open in the browser, never in the app window", () => {
  const opened: string[] = [];
  const { node, click } = container();
  const action = externalLinks(node, (url) => opened.push(url));
  assert.equal(click("https://example.com/spec"), true);
  assert.equal(click("javascript:alert(1)"), true, "followed by nobody");
  assert.equal(click("file:///etc/passwd"), true);
  assert.equal(click(null), false, "a click outside a link is left alone");
  assert.deepEqual(opened, ["https://example.com/spec"]);
  action.destroy();
  assert.equal(click("https://example.com/other"), false, "gone after destroy");
});
