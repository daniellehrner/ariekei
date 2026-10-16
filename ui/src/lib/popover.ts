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
