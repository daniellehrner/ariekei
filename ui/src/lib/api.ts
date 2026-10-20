// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

// Typed wrappers around the backend commands (src-tauri/src/lib.rs).

import { Channel, invoke } from "@tauri-apps/api/core";
import type { AgentStatus } from "./agent.svelte";
import type { Hunk } from "./gitDiff";
import type { LineComment } from "./reviewComments";
import type { ReviewFile } from "./review";

export interface Repo {
  id: number;
  path: string;
  owner: string;
  name: string;
  baseRemote: string;
  viewer: string;
}

/** A connected repo, and whether this window or another one shows it. */
export interface RepoEntry extends Repo {
  here: boolean;
  elsewhere: boolean;
}

/** `here` false: another window already shows the repo and got the focus instead. */
export interface Shown {
  repo: Repo;
  here: boolean;
}

export interface Row {
  id: number;
  /** A task's number is negative: task T1 is -1 (see `shortName`). */
  number: number;
  kind: "pr" | "issue" | "task";
  issueNumber: number | null;
  cleanupBlockers: string[] | null;
  title: string;
  author: string;
  /** Why it is in the Inbox, the strongest reason (see `poller::role_of`). */
  role: "assignee" | "author" | "requested" | "reviewed" | "other";
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
  /** The agent finished since the user last opened the item's review. */
  readyForReview: boolean;
  worktree: string | null;
  /** The item's local branch, once it has a worktree. */
  branch: string | null;
  /** The branch its worktree is on instead ("": a detached HEAD), when someone switched it;
   *  the agent gets nothing until it is back. */
  movedTo: string | null;
  /** Its worktree was there before (e.g. the user's own checkout): cleanup keeps it. */
  adopted: boolean;
  attention: string | null;
  /** For a PR or issue a task's agent opened: the task. */
  taskId: number | null;
  /** Whose agent works on this item: its own (it has a worktree), or its task's. */
  agentItem: number | null;
  /** A task's notes (what to look into). */
  notes: string | null;
  /** A task whose PRs and issues are all closed or merged (it has some). */
  outcomesClosed: boolean;
  /** The user starred it: it heads the Inbox. */
  starred: boolean;
  /** The user added it and can take it out again. */
  removable: boolean;
}

/** `T1` for a task, `#12` for a PR or issue. */
export function shortName(row: Pick<Row, "kind" | "number">): string {
  return row.kind === "task" ? `T${-row.number}` : `#${row.number}`;
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
  /** How many files it changes. */
  files: number;
}

export interface ReviewState {
  mode: "worktree" | "ref";
  dir: string;
  base: string;
  baseRef: string;
  head: string;
  prHead: string;
  commits: Commit[];
  dirty: boolean;
  own: boolean;
}

export type Cleanup = { outcome: "cleaned"; removed: string[] } | { outcome: "blocked"; blockers: string[] };

export interface SearchHit {
  kind: "pr" | "issue";
  number: number;
  title: string;
  author: string;
  isDraft: boolean;
  labels: string[];
  updatedAt: string;
  /** The work item, if it is in the Inbox. */
  itemId: number | null;
  /** The user added it and can take it out again. */
  removable: boolean;
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
  /** The new version is not valid UTF-8 (`new` has U+FFFD for what is not): not editable. */
  lossy: boolean;
  hunks: Hunk[];
}

/** An agent whose work quitting would stop (quit.rs). */
export interface Running {
  item: number;
  /** As the Inbox names it: "T1 …", "#42 …". */
  name: string;
  repo: string;
  worktree: string | null;
  /** In the middle of a turn. */
  busy: boolean;
  /** What it started that still runs, e.g. its background shells. */
  processes: { pid: number; command: string }[];
}

