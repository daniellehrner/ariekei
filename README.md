# Ariekei

A review workbench for coding agents. The unit of work is a GitHub pull request or issue, or a
task that has neither yet: each work item gets its own worktree and its own Claude Code session, and an
Inbox orders them by what needs you. The agent writes the code; you review it here. For deep
code navigation or debugging, use your IDE on the same worktree. Tauri 2 (Rust) + Svelte 5.

How to use it: [docs/manual.md](docs/manual.md) (user manual).

## What it does

- **Inbox:** sections **Starred**, **Tasks**, **Assigned** (PRs and issues), **Approved** (your
  PRs ready to merge), **Review requested**, **My PRs**, **Reviewed** (PRs you reviewed) and
  **Open** (what you added); each item is in the first section that fits, and within a section
  ordered by attention (agent needs you, CI failing, new comments, ready for review, …). Star
  any item to keep it at the top. **Browse** (Ctrl+K) lists the repo's open PRs and issues with
  GitHub's search syntax and starts work on any of them (Enter) or adds them to Open; what you added can be removed again. Notifications for the same events: in the window while Ariekei is in front
  (click one to open its item), OS notifications otherwise.
- **Work item:** Overview (description, checks, reviewers), Comments (review threads and
  conversation; reply, resolve, comment), Review, and Terminal.
- **Agent:** your installed `claude`, unmodified, in an embedded terminal in the item's
  worktree, with the PR or issue as context. Status comes from Claude Code hooks; conversations
  resume after switching items or restarting.
- **Review:** all changes vs the base branch, or single commits; a review order instead of
  alphabetical, whitespace and unified toggles, keyboard navigation; works without a worktree
  for other people's PRs. Comments go to GitHub as a review or to the agent; right-click code
  to ask the agent about it (file, lines and code go along). On someone else's PR the agent
  drafts its review comments into the diff through Ariekei's own MCP tools: its drafts and yours
  are cards under their lines, to edit or send back to the agent, and Submit posts them all. See
  [docs/review.md](docs/review.md).
- **Repos and windows:** the repo name in the Inbox header switches the window to another
  connected repo, opens one in a new window, or connects another. Each repo is shown in one
  window at a time; every open repo is kept current, and the windows come back after a
  restart.
