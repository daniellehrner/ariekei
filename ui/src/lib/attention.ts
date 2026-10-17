// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

// The Inbox's attention reasons (inbox.rs, most urgent first) as pip kinds and labels.

import type { Attention } from "../controls/AttentionPip.svelte";

const KIND: Record<string, Attention> = {
  "agent needs you": "needs-you",
  "finished, cleanup blocked": "blocked",
  "CI failing": "ci-failing",
  "new reviews or comments": "activity",
  "all its PRs and issues are closed": "all-closed",
  "agent working": "working",
};

export const ATTENTION_LABEL: Record<Attention, string> = {
  "needs-you": "Agent needs you",
  blocked: "Cleanup blocked",
  "ci-failing": "CI failing",
  activity: "New activity",
  // Only a notice's pip (the agent finished: your turn), no Inbox reason.
  review: "Agent finished",
  "all-closed": "All closed",
  working: "Agent working",
  none: "",
};

export function attentionKind(reason: string | null): Attention {
  return (reason && KIND[reason]) || "none";
}

export type Tab = "overview" | "comments" | "review" | "terminal";

/** The tab that resolves the item's attention reason, if any: the agent needs you → Terminal,
 *  new comments → Comments, CI failing / cleanup blocked / a
 *  task's PRs and issues all closed → Overview. */
export function landingTab(reason: string | null, has: { terminal: boolean }): Tab | null {
  switch (attentionKind(reason)) {
    case "needs-you":
      return has.terminal ? "terminal" : null;
    case "activity":
      return "comments";
    case "blocked":
    case "ci-failing":
    case "all-closed":
      return "overview";
    default:
      return null;
  }
}
