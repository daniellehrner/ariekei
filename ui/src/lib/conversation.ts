// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

import type { Thread } from "./api";

/** When the thread last got a comment (ISO time; "" without comments). */
export const latestAt = (t: Thread) => t.comments.reduce((at, c) => (c.createdAt > at ? c.createdAt : at), "");

/** Threads with the latest comments first: a reply to an old thread is not lost among the others. */
export const byLatest = (threads: Thread[]) => [...threads].sort((a, b) => latestAt(b).localeCompare(latestAt(a)));

/** Someone else wrote it after `since`, how far the user had seen the item when they opened it
 *  (null: nothing to tell new from old; "0": seen with no activity, so all of it is new). */
export const isNew = (c: { author: string; at: string }, since: string | null, viewer: string) =>
  !!since && c.at > since && c.author !== viewer;

/** The thread has a comment that `isNew`: also a resolved one, folded with its replies in it. */
export const threadHasNew = (t: Thread, since: string | null, viewer: string) =>
  t.comments.some((c) => isNew({ author: c.author, at: c.createdAt }, since, viewer));
