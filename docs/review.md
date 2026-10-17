# Reviewing in Ariekei

How the review view works, and the research it is based on.

## What it does

- **Scopes:** all changes vs the base branch (like GitHub's Files tab), or one commit, picked
  from the scope menu in the review bar.
- **Review order, not alphabetical:** Source (grouped by directory) → Tests → Config & build →
  Generated, lock & vendored (folded; includes `linguist-generated`/`linguist-vendored` from
  `.gitattributes`). Switch to **Tree** (alphabetical) order.
- **Noise and layout:** ignore whitespace (`w`, whitespace-only files labelled), unified or
  side-by-side (`u`; unified by default when the diff column is under 880 px, and always for an
  added or deleted file), syntax highlighting for any language, git-exact hunks, unchanged code
  collapsed; the Inbox and the file panel collapse (below a 1200 px window the Inbox steps aside
  by itself once the file panel opens).
- **Keys:** `n`/`p` next/previous change (the change you are on is marked in the gutter;
  past a file's last change `n` goes on to the next file's first one, `p` back to the previous
  file's last one, skipping folded sections; at the ends of the scope a note by the change says
  there is no change further on),
  `]`/`[` next/previous file, `u`, `w`, `a` to ask the agent about the selection, `c` to comment
  on it, `?` for the list, `Esc` to drop a selection. They do nothing while you type or when the terminal has
  focus.
- **Ask the agent about code:** right-click a line, or a selection of lines on either side of
  the diff, and choose **Ask agent…** (the ContextMenu key or `a` works too). Your prompt goes to
  the item's agent (a PR worked on by a task goes to the task's) with the file, the lines, which
  version they are from (old or new side, the commit, or the working tree with unsaved edits)
  and the code itself (up to 80 lines). Then the agent's Terminal opens (the task's, for such
  a PR), where the answer appears; the Review tab keeps your place. While the agent waits for
  permission nothing can be sent, since the text would answer its prompt; while it works,
  Claude Code reads it after the turn. The prompt is sent as you wrote it: ask a question or
  ask for a change.
- **Comments** are drafted in the dock under the diff (their lines keep a mark in the gutter)
  and go to GitHub as a review (line comments where GitHub accepts them) or to the agent (not
  while it waits for permission).

## Why

- **Where to start matters:** files shown later get fewer comments, and in an experiment a bug
  was 64% less likely to be found when its file came last
  ([Fregnan et al. 2022](https://arxiv.org/pdf/2208.04259)); ~90% of developers do not think
  alphabetical order is best ([ICSE 2026](https://arxiv.org/abs/2609.04207)). Hence the review
  order.
- **Stay within a session's capacity:** defect detection falls above ~400 lines and ~500
  lines/hour, and attention fades after 60–90 minutes
  ([SmartBear/Cisco](https://smartbear.com/learn/code-review/best-practices-for-peer-code-review/)).
  Single-commit scopes keep each pass small.

## Not yet

- Tracking comments sent to the agent until they are verified (Done/Ack, like Critique and
  Gerrit).
- Highlighting weakened tests (removed assertions, loosened tolerances) and build/test results
  next to the diff (planned as "checks").
- Moved-code detection, and hunk-level accept/reject.
