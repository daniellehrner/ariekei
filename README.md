# Ariekei

A review workbench for coding agents. The unit of work is a GitHub pull request or issue: each
work item gets its own worktree, its own Claude Code session and its own review progress, and an
Inbox orders them by what needs you. The agent writes the code; you review it here. For deep
code navigation or debugging, use your IDE on the same worktree. Tauri 2 (Rust) + Svelte 5.

## What it does

- **Inbox:** your open PRs, PRs requesting your review, your assigned issues, and any PR you
  opened from search, ordered by attention (agent needs you, CI failing, new comments, changes
  to review, …). OS notifications for the same events.
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
- **Lifecycle:** an issue gets a branch and worktree; its first push opens a draft PR. Merged
  or closed items are cleaned up automatically, never losing uncommitted or unpushed work.

Language-agnostic: the review diff comes from git, any file type is syntax-highlighted, and the
agent works in any language. Code navigation and debugging are left to your IDE.

Details: [docs/m1.md](docs/m1.md) (Inbox, PRs, review) and [docs/m2.md](docs/m2.md) (issues,
cleanup). How the agent integration came about: [docs/spike-results.md](docs/spike-results.md).

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

`e2e/` drives the release build through `tauri-driver` under Xvfb (Linux; needs
`webkit2gtk-driver`, `xvfb` and `cargo install tauri-driver`). Each scenario's header has the
exact command.

- `review_scenario.py`: search, Overview, Comments, review without a worktree, submitting a
  review. No agent.
- `lifecycle_scenario.py`: issue → worktree → push → draft PR → merge → cleanup. No agent.
- `m1_scenario.py`: the agent in the terminal, review loop, idle shutdown and resume. Uses
  Claude Code (two short turns).

The first two run against `e2e/fake_gh.py`, a stand-in for `gh` that serves fixture PRs and
issues and records every write instead of sending it. Test-only environment variables:

| Variable | Effect |
|---|---|
| `ARIEKEI_DATA_DIR` | Use this directory instead of the app data dir (database, hooks, context) |
| `ARIEKEI_GH` | Use this binary instead of `gh` |
| `ARIEKEI_NO_POLL` | Do not poll GitHub |
| `ARIEKEI_POLL_SECS` | Poll every N seconds instead of 60 |
| `ARIEKEI_IDLE_SECS` | Close idle, unwatched agents after N seconds instead of 15 minutes |
| `ARIEKEI_CLAUDE_ARGS` | Extra arguments for `claude` |
| `ARIEKEI_CLAUDE` | Use this `claude` binary |

Screenshots via WebKitWebDriver are slow under Xvfb (40 s or more each); `E2E_SHOTS=0` skips
them. The scenarios check the DOM, not the pictures.