- **Tasks:** start work with no PR or issue yet (Ctrl+N or + in the Inbox, e.g. to investigate a bug). The
  task gets a new branch named after its title, or an existing one such as `owner:branch` on a
  fork (the fork's remote is added if the clone has none, and `git push` goes back there), plus a
  worktree and agent. The PRs and issues its agent opens (`gh pr create`, `gh issue create`)
  become items of their own, in their sections, and keep how they came about (the task's notes
  and conversation), so Start working gives each its own agent with that context. Done or
  Discard ends it.
- **Lifecycle:** an issue gets a branch and worktree; once a PR is opened for the branch (by you or the agent), the item follows it. A
  worktree switched to another branch can be switched back or follow it, a PR's branch updated to
  GitHub's head, and a PR you reviewed pulls the author's fixes by itself. Worktrees an agent
  makes itself are listed on its item, to remove or make a task of. Merged or closed items are
  cleaned up automatically (one you have open asks first), never losing uncommitted or unpushed
  work, also in the worktrees the agent made.

Language-agnostic: the review diff comes from git, any file type is syntax-highlighted, and the
agent works in any language. Code navigation and debugging are left to your IDE.


Details: [docs/pull-requests.md](docs/pull-requests.md) (Inbox, PRs, the agent, review) and
[docs/lifecycle.md](docs/lifecycle.md) (issues, tasks, cleanup). The UI's design rules: [docs/design.md](docs/design.md). How the agent integration came
about: [docs/spike-results.md](docs/spike-results.md).

## Requirements

- `git`, the GitHub CLI `gh` (logged in), and Claude Code (`claude`).
- To build: Rust 1.89+, Node 22+, and on Linux the WebKitGTK 4.1 / GTK 3 development packages
  (see `.github/workflows/ci.yml`).

## Develop

```
npm ci
npm run tauri dev        # run the app
npm run check && npm test
(cd src-tauri && cargo test)
npx tauri build --no-bundle
```

`cargo test` also runs the logic behind the commands (Inbox rows, polling, cleanup, tasks, hooks) against temp git repos, without a Tauri app
(`src-tauri/src/testkit.rs`): GitHub is `e2e/fake_gh.py` and agents are `e2e/fake_claude.py`,
as in the e2e tests, so it needs Python 3 (`ARIEKEI_PYTHON` names another interpreter).

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

- `review`: Inbox sections, browsing and adding, starring and removing, Overview, Comments, review without a worktree, review order,
  n / p across files, submitting a review, a push showing up in the review, the agent's drafted
  review comments (edited, redrafted, submitted).
- `windows`: two repos, switching, a window per repo, restoring windows after a restart.
- `lifecycle`: issue → worktree → push → its PR → merge → cleanup.
- `task`: new task → the agent opens a PR and an issue, each an item of its own → asking the
  agent about code from the review → discard → worktrees the agent makes (removed, made a task)
  → a task on a fork's branch, its PR added.
- `agent`: the agent in the terminal (turns that commit, a permission prompt), the review loop,
  idle shutdown (not while a background job runs) and resume after a restart.

Nothing leaves the machine: GitHub is `e2e/fake_gh.py`, a stand-in for `gh` that serves fixture
PRs and issues and records every write instead of sending it, and the agent is
`e2e/fake_claude.py`, a small terminal agent that commits when asked and reports its status through the hooks like
Claude Code. `e2e/gallery.py` is not a test: it takes screenshots of every screen through
tauri-driver (Linux), in light or dark and at any window size, for comparing the UI with the
design. Test-only environment variables:

| Variable | Effect |
|---|---|
| `ARIEKEI_DATA_DIR` | Use this directory instead of the app data dir (database, hooks, context) |
| `ARIEKEI_GH` | Use this binary instead of `gh` |
| `ARIEKEI_NO_POLL` | Do not poll GitHub |
| `ARIEKEI_POLL_SECS` | Poll every N seconds instead of 60 |
| `ARIEKEI_IDLE_SECS` | Close idle, unwatched agents (without background processes) after N seconds instead of 45 minutes |
| `ARIEKEI_QUIET_SECS` | Take a working agent whose terminal has shown nothing new for N seconds as done with its turn, instead of 30 |
| `ARIEKEI_AGENT_MEMORY_MAX` | Cap each agent's memory at this (bytes, or with K, M or G) instead of half the RAM (Linux, Windows) |
| `ARIEKEI_CLAUDE_ARGS` | Extra arguments for `claude` |
| `ARIEKEI_CLAUDE` | Use this `claude` binary |
| `ARIEKEI_PYTHON` | Interpreter for a `.py` given as `ARIEKEI_GH` or `ARIEKEI_CLAUDE` (default `python3`) |
| `ARIEKEI_E2E_PORT` | Serve the e2e bridge on this port (test builds only) |

## Release

`.github/workflows/release.yml` builds the installers (AppImage, .deb and .rpm on Linux, a universal
.dmg on macOS, .msi and setup.exe on Windows) and attaches them to a draft GitHub Release.

1. Set the same version in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`
   (`npm install` and `cargo check` update the lock files), and commit.
2. `git tag v<version>` and push the tag; the workflow fails if the tag and the version differ.
3. Check the draft release, edit its notes, publish it.
4. Redeploy the website, which picks up the new download links (see below).

The builds are not signed with a developer certificate yet (macOS gets an ad-hoc signature).

## Website

`site/` is the homepage and the docs from `docs/` (Astro + Starlight), deployed with
`site/deploy.sh`. See [site/README.md](site/README.md).

## License

Apache License 2.0, see [LICENSE](LICENSE). Every source file starts with an SPDX header:

```
// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0
```

(in the file's own comment syntax; CI fails when a `.rs`, `.ts`, `.js`, `.svelte`, `.py`,
`.css` or `.html` file lacks it).
