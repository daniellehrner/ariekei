// Keyboard shortcuts of the review view.

export const SHORTCUTS: [string[], string][] = [
  [["n", "p"], "Next / previous change"],
  [["]", "["], "Next / previous file"],
  [["r"], "Reviewed, next unreviewed"],
  [["u"], "Unified / side-by-side"],
  [["w"], "Hide / show whitespace"],
  [["?"], "Shortcuts"],
  [["Esc"], "Close this, or the comment"],
];

// Keys belong to whatever the user is typing in: fields, the editable diff, the terminal.
export function isTyping(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el || typeof el.closest !== "function") return false;
  return !!el.closest("input, textarea, select, [contenteditable='true'], .xterm");
}
