// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

// The Inbox's sections. Each item is listed once, in the first section that fits; a task's PRs
// and issues are listed under the task, wherever the task is.

import type { Row } from "./api";

export type SectionKey = "starred" | "tasks" | "assigned" | "approved" | "requested" | "mine" | "reviewed" | "open";

export const SECTIONS: { key: SectionKey; title: string }[] = [
  { key: "starred", title: "Starred" },
  { key: "tasks", title: "Tasks" },
  { key: "assigned", title: "Assigned" },
  { key: "approved", title: "Approved" },
  { key: "requested", title: "Review requested" },
  { key: "mine", title: "My PRs" },
  { key: "reviewed", title: "Reviewed" },
  { key: "open", title: "Open" },
];

/** The section of a row that is not under a task. The backend's `role` is the strongest
 *  reason it is in the Inbox: assigned, written by the user, review requested, reviewed. */
export function sectionOf(row: Row): SectionKey {
  if (row.starred) return "starred";
  if (row.kind === "task") return "tasks";
  switch (row.role) {
    case "assignee":
      return "assigned";
    case "author":
      return row.kind === "pr" && row.reviewDecision === "APPROVED" ? "approved" : "mine";
    case "requested":
      return "requested";
    case "reviewed":
      return "reviewed";
    default:
      return "open";
  }
}

/** The rows of each section, in the Inbox's order (rows under a task are left out). */
export function bySection(rows: Row[]): Record<SectionKey, Row[]> {
  const out = Object.fromEntries(SECTIONS.map((s) => [s.key, [] as Row[]])) as Record<SectionKey, Row[]>;
  for (const row of rows) if (row.taskId == null) out[sectionOf(row)].push(row);
  return out;
}
