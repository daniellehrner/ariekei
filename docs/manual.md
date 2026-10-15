# Ariekei user manual

Ariekei is a review workbench for coding agents. You pick a GitHub pull request, issue or task;
Ariekei gives it its own git worktree and its own Claude Code session; the agent writes the
code and you review it in Ariekei. The Inbox tells you which item needs you next.

Contents:

1. [Before you start](#1-before-you-start)
2. [Connecting a repository](#2-connecting-a-repository)
3. [The window at a glance](#3-the-window-at-a-glance)
4. [The Inbox](#4-the-inbox)
5. [Work items](#5-work-items)
6. [Working with the agent](#6-working-with-the-agent)
7. [Reviewing changes](#7-reviewing-changes)
8. [Comments and GitHub reviews](#8-comments-and-github-reviews)
9. [Issues: from issue to PR](#9-issues-from-issue-to-pr)
10. [Tasks: work without a PR or issue](#10-tasks-work-without-a-pr-or-issue)
11. [Finishing and cleanup](#11-finishing-and-cleanup)
12. [Notifications](#12-notifications)
13. [Several repositories and windows](#13-several-repositories-and-windows)
14. [Keyboard shortcuts](#14-keyboard-shortcuts)
15. [Colours and symbols](#15-colours-and-symbols)
16. [Where your data lives](#16-where-your-data-lives)
17. [Troubleshooting and limits](#17-troubleshooting-and-limits)

---

## 1. Before you start

You need, on your `PATH`:

- `git`
- the GitHub CLI `gh`, logged in (`gh auth login`). Ariekei uses your `gh` login for all
  GitHub access; it has no login of its own.
- Claude Code (`claude`). Ariekei runs your installed `claude`, unmodified, with your own
  settings and plan.

You also need a local clone of the repository you want to work on, with a GitHub remote.

## 2. Connecting a repository

On first start, Ariekei shows **Connect a repository**:

- **Choose folder…** opens a folder picker, or type a path into the field and press
  **Connect**. Any folder inside the clone works, including one of its worktrees; Ariekei finds
  the main checkout.
- Ariekei finds the GitHub repository from the clone's remotes: `origin`, else `upstream`,
  else any GitHub remote.
- **Connected before** lists repositories you connected earlier. Click one to show it in this
  window. If it is already open in another window, **Switch to window** brings that window to
  the front instead.

Each window shows one repository, and a repository is shown in at most one window at a time.

## 3. The window at a glance

- **Left: the Inbox.** Your work items, by section and by what needs you. Collapse and expand
  it with **Ctrl+B** (Cmd+B on macOS) or the sidebar button in its header. Collapsed, it shows
  a column of coloured pips, one per item that needs attention, most urgent first.
- **Right: the selected work item**, with a header (title, state, actions) and the tabs
  **Overview**, **Comments**, **Review** and **Terminal**.
- **Notices** appear in the bottom right corner for events on other items while Ariekei is in
  front (see [Notifications](#12-notifications)).

Light and dark mode follow your system setting. Below a 1200 px wide window, the Inbox steps
aside by itself while the review's side panel is open; **Ctrl+B** brings it back.

## 4. The Inbox

### Sections

Each item is listed once, in the first section that fits:

| Section | What is in it |
|---|---|
| **Starred** | Items you starred, whatever else they are |
| **Tasks** | Your tasks (T1, T2, …), with the PRs and issues opened for them nested under them |
| **Assigned** | Pull requests and issues assigned to you |
| **Approved** | Your pull requests that are approved, i.e. ready to merge |
| **Review requested** | Pull requests that request your review |
| **My PRs** | Your other open pull requests |
| **Reviewed** | Pull requests you reviewed |
| **Open** | Pull requests and issues you added yourself, with **Browse…** to add more |

Empty sections are left out, except **Open**. Click a section's title to fold or unfold it.

An issue you started working on stays in the Inbox even if it is unassigned later. A pull
request you opened stays, too, e.g. after the review request is gone because you reviewed it.

### What a row shows

- A **status dot** for the item's agent (see [Colours and symbols](#15-colours-and-symbols)).
- The number and title. A violet diamond after the title means new reviews or comments since
  you last opened the item.
- A second line with the author (not in My PRs, Approved and Tasks), `from #N` for a PR that
  came from an issue, CI state (✓ passing, ✗ failing, pending), `draft`, `closed`, the review
  decision, the number of unresolved review threads, and the **attention reason** on the
  right. A task without PRs or issues says `no PR or issue yet`.
- On hover or focus: the **star**, and **×** (Remove) on items you added yourself that have no
  worktree. A set star is always shown.

### Starring

Click the star on a row, or the one after the title in the item's header, to keep an item at
the top of the Inbox under **Starred**. Click it again to unstar it. A task's PRs and issues
stay under the task, so you star the task.

### Order

Within a section, items are ordered by their most urgent reason, then by last update:

1. **Agent needs you**: the agent is waiting for your permission.
2. **Cleanup blocked** (a finished item that cannot be cleaned up without losing work) and
   **CI failing**.
3. **New activity** (new reviews or comments) and **All closed** (a task whose PRs and issues
   are all closed).
4. **Agent working**.
5. Everything else.

### Refreshing

The Inbox polls GitHub every 60 seconds. The **↻** button in the Inbox header refreshes now;
its arrow turns until GitHub has answered. Polling is designed to be cheap on GitHub's API
budget; if the budget is used up anyway, the Inbox says so and waits for GitHub's reset. If a
refresh fails, a "Could not refresh from GitHub" line shows the error.

### Browse: adding PRs and issues

**Browse** lists the repository's **open** pull requests and issues, so you can add any of
them to the Inbox. Open it with **Ctrl+K** (Cmd+K on macOS), the field at the top of the Inbox,
or **Browse…** in the Open section.

- Switch between **Pull requests** and **Issues** at the top.
- Search with GitHub's search syntax: words, `#123`, `author:name`, `label:bug`, and so on.
  Without a search, the 50 most recently updated are listed.
- **Add to Open** adds an item to the Inbox's **Open** section. Browse stays open, so you can
  add several.
- An item already in the Inbox is marked `in Inbox`, with **Open** to show it and, for one you
  added, **Remove** to take it out again.
- **↑** / **↓** move through the list, **Enter** adds the item (or opens it, when it is already
  in the Inbox), **Esc** closes Browse.

## 5. Work items

A work item is a pull request, an issue or a task. Selecting it shows its header and tabs.

### The header

- Kind, number and title, and the star.
- A meta line: kind, `from issue #N` or `from task Tn` (click to open the task), author, CI
  state, review decision, unresolved threads, the worktree path (once there is one) and the
  agent's status.
- Actions:
  - **Start working** creates (or reuses) a worktree and starts the agent in it. Shown when
    the item has no worktree yet.
  - **Done** cleans the item up (see [Finishing and cleanup](#11-finishing-and-cleanup)).
  - **Discard** (tasks only) deletes a task even if work would be lost, after showing what.
- **Send PR feedback to agent** (on the tab bar, for PRs with an agent) sends the PR's current
  review threads, review summaries and failing checks to the agent as a message.

### Tabs

| Tab | Content |
|---|---|
| **Overview** | State, branches (`author wants to merge head into base`), labels, description, reviewers, checks (click a check to open it), the discussion and a link to GitHub. For a task: what to look into, and its linked PRs and issues. |
| **Comments** | Review threads by file, unresolved first, with their diff snippet; the conversation; reply, resolve and comment. Not shown for tasks. |
| **Review** | The diff review (see [Reviewing changes](#7-reviewing-changes)). |
| **Terminal** | The agent, running in the item's worktree. Only with a worktree. |

An item opens on the tab that deals with why it needs you: agent needs you → Terminal, new
activity → Comments, CI failing, cleanup blocked or all closed → Overview. Otherwise it opens
on the tab you last used for it.

Each item remembers its tab, review scope, selected file and draft comments, so you can switch
between items freely.

### Starting work on a pull request

**Start working** on a PR creates `../<clone>_pr<N>` next to your clone, on the PR's branch and
tracking the PR head, so a plain `git push` in the worktree updates the PR. For PRs from forks,
the fork's remote is added the way `gh pr checkout` does it.

If a worktree with the PR's branch (or `pr-<N>`, or a branch tracking the PR head) already
exists, Ariekei reuses it. A reused branch is fast-forwarded to the PR head; if it has commits
of its own it is left as it is and Ariekei tells you.

You do not need a worktree to review someone else's PR: opening any PR fetches its head
read-only, and the Review tab works on that.

## 6. Working with the agent

### The Terminal tab

The Terminal tab is your installed `claude`, running in the item's worktree. Everything you
know from Claude Code works there, including slash commands. Keys typed in the terminal belong
to Claude Code, not to Ariekei.

The first time Claude Code sees a repository, it asks whether to trust it, with "No, exit"
preselected. Choose to trust it to continue.

### Context the agent gets

A new conversation starts with the item as context:

- **PR:** the description, unresolved review threads with file and line, review summaries, and
  failing checks with the failing part of the Actions job log.
- **Issue:** the issue's text and comments.
- **Task:** its title and notes, and the branch it works on.

The agent then waits for your instructions. The context is fixed at the conversation's first
request; use **Send PR feedback to agent** to give it the PR's current state later.

You can also send the agent review comments and questions about code from the Review tab (see
[Asking the agent about code](#asking-the-agent-about-code)).

### Agent status

The status dot and label come from Claude Code's hooks:

| Status | Meaning |
|---|---|
| Starting | Claude Code is starting |
| Ready | Waiting for your instructions |
| Working | In the middle of a turn |
| Waiting for permission | Asks to run something; answer in the Terminal |
| Finished | Finished its turn |
| Error | Stopped with an error |
| Stopped | Not running (it resumes when you open the item) |

While the agent waits for permission, a strip under the header shows what it asks to run, with
**Answer in Terminal** if you are on another tab.

### Switching items and restarting

Terminals keep running while you look at other items. A terminal nobody looks at whose agent
has been waiting for 15 minutes is closed, unless the agent still runs processes of its own
(e.g. shells it started in the background, which would end with it); opening the item again
resumes the conversation. Restarting Ariekei does the same: conversations continue where they
were.

## 7. Reviewing changes

The Review tab shows what changed, and collects comments for GitHub or the agent.

### Header

- Where the diff comes from: **Worktree** (with a worktree; includes uncommitted changes) or
  **PR head** (read-only), the number of commits vs the base branch, and a warning for
  uncommitted changes.
- **↻ Reload**.
- Buttons to show or hide the side panel and to open the keyboard shortcuts.

### Scope

In the side panel under **Scope**:

- **All changes vs `<base>`**: from the merge base to the head, like GitHub's Files tab.
- **A single commit**: pick one from the list, to review a change in small steps.

When the Scope section is folded, it shows the active scope.

### Files

The file list has two orders:

- **Review order** (default): Source (grouped by directory) → Tests → Config & build →
  Generated, lock & vendored (folded). Files marked `linguist-generated` or `linguist-vendored`
  in `.gitattributes` count as generated.
- **Tree**: alphabetical.

Each file has a status letter (added, modified, deleted, …) and, if only whitespace changed, a
`whitespace` label.

### The diff

- Side-by-side or unified (**u**). Unified is the default when the diff column is narrower than
  720 px.
- Hide or show whitespace changes (**w**).
- Syntax highlighting for any language; unchanged code is collapsed. Code is shown as written
  (no font ligatures, so `->` or `<=` never turn into arrows).
- **n** / **p** jump to the next or previous change. Past a file's last change, **n** goes on
  to the next file's first one, and **p** back to the previous file's last one (files in
  folded groups are skipped). At either end of the scope a note says there is no change
  further on.
- **]** / **[** go to the next or previous file.
- With the side panel hidden, the toolbar shows the current file, its position and up/down
  buttons for files.
- When you review the worktree, the right side is editable: change the code and press
  **Save `<file>`**.

### Asking the agent about code

Right-click a line, or a selection of lines on either side of the diff, for the code menu
(the ContextMenu key opens it too):

- **Ask agent…** opens a prompt box below the lines. Type a question or ask for a change and
  press **Send to agent** (Enter; Shift+Enter starts a new line). The agent gets the file, the
  lines, which version they are from (old or new side, the commit, or the working tree with
  unsaved edits), the code itself (up to 80 lines) and your prompt as written. **a** asks
  about the current selection.
- **Comment on lines** (new side only) starts a draft comment.
- **Copy** copies the code.

After sending, the agent's Terminal opens, where the answer appears; the Review tab keeps your
place. For a PR that a task works on, the question goes to the task's agent and the task's
Terminal opens. While the agent waits for permission nothing can be sent, since the text would
answer its prompt; while it works, it reads the question when its turn ends.

### Commenting

1. Select code on the right (new) side of the diff (or use **Comment on lines** in the code
   menu).
2. Type into **Comment on these lines** and press **Add comment**. **Esc** drops the selection.
3. Drafts collect in **Draft comments** at the bottom, per item. Remove one with its trash
   button.
4. Send them:
   - **Send to the agent** (items with an agent) delivers all drafts to the agent as one
     message.
   - **Submit to GitHub…** opens a form with an optional **Summary** and the event
     **Comment**, **Approve** or **Request changes** (only **Comment** on your own PR). The
     form tells you which drafts become line comments (lines in the PR's diff at its head
     commit) and which go into the review text instead. **Submit to GitHub** sends it.

With no drafts, **Review on GitHub…** submits a review with just a summary and event, e.g. an
approval.

## 8. Comments and GitHub reviews

The **Comments** tab shows:

- **Review threads**, unresolved first, each with the file and line, an `outdated` label where
  it applies, the last lines of the diff it refers to, and its comments.
  - **Reply** → **Reply on GitHub**.
  - **Resolve** / **Unresolve** (resolved threads are folded; click to open).
  - **Show in Review** opens the file in the Review tab.
- The **Conversation**: comments and reviews in order.
- **New comment** → **Comment on GitHub**.

Markdown is rendered by GitHub and sanitized; links open in your system browser.

## 9. Issues: from issue to PR

1. Issues assigned to you appear under **Assigned**; add any other open issue with
   [Browse](#browse-adding-prs-and-issues). Overview and Comments show the issue.
2. **Start working** creates the branch `issue-<N>-<words of the title>` from the default
   branch, in `../<clone>_issue<N>`. A plain `git push` creates the branch on your fork (a
   remote pointing at `<you>/<repo>`), or on the base remote when you have no fork.
3. The agent gets the issue as context and waits for you. The Review tab works on the worktree
   before anything is pushed.
4. Ariekei opens no PR itself. Open one for the pushed branch, or ask the agent to
   (`gh pr create`). On the next poll the item becomes that PR ("from issue #N") and keeps its
   worktree, agent conversation and view state. The issue is not listed again while the PR is
   open; if the PR is closed without merging, the issue comes back.

## 10. Tasks: work without a PR or issue

A task is work that starts with nothing on GitHub, e.g. investigating a bug. It may end as one
or more PRs and issues, or be thrown away.

### Creating a task

Press **Ctrl+N** (Cmd+N on macOS) or **+** in the Inbox header. The form has:

- a **title**;
- optionally an **existing branch** (see below);
- optionally **what to look into**; the agent gets it as context with the title.

**Start task** creates the branch `task-<n>-<words of the title>` from the default branch in
`../<clone>_task<n>` (set up for `git push` like an issue's) and starts the agent. Tasks are
numbered per repository: T1, T2, …

### A task on an existing branch

To work on a branch that already exists, e.g. on a fork, with no PR yet, give the task that
branch:

- `owner:branch` (a fork with the repository's name),
- `owner/repo:branch`,
- a GitHub branch URL (`https://github.com/owner/repo/tree/<branch>`), or
- the name of a branch of the repository itself.

Ariekei uses the clone's remote for that repository, or adds one named after the owner (as
`gh pr checkout` does), fetches the branch and checks it out in the task's worktree, tracking
it, so a plain `git push` updates it. Without a title, the task is named after the branch. If
a local branch of that name tracks something else, it is left alone and the worktree gets a
`task-<n>-…` branch instead. If the branch cannot be fetched, no task is created. The review
compares the branch with the default branch.

### PRs and issues from a task

Ask the agent to open them when the work is ready. When it runs `gh pr create` or
`gh issue create` (or a GitHub tool that creates a pull request or issue), Ariekei links the
result to the task. A PR opened for the task's pushed branch some other way (in the browser,
or by the fork's owner) is linked on the next poll.

- Linked PRs and issues are listed under the task in the Inbox (also when closed or merged)
  and in the task's Overview, not in their own sections.
- A linked PR has no worktree of its own. Its Review tab reads the PR head; **Send PR feedback
  to agent**, review comments and questions about code go to the task's agent. Its header says
  "from task Tn".

### Ending a task

- **Done** cleans the task up like any item (refused if work would be lost) and deletes it.
  Its PRs and issues go back to their own sections.
- **Discard** first lists what would be lost (uncommitted changes, commits not on GitHub),
  then, after you confirm, removes the worktree and branch, the agent session and the task.
  Refused only while the agent is in the middle of a turn.

Tasks are never cleaned up automatically. When all of a task's PRs and issues are closed, the
Inbox shows **All closed** and a notice suggests marking it done.

## 11. Finishing and cleanup

Items are cleaned up:

- **automatically** when GitHub reports the PR merged or closed, or the issue closed (never
  just because a PR dropped out of a search result), and
- **on Done**, at any time, after you confirm (**Clean up**).

Cleanup is **refused while anything would be lost**. The reasons are shown on the item, and a
finished item says "Cleanup blocked" in the Inbox:

- uncommitted changes, including untracked files;
- commits that are neither on a remote branch nor in GitHub's copy of the PR head (so
  squash-merged PRs with deleted branches are recognised as safe);
- an agent in the middle of a turn.

Commit and push, or discard the changes, then press **Done** again.

Cleanup removes the agent process and its Claude Code session transcript, the worktree and its
branch **if Ariekei created them** (worktrees Ariekei adopted are only detached), Ariekei's ref
for the PR head, its context file, and the Inbox entry. Adding the PR or issue again with
Browse brings it back while it is open.

## 12. Notifications

Ariekei notifies you when:

- an agent waits for permission, finishes a turn or stops with an error;
- a PR's CI starts failing;
- there are new reviews or comments;
- something happens in an item's lifecycle: an issue now follows its PR, a PR is linked to a
  task, an item was cleaned up or kept so no work is lost, or a task's PRs and issues are all
  closed.

Nothing is shown for the item you are looking at in a focused window. While an Ariekei window
is in front, notices appear inside the window, in the bottom right corner; otherwise they are
OS notifications.

Click a notice to open its item: on its Terminal when the notice is from the agent (finished,
waiting for permission, stopped with an error), otherwise on the tab that deals with it. **×**
dismisses it. A permission request stays until the agent moves on; the others go after ten
seconds.

## 13. Several repositories and windows

Click the repository name at the top of the Inbox to open the repository menu:

- **Connected repositories**: pick one to show it in this window. One open in another window
  shows "open in another window"; picking it switches to that window. The window button next
  to an entry opens it in a new window.
- **Connect another repository…**
- **New window** (**Ctrl+Shift+N** / Cmd+Shift+N).

Every open repository is kept up to date, and the windows come back after a restart.

## 14. Keyboard shortcuts

Press **?** to see them in the app. On macOS, use Cmd instead of Ctrl.

**Anywhere**

| Key | Action |
|---|---|
| Ctrl+N | New task |
| Ctrl+B | Show / hide the Inbox |
| Ctrl+K | Browse open PRs and issues |
| Ctrl+Shift+N | New window |
| ? | Keyboard shortcuts |
| Esc | Close the dialog or overlay |

**In the review**

| Key | Action |
|---|---|
| n / p | Next / previous change, on into the next / previous file |
| ] / [ | Next / previous file |
| u | Unified / side-by-side |
| w | Hide / show whitespace |
| a | Ask the agent about the selection |
| ContextMenu | The code menu (or right-click code) |
| Esc | Drop the selection (comment) |

**In Browse:** ↑ / ↓ move, Enter adds or opens the item, Esc closes.

Shortcuts do nothing while you type in a field, in the editable diff or in the terminal. Every
control is reachable with Tab.

## 15. Colours and symbols

Ariekei keeps three sources of state apart by shape and colour:

| Who | Shape | Colour | Examples |
|---|---|---|---|
| The agent | circle | copper | status dot, Terminal tab, waiting strip, **Ask agent…**, **Send to the agent** |
| You | square | blue | draft comments, the star, the selected item and file, focus |
| GitHub | diamond, outlined pill | violet | new activity, CI and review facts, **Submit to GitHub** |

Red means broken (CI failing, errors, work that would be lost), amber caution or pending, green
done or passing. Everything else is grey.

**Agent status dot:** a dotted grey ring is no agent; a dashed ring is starting; a plain ring
is ready; a turning ring with a dot inside is working; a filled ring with a halo is waiting for
permission; a filled ring with a hole is finished; a red disc is an error; a grey ring with a
small square is stopped.

**Notices** have the colour of their kind: copper when the agent needs you, blue when it
finished (your review is next), red for CI failing or an error, violet for new GitHub
activity.

## 16. Where your data lives

- Ariekei's state (work items, stars, view state, drafts) is in an SQLite database
  `ariekei.db` in the app data directory.
- Worktrees are created next to your clone (`../<clone>_pr<N>`, `_issue<N>`, `_task<n>`).
- Branches and Ariekei's refs live in your clone: `refs/ariekei/pull/<N>/head` holds the PR
  head fetched for read-only review.
- Agent conversations are Claude Code's own sessions.

## 17. Troubleshooting and limits

- **"Could not connect"**: the folder is not a git clone, or the clone has no GitHub remote,
  or `gh` is not logged in.
- **"Could not refresh from GitHub"**: check `gh auth status` and your network. If GitHub's API
  budget is used up, the Inbox says so and waits for the reset.
- **The agent exits immediately on a new repository**: Claude Code's trust prompt preselects
  "No, exit". Open the item again and choose to trust the folder.
- **Done is refused**: read the listed reasons; commit and push or discard the work, then press
  Done again.
- **Ask agent is refused**: the agent is waiting for your permission. Answer it in the
  Terminal first.

Not supported yet:

- Pending (multi-session) GitHub reviews, suggestions, comments on deleted lines, editing or
  deleting your comments, a markdown preview.
- Choosing the base branch or push remote for an issue's branch.
- Accepting or rejecting single hunks, build/test results next to the diff, and opening a file
  in your editor from the review (planned; see the README's Roadmap).
