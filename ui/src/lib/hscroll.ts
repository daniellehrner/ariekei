// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

// A horizontal scrollbar for each editor, in a strip below the diff (`host`). The editor is as
// tall as its document (the diff around it scrolls), so its own scrollbar sits below the last
// line, out of sight; a bar inside the diff would have to stick to its bottom, which WKWebView
// does not do for CodeMirror's panels. Each bar spans its editor (side by side, its pane) and
// scrolls with it; the strip has no height when every line fits.

import { type Extension } from "@codemirror/state";
import { EditorView, ViewPlugin, type ViewUpdate } from "@codemirror/view";

class Bar {
  dom = document.createElement("div");
  inner = document.createElement("div");

  constructor(readonly view: EditorView, readonly host: HTMLElement) {
    this.dom.className = "ak-hscroll";
    this.dom.hidden = true;
    this.dom.appendChild(this.inner);
    host.appendChild(this.dom);
    this.dom.addEventListener("scroll", this.fromBar);
    view.scrollDOM.addEventListener("scroll", this.fromScroller);
    this.measure();
  }

  fromBar = () => {
    const scroller = this.view.scrollDOM;
    if (scroller.scrollLeft !== this.dom.scrollLeft) scroller.scrollLeft = this.dom.scrollLeft;
  };

  fromScroller = () => {
    const scroller = this.view.scrollDOM;
    if (this.dom.scrollLeft !== scroller.scrollLeft) this.dom.scrollLeft = scroller.scrollLeft;
  };

  // CodeMirror draws the lines in view only, and widens the content as wider ones come in.
  measure() {
    this.view.requestMeasure({
      key: this,
      read: () => {
        const scroller = this.view.scrollDOM;
        return {
          left: scroller.getBoundingClientRect().left - this.host.getBoundingClientRect().left,
          width: scroller.clientWidth,
          content: scroller.scrollWidth,
          overflows: scroller.scrollWidth > scroller.clientWidth + 1,
        };
      },
      write: ({ left, width, content, overflows }) => {
        this.dom.style.marginLeft = `${left}px`;
        this.dom.style.width = `${width}px`;
        this.inner.style.width = `${content}px`;
        this.dom.hidden = !overflows;
        this.fromScroller();
      },
    });
  }

  update(u: ViewUpdate) {
    if (u.geometryChanged || u.viewportChanged || u.docChanged) this.measure();
  }

  destroy() {
    this.view.scrollDOM.removeEventListener("scroll", this.fromScroller);
    this.dom.remove();
  }
}

/** Bars for the editors it is given to, in `host` (a strip right below the diff). */
export function hscroll(host: HTMLElement): Extension {
  return ViewPlugin.define((view) => new Bar(view, host));
}
