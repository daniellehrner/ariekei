// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

// A horizontal scrollbar pinned to the bottom of the visible diff. The editor is as tall as its
// document (the diff around it scrolls), so its own scrollbar sits below the last line, out of
// sight. This one is a bottom panel, which CodeMirror keeps sticky in view and leaves room for
// when it scrolls a change into view. It hides when the lines fit.

import { type Extension } from "@codemirror/state";
import { EditorView, showPanel, type Panel, type ViewUpdate } from "@codemirror/view";

function panel(view: EditorView): Panel {
  const dom = document.createElement("div");
  dom.className = "ak-hscroll";
  dom.hidden = true;
  const inner = dom.appendChild(document.createElement("div"));
  const scroller = view.scrollDOM;

  const fromBar = () => {
    if (scroller.scrollLeft !== dom.scrollLeft) scroller.scrollLeft = dom.scrollLeft;
  };
  const fromScroller = () => {
    if (dom.scrollLeft !== scroller.scrollLeft) dom.scrollLeft = scroller.scrollLeft;
  };
  // CodeMirror draws the lines in view only, and widens the content as wider ones come in.
  const measure = () =>
    view.requestMeasure({
      read: () => ({ width: scroller.scrollWidth, overflows: scroller.scrollWidth > scroller.clientWidth + 1 }),
      write: ({ width, overflows }) => {
        inner.style.width = `${width}px`;
        dom.hidden = !overflows;
        fromScroller();
      },
    });

  dom.addEventListener("scroll", fromBar);
  scroller.addEventListener("scroll", fromScroller);
  return {
    dom,
    top: false,
    mount: measure,
    update(u: ViewUpdate) {
      if (u.geometryChanged || u.viewportChanged || u.docChanged) measure();
    },
    destroy: () => scroller.removeEventListener("scroll", fromScroller),
  };
}

export const hscroll: Extension = showPanel.of(panel);
