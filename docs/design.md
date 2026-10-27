# Design

The UI follows a design made with Claude Design from [design-prompt.md](design-prompt.md), and
its review of 2026-10-17. This page holds the rules to keep when changing the UI.

## Who is speaking

Three sources of state are never confused. Each has its own shape, hue and place, so they can be
told apart in grey-scale and with colour blindness:

| Source | Shape | Hue | Where |
|---|---|---|---|
| Agent (Claude Code) | circle | `--agent` (copper) | Status dot before an item, Terminal tab, waiting strip, terminal cursor, Ask agent, Send to agent |
| You (the reviewer) | square | `--you` (blue, also the focus colour) | Draft comments and their gutter marks, the star, the selected item and file, focus, "Ready for review" |
| GitHub | diamond, outlined pill | `--gh` (violet) | New activity, Comments badge, CI and review facts, submitting to GitHub |

- Red (`--bad`) means broken (CI failing, errors, work that would be lost). It replaces the hue,
  never the shape: an agent error is a red circle, CI failing a red diamond. Amber (`--warn`) is
  caution or pending, green (`--ok`) done or passing. CI pending is a hollow amber diamond
  (`CiMark`): a GitHub fact, so never a circle.
- News that asks nobody (lifecycle notices, a task's PRs all closed) is a neutral bar in
  `--info`, which is grey, not a fourth blue.
- GitHub facts are outlined pills (state, labels, review decisions, the Comments count); local
  state is a tinted fill ("In Inbox" is a neutral tint: it wants nothing).
- Everything else is grey: colour means someone wants something. Links are underlined text, not
  blue: blue is "yours", not "clickable".
- Filled agent and GitHub buttons appear only where an action leaves Ariekei for that party
  (Send to agent, Submit to GitHub). "Answer in Terminal" only switches tab, so it is the soft
  agent variant (`agent-soft`). Where an action or line belongs to one voice, its `Mark` (the
  small solid circle, square or diamond) stands in for an icon.
- No circle that isn't the agent: authors have no avatars, CI pending is a diamond.

## Where things are

- `ui/src/tokens.css`: every colour (light and dark), type, space, radius, size and motion
  token. Components never hard-code a colour, size or duration; CodeMirror
  (`lib/codemirrorTheme.ts`) and xterm (`lib/xtermTheme.ts`) read the same tokens.
- Code (the diff, the terminal, inline code) is shown as written: font ligatures and
  contextual alternates are off (`app.css`), so `->` or `<=` never turn into arrows.
- `ui/src/controls/`: the shared controls (`Button`, `IconButton`, `Toggle`, `Segmented`,
  `Pill`, `Banner`, `InlineConfirm`, `Kbd`, `StarToggle`, `Spinner`), the icon set (`icons.ts`,
  16 px grid, 1.5 px stroke; `Icon`), the voices' `Mark`, `CiMark`, the agent `StatusDot`, the
  `AttentionPip`, `NoticeCard` and the `Logo`.
- The app icon (`src-tauri/icons/`) is the same mark: a circle (the agent) and a square (you)
  overlapping where the work is decided.
- Dialogs (Browse, the shortcuts) are `<dialog>` elements shown with `showModal()`
  (`lib/modal.ts`): in the top layer, with the focus kept in them and given back on close.

## The Inbox

- **Sections**, in this order: Starred, Tasks, Assigned, Approved (the user's PRs ready to
  merge), Review requested, My PRs, Reviewed, Open. An item is listed once, in the first
  section that fits (`lib/inboxSections.ts`, from the backend's `role`); a task's PRs and issues
  are items like any other. Empty sections are left out; an empty Inbox says how to fill it.
- **Section headers** are light dividers (24 px, `label-caps`, the count in faint mono right
  after the label) and stay at the top while their section scrolls.
- **Rows** have two lines. The dot slot is empty when there is no agent. Issues say `issue` on
  line 2; Approved says `ready to merge` (unless a draft or CI holds it up). A task's line 2 is `task · <branch>` or `no PR or issue
  yet`; its PRs and issues hang under it on a 1 px guide line, with their kind icon in the dot
  slot, and more than three fold (except those that want something) under "+N more".
- **The star** is the user's own mark, filled in `--you`. In rows it is a hover/focus action
  with **Remove** (the `inbox-remove` icon, for items the user added); Starred already says
  which items are starred. The item header has the star after its title, always shown.
- **Browse** (Ctrl+K, the Browse field at the top of the Inbox) is a raised
  dialog near the top (`--w-dialog`), as tall as its results: Pull requests | Issues, a search
  field, and per hit a fixed 200 px slot with a quiet **Add** and **Start working** (which
  closes it, opens the item and starts it as the item's own button does), or the neutral
  `In Inbox` pill, replaced on the pointer or keyboard row by **Open** (and **Remove** for
  items the user added). The keyboard row is `--surface-selected`. It stays open after adding.
- **Refresh** turns its arrow (`--dur-spin`, like the `Spinner`) until the poll it asked for has
  answered, with new data or an error; with reduced motion it only dims.
- **New task** is a raised form at the top of the Inbox with visible labels (the branch field in
  mono), Ctrl+Enter to start and Esc to close.

## The work item

- The meta line names the worktree by its folder (`proj_pr8012`; the full path is its title and
  a click copies it) and ends with the agent's status at the right.
- While the agent waits for permission, a strip under the header shows what it asks to run, with
  "Answer in Terminal". Below a 1200 px window the command wraps instead of being cut.
- When the agent's worktree is on another branch than the item's (it may be the user's own
  checkout), a red banner says so, with **Switch back to `<branch>`** (and, on a detached HEAD,
  **Move `<branch>` here**; on another branch, **Continue on `<branch>`**, which makes it the
  item's branch; for a PR the Update bar then waits until GitHub has another head), and every way to send the agent work is disabled with that
  reason. The terminal still takes input: what the user types is their own call. Neither
  Switch back nor Move here leaves commits GitHub lacks behind.
- When GitHub's PR head is one the worktree's branch lacks (pushed to, or rebased by the
  author), a bar offers **Update `<branch>`**, so getting the author's version never takes a
  detached checkout. Not for a branch rewritten locally that had GitHub's head (its reflog),
  and not on a PR the user reviewed: the poll pulls it there unless that would lose work.
- When the agent exits by itself, a bar under the terminal's title bar says so ("The agent
  exited (exit 1).") with **Start again**; its last output stays, dimmed and inert. Not for a merged or closed item: its branch is done
  with.
- Lifecycle notes ("Created the worktree …") are one line and go after six seconds.
- A PR or issue that GitHub finishes while it is open in a window is not cleaned up under the
  user (they may be reading its agent's answer): a bar asks, **Clean up** or **Keep until
  Done**; left without an answer, it is cleaned up on the next poll.
- Done asks with a neutral (amber) confirm: it only cleans up what is safe. Discard is a quiet
  button and asks with a red confirm, listing what would be lost.

## The review

- **One review bar** (`--h-review-bar`): the scope menu (all changes, or one commit with its
  file count), the head commit and commit count, file navigation when the file panel is hidden,
  Save file while editing, the Whitespace and Unified toggles, then Reload, the file panel and
  the shortcuts.
- **Newer on GitHub** is a GitHub-tinted note under the review bar (`Mark` gh, the text, a quiet
  Reload): the diff never changes by itself while it is read.
- **The file panel** (`--w-panel`) holds only the files: Review order | Tree, groups with 24 px
  headers, and your `Mark` on files with drafts.
- **The dock** under the diff is the one home of your review: the hint, the line composer (with
  a selection), the drafts and their two send actions. Draft lines keep a square
  (`--diff-draft-mark`) in the gutter. Submit to GitHub opens a popover above its button. Send
  to agent is held back while the agent waits for permission.
- Below an 880 px diff column the diff is unified, unless chosen otherwise with `u`; an added or
  deleted file always shows as one pane.
- Only the changed lines' gutter markers are coloured, not the whole change column.

## Notices

A notice tells the focused window about another item (`Notices.svelte`, `NoticeCard`;
unfocused, it is an OS notification). It sits in the bottom right corner (above the review's
dock), so it has to be seen without being loud:

- A neutral raised card, like an Inbox row lifted out of the list: the pip says who has to act,
  and the reason line says it in the Inbox's words and colours ("Agent needs you", "Ready for
  review", "CI failing", "New activity", "Agent error"), then the detail. Only a permission
  request is tinted (`--agent-bg`, `--agent-line`): only it blocks work until answered.
- It fades in with a short slide from the edge (`--dur-slow`, `--ease`); its pip pulses once
  (`--dur-pulse`), three times for a permission request. Nothing keeps moving after that, and
  with reduced motion nothing moves.
- The newest three show; older ones fold into "+N more".
- Clicking it opens the item: on its Terminal when the notice is from the agent (finished,
  waiting for permission, stopped with an error), else on the tab that resolves it. A
  permission request stays until the agent moves on; the rest go after ten seconds.

## The code menu

Right-clicking code in the review opens a menu (`CodeMenu.svelte`) instead of the browser's,
headed by the lines and their side ("Lines 34–36 · new version"): **Ask agent…** with the
agent's `Mark` and `a`, **Comment on lines** (new side only) with your `Mark` and `c`, and
**Copy**. Without an agent, Ask agent says why on a second line. Ask agent opens a prompt box
(`AskAgent.svelte`) below the lines, else above them, else docked over the dock, never covering
them, and within the new pane; while the agent waits for permission it says so with "Answer in
Terminal" and keeps the prompt editable. Its one filled button is **Send to agent** (an action
that leaves Ariekei for the agent). Both are raised surfaces (`--surface-raised`,
`--shadow-pop`) kept inside the window (`lib/popover.ts`).

## Behaviour that comes with the design

- An item opens on the tab last used for it. Opened for the first time, on the tab that
  resolves its attention reason: the agent needs you → Terminal, new reviews or comments →
  Comments, ready for review → Review, CI failing or cleanup blocked → Overview.
- "Ready for review": the agent finished a turn and the user hasn't opened the item's Review tab
  since.
- Below a 1200 px window the Inbox steps aside once the review's file panel opens, and stays
  aside until Ctrl+B or its button, so the layout doesn't jump.
- With nothing selected, the work area lists up to five items that want the user.
- `n` / `p` past the last (first) change of the scope show a note by that change ("Last change
  in this scope"); it fades after `--dur-note`, nothing else moves.
- Keyboard: Ctrl/Cmd+B toggles the Inbox, Ctrl/Cmd+K browses, Ctrl/Cmd+N starts a task,
  Ctrl/Cmd+Shift+N opens a new window; the review keys are listed under `?`. Every control is
  reachable with Tab and shows a focus ring (full-width rows: offset −2 px).
