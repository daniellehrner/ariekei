# Issues, tasks and cleanup

From an issue (or a task) to a merged PR, and back to a clean clone. Working on a PR and
reviewing it: [pull-requests.md](pull-requests.md).

## Issues

- Issues assigned to you appear in the Inbox under **Issues** (Overview and Comments show the
  issue). Issues you started working on stay listed even if they are unassigned later.
- **Start working** on an issue creates the branch `issue-<N>-<words of the title>` from the
  default branch (GitHub's, so a renamed one is followed: the clone's `<remote>/HEAD` is moved
  to it; the clone's when GitHub can't be asked) in `../<clone>_issue<N>`, and sets
  it up so that a plain `git push` creates it on your fork (a remote pointing at
  `<you>/<repo>`), or on the base remote when there is no fork. The agent gets the issue's text
  and comments as context and waits for you. **Review** works on the worktree before anything
  is pushed (no "Submit to GitHub" yet).
- **The issue's PR:** Ariekei opens no PR. Once the branch is on its push remote and a PR is
  open for it (opened by you or the agent, e.g. `gh pr create`), the next poll finds it, or a
  merged one if none is open (opened and merged between two polls; a closed one may be an
  earlier attempt and is not followed). The item becomes that PR ("from issue #N") and keeps
  its worktree, agent conversation and view state.

## Tasks

Work that starts with nothing on GitHub, e.g. investigating a bug. It may end as one or more
PRs and issues, or be thrown away.

- **New task** (Ctrl+N, or + in the Inbox header): a title and, optionally, what to look into. Ariekei
  creates the branch `task-<n>-<words of the title>` from the default branch in
  `../<clone>_task<n>`, set up for `git push` like an issue's, and starts the agent there with
  the title and notes as context. Tasks are numbered per repo: T1, T2, …
- **On an existing branch:** give New task a branch instead of (or as well as) a title, e.g. a
  branch on a fork that has no PR yet: `owner:branch` (the fork has the repo's name),
  `owner/repo:branch`, a GitHub branch URL (`https://github.com/owner/repo/tree/<branch>`), or
  a plain branch name of the repo itself. Ariekei uses the clone's remote for that repository,
  or adds one named after the owner as `gh pr checkout` does, fetches the branch and checks it
  out in `../<clone>_task<n>` tracking it, so a plain `git push` updates it. Without a title the
  task is named after the branch. If a local branch of that name tracks something else, the
  worktree gets `task-<n>-…` instead and leaves it alone. A branch that can't be fetched leaves
  no task and no remote behind. The review compares the branch with the default branch.
- **The agent opens the PRs and issues**, when you ask it to: `gh pr create` / `gh issue create`
  (or a GitHub tool whose name contains `create_pull_request` / `create_issue`). Ariekei reads
  the URL they print from the hook event (`PostToolUse`) and adds the PR or issue to the Inbox
  as an item of its own, tracked (polled by number: an issue you opened is not assigned to
  you). A PR opened for the task's pushed branch some other way (in the browser, or by the
  fork's owner) is added on the next poll, found by the branch and the owner of the repository
  it is pushed to.
- **They are not tied to the task:** they are listed in their own sections and finish and clean
  up like any PR or issue. Each keeps how it came about (`origin`: the task's title, notes and
  branch, and its conversation up to then from the transcript, `context::render_origin`), and
  **Start working** gives it an agent of its own whose context ends with that. A PR whose
  branch the task's worktree has checked out adopts that worktree, so both agents work there;
  cleanup of either only drops Ariekei's link while the other still uses it. Links of older
  versions (`task_id`) are turned into origins at startup (`workitem::unlink_from_tasks`).
- **Ending a task:** **Done** cleans it up like any item (refused while work would be lost),
  then deletes it. **Discard** first lists
  what would be lost (uncommitted changes, commits not on GitHub), then removes the worktree
  (`--force`) and branch, the agent session and the task. Refused only while the agent is in the
  middle of a turn.
- **Never cleaned up automatically:** a task may open another PR after the first one merged,
  and cleanup deletes the agent's conversation. When all of its PRs and issues are closed, the
  task says so in the Inbox ("all closed") and a notice suggests marking it done.
- Stored as a work item of kind `task` with a negative number (T1 is -1), so it never collides
  with GitHub's numbers; PRs and issues point to it with `task_id`.

## Cleanup

- **Automatic** when GitHub reports the PR merged/closed (or the issue closed), or no longer
  knows an item Ariekei follows by number (deleted, or an issue moved to another repository;
  a notice says so), and **Done** (header button, with confirmation) at any time.
- Refused while anything would be lost, with the reasons shown on the item and in the Inbox
  ("finished, cleanup blocked"):
  - uncommitted changes, including untracked files (listed);
  - commits at the worktree's HEAD or on the branch deleted with it (HEAD may be detached or
    elsewhere) that are neither on a remote branch nor in GitHub's copy of a PR head
    (`refs/pull/<N>/head` of the PR, or of the PRs for a task's branch, fetched before the check, so
    squash-merged PRs with deleted branches are recognised), nor came from a remote by the
    reflogs (`git::from_remotes`, newest 500 entries each): the upstream's except "update by
    push", and the branch's own "Created from", fast-forward, reset and pull entries naming a
    remote branch. That covers the old version of a PR whose author rebased it, also after
    `git fetch --prune`; a pushed commit a force-push dropped still blocks;
  - git refusing to remove the worktree (e.g. locked): recorded like the others. One item's
    failure never stops the poll's other cleanups.
