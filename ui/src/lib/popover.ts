// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

// Where a popover opened at a point goes: at the point, flipped above it or moved left when
// it would leave the window.

const MARGIN = 8;

export function clampTo(x: number, y: number, w: number, h: number, width: number, height: number) {
  const left = Math.max(MARGIN, Math.min(x, width - w - MARGIN));
  const top = y + h > height - MARGIN ? Math.max(MARGIN, y - h) : y;
  return { left, top };
}

/** The position of `el` (already rendered) opened at the point (x, y). */
export function place(el: HTMLElement, x: number, y: number) {
  const r = el.getBoundingClientRect();
  return clampTo(x, y, r.width, r.height, window.innerWidth, window.innerHeight);
}

export type Area = { left: number; right: number; top: number; bottom: number };

/** Where a box (w × h) about some lines (`lines`: their top and bottom) goes in `area` (the
 *  pane the lines are in; its bottom is where the review's dock starts): below the lines if it
 *  fits, else above them, else docked at the bottom of the area. Never over the lines, unless
 *  it is docked. Horizontally at `x`, kept in the area. */
export function nearLines(w: number, h: number, x: number, lines: { top: number; bottom: number }, area: Area) {
  const left = Math.max(area.left + MARGIN, Math.min(x, area.right - w - MARGIN));
  const gap = 4;
  if (lines.bottom + gap + h <= area.bottom - MARGIN) return { left, top: lines.bottom + gap, docked: false };
  if (lines.top - gap - h >= area.top + MARGIN) return { left, top: lines.top - gap - h, docked: false };
  return { left: area.left + MARGIN, top: area.bottom - h - MARGIN, docked: true };
}
