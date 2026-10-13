// Typed wrappers around the backend commands (src-tauri/src/lib.rs).

import { Channel, invoke } from "@tauri-apps/api/core";
import type { AgentStatus } from "./agent.svelte";
import type { Hunk } from "./gitDiff";
import type { LineComment } from "./reviewComments";

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
  kind: "pr" | "issue";
  issueNumber: number | null;
  cleanupBlockers: string[] | null;
  title: string;
  author: string;
  role: "author" | "requested" | "assignee" | "other";
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
  afterReview: boolean;
}

export interface ReviewState {
  mode: "worktree" | "ref";
  dir: string;
  base: string;
  baseRef: string;
  reviewed: string | null;
  head: string;
  prHead: string;
  commits: Commit[];
  dirty: boolean;
  own: boolean;
}

export type Cleanup = { outcome: "cleaned"; removed: string[] } | { outcome: "blocked"; blockers: string[] };

export interface SearchHit {
  number: number;
  title: string;
  author: string;
  isDraft: boolean;
  updatedAt: string;
  itemId: number | null;
}

export interface GhComment {
  id: number;
  author: string;
  bodyHtml: string;
  createdAt: string;
  url: string;
}

export interface Thread {
  id: string;
  path: string;
  line: number | null;
  resolved: boolean;
  outdated: boolean;
  diffHunk: string;
  comments: GhComment[];
}

export interface Conversation {
  number: number;
  title: string;
  url: string;
  author: string;
  state: string;
  isDraft: boolean;
  createdAt: string;
  headRef: string;
  headSha: string;
  baseRef: string;
  reviewDecision: string | null;
  bodyHtml: string;
  labels: string[];
  requestedReviewers: string[];
  comments: GhComment[];
  reviews: { author: string; state: string; bodyHtml: string; submittedAt: string }[];
  threads: Thread[];
  checks: { name: string; outcome: string; url: string }[];
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
  selectedItem: () => invoke<number | null>("selected_item"),
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
  searchPrs: (text: string) => invoke<SearchHit[]>("search_prs", { text }),
  trackPr: (number: number) => invoke<number>("track_pr", { number }),
  conversation: (id: number) => invoke<Conversation>("pr_conversation", { id }),
  fetchPr: (id: number) => invoke<string>("fetch_pr", { id }),
  commentRanges: (id: number, paths: string[]) => invoke<[string, [number, number][]][]>("comment_ranges", { id, paths }),
  submitReview: (id: number, event: string, body: string, comments: LineComment[]) =>
    invoke<string>("submit_review", { id, event, body, comments }),
  replyComment: (id: number, commentId: number, body: string) => invoke<void>("reply_comment", { id, commentId, body }),
  postComment: (id: number, body: string) => invoke<void>("post_comment", { id, body }),
  resolveThread: (threadId: string, resolved: boolean) => invoke<void>("resolve_thread", { threadId, resolved }),
  markDone: (id: number) => invoke<Cleanup>("mark_done", { id }),
  saveFile: (id: number, path: string, content: string) => invoke<void>("save_file", { id, path, content }),
  selfRssKib: () => invoke<number | null>("self_rss_kib"),
};
