// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

import type { Tab } from "./attention";

// Layout facts shared between views: the review's side panel is open (then the Inbox steps
// aside on narrow windows, so the diff keeps its room).

export const layout = $state({ reviewPanel: false });

// The tab an item opens on next, once (e.g. the task's Terminal after a PR of the task sent its
// agent something); `start`: it starts working, as its Start working button does (Browse's).
export const openOn = $state<{ item: number | null; tab: Tab | null; start?: boolean }>({ item: null, tab: null });

// Whether the keyboard shortcuts overlay is open (? anywhere, or the review's keyboard button).
export const shortcuts = $state({ open: false });

// The browser of the repo's open PRs and issues (Ctrl+K, the Inbox's Browse field): open, and
// what it shows first.
export const browser = $state<{ open: boolean; kind: "pr" | "issue"; text: string }>({ open: false, kind: "pr", text: "" });
