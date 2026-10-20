// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

/** Loads of the same thing that may finish out of order (a slow one, then a newer one): a
 *  result is shown only if no newer one is, and a failure only counts while no newer load has
 *  succeeded. */
export function newestLoad() {
  let started = 0;
  let shown = 0;
  return {
    /** A load starts: its number. */
    start: () => ++started,
    /** Load `n` succeeded: whether to show its result (then it is the one shown). */
    succeeded(n: number) {
      if (n < shown) return false;
      shown = n;
      return true;
    },
    /** Load `n` failed: whether that says what is shown is out of date. */
    failed: (n: number) => n > shown,
  };
}
