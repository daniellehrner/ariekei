# Ariekei

A review workbench for coding agents. The unit of work is a GitHub pull request or issue, or a
task that has neither yet: each work item gets its own worktree, its own Claude Code session and its own review progress, and an
Inbox orders them by what needs you. The agent writes the code; you review it here. For deep
code navigation or debugging, use your IDE on the same worktree. Tauri 2 (Rust) + Svelte 5.

## What it does

- **Inbox:** your open PRs, PRs requesting your review, your assigned issues, and any PR you
  opened from search, ordered by attention (agent needs you, CI failing, new comments, changes
  to review, …). Notifications for the same events: in the window while Ariekei is in front
  (click one to open its item), OS notifications otherwise.
- **Work item:** Overview (description, checks, reviewers), Comments (review threads and
  conversation; reply, resolve, comment), Review, and Terminal.
- **Agent:** your installed `claude`, unmodified, in an embedded terminal in the item's
  worktree, with the PR or issue as context. Status comes from Claude Code hooks; conversations
  resume after switching items or restarting.
- **Review:** all changes vs the base branch, changes since your last review (rebase-aware),
  or single commits; per-file reviewed marks, a review order instead of alphabetical, an agent
  review guide (risk-ordered groups, claims checked against the diff), whitespace and unified
  toggles, keyboard navigation; works without a worktree for other people's PRs. Comments go
  to GitHub as a review or to the agent. See [docs/review.md](docs/review.md).
- **Repos and windows:** the repo name in the Inbox header switches the window to another
  connected repo, opens one in a new window, or connects another. Each repo is shown in one
  window at a time; every open repo is kept current, and the windows come back after a
  restart.
- **Tasks:** start work with no PR or issue yet (+ in the Inbox, e.g. to investigate a bug). The
  task gets a branch, worktree and agent; the PRs and issues its agent opens (`gh pr create`,
  `gh issue create`) are linked and listed under it. Done or Discard ends it.
- **Lifecycle:** an issue gets a branch and worktree; its first push opens a draft PR. Merged
  or closed items are cleaned up automatically, never losing uncommitted or unpushed work.

Language-agnostic: the review diff comes from git, any file type is syntax-highlighted, and the
agent works in any language. Code navigation and debugging are left to your IDE.

Details: [docs/m1.md](docs/m1.md) (Inbox, PRs, review) and [docs/m2.md](docs/m2.md) (issues,
tasks, cleanup). The UI's design rules: [docs/design.md](docs/design.md). How the agent integration came
about: [docs/spike-results.md](docs/spike-results.md).

## Roadmap

- **Checks for what you're reviewing:** run the repo's build/test command in the worktree
  (presets detected for Gradle, Maven, npm, Cargo, Go, Make; editable), with per-test results
  from JUnit XML reports next to the diff, and failures sendable to the agent.
- **Open in editor:** jump from the review to the same file and line in your IDE (configurable
  command, e.g. `idea --line {line} {file}` or `code -g {file}:{line}`).
- **Accept/reject single hunks** in the review, reverting rejected ones and telling the agent.

## Requirements

- `git`, the GitHub CLI `gh` (logged in), and Claude Code (`claude`).
- To build: Rust 1.88+, Node 22+, and on Linux the WebKitGTK 4.1 / GTK 3 development packages
  (see `.github/workflows/ci.yml`).

## Develop

```
npm ci
npm run tauri dev        # run the app
npm run check && npm test
(cd src-tauri && cargo test)
npx tauri build --no-bundle
```

Live, read-only checks against a real repository:
`ARIEKEI_LIVE_REPO=owner/name cargo test -- --ignored live` (in `src-tauri`).

## End-to-end tests

`e2e/` drives a test build of the app on Linux, macOS and Windows; CI runs every scenario on
all three. The test build has the e2e bridge (`--features e2e`, `src-tauri/src/e2e.rs`): with
`ARIEKEI_E2E_PORT` set, the app runs JavaScript in its windows for `e2e/wd.py`, which starts
the app itself. Release builds do not contain it.

```
npx tauri build --no-bundle --features e2e
python3 e2e/run.py src-tauri/target/release/ariekei            # all scenarios
python3 e2e/run.py src-tauri/target/release/ariekei review     # one of them
```

On Linux the app runs on a private Xvfb display and D-Bus session (it needs `Xvfb`), so its
windows never show up on your desktop or take focus, and its notifications stay out of yours.
`E2E_HEADED=1` shows the windows on your display instead.

- `review`: search, Overview, Comments, review without a worktree, review order and marks,
  the review guide, submitting a review, rebase-aware "since your review".
- `windows`: two repos, switching, a window per repo, restoring windows after a restart.
- `lifecycle`: issue → worktree → push → draft PR → merge → cleanup.
- `task`: new task → the agent opens a PR and an issue → linked under the task → discard.
- `m1`: the agent in the terminal (turns that commit, a permission prompt), the review loop,
  idle shutdown and resume after a restart.

Nothing leaves the machine: GitHub is `e2e/fake_gh.py`, a stand-in for `gh` that serves fixture
PRs and issues and records every write instead of sending it, and the agent is
`e2e/fake_claude.py`, which writes a canned review guide for `claude -p` and otherwise is a
small terminal agent that commits when asked and reports its status through the hooks like
Claude Code. `e2e/gallery.py` is not a test: it takes screenshots of every screen through
tauri-driver (Linux), in light or dark and at any window size, for comparing the UI with the
design. Test-only environment variables:

| Variable | Effect |
|---|---|
| `ARIEKEI_DATA_DIR` | Use this directory instead of the app data dir (database, hooks, context) |
| `ARIEKEI_GH` | Use this binary instead of `gh` |
| `ARIEKEI_NO_POLL` | Do not poll GitHub |
| `ARIEKEI_POLL_SECS` | Poll every N seconds instead of 60 |
| `ARIEKEI_IDLE_SECS` | Close idle, unwatched agents after N seconds instead of 15 minutes |
| `ARIEKEI_CLAUDE_ARGS` | Extra arguments for `claude` |
| `ARIEKEI_CLAUDE` | Use this `claude` binary |
| `ARIEKEI_PYTHON` | Interpreter for a `.py` given as `ARIEKEI_GH` or `ARIEKEI_CLAUDE` (default `python3`) |
| `ARIEKEI_E2E_PORT` | Serve the e2e bridge on this port (test builds only) |
