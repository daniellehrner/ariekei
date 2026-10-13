// Typed wrappers around the backend commands (src-tauri/src/lib.rs).

import { Channel, invoke } from "@tauri-apps/api/core";
import type { AgentStatus } from "./agent.svelte";
import type { Hunk } from "./gitDiff";

export interface Repo {
  id: number;
  path: string;
  owner: string;
  name: string;
  baseRemote: string;
  viewer: string;
}

export interface Row {
  id: number;
  number: number;
  title: string;
  url: string;
  isDraft: boolean;
  open: boolean;
  updatedAt: string;
  ci: string | null;
  failingChecks: string[];
  reviewDecision: string | null;
  unresolvedThreads: number;
  newActivity: boolean;
  agent: AgentStatus | null;
  worktree: string | null;
  unreviewed: boolean;
  attention: string | null;
}

export interface Prepared {
  path: string;
  branch: string;
  adopted: boolean;
  note: string | null;
}

export interface Commit {
  sha: string;
  subject: string;
  author: string;
  date: string;
}

export interface ReviewState {
  worktree: string;
  marker: string;
  markerKind: "reviewed" | "base";
  head: string;
  commits: Commit[];
  dirty: boolean;
}

export interface ChangedFile {
  path: string;
  status: string;
}

export interface FileDiff {
  old: string;
  new: string;
  binary: boolean;
  hunks: Hunk[];
}

export const api = {
  repo: () => invoke<Repo | null>("repo"),
  connectRepo: (path: string) => invoke<Repo>("connect_repo", { path }),
  inbox: () => invoke<Row[]>("inbox"),
  refresh: () => invoke<void>("refresh"),
  markSeen: (id: number) => invoke<void>("mark_seen", { id }),
  setActiveItem: (id: number | null) => invoke<void>("set_active_item", { id }),
  uiState: <T>(id: number) => invoke<T | null>("ui_state", { id }),
  setUiState: (id: number, value: unknown) => invoke<void>("set_ui_state", { id, value }),
  prepareItem: (id: number) => invoke<Prepared>("prepare_item", { id }),
  termOpen: (id: number, cols: number, rows: number, output: Channel<ArrayBuffer>) =>
    invoke<{ sessionId: string; resumed: boolean; reattached: boolean }>("term_open", { id, cols, rows, output }),
  termDetach: (id: number, view: number) => invoke<void>("term_detach", { id, view }),
  termWrite: (id: number, data: string) => invoke<void>("term_write", { id, data }),
  termResize: (id: number, cols: number, rows: number) => invoke<void>("term_resize", { id, cols, rows }),
  agentSend: (id: number, text: string) => invoke<void>("agent_send", { id, text }),
  sendPrFeedback: (id: number) => invoke<void>("send_pr_feedback", { id }),
  reviewState: (id: number) => invoke<ReviewState>("review_state", { id }),
  markReviewed: (id: number) => invoke<string>("mark_reviewed", { id }),
  reviewFiles: (id: number, from: string, to: string | null) => invoke<ChangedFile[]>("review_files", { id, from, to }),
  reviewFile: (id: number, from: string, to: string | null, path: string) =>
    invoke<FileDiff>("review_file", { id, from, to, path }),
  selfRssKib: () => invoke<number | null>("self_rss_kib"),
};
