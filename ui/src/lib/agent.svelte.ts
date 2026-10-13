// Per-worktree agent state, rebuilt from the backend's `agent-event` stream.
// Keeping every session's transcript in memory is what makes switching instant.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type AgentStatus =
  | "starting"
  | "ready"
  | "working"
  | "waitingForPermission"
  | "finished"
  | "error"
  | "stopped";

export type Entry =
  | { kind: "user" | "agent" | "thought"; text: string }
  | { kind: "tool"; id: string; title: string; status: string; toolKind: string };

export interface PermissionOption {
  id: string;
  name: string;
  kind: string;
}

export interface PendingPermission {
  requestId: string;
  title: string | null;
  options: PermissionOption[];
}

export interface Session {
  key: string;
  status: AgentStatus;
  detail: string | null;
  entries: Entry[];
  permission: PendingPermission | null;
  plan: { content: string; status: string }[];
}

type AgentEvent =
  | { type: "status"; key: string; status: AgentStatus; detail: string | null }
  | { type: "update"; key: string; update: Record<string, any> }
  | ({ type: "permission"; key: string } & PendingPermission)
  | { type: "permissionResolved"; key: string; requestId: string };

export const sessions: Record<string, Session> = $state({});

function session(key: string): Session {
  return (sessions[key] ??= {
    key,
    status: "stopped",
    detail: null,
    entries: [],
    permission: null,
    plan: [],
  });
}

function appendText(s: Session, kind: "user" | "agent" | "thought", content: any) {
  const text = content?.type === "text" ? content.text : "";
  if (!text) return;
  const last = s.entries.at(-1);
  if (last && last.kind === kind) last.text += text;
  else s.entries.push({ kind, text });
}

// Exported for tests: applies one raw ACP SessionUpdate to a session.
export function applyUpdate(s: Session, u: Record<string, any>) {
  switch (u.sessionUpdate) {
    case "user_message_chunk":
      return appendText(s, "user", u.content);
    case "agent_message_chunk":
      return appendText(s, "agent", u.content);
    case "agent_thought_chunk":
      return appendText(s, "thought", u.content);
    case "tool_call":
      s.entries.push({
        kind: "tool",
        id: u.toolCallId,
        title: u.title ?? "tool call",
        status: u.status ?? "pending",
        toolKind: u.kind ?? "other",
      });
      return;
    case "tool_call_update": {
      const t = s.entries.find((e) => e.kind === "tool" && e.id === u.toolCallId);
      if (t && t.kind === "tool") {
        if (u.title) t.title = u.title;
        if (u.status) t.status = u.status;
        if (u.kind) t.toolKind = u.kind;
      }
      return;
    }
    case "plan":
      s.plan = u.entries ?? [];
      return;
  }
}

let listening: Promise<unknown> | null = null;

export function listenToAgents() {
  listening ??= listen<AgentEvent>("agent-event", ({ payload: e }) => {
    const s = session(e.key);
    switch (e.type) {
      case "status":
        s.status = e.status;
        s.detail = e.detail;
        // A (re)starting session replays its history on load; start from scratch.
        if (e.status === "starting") Object.assign(s, { entries: [], plan: [], permission: null });
        if (e.status === "stopped" || e.status === "error") s.permission = null;
        break;
      case "update":
        applyUpdate(s, e.update);
        break;
      case "permission":
        s.permission = { requestId: e.requestId, title: e.title, options: e.options };
        break;
      case "permissionResolved":
        if (s.permission?.requestId === e.requestId) s.permission = null;
        break;
    }
  });
  return listening;
}

export interface Started {
  key: string;
  sessionId: string;
  loaded: boolean;
}

/** Starts the agent for a worktree; the returned key is its canonical path. */
export async function startAgent(cwd: string): Promise<Started> {
  await listenToAgents();
  return invoke<Started>("agent_start", { cwd });
}

export async function sendPrompt(key: string, text: string) {
  session(key).entries.push({ kind: "user", text });
  await invoke("agent_prompt", { key, text });
}

export const cancel = (key: string) => invoke("agent_cancel", { key });
export const stop = (key: string) => invoke("agent_stop", { key });

export async function answerPermission(key: string, requestId: string, optionId: string | null) {
  await invoke("agent_permission", { requestId, optionId });
  const s = session(key);
  if (s.permission?.requestId === requestId) s.permission = null;
}
