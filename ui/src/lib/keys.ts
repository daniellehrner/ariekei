// Keyboard shortcuts: the app's (anywhere in the window) and the review view's.

export const APP_SHORTCUTS: [string[], string][] = [
  [["Ctrl+N"], "New task"],
  [["Ctrl+B"], "Show / hide the Inbox"],
  [["Ctrl+K"], "Browse open PRs and issues"],
  [["Ctrl+Shift+N"], "New window"],
  [["?"], "These shortcuts"],
  [["Esc"], "Close this"],
];

export const SHORTCUTS: [string[], string][] = [
  [["n", "p"], "Next / previous change, on into the next / previous file"],
  [["]", "["], "Next / previous file"],
  [["r"], "Reviewed, next unreviewed"],
  [["u"], "Unified / side-by-side"],
  [["w"], "Hide / show whitespace"],
  [["a"], "Ask the agent about the selection (or right-click code)"],
  [["Esc"], "Drop the selection (comment)"],
];

// Keys belong to whatever the user is typing in: fields, the editable diff, the terminal.
export function isTyping(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el || typeof el.closest !== "function") return false;
  return !!el.closest("input, textarea, select, [contenteditable='true'], .xterm");
}
