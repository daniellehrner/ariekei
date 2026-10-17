// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

/**
 * Ariekei icon set — 16 px grid, 1.5 px stroke, round caps and joins, stroke="currentColor".
 * Each value is the SVG path data for <path d="…"> in a 0 0 16 16 viewBox.
 * Status dots, attention pips, CI marks and the voices' marks (Mark) are drawn by components, not here.
 */
export const icons = {
  "sidebar-collapse": "M4 2.5h8a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2zM6 2.5v11M10.5 6.5L9 8l1.5 1.5", // Collapse Inbox (Ctrl/Cmd+B) (replaces «)
  "sidebar-expand": "M4 2.5h8a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2zM6 2.5v11M9 6.5L10.5 8 9 9.5", // Expand Inbox from the strip (replaces »)
  "panel": "M4 2.5h8a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2zM6 2.5v11", // Review side-panel toggle
  "refresh": "M13.5 8a5.5 5.5 0 1 1-1.6-3.9M13.5 2.5v3h-3", // Refresh Inbox, Reload diff (replaces ↻)
  "chevron-down": "M4 6l4 4 4-4", // Repo menu, open sections (rotate −90° when folded) (replaces ▾)
  "chevron-up": "M4 10l4-4 4 4", // Previous file in the review bar
  "chevron-right": "M6 4l4 4-4 4", // Menu submenus, breadcrumbs
  "check": "M3.5 8.5l3 3 6-7", // CI passing, current repo, scope menu's active row
  "x": "M4 4l8 8M12 4l-8 8", // CI failing, close, dismiss
  "search": "M2.5 7a4.5 4.5 0 1 0 9 0a4.5 4.5 0 1 0 -9 0M10.5 10.5L14 14", // Inbox search field
  "pull-request": "M2.5 3.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0 -3 0M2.5 12.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0 -3 0M10.5 12.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0 -3 0M4 5v6M12 11V6.5a2 2 0 0 0-2-2H7.5M9 3L7.5 4.5 9 6", // Work item header, PR rows
  "issue": "M2.5 8a5.5 5.5 0 1 0 11 0a5.5 5.5 0 1 0 -11 0M8 7.5v1", // Issue header and rows
  "task": "M5.5 2.5h5v2h-5zM5.5 3.5H4a1.5 1.5 0 0 0-1.5 1.5v7.5A1.5 1.5 0 0 0 4 14h8a1.5 1.5 0 0 0 1.5-1.5V5A1.5 1.5 0 0 0 12 3.5h-1.5M5.5 9l1.75 1.75L10.5 7.5", // Task header and rows (work with no PR or issue yet)
  "commit": "M5.5 8a2.5 2.5 0 1 0 5 0a2.5 2.5 0 1 0 -5 0M1.5 8h4M10.5 8h4", // Scope rows (commits)
  "branch": "M2.5 3.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0 -3 0M2.5 12.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0 -3 0M10.5 4.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0 -3 0M4 5v6M12 6a4 4 0 0 1-4 4H5.5", // Review bar (scope), task meta line (branch)
  "folder": "M2 4.5A1.5 1.5 0 0 1 3.5 3h3l1.5 1.5h4.5A1.5 1.5 0 0 1 14 6v5.5a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5z", // Choose folder…, worktree path, recent repos
  "external": "M9 3h4v4M13 3L7.5 8.5M11 9.5V12a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1h2.5", // Open on GitHub, switch to other window (replaces ⧉)
  "window-new": "M3.5 3h9A1.5 1.5 0 0 1 14 4.5v7a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5v-7A1.5 1.5 0 0 1 3.5 3zM2 6h12M8 8v3M6.5 9.5h3", // Open repo in new window, New window
  "plus": "M8 3v10M3 8h10", // Connect another repository…
  "terminal": "M3.5 3h9A1.5 1.5 0 0 1 14 4.5v7a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5v-7A1.5 1.5 0 0 1 3.5 3zM5 7l2 1.5L5 10M8.5 10.5h2.5", // Answer in Terminal, Terminal tab
  "play": "M5 3.5l7 4.5-7 4.5z", // Start working (filled)
  "done": "M2.5 8a5.5 5.5 0 1 0 11 0a5.5 5.5 0 1 0 -11 0M5.5 8l2 2 3-3.5", // Done
  "star": "M8 2.25l1.75 3.6 3.95.55-2.87 2.77.7 3.93L8 11.23 4.47 13.1l.7-3.93L2.3 6.4l3.95-.55z", // Star / unstar an item (filled in --you when starred)
  "inbox-remove": "M2.5 9.5l1.5-5.5h8l1.5 5.5v2.5a1 1 0 0 1-1 1h-9a1 1 0 0 1-1-1zM2.5 9.5h3l1 1.5h3l1-1.5h3M6 6.5h4", // Remove an added item from the Inbox
  "keyboard": "M3 4h10a1.5 1.5 0 0 1 1.5 1.5v5A1.5 1.5 0 0 1 13 12H3a1.5 1.5 0 0 1-1.5-1.5v-5A1.5 1.5 0 0 1 3 4zM4 7h.01M6.5 7h.01M9 7h.01M11.5 7h.01M5 9.5h6", // Shortcut overlay (?)
  "whitespace": "M7 13V3h5M10 3v10M7 3a2.5 2.5 0 0 0 0 5", // Whitespace toggle (w)
  "unified": "M3.5 3h9A1.5 1.5 0 0 1 14 4.5v7a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5v-7A1.5 1.5 0 0 1 3.5 3zM5 6.5h6M5 9.5h6", // Unified toggle (u)
  "comment": "M3 3.5h10a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1H7l-3 2.5v-2.5H3a1 1 0 0 1-1-1v-6a1 1 0 0 1 1-1z", // Unresolved thread count (replaces 💬)
  "reply": "M6 4L2.5 7.5 6 11M2.5 7.5H10a3.5 3.5 0 0 1 3.5 3.5V12", // Reply in a thread
  "trash": "M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.5 8.5h6l.5-8.5", // Remove a draft comment
  "copy": "M6.5 5.5h5a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-5a1 1 0 0 1-1-1v-6a1 1 0 0 1 1-1zM3.5 10.5V4a1 1 0 0 1 1-1h5", // Copy (code menu)
  "end": "M4 12.5h8M8 3v6.5M5.5 7L8 9.5 10.5 7", // Change-end note (rotated 180° for the first change)
  "lock": "M5 7h6a1.5 1.5 0 0 1 1.5 1.5v3.5a1.5 1.5 0 0 1-1.5 1.5H5a1.5 1.5 0 0 1-1.5-1.5V8.5A1.5 1.5 0 0 1 5 7zM5.5 7V5a2.5 2.5 0 0 1 5 0v2", // Read-only diff (no worktree)
  "warning": "M8 2.5l6 11H2zM8 6.5v3M8 11.5h.01", // Rate limit, gh not logged in
  "error": "M2.5 8a5.5 5.5 0 1 0 11 0a5.5 5.5 0 1 0 -11 0M8 5v3.5M8 11h.01", // Errors (no GitHub remote)
  "info": "M2.5 8a5.5 5.5 0 1 0 11 0a5.5 5.5 0 1 0 -11 0M8 7.5V11M8 5h.01", // Notices
  "blocked": "M4.5 2.5h7a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2h-7a2 2 0 0 1-2-2v-7a2 2 0 0 1 2-2zM8 5v3.5M8 11h.01", // Cleanup blocked banner (your work at risk)
} as const;

export type IconName = keyof typeof icons;
