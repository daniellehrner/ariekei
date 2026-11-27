// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

// HTML rendered by GitHub (descriptions, comments), and markdown the agent wrote, shown inside
// the app.

import DOMPurify from "dompurify";
import { openUrl } from "@tauri-apps/plugin-opener";
import { marked } from "marked";

// GitHub already sanitizes bodyHTML; this is defense in depth, since the app has a
// privileged bridge to the backend: no scripts, forms, frames or inline styles.
export function sanitize(html: string): string {
  return DOMPurify.sanitize(html, {
    USE_PROFILES: { html: true },
    FORBID_TAGS: ["style", "form", "input", "button", "textarea", "select", "iframe"],
    FORBID_ATTR: ["style"],
  });
}

// Markdown the agent wrote (its impact map), as sanitized HTML. Raw HTML in it is kept only as
// far as `sanitize` allows.
export function markdown(md: string): string {
  return sanitize(marked.parse(md, { async: false, gfm: true }));
}

// Svelte action: links open in the system browser instead of navigating the app window;
// links that are not http(s) (javascript:, file:, …) do nothing. `open`: for tests.
export function externalLinks(node: HTMLElement, open: (url: string) => unknown = openUrl) {
  const onClick = (e: MouseEvent) => {
    const a = (e.target as Element | null)?.closest("a");
    if (!a) return;
    e.preventDefault();
    if (/^https?:\/\//.test(a.href)) open(a.href);
  };
  node.addEventListener("click", onClick);
  return { destroy: () => node.removeEventListener("click", onClick) };
}
