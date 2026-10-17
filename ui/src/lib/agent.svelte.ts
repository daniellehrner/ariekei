// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

// Agent status per work item, as reported by Claude Code hooks (see hooks.rs).
// The conversation itself lives in the embedded terminal.

import { listen } from "@tauri-apps/api/event";

export type AgentStatus =
  | "starting"
  | "ready"
  | "working"
  | "waitingForPermission"
  | "finished"
  | "error"
  | "stopped";

export interface AgentState {
  status: AgentStatus;
  detail: string | null;
}

type AgentEvent = { type: "status"; key: string; status: AgentStatus; detail: string | null };

export const agents: Record<string, AgentState> = $state({});

let listening: Promise<unknown> | null = null;

export function listenToAgents() {
  listening ??= listen<AgentEvent>("agent-event", ({ payload: e }) => {
    agents[e.key] = { status: e.status, detail: e.detail };
  });
  return listening;
}
