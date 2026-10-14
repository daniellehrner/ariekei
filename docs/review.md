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
  `.gitattributes`). Switch to **Tree** (alphabetical) or **Guide** order.
- **Agent review guide** (*Prepare review guide*): Claude reads the change headless (read-only
  tools, no saved session) and returns a short focus, the files grouped by concern in risk
  order with a one-line intent each, the description's and commit messages' claims checked
  against the diff (✓ implemented, ◐ partial, ✗ missing), and files no claim explains. It is
  marked outdated when the head moves. Takes seconds and uses your Claude plan. Above the
  diff, a line shows what the guide says about the file you are looking at (its group's
  intent and risk, the claims it backs, or that no claim explains it), so the guide is there
  with the side panel closed too; the chevron expands it and adds the focus for the whole change.
- **Noise and layout:** ignore whitespace (`w`, whitespace-only files labelled), unified or
  side-by-side (`u`; unified by default when the diff column is under 720 px), syntax
  highlighting for any language, git-exact hunks, unchanged code collapsed; the Inbox, the
  commit list and the side panel collapse (below a 1200 px window the Inbox steps aside by
  itself while the side panel is open).
- **Keys:** `n`/`p` next/previous change (the change you are on is marked in the gutter),
  `]`/`[` next/previous file, `r`, `u`, `w`, `?` for the list, `Esc` to drop a selection. They
  do nothing while you type or when the terminal has focus.
- **Comments** go to GitHub as a review (line comments where GitHub accepts them) or to the
  agent.

## Why

- **Where to start matters:** files shown later get fewer comments, and in an experiment a bug
  was 64% less likely to be found when its file came last
  ([Fregnan et al. 2022](https://arxiv.org/pdf/2208.04259)); ~90% of developers do not think
  alphabetical order is best ([ICSE 2026](https://arxiv.org/abs/2609.04207)). Hence the review
  order and the guide's risk order.
- **Track what you have seen, per file and per version:** the common pattern of Critique,
  Gerrit, Reviewable and GitHub ("viewed" resets when the file changes). Agents commit often,
  so per-file marks avoid re-reading files that did not change.
- **Diffs must survive rebases:** Gerrit and Reviewable separate base-only changes; GitHub's
  "changes since last review" breaks after a rebase.
- **A short focus beats checklists:** checklists help little
  ([Gonçalves et al.](https://link.springer.com/article/10.1007/s10664-022-10123-8)); being told
  what to look for made reviewers 8× more likely to find a vulnerability
  ([Braz et al.](https://dl.acm.org/doi/10.1145/3510003.3511560)).
- **Check the agent's claims:** in 23k agent PRs, the most common description problem was
  claiming changes that were not made ([2026](https://arxiv.org/abs/2601.04886)).
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
