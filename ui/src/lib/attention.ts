// The Inbox's attention reasons (inbox.rs, most urgent first) as pip kinds and labels.

import type { Attention } from "../controls/AttentionPip.svelte";

const KIND: Record<string, Attention> = {
  "agent needs you": "needs-you",
  "finished, cleanup blocked": "blocked",
  "CI failing": "ci-failing",
  "new reviews or comments": "activity",
  "changes to review": "review",
  "agent working": "working",
};

export const ATTENTION_LABEL: Record<Attention, string> = {
  "needs-you": "Agent needs you",
  blocked: "Cleanup blocked",
  "ci-failing": "CI failing",
  activity: "New activity",
  review: "Changes to review",
  working: "Agent working",
  none: "",
};

export function attentionKind(reason: string | null): Attention {
  return (reason && KIND[reason]) || "none";
}

export type Tab = "overview" | "comments" | "review" | "terminal";

/** The tab that resolves the item's attention reason, if any: the agent needs you → Terminal,
 *  new comments → Comments, changes to review → Review, CI failing / cleanup blocked → Overview. */
export function landingTab(reason: string | null, has: { terminal: boolean; review: boolean }): Tab | null {
  switch (attentionKind(reason)) {
    case "needs-you":
      return has.terminal ? "terminal" : null;
    case "activity":
      return "comments";
    case "review":
      return has.review ? "review" : null;
    case "blocked":
    case "ci-failing":
      return "overview";
    default:
      return null;
  }
}
