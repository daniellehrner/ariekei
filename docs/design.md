# Design

The UI follows a design made with Claude Design from [design-prompt.md](design-prompt.md). This
page holds the rules to keep when changing the UI.

## Who is speaking

Three sources of state are never confused. Each has its own shape, hue and place, so they can be
told apart in grey-scale and with colour blindness:

| Source | Shape | Hue | Where |
|---|---|---|---|
| Agent (Claude Code) | circle | `--agent` (copper) | Status dot before an item, Terminal tab, waiting strip, terminal cursor |
| You (the reviewer) | square | `--you` (blue, also the focus colour) | Review progress, reviewed checkboxes, "Reviewed, next", draft comments |
| GitHub | diamond, outlined pill | `--gh` (violet) | New activity, Comments badge, CI and review facts, submitting to GitHub |

- Red (`--bad`) means broken (CI failing, errors, work that would be lost). It replaces the hue,
  never the shape. Amber (`--warn`) is caution or pending, green (`--ok`) done or passing.
- GitHub facts are outlined pills; local state is a tinted fill.
- Everything else is grey: colour means someone wants something.
- Filled agent and GitHub buttons appear only where an action leaves Ariekei for that party
  (Send to the agent, Submit to GitHub).

## Where things are

- `ui/src/tokens.css`: every colour (light and dark), type, space, radius, size and motion
  token. Components never hard-code a colour; CodeMirror (`lib/codemirrorTheme.ts`) and xterm
  (`lib/xtermTheme.ts`) read the same tokens.
- `ui/src/controls/`: the shared controls (`Button`, `IconButton`, `Checkbox`, `Toggle`,
  `Segmented`, `Pill`, `Banner`, `InlineConfirm`, `Kbd`, `Risk`, `ClaimMark`), the icon set
  (`icons.ts`, 16 px grid, 1.5 px stroke; `Icon`), the agent `StatusDot`, the Inbox
  `AttentionPip` and the `Logo`.
- The app icon (`src-tauri/icons/`) is the same mark: a circle (the agent) and a square (you)
  overlapping where the work is decided.

## Behaviour that comes with the design

- An item opens on the tab that resolves its attention reason: the agent needs you → Terminal,
  new reviews or comments → Comments, changes to review → Review, CI failing or cleanup blocked
  → Overview. Otherwise on the tab last used for it.
- The Review tab shows the progress of the review scope (`7/12`).
- While the agent waits for permission, a strip under the header shows what it asks to run, with
  "Answer in Terminal".
- Below a 720 px diff column the diff is unified, unless chosen otherwise with `u`; below a
  1200 px window the Inbox collapses while the review side panel is open (Ctrl+B opens it again).
- Keyboard: Ctrl/Cmd+B toggles the Inbox, Ctrl/Cmd+Shift+N opens a new window; the review keys
  are listed under `?`. Every control is reachable with Tab and shows a focus ring.
