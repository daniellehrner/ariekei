// Keyboard shortcuts of the review view.

export const SHORTCUTS: [string, string][] = [
  ["n / p", "next / previous change"],
  ["] / [", "next / previous file"],
  ["r", "mark the file reviewed and go to the next unreviewed one"],
  ["u", "unified / side-by-side diff"],
  ["w", "ignore whitespace"],
  ["?", "show these shortcuts"],
];

// Keys belong to whatever the user is typing in: fields, the editable diff, the terminal.
export function isTyping(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el || typeof el.closest !== "function") return false;
  return !!el.closest("input, textarea, select, [contenteditable='true'], .xterm");
}
