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
