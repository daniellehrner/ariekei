// File lists of the review view: grouping, order, progress and the next file to look at.

import type { ReviewState } from "./api";

export type Group = "source" | "tests" | "config" | "generated";

export interface ReviewFile {
  path: string;
  status: string;
  group: Group;
  reviewed: boolean;
  whitespaceOnly: boolean;
}

export type Order = "review" | "tree";

export interface Section {
  key: string;
  title: string;
  // Shown folded until opened (generated, lock and vendored files).
  folded: boolean;
  files: ReviewFile[];
}

const GROUP_TITLES: Record<Group, string> = {
  source: "Source",
  tests: "Tests",
  config: "Config & build",
  generated: "Generated, lock & vendored",
};

// Files arrive in review order from the backend (source by directory, tests, config,
// generated); "tree" is plain path order.
export function sections(files: ReviewFile[], order: Order): Section[] {
  if (order === "tree") {
    return [{ key: "tree", title: "Files", folded: false, files: [...files].sort((a, b) => a.path.localeCompare(b.path)) }];
  }
  return (Object.keys(GROUP_TITLES) as Group[])
    .map((g) => ({ key: g, title: GROUP_TITLES[g], folded: g === "generated", files: files.filter((f) => f.group === g) }))
    .filter((s) => s.files.length);
}

// The files in the order they are shown.
export function flatten(list: Section[]): ReviewFile[] {
  return list.flatMap((s) => s.files);
}

export function progress(files: ReviewFile[]) {
  return { reviewed: files.filter((f) => f.reviewed).length, total: files.length };
}

// The next unreviewed file after `current` (wrapping around), or null when all are reviewed.
export function nextUnreviewed(ordered: ReviewFile[], current: string | null): string | null {
  const start = Math.max(ordered.findIndex((f) => f.path === current), -1);
  for (let i = 1; i <= ordered.length; i++) {
    const f = ordered[(start + i) % ordered.length];
    if (!f.reviewed && f.path !== current) return f.path;
  }
  return null;
}

// The scope shown: the one chosen if it still exists, else "review" (since the user's
// review) when there is one, else "all" (vs the base branch). A commit sha is a scope too.
export function scopeOf(review: ReviewState | null, wanted: string): string {
  if (!review) return "all";
  if (wanted === "review" && review.reviewed) return "review";
  if (wanted && wanted !== "all" && wanted !== "review" && review.commits.some((c) => c.sha === wanted)) return wanted;
  if (!wanted && review.reviewed) return "review";
  return "all";
}

// [from, to] of a scope's diff; to = null is the working tree (worktree mode only).
export function rangeOf(review: ReviewState, scope: string): [string, string | null] {
  const head = review.mode === "worktree" ? null : review.head;
  if (scope === "all") return [review.base, head];
  if (scope === "review") return review.reviewed ? [review.sinceReviewFrom ?? review.reviewed, head] : [review.base, head];
  return [`${scope}^`, scope];
}