- Waits while the agent is in the middle of a turn (working, or waiting for permission): not a
  blocker, nothing is recorded or told; the first poll after the turn cleans up, and Done is
  refused until then.
- Removes, in this order: the agent process; the worktree **if Ariekei created it** and no
  other item works in it (`git worktree remove` without `--force`, so git refuses too if
  something changed; `--force` only for initialized submodules, which git always refuses and
  whose changes `git status` already showed; `git worktree prune` if the folder was deleted by
  hand) and its branch if Ariekei created that too (`branch_created`: a reused local branch is
  kept); its Claude session transcripts, also those of conversations before a `/clear` (and
  subagent transcripts), last, so a refused removal leaves the agent resumable;
  `refs/ariekei/pull/<N>/head`; the context file; the Inbox entry (hidden; opening the PR again
  from search brings it back, and so does a poll that finds it reopened or with a new reason: a
  review request or an assignment it did not have when last found). Worktrees Ariekei adopted
  are only detached. Ignored files go with the worktree.
- Only GitHub's own state triggers automatic cleanup: a PR that merely dropped out of a search
  is never treated as finished. The poll's searches read all pages of results (up to 10); when
  one stops short, items of its kind that it did not find stay as they were.

## Testing

- Unit tests: issue parsing, branch names, issue worktree (from the default branch, push to
  the fork), cleanup blockers on a real repo (clean, busy agent, uncommitted file, unpushed
  commit, pushed), transcript removal, issue context.
- E2E: `e2e/lifecycle_scenario.py` (fake GitHub, `ARIEKEI_POLL_SECS=3`): issue in the Inbox →
  Start working → Done refused (uncommitted file + unpushed commit) → review before pushing →
  `git push` → no PR opened → a PR opened for the branch, item becomes #100 → merged → worktree and branch removed, agent gone, item hidden; an adopted
  worktree kept when its PR is merged.
- Unit tests for tasks: numbering, polls leaving tasks open, the task worktree and context, a
  task on a fork's branch (its remote added or reused, the push going back, a missing branch
  leaving nothing behind, a local branch of that name kept), the ways to name a branch, the PR
  for a task's branch added once, which tool calls create a PR or issue
  (`gh pr create` output, `gh issue create`, an MCP tool, another repo's URL, `gh pr view`),
  what a task's agent opens keeping its origin (the conversation without tool calls, cut when
  long), and old links turned into origins.
- E2E: `e2e/task_scenario.py`: New task → worktree, notes in the context, agent started →
  the agent opens PR #100 and issue #101, both in their own sections → Start working on #100
  adopts the task's checkout, its agent's context says how it came about, PR feedback goes to
  it, Done takes it out → a closed issue is cleaned up → Discard lists the uncommitted file,
  then the worktree, branch and task are gone → a task on `friend:feature-x` (a fork): the
  branch checked out tracking the fork's remote, the context naming it, and its PR, opened on
  GitHub, added to the Inbox.
- Live, read-only on besu: assigned issues, issue conversation, default branch.

## Not covered yet

- Choosing the base branch of an issue's branch, or the push remote.
- Cleaning up items of repos that are not the active one.
