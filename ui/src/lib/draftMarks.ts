// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

// Marks the lines your draft comments are on: a small square (yours) in a gutter left of the
// line numbers, so they stay visible once the selection is gone.

import { RangeSet, StateEffect, StateField } from "@codemirror/state";
import { GutterMarker, gutter } from "@codemirror/view";

/** The new side's lines (1-based) that draft comments cover. */
export const setDraftLines = StateEffect.define<number[]>();

const mark = new (class extends GutterMarker {
  toDOM() {
    const el = document.createElement("span");
    el.className = "ak-draft-mark";
    return el;
  }
})();

const lines = StateField.define<RangeSet<GutterMarker>>({
  create: () => RangeSet.empty,
  update(value, tr) {
    value = value.map(tr.changes);
    for (const e of tr.effects) {
      if (!e.is(setDraftLines)) continue;
      const doc = tr.state.doc;
      const wanted = [...new Set(e.value)].filter((n) => n >= 1 && n <= doc.lines).sort((a, b) => a - b);
      value = RangeSet.of(wanted.map((n) => mark.range(doc.line(n).from)));
    }
    return value;
  },
});

/** Goes before the line numbers in the extensions, so its gutter is left of them. */
export const draftMarks = [
  lines,
  gutter({ class: "ak-drafts", markers: (v) => v.state.field(lines), initialSpacer: () => mark }),
];

/** Every line from..to of each range. */
export function draftLines(ranges: { fromLine: number; toLine: number }[]): number[] {
  return ranges.flatMap((r) => Array.from({ length: r.toLine - r.fromLine + 1 }, (_, i) => r.fromLine + i));
}
