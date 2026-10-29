# Pull requests and the agent

Connecting a repository, the Inbox, working on a PR with the agent, and reviewing it. Issues,
tasks and cleanup: [lifecycle.md](lifecycle.md). The review view in detail:
[review.md](review.md).

## What it does

- **Connect a repo:** point Ariekei at a local clone. It finds the GitHub remote (`origin`, else
  `upstream`, else any GitHub remote) and uses your `gh` login.
- **Inbox:** your PRs and issues, polled from GitHub every 60 s (↻ refreshes now), in sections by
  your role (see [design.md](design.md#the-inbox)), and within a section by what needs you:
  agent waiting for permission → finished but cleanup blocked → CI failing → new
  reviews/comments since you last opened the item → agent working → the rest by last update.
  Each row shows CI, draft, review decision, unresolved threads and agent status.
  Polling is cheap on GitHub's GraphQL budget (5000 points an hour, shared with `gh` and the
  agents): it asks which PRs exist and when each last changed (1 point per query) and fetches
  details only for PRs that changed, are new, or have checks running; every tenth poll and ↻
  fetch them all. If the budget is used up anyway, the Inbox says so and polling waits for
  GitHub's reset.
- **Browse** (Ctrl+K): search the repo's open PRs and issues with GitHub's search syntax (words,
  `#123`, `author:name`, …) and add them to the Inbox's Open section; star an item to keep it
  at the top.
- **Work item tabs:** **Overview** (title, author, branches, labels, description, reviewers,
  checks) · **Comments** (review threads by file with their diff snippet, unresolved first; the
  conversation; reply, resolve/unresolve, new comment) · **Review** · **Terminal** (with a
  worktree). GitHub renders the markdown; Ariekei sanitizes it (DOMPurify) and opens links in the
  system browser.
- **Reviewing without a worktree:** opening a PR fetches GitHub's `refs/pull/<N>/head` into the
  clone as `refs/ariekei/pull/<N>/head`; no checkout, no agent.
- **Start working:** one click per PR. Creates `../<clone>_pr<N>` on the PR's branch, tracking the
  PR head (fetched from the head repository's remote, added like `gh pr checkout` if missing), so a
  plain `git push` updates the PR. Reuses an existing worktree that has the PR's branch, `pr-<N>`,
  or a branch tracking the PR head. A reused local branch is fast-forwarded to the PR head; if it
  has its own commits it is left as it is and the UI says so.
- **Agent:** your installed `claude` in the worktree (see `docs/spike-results.md`, agent
  integration v2). A new conversation gets the PR as context via `--append-system-prompt-file`:
  description, unresolved review threads with file/line, review summaries, failing checks with the
  failing part of the Actions job log. The agent waits for your instructions. Claude Code records
  the system prompt at the conversation's first request, so the context is frozen there; **Send PR
  feedback to agent** sends the current threads, reviews and CI state as a message.
  Each agent, with what it starts, may use up to half the RAM: a command beyond that fails
  without taking Ariekei or the other agents along (`src-tauri/src/confine.rs`: a systemd scope
  per agent on Linux, a job object on Windows; macOS stops no process group to begin with).
- **Review:** all changes vs the PR's base (merge base → head, like GitHub's Files tab),
  including uncommitted ones, or any single commit. Chunks come from git's own hunks
  (CodeMirror's diff only adds character-level detail inside them). When viewing the working
  tree, the right side is editable and can be saved.
- **Review comments → GitHub or agent:** select code on the right to attach a comment; drafts are
  collected per item and shown in the diff below their lines. On others' PRs the agent drafts
  review comments there too (its context and Ariekei's MCP tools tell it how), which you edit,
  delete or send back to it with a prompt. **Send to agent** delivers your drafts on your own
  work as one message (with a worktree). **Submit to GitHub…** sends them as a review (Comment, or Approve / Request changes
  on others' PRs): drafts on lines of the PR diff at its head commit become line comments, the
  others are listed in the review text, and the panel says which is which.
- **Switching and resume:** each item's tab, review scope, selected file and pending comments are
  stored with the item. Terminals keep running while you look at other items. A terminal nobody
  looks at whose agent has been waiting for 45 minutes is closed, unless the agent still runs
  processes of its own (e.g. background shells, which would end with it and which resuming does
  not restart); opening the item resumes the conversation (`--resume`). Restarting the app does
  the same.
- **Notifications:** an OS notification when an agent waits for permission, finishes a turn or
  stops with an error, when a PR's CI starts failing, and on new reviews or comments. Nothing for
  the item you are looking at in a focused window. While an Ariekei window is focused, the
  notice shows in that window instead (an OS notification is easy to miss with the app in
  front); clicking it opens the item.

State lives in SQLite (`ariekei.db` in the app data dir); worktrees, branches and the review refs
live in your clone.

## Testing

- `npm run check`, `npm test` (git-hunk → CodeMirror mapping), `cargo test` in `src-tauri`
  (store, GitHub parsing, worktree create/adopt/fast-forward against a fake GitHub remote, inbox
  ordering, hooks, context rendering, notifications).
- Live, read-only GitHub check: `ARIEKEI_LIVE_REPO=owner/name cargo test -- --ignored live`.
- E2E `e2e/agent_scenario.py` (run with `e2e/run.py`; the agent is the fake in
  `e2e/fake_claude.py`): connect, Inbox, Start working, slash-command autocomplete, an agent
  commit after a permission prompt, review since base, a second commit showing up alone, Inbox
  attention, idle shutdown (not while a background job runs) and resume, restart.
- E2E `e2e/review_scenario.py` runs against `e2e/fake_gh.py` (set `ARIEKEI_GH`), which serves
  fixture PRs and records every write; the scenario checks the exact review payload.
- Verified on besu, read-only: Inbox with 23 PRs, search, Overview, Comments and review of
  another author's PR (#11206); Start working on #11577 (worktree created on its branch in
  ~5 s, agent ready ~1.5 s later, context written).

## Not covered yet

- Pending (multi-session) GitHub reviews, suggestions, comments on deleted lines (left side),
  editing or deleting your comments, and a markdown preview for what you write.
- Accepting or rejecting single hunks; build/test status in the review view (planned).
- Not exercised end to end: **Send PR feedback to agent** against real GitHub, and
  notifications on a real desktop (Xvfb has no notification daemon).
- The first time Claude Code sees a repository it asks for trust in the terminal, with "No, exit"
  preselected.
- WebKitWebDriver screenshots take 40+ s on this setup (an X capture of the display shows only
  black under automation), and its synthesized clicks miss elements at the window's bottom edge;
  the scenarios click those in the DOM.
