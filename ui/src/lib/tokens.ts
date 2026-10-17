// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

/** A duration as written in CSS ("120ms", or "1.8s" as some webviews report it), in ms. */
export function parseDuration(value: string): number | null {
  const m = /^\s*(-?[\d.]+)\s*(ms|s)\s*$/.exec(value);
  if (!m) return null;
  return parseFloat(m[1]) * (m[2] === "s" ? 1000 : 1);
}

/** A duration token of tokens.css (e.g. "--dur-note"), in ms. */
export function durationToken(name: string, fallback: number): number {
  return parseDuration(getComputedStyle(document.documentElement).getPropertyValue(name)) ?? fallback;
}
