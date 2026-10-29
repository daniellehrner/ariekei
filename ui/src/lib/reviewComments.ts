// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

// Draft review comments and where they can go: GitHub (as line comments when possible)
// or the agent. The user drafts them in the Review tab; the item's agent with Ariekei's tools
// (src-tauri/src/drafts.rs).

export interface DraftComment {
  path: string;
  fromLine: number;
  toLine: number;
  code: string;
  text: string;
  // The commit whose version of the file the lines refer to; null for uncommitted changes.
  rev: string | null;
}

// An existing review thread on GitHub: its GraphQL id, and who started it with what.
export interface ThreadRef {
  thread: string;
  author: string;
  text: string;
}

// A draft as stored, the user's or the agent's; `edited`: the user changed the agent's;
// `replyTo`: it answers in that thread (its lines are the thread's).
export interface ReviewDraft extends DraftComment {
  id: number;
  author: "user" | "agent";
  edited: boolean;
  replyTo: ThreadRef | null;
}

export const draftLines = (c: DraftComment) => (c.fromLine === c.toLine ? `${c.fromLine}` : `${c.fromLine}–${c.toLine}`);

/** The message for the agent about one draft: which, what it says, and the user's prompt. */
export function draftMessage(d: ReviewDraft, prompt: string): string {
  const by = d.author === "agent" ? (d.edited ? "drafted by you, edited by me" : "drafted by you") : "drafted by me";
  const quote = d.text.trim().split("\n").map((l) => `> ${l}`).join("\n");
  const what = d.replyTo ? `reply #${d.id} in ${d.replyTo.author}'s thread ${d.replyTo.thread}` : `review comment #${d.id}`;
  return `About ${what} on ${d.path}:${draftLines(d)} (${by}):\n\n${quote}\n\n${prompt.trim()}\n\nIf it should change, change it with update_review_comment.`;
}

export interface LineComment {
  path: string;
  line: number;
  start_line?: number;
  side: "RIGHT";
  body: string;
}

// A reply in an existing thread, as submitted with the review.
export interface ThreadReply {
  thread: string;
  body: string;
}

export type Ranges = Record<string, [number, number][]>;

const lines = (c: DraftComment) => (c.fromLine === c.toLine ? `${c.fromLine}` : `${c.fromLine}-${c.toLine}`);

// GitHub takes a line comment only on lines of the PR diff at its head commit. A draft
// made on another version of the file, or outside the diff's hunks, goes into the body.
// A reply goes into its thread, wherever that is.
export function classify(comments: (DraftComment & { replyTo?: ThreadRef | null })[], prHead: string, ranges: Ranges) {
  const line: LineComment[] = [];
  const body: DraftComment[] = [];
  const replies: ThreadReply[] = [];
  for (const c of comments) {
    if (c.replyTo) {
      replies.push({ thread: c.replyTo.thread, body: c.text.trim() });
      continue;
    }
    const inHunk = (ranges[c.path] ?? []).some(([a, b]) => c.fromLine >= a && c.toLine <= b);
    if (c.rev === prHead && inHunk) {
      line.push({
        path: c.path,
        line: c.toLine,
        ...(c.fromLine < c.toLine ? { start_line: c.fromLine } : {}),
        side: "RIGHT",
        body: c.text.trim(),
      });
    } else {
      body.push(c);
    }
  }
  return { line, body, replies };
}

export function reviewBody(summary: string, folded: DraftComment[]): string {
  const parts = [summary.trim()];
  if (folded.length) {
    const items = folded.map((c) => `- \`${c.path}:${lines(c)}\` — ${c.text.trim().replaceAll("\n", "\n  ")}`);
    parts.push(items.join("\n"));
  }
  return parts.filter(Boolean).join("\n\n");
}

export function feedbackMessage(comments: (DraftComment & { replyTo?: ThreadRef | null })[]): string {
  const items = comments.map((c, i) => {
    const quote = c.code.split("\n").slice(0, 6).map((l) => `   > ${l}`).join("\n");
    const thread = c.replyTo ? ` (answering ${c.replyTo.author}'s review thread there)` : "";
    return `${i + 1}. ${c.path}:${lines(c)}${thread}\n${quote}\n   ${c.text.trim().replaceAll("\n", "\n   ")}`;
  });
  return `Review feedback on your changes:\n\n${items.join("\n\n")}\n\nPlease address each point.`;
}

// Code the user points the agent at from the review (right-click → Ask agent).
export interface CodeRef {
  path: string;
  fromLine: number;
  toLine: number;
  code: string;
  // The diff's side: the version after the change, or the one before it.
  side: "new" | "old";
  // The commit of that version; null for the working tree.
  rev: string | null;
  // The new side has edits not saved yet.
  unsaved?: boolean;
}

export const MAX_ASK_LINES = 80;

export const refLines = (r: CodeRef) => (r.fromLine === r.toLine ? `${r.fromLine}` : `${r.fromLine}–${r.toLine}`);

/** Which version the lines are from, in words. */
export function versionOf(r: CodeRef): string {
  // A full sha is shortened; a revision like `<sha>^` keeps its suffix.
  const commit = r.rev ? `, commit ${r.rev.replace(/^([0-9a-f]{8})[0-9a-f]{32}/, "$1")}` : "";
  if (r.side === "old") return `old version, before the change${commit}`;
  if (r.unsaved) return "working tree, with unsaved edits";
  return r.rev ? `new version${commit}` : "working tree, uncommitted";
}

/** The message for the agent: where the code is, the code, and the user's prompt. */
export function askMessage(r: CodeRef, prompt: string): string {
  const all = r.code.replace(/\n+$/, "").split("\n");
  const shown = all.slice(0, MAX_ASK_LINES);
  const longest = Math.max(0, ...(r.code.match(/`+/g) ?? []).map((run) => run.length));
  const fence = "`".repeat(Math.max(3, longest + 1));
  const name = r.path.slice(r.path.lastIndexOf("/") + 1);
  const lang = name.includes(".") ? name.slice(name.lastIndexOf(".") + 1).toLowerCase() : "";
  const more = all.length > shown.length ? `\n… ${all.length - shown.length} more lines, see the file` : "";
  const where = `${r.path} ${r.fromLine === r.toLine ? "line" : "lines"} ${refLines(r)}`;
  return `About ${where} (${versionOf(r)}):\n\n${fence}${lang}\n${shown.join("\n")}\n${fence}${more}\n\n${prompt.trim()}`;
}
