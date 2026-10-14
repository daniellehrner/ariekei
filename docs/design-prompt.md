# Design prompt for Claude Design

Paste everything below the line into Claude Design. The result is meant to be implemented later
in the existing Svelte code, so the prompt asks for tokens and component specs that map onto it.

---

Design the desktop app **Ariekei**: a review workbench for coding agents. It exists and
works; it was built function-first with a plain look. I want a coherent visual identity and a
considered layout for every screen and state below, which we will then implement in the
existing code. Design for implementation, not for a marketing shot.

## What Ariekei is

Developers run coding agents (Claude Code) on several pull requests and issues in parallel.
The agent writes the code; the human's job becomes **reviewing** it, deciding, and steering the
agent. Ariekei is where that happens. It is not an IDE: no file tree, no code navigation, no
debugger (the user keeps their IDE for that).

- The unit of work is a **work item**: a GitHub pull request or issue. Each one gets its own git
  worktree, its own Claude Code session (the real `claude` terminal UI, embedded) and its own
  review progress.
- The **Inbox** lists work items ordered by what needs the user, most urgent first.
- **Reviewing is the central activity.** Most time is spent in the Review tab, reading diffs.
- Users: experienced developers (e.g. Java/Rust/TypeScript on large open-source codebases),
  keyboard-heavy, working with 3–10 items at once, often across several repositories. They
  run it all day next to a terminal and an IDE.
- Cross-platform desktop (Linux, macOS, Windows), Tauri webview, native window chrome. Light
  and dark, following the OS setting.
- The name comes from the alien race in China Miéville's *Embassytown*, who speak with two
  mouths at once. You may draw on that lightly (two voices: the human and the agent), but the
  app must read as a calm professional tool, not a sci-fi theme.

## Design goals

1. **Maximum room for the diff.** Everything around the code must be collapsible or compact.
   The user often hides the Inbox and the review side panel entirely.
2. **Attention at a glance.** From the Inbox (even when collapsed to a thin strip) the user
   sees which item needs them: an agent waiting for permission, CI failing, new comments,
   changes to review.
3. **Clear distinction between the agent's work, the human's review, and GitHub.** Three
   sources of state that are currently hard to tell apart.
