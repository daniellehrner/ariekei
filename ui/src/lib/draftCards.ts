// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

// The draft comments in the diff: each one a card (DraftCard.svelte) right below its last line
// on the new side, where it will be on GitHub.

import { StateEffect, StateField } from "@codemirror/state";
import { Decoration, EditorView, ViewPlugin, WidgetType, type DecorationSet } from "@codemirror/view";
import { mount, unmount } from "svelte";
import DraftCard from "../DraftCard.svelte";
import type { ReviewDraft } from "./reviewComments";

/** What a card does, given by the review; `agent` is reactive state the cards read. */
export interface CardActions {
  edit: (d: ReviewDraft, text: string) => Promise<void>;
  remove: (d: ReviewDraft) => Promise<void>;
  ask: (d: ReviewDraft, prompt: string) => Promise<void>;
  agent: {
    // There is an agent to ask, and why it takes nothing now (null: it does).
    present: boolean;
    away: string | null;
    status: string | undefined;
  };
}

/** The drafts on the file shown, and the commit it shows (null: the working tree). */
export const setDraftCards = StateEffect.define<{ drafts: ReviewDraft[]; shownRev: string | null; actions: CardActions }>();

const cards = new WeakMap<HTMLElement, ReturnType<typeof mount>>();

class Card extends WidgetType {
  constructor(
    readonly draft: ReviewDraft,
    // Made on another version of the file than the one shown.
    readonly elsewhere: boolean,
    readonly actions: CardActions,
  ) {
    super();
  }

  // An unchanged draft keeps its card (and what is typed in it).
  eq(other: Card) {
    const [a, b] = [this.draft, other.draft];
    return a.id === b.id && a.text === b.text && a.edited === b.edited && a.rev === b.rev && this.elsewhere === other.elsewhere;
  }

  toDOM() {
    const el = document.createElement("div");
    el.className = "ak-draft-card";
    cards.set(el, mount(DraftCard, { target: el, props: { draft: this.draft, elsewhere: this.elsewhere, actions: this.actions } }));
    return el;
  }

  destroy(el: HTMLElement) {
    const card = cards.get(el);
    if (card) unmount(card);
  }

  // Clicks and keys in the card are its own, not the editor's.
  ignoreEvent() {
    return true;
  }
}

const placed = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, tr) {
    value = value.map(tr.changes);
    for (const e of tr.effects) {
      if (!e.is(setDraftCards)) continue;
      const doc = tr.state.doc;
      const { drafts, shownRev, actions } = e.value;
      const widgets = drafts
        .filter((d) => d.toLine >= 1 && d.toLine <= doc.lines)
        .map((d) => {
          const widget = new Card(d, d.rev !== null && d.rev !== shownRev, actions);
          return Decoration.widget({ widget, block: true, side: 1 }).range(doc.line(d.toLine).to);
        });
      value = Decoration.set(widgets, true);
    }
    return value;
  },
  provide: (f) => EditorView.decorations.from(f),
});

// A card stays in the visible part of the pane however wide the code is (draft-card styles in
// codemirrorTheme.ts): this tells it how wide that is, and where the code starts.
const visible = ViewPlugin.define((view) => {
  const measure = {
    read: () => {
      const gutters = view.scrollDOM.querySelector<HTMLElement>(".cm-gutters")?.offsetWidth ?? 0;
      return { gutters, width: view.scrollDOM.clientWidth - gutters };
    },
    write: ({ gutters, width }: { gutters: number; width: number }) => {
      view.dom.style.setProperty("--ak-gutters-w", `${gutters}px`);
      view.dom.style.setProperty("--ak-visible-w", `${width}px`);
    },
  };
  view.requestMeasure(measure);
  return {
    update(u) {
      if (u.geometryChanged) view.requestMeasure(measure);
    },
  };
});

export const draftCards = [placed, visible];
