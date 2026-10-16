# Reviewing in Ariekei

How the review view works, and the research it is based on.

## What it does

- **Scopes:** all changes vs the base branch (like GitHub's Files tab), since your review, or
  one commit. Commits newer than your review are highlighted.
- **Since your review, rebase-aware:** when the branch was rebased or merged with the base since
  you reviewed, your reviewed state is moved onto the new base (`git merge-tree`) before
  comparing, so changes that only came from the base are hidden ("Rebased since your review…").
  If the reviewed changes conflict with the new base, the plain diff is shown with a warning.
  Rebased copies of commits you already reviewed are not counted as new.
- **Your own commits are not "changes to review":** the Inbox flags a worktree only for
  uncommitted changes or commits someone else made since your review (or the branch point).
  Commits GitHub attributes to your account (author or committer, any of your emails) count
  as seen, and so does everything a merge of yours brought in, e.g. merged sub-PRs. Local
  commits GitHub does not know yet, like an agent's unpushed work, still need review.
- **Per-file reviewed marks:** a checkbox per file, `N/M reviewed` in the header, and
  **Reviewed, next** (`r`) to mark a file and jump to the next unreviewed one. A mark is tied
  to the file's content: when the agent changes a reviewed file again, it becomes unreviewed;
  files it did not touch keep their marks. **Mark reviewed** (the whole PR) marks every file,
  and asks first if files in the scope were not looked at.
- **Review order, not alphabetical:** Source (grouped by directory) → Tests → Config & build →
  Generated, lock & vendored (folded; includes `linguist-generated`/`linguist-vendored` from
  `.gitattributes`). Switch to **Tree** (alphabetical) order.
- **Noise and layout:** ignore whitespace (`w`, whitespace-only files labelled), unified or
  side-by-side (`u`; unified by default when the diff column is under 720 px), syntax
  highlighting for any language, git-exact hunks, unchanged code collapsed; the Inbox, the
  commit list and the side panel collapse (below a 1200 px window the Inbox steps aside by
  itself while the side panel is open).
- **Keys:** `n`/`p` next/previous change (the change you are on is marked in the gutter;
  past a file's last change `n` goes on to the next file's first one, `p` back to the previous
  file's last one, skipping folded sections; at the ends of the scope a note says there is no
  change further on),
  `]`/`[` next/previous file, `r`, `u`, `w`, `a` to ask the agent about the selection, `?` for
  the list, `Esc` to drop a selection. They do nothing while you type or when the terminal has
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
- **Comments** go to GitHub as a review (line comments where GitHub accepts them) or to the
  agent.

## Why

- **Where to start matters:** files shown later get fewer comments, and in an experiment a bug
  was 64% less likely to be found when its file came last
  ([Fregnan et al. 2022](https://arxiv.org/pdf/2208.04259)); ~90% of developers do not think
  alphabetical order is best ([ICSE 2026](https://arxiv.org/abs/2609.04207)). Hence the review
  order.
- **Track what you have seen, per file and per version:** the common pattern of Critique,
  Gerrit, Reviewable and GitHub ("viewed" resets when the file changes). Agents commit often,
  so per-file marks avoid re-reading files that did not change.
- **Diffs must survive rebases:** Gerrit and Reviewable separate base-only changes; GitHub's
  "changes since last review" breaks after a rebase.
- **Stay within a session's capacity:** defect detection falls above ~400 lines and ~500
  lines/hour, and attention fades after 60–90 minutes
  ([SmartBear/Cisco](https://smartbear.com/learn/code-review/best-practices-for-peer-code-review/)).
  Progress per file and since-review scopes keep each pass small.

## Not yet

- Tracking comments sent to the agent until they are verified (Done/Ack, like Critique and
  Gerrit).
- Highlighting weakened tests (removed assertions, loosened tolerances) and build/test results
  next to the diff (planned as "checks").
- Moved-code detection, and hunk-level accept/reject.
