// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

// In-app notices (see Notices.svelte): which to keep showing, and how a new one is added.

export type NoticeKind = "needs-you" | "finished" | "error" | "ci-failing" | "activity" | "lifecycle";
export type Notice = { id: number; item: number | null; kind: NoticeKind; title: string; body: string };

export const MAX_NOTICES = 4;
export const FADE_MS = 10_000;

/** Still worth showing: not about the item on screen (`selected`), and a permission request
 *  only while its agent (`status` of an item's agent) still waits. */
export function stillCurrent(n: Notice, selected: number | null, status: (item: number) => string | undefined): boolean {
  return n.item !== selected && (n.kind !== "needs-you" || n.item == null || status(n.item) === "waitingForPermission");
}

/** The list with `notice` added: it replaces an older one about the same item, and only the
 *  newest MAX_NOTICES stay. */
export function withNotice(list: Notice[], notice: Notice): Notice[] {
  return [...list.filter((n) => n.item == null || n.item !== notice.item), notice].slice(-MAX_NOTICES);
}

/** Whether the notice goes away by itself (a permission request stays until answered). */
export function fades(n: Notice): boolean {
  return n.kind !== "needs-you";
}