4. **Dense but calm.** Developer-tool density (think Linear, Zed, GitHub's newer UI, Graphite),
   no decoration that costs space. Excellent in both light and dark.
5. **Keyboard-first.** Visible focus, shortcut hints where they help, a shortcut overlay.
6. **Lightweight.** Little motion (respect reduced motion), no heavy effects.

## Screens and what is on them

Use realistic sample data: a Java repo `besu-eth/besu` and a second repo `owner/ariekei`
(Rust + Svelte); PR titles like "Fix snap sync stall when pivot block is reorged", files like
`ethereum/eth/src/main/java/org/hyperledger/besu/ethereum/eth/sync/snapsync/SnapSyncState.java`,
authors `me`, `carol`, `dmitri`.

### 1. Connect / repository picker
Shown on first launch and in a new empty window.
- "Choose folder…" (native directory dialog) and a path field with Connect.
- List of repositories connected before (owner/name, local path, "open in another window").
- Cancel when the window already shows a repo.
- Errors: "no GitHub remote", `gh` not logged in.

### 2. Main window layout
Inbox sidebar on the left (collapsible to a ~36px strip, Ctrl/Cmd+B), the work item on the
right. One repository per window; a window per repo can be opened.

### 3. Inbox (sidebar)
- Header: **repo menu** (owner/name ▾ → list of connected repos with ✓ for this window,
  "open in another window", an "open in new window" action per repo, "Connect another
  repository…", "New window"), Refresh, Collapse.
- Search field over all open PRs of the repo (words, `#123`, `author:name`); results replace
  the list, with tags "draft", "in inbox".
- Sections: **Mine**, **Review requested**, **Issues**, **Opened** (PRs opened from search),
  each with a count.
- A row: agent status dot, `#number`, title, new-activity dot; second line: author (not in
  Mine), "from #issue", CI (passing ✓ / failing ✗ / pending ●), draft, closed, review decision
  (approved / changes requested / review required), unresolved thread count, and the
  **attention reason**.
- Attention reasons, most urgent first: agent needs you (waiting for permission) → finished
  but cleanup blocked / CI failing → new reviews or comments → changes to review → agent
  working → none.
- Agent statuses: none, starting, ready, working, waiting for permission, finished, error,
  stopped.
- Collapsed strip: only items that need attention, as colored pips; the selected one marked.
- States: loading, empty section, GitHub poll error (e.g. rate limit) shown inline.

### 4. Work item
- Header: `#number` title; meta line: issue/PR, "from issue #5", author, CI with failing check
  names, review decision, unresolved threads, worktree path, latest agent detail (e.g. the
  permission it is asking for).
- Actions: **Start working** (creates/reuses a worktree and starts the agent; shows
  "Preparing the worktree…"), **Done** (with inline confirm "Clean up the worktree, agent and
  this item?").
- Banner when cleanup is blocked: "Not cleaned up — this would lose work:" with a list
  ("1 uncommitted change: notes.txt", "1 commit is not on GitHub: 634e638 Fix the crash").
- Notices (e.g. "Using the existing worktree …") and errors.
- Tabs: **Overview**, **Comments** (badge: unresolved threads), **Review**, **Terminal** (only
  with a worktree). Right side of the tab bar: "Send PR feedback to agent".

### 5. Overview tab
PR/issue title, state (open/draft/merged/closed), "carol wants to merge `feature/x` into
`main`", opened date, link to GitHub, labels; the rendered Markdown description; a side column
with reviews (per reviewer: approved / changes requested / commented / requested), checks
(name, outcome, link), and a discussion summary.

### 6. Comments tab
Review threads (file:line, "outdated", "resolved", the last lines of the diff hunk, comments
with author/date/Markdown, Reply, Resolve/Unresolve), resolved ones folded; then the
conversation timeline (comments and reviews with their state) and a new-comment box.

### 7. Review tab — the core; spend the most effort here
- **Header:** scope marker ("Worktree 634e6382 · 3 commits vs main · reviewed 7cc00c84, 1 new
  since · uncommitted changes"), progress pill "7/12 reviewed" (complete state), Reload,
  **Mark reviewed** (with a coverage guard: "3 files not marked reviewed in this scope — Mark
  anyway / Cancel"), **Prepare review guide** / "Preparing the guide…" / "Regenerate guide",
  side-panel toggle, `?` shortcuts. A notice area ("Changes requested on GitHub with 1 line
  comment. open").
- **Rebase note** (when applicable): "Rebased since your review: changes that came only from
  main are hidden." (and a conflict variant that warns the plain diff is shown).
- **Side panel** (collapsible, often hidden):
  - **Review guide** (written by the agent on request): a two-sentence focus; claims from the
    PR description and commit messages checked against the diff, each ✓ implemented,
    ◐ partial or ✗ missing; "Not explained by the description: file, file"; "outdated" state.
  - **Scope**: collapsed shows only the active scope plus quick switches; expanded lists "All
    changes vs main", "Since your review", and every commit (short sha, subject; commits newer
    than the review highlighted).
  - **Files** with an order switch: Review order (Source by directory → Tests → Config & build
    → Generated, lock & vendored, the last folded), Tree, Guide (the guide's groups, riskiest
    first, each with a risk badge high/medium/low and a one-line intent). Each file: reviewed
    checkbox, status letter (A/M/D/R/U), path, "whitespace" tag; per-group reviewed counts.
- **Toolbar** above the diff: when the side panel is hidden, previous/next file with
  "SnapSyncState.java 3/12 · All changes vs main"; a hint "Select code to comment on it; edit
  it to change the file"; with a selection: "Lines 40–44:" + comment input + Add comment;
  toggles **whitespace** (w) and **unified** (u); **Reviewed, next (r)** / "Next unreviewed (r)".
- **Guide line** (just added) above the diff: what the guide says about the current file —
  risk badge, group title and intent, the claims this file backs with ✓/◐/✗, "not explained
  by the description", "guide outdated"; folded to one line, expandable to show the focus for
  the whole change.
- **Diff**: CodeMirror 6 merge view, side-by-side or unified, syntax-highlighted for any
  language, unchanged regions collapsed, the right side editable in the user's own worktree.
  Specify diff colors (added/removed/changed line and word backgrounds, collapsed-region bar,
  gutters, selection, search) for both themes; the editor's chrome is ours to style.
- **Draft comments** under the diff: a list "a.txt:10 Name this better." with remove; actions
  **Send to the agent** and **Submit to GitHub…**. The submit panel: summary textarea, event
  Comment / Approve / Request changes (no approve on your own PR), a preview "1 as line comment
  on the PR diff; 1 in the review text (not on lines of the PR diff at its head commit)",
  Cancel / Submit to GitHub. Without drafts: a quiet "Review on GitHub…".
- **Shortcut overlay** (`?`): n/p next/previous change, ]/[ next/previous file, r reviewed +
  next, u unified, w whitespace, ? shortcuts.
- Reviewing someone else's PR works without a worktree (read-only diff, "Fetching the PR
  head…").

### 8. Terminal tab
The real Claude Code TUI in an embedded terminal (xterm.js), unmodified — we only style the
frame and the terminal theme (16 ANSI colors, background, foreground, cursor, selection) for
both modes. It stays alive while other tabs are shown. Agent status should be visible from
other tabs too (where?).

### 9. System notifications (OS)
"#42 Fix snap sync stall — The agent is waiting for your permission." / "CI is failing:
unit-tests" / "New reviews or comments." — only text; no design needed beyond wording, but
consider an in-app equivalent if useful.

## Constraints for the implementation

- Svelte 5 components with plain scoped CSS and CSS custom properties. No Tailwind, no
  component library. Keep the token approach; today's tokens are `--bg`, `--sidebar`,
  `--panel`, `--user-bg`, `--border`, `--text`, `--muted`, `--accent`, `--ok`, `--warn`,
  `--warn-bg`, `--bad`, `--mono` (light and dark via `prefers-color-scheme`). Extend or rename
  them as you see fit, but give a complete mapping.
- Fonts: the system UI font plus one monospace (JetBrains Mono or similar, bundled or system
  fallback). Base size around 13–14px.
- Icons: today Unicode glyphs (↻ « » ⧉ ▾ ✓ ◐ ✗ 💬 ●). Propose a small consistent inline-SVG
  icon set (≈30 icons, 16px grid, stroke style) and list which icon goes where.
- Native window title bar (no custom chrome) unless you make a strong case.
- Minimum window ~1024×700; design at 1440×900 and check 1024 wide.
- Third-party surfaces to theme, not redesign: CodeMirror 6 (merge view), xterm.js.
- No new features beyond what is listed; if a layout change implies one, call it out
  separately as a suggestion.

## Deliverables

1. **Identity**: app icon (works at 16px and 512px, light and dark docks), wordmark, color
   palette with rationale.
2. **Design tokens** for light and dark: colors (surfaces, text, borders, accent, status:
   ok/warn/bad/info, agent statuses, attention levels, risk levels, diff colors, syntax
   palette, terminal ANSI palette), type scale, spacing, radii, shadows, focus ring — as a
   CSS-variables listing ready to paste.
3. **Components** with all states (default, hover, focus, active, disabled, loading): buttons
   (primary, secondary, quiet, danger), icon buttons, toggles/checkboxes, select, inputs,
   tabs with badges, pills/tags/badges (CI, review decision, risk, agent status, attention),
   status dots, menus (repo menu), banners (blocked, rebase note, error, notice), inline
   confirm, list rows (Inbox row, file row, scope row), section headers with counts and
   folding, the guide line, the comment composer, the submit panel, the shortcut overlay,
   empty and loading states.
4. **High-fidelity screens**, light and dark: Connect (first run and with recent repos);
   main window with Inbox expanded and collapsed; repo menu open; work item Overview,
   Comments, Terminal; Review with side panel open (Review order and Guide order) and with
   side panel hidden; Review with selection + comment composer; Review with drafts and the
   submit panel; Review with the rebase note; cleanup-blocked banner; agent waiting for
   permission across Inbox, work item header and Terminal.
5. **Interactive prototype** of the review loop: pick an item in the Inbox → Review → read
   with the guide → mark files reviewed with r → draft a comment → send it to the agent or
   submit to GitHub.
6. **Implementation notes**: the token mapping from today's variables, a component inventory
   mapped to the existing Svelte files (`App`, `Inbox`, `RepoMenu`, `Connect`, `WorkItem`,
   `Overview`, `Comments`, `ReviewView`, `TerminalPanel`), and the CodeMirror and xterm
   theme values.

## Questions I want your opinion on

- How should the agent's status and the human's review state be shown so they never get
  confused (color, shape, placement)?
- Where does agent status live when the user is not on the Terminal tab?
- Should Review be the default tab for items with changes to review?
- How should the side panel, guide line and toolbar share space when everything is shown at
  1024px wide?