export const api = {
  repo: () => invoke<Repo | null>("repo"),
  repos: () => invoke<RepoEntry[]>("repos"),
  connectRepo: (path: string) => invoke<Shown>("connect_repo", { path }),
  switchRepo: (id: number) => invoke<Shown>("switch_repo", { id }),
  /** The worktrees Ariekei made for repo `id` that removing it leaves on disk. */
  repoLeftovers: (id: number) => invoke<string[]>("repo_leftovers", { id }),
  /** Forgets repo `id` (its items, stars, drafts); nothing on disk is touched. */
  removeRepo: (id: number) => invoke<void>("remove_repo", { id }),
  /** Quits although agents' work stops (the user confirmed `confirm-quit`). */
  quitApp: () => invoke<void>("quit_app"),
  /** A new window for repo `id`, or an empty one (Connect screen) for null. */
  openWindow: (id: number | null) => invoke<void>("open_window", { id }),
  openItem: (id: number) => invoke<void>("open_item", { id }),
  inbox: () => invoke<Row[]>("inbox"),
  refresh: () => invoke<void>("refresh"),
  /** How far the user had seen the item's activity (its latest by others) before this, and now. */
  markSeen: (id: number) => invoke<{ before: string | null; after: string | null }>("mark_seen", { id }),
  reviewSeen: (agentItem: number) => invoke<void>("review_seen", { agentItem }),
  setActiveItem: (id: number | null) => invoke<void>("set_active_item", { id }),
  selectedItem: () => invoke<number | null>("selected_item"),
  uiSetting: (key: string) => invoke<string | null>("ui_setting", { key }),
  setUiSetting: (key: string, value: string) => invoke<void>("set_ui_setting", { key, value }),
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
  /** As the clone has it; `refresh`: fetched from GitHub first (Reload, or nothing to show). */
  reviewState: (id: number, refresh = false) => invoke<ReviewState>("review_state", { id, refresh }),
  /** Fetches what the review compares if GitHub may have newer; true if that moved it. */
  reviewFetch: (id: number, force = false) => invoke<boolean>("review_fetch", { id, force }),
  reviewFiles: (id: number, from: string, to: string | null, ignoreWs: boolean) =>
    invoke<ReviewFile[]>("review_files", { id, from, to, ignoreWs }),
  reviewFile: (id: number, from: string, to: string | null, path: string, ignoreWs: boolean) =>
    invoke<FileDiff>("review_file", { id, from, to, path, ignoreWs }),
  searchItems: (kind: "pr" | "issue", text: string) => invoke<SearchHit[]>("search_items", { kind, text }),
  trackItem: (kind: "pr" | "issue", number: number) => invoke<number>("track_item", { kind, number }),
  untrack: (id: number) => invoke<void>("untrack", { id }),
  setStarred: (id: number, starred: boolean) => invoke<void>("set_starred", { id, starred }),
  createTask: (title: string, notes: string, branch: string) => invoke<number>("create_task", { title, notes, branch }),
  conversation: (id: number) => invoke<Conversation>("pr_conversation", { id }),
  /** The conversation as it was last loaded: shown while `conversation` loads. */
  cachedConversation: (id: number) => invoke<Conversation | null>("cached_conversation", { id }),
  // `commit`: the PR head the review shows, which the drafts' lines are from (GitHub's may be
  // newer by now).
  commentRanges: (id: number, paths: string[], commit: string) =>
    invoke<[string, [number, number][]][]>("comment_ranges", { id, paths, commit }),
  submitReview: (id: number, event: string, body: string, comments: LineComment[], commit: string) =>
    invoke<string>("submit_review", { id, event, body, comments, commit }),
  replyComment: (id: number, commentId: number, body: string) => invoke<void>("reply_comment", { id, commentId, body }),
  postComment: (id: number, body: string) => invoke<void>("post_comment", { id, body }),
  resolveThread: (threadId: string, resolved: boolean) => invoke<void>("resolve_thread", { threadId, resolved }),
  markDone: (id: number) => invoke<Cleanup>("mark_done", { id }),
  discardLosses: (id: number) => invoke<string[]>("discard_losses", { id }),
  discardTask: (id: number) => invoke<Cleanup>("discard_task", { id }),
  // `loaded`: the file as the editor got it; the save is refused if it changed on disk since.
  saveFile: (id: number, path: string, content: string, loaded: string) => invoke<void>("save_file", { id, path, content, loaded }),
  selfRssKib: () => invoke<number | null>("self_rss_kib"),
};
