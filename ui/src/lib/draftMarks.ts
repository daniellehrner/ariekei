// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

// Marks the lines draft comments are on, in a gutter left of the line numbers: a small square
// for yours, a circle for the agent's (their voices' marks), so they stay visible once the
// selection is gone.

import { RangeSet, StateEffect, StateField } from "@codemirror/state";
import { GutterMarker, gutter } from "@codemirror/view";

type Who = "you" | "agent";

/** The new side's lines (1-based) that draft comments cover, and whose drafts they are. */
export const setDraftLines = StateEffect.define<{ line: number; who: Who }[]>();

class DraftMark extends GutterMarker {
  constructor(readonly who: Who) {
    super();
  }
  eq(other: DraftMark) {
    return other.who === this.who;
  }
  toDOM() {
    const el = document.createElement("span");
    el.className = `ak-draft-mark ${this.who}`;
    return el;
  }
}
const marks = { you: new DraftMark("you"), agent: new DraftMark("agent") };
const mark = marks.you;

const lines = StateField.define<RangeSet<GutterMarker>>({
  create: () => RangeSet.empty,
  update(value, tr) {
    value = value.map(tr.changes);
    for (const e of tr.effects) {
      if (!e.is(setDraftLines)) continue;
      const doc = tr.state.doc;
      // One mark a line: yours where both of you commented on it.
      const who = new Map<number, Who>();
      for (const { line, who: w } of e.value) if (line >= 1 && line <= doc.lines && who.get(line) !== "you") who.set(line, w);
      const wanted = [...who].sort(([a], [b]) => a - b);
      value = RangeSet.of(wanted.map(([n, w]) => marks[w].range(doc.line(n).from)));
    }
    return value;
  },
});

/** Goes before the line numbers in the extensions, so its gutter is left of them. */
export const draftMarks = [
  lines,
  gutter({ class: "ak-drafts", markers: (v) => v.state.field(lines), initialSpacer: () => mark }),
];

/** Every line from..to of each draft, with whose it is. */
export function draftLines(drafts: { fromLine: number; toLine: number; author: "user" | "agent" }[]): { line: number; who: Who }[] {
  return drafts.flatMap((d) =>
    Array.from({ length: d.toLine - d.fromLine + 1 }, (_, i) => ({ line: d.fromLine + i, who: d.author === "agent" ? ("agent" as const) : ("you" as const) })),
  );
}
