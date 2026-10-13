// HTML rendered by GitHub (descriptions, comments) shown inside the app.

import DOMPurify from "dompurify";
import { openUrl } from "@tauri-apps/plugin-opener";

// GitHub already sanitizes bodyHTML; this is defense in depth, since the app has a
// privileged bridge to the backend: no scripts, forms, frames or inline styles.
export function sanitize(html: string): string {
  return DOMPurify.sanitize(html, {
    USE_PROFILES: { html: true },
    FORBID_TAGS: ["style", "form", "input", "button", "textarea", "select", "iframe"],
    FORBID_ATTR: ["style"],
  });
}

// Svelte action: links open in the system browser instead of navigating the app window.
export function externalLinks(node: HTMLElement) {
  const onClick = (e: MouseEvent) => {
    const a = (e.target as Element | null)?.closest("a");
    if (!a) return;
    e.preventDefault();
    if (/^https?:\/\//.test(a.href)) openUrl(a.href);
  };
  node.addEventListener("click", onClick);
  return { destroy: () => node.removeEventListener("click", onClick) };
}
