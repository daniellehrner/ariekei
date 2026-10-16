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
  `Segmented`, `Pill`, `Banner`, `InlineConfirm`, `Kbd`, `StarToggle`), the icon set
  (`icons.ts`, 16 px grid, 1.5 px stroke; `Icon`), the agent `StatusDot`, the Inbox
  `AttentionPip` and the `Logo`.
- The app icon (`src-tauri/icons/`) is the same mark: a circle (the agent) and a square (you)
  overlapping where the work is decided.

## The Inbox

- **Sections**, in this order: Starred, Tasks, Assigned, Approved (the user's PRs ready to
  merge), Review requested, My PRs, Reviewed, Open. An item is listed once, in the first
  section that fits (`lib/inboxSections.ts`, from the backend's `role`); a task's PRs and issues
  stay under the task. Empty sections are left out, except Open, which carries **Browse…**.
- **The star** is the user's own mark: a set star is filled in `--you` and always shown; an
  unset one is a grey outline, shown on hover or focus (`StarToggle`), like **Remove** on items
  the user added. The item header has the same star after its title.
- **Browse** (Ctrl+K, the field at the top of the Inbox, Browse… in Open) is a raised overlay
  (`--surface-raised`, `--shadow-pop`, over `--scrim`), wider than the Inbox: Pull requests |
  Issues, a search field, and per hit **Add to Open**, or the `in Inbox` pill (a tinted fill:
  local state) with **Open** and, for items the user added, **Remove**. It stays open after
  adding.
- **Refresh** turns its arrow (like the busy `Spinner`) until the poll it asked for has
  answered, with new data or an error; with reduced motion it only dims.

## Notices

A notice tells the focused window about another item (`Notices.svelte`; unfocused, it is an OS
notification). It sits in the bottom right corner, so it has to be seen without being loud:

- Its kind's colour, the same as the item's attention pip: copper when the agent needs you, blue
  when it finished (your review is next), red for CI failing or an error, violet for new GitHub
  activity, the info tone for lifecycle news. The colour is a tinted fill and a border, nothing
  more (no glow, no accent bar).
- It fades in with a short slide from the edge (`--dur-slow`, `--ease`) and pulses once
  (`--dur-pulse`); a permission request pulses three times. Nothing keeps moving after that,
  and with reduced motion nothing moves.
- Clicking it opens the item: on its Terminal when the notice is from the agent (finished,
  waiting for permission, stopped with an error), else on the tab that resolves it. A permission request stays until the agent moves on; the rest go
  after ten seconds.

## The code menu

Right-clicking code in the review opens a menu (`CodeMenu.svelte`) instead of the browser's:
**Ask agent…** carries the agent's circle in copper, **Comment on lines** (new side only) and
**Copy** stay grey. Ask agent opens a prompt box (`AskAgent.svelte`) at the same point, headed
by the agent's circle and the lines it is about; its one filled button is **Send to agent**
(an action that leaves Ariekei for the agent). Both are raised surfaces (`--surface-raised`,
`--shadow-pop`) kept inside the window (`lib/popover.ts`).

## Behaviour that comes with the design

- An item opens on the tab that resolves its attention reason: the agent needs you → Terminal,
  new reviews or comments → Comments, changes to review → Review, CI failing or cleanup blocked
  → Overview. Otherwise on the tab last used for it.
- The Review tab shows the progress of the review scope (`7/12`).
- While the agent waits for permission, a strip under the header shows what it asks to run, with
  "Answer in Terminal".
- Below a 720 px diff column the diff is unified, unless chosen otherwise with `u`; below a
  1200 px window the Inbox collapses while the review side panel is open (Ctrl+B opens it again).
- `n` / `p` past the last (first) change of the scope show a raised note at that end of the
  diff for a moment ("That was the last change in this scope"); it fades, nothing else moves.
- Keyboard: Ctrl/Cmd+B toggles the Inbox, Ctrl/Cmd+Shift+N opens a new window; the review keys
  are listed under `?`. Every control is reachable with Tab and shows a focus ring.
