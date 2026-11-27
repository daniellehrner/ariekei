// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

// The diff's links to the impact map: at the end of a changed line that entries of the map
// follow from, a quiet "Impact" chip with how many; clicking it shows them in the Impact tab.

import { StateEffect, StateField } from "@codemirror/state";
import { Decoration, EditorView, WidgetType, type DecorationSet } from "@codemirror/view";

/** The new side's lines (1-based) with the entries that follow from each, and what a click
 *  does. */
export const setImpactLinks = StateEffect.define<{ links: { line: number; entries: number[] }[]; open: (entries: number[]) => void }>();

class Link extends WidgetType {
  constructor(
    readonly entries: number[],
    readonly open: (entries: number[]) => void,
  ) {
    super();
  }

  eq(other: Link) {
    return other.entries.join() === this.entries.join();
  }

  toDOM() {
    const el = document.createElement("button");
    el.className = "ak-impact-link";
    el.type = "button";
    el.textContent = `Impact ${this.entries.length}`;
    el.title = `What the agent's map says follows from this line: ${this.entries.length} entr${this.entries.length === 1 ? "y" : "ies"} in the Impact tab`;
    el.dataset.impactLink = this.entries.join(",");
    el.addEventListener("click", (e) => {
      e.preventDefault();
      this.open(this.entries);
    });
    return el;
  }

  ignoreEvent() {
    return true;
  }
}

const links = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, tr) {
    value = value.map(tr.changes);
    for (const e of tr.effects) {
      if (!e.is(setImpactLinks)) continue;
      const doc = tr.state.doc;
      value = Decoration.set(
        e.value.links
          .filter((l) => l.line >= 1 && l.line <= doc.lines)
          .map((l) => Decoration.widget({ widget: new Link(l.entries, e.value.open), side: 1 }).range(doc.line(l.line).to)),
        true,
      );
    }
    return value;
  },
  provide: (f) => EditorView.decorations.from(f),
});

export const impactLinks = [links];
