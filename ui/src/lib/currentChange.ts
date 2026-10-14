// Marks the change the cursor is in (where n / p moved to) with a bar on the line-number gutter.

import { getChunks } from "@codemirror/merge";
import { RangeSet, StateField, type EditorState } from "@codemirror/state";
import { GutterMarker, gutterLineClass } from "@codemirror/view";

const marker = new (class extends GutterMarker {
  elementClass = "ak-current-change";
})();

function mark(state: EditorState): RangeSet<GutterMarker> {
  const info = getChunks(state);
  if (!info) return RangeSet.empty;
  const head = state.selection.main.head;
  const a = info.side === "a";
  const chunk = info.chunks.find((c) => (a ? c.fromA <= head && head <= c.toA : c.fromB <= head && head <= c.toB));
  if (!chunk) return RangeSet.empty;
  const [from, to] = a ? [chunk.fromA, chunk.toA] : [chunk.fromB, chunk.toB];
  const doc = state.doc;
  const ranges = [];
  // `to` is the start of the line after the change (or `from` when the change only deletes).
  for (let pos = from; pos <= Math.max(from, Math.min(to, doc.length)); ) {
    const line = doc.lineAt(pos);
    if (line.from >= to && line.from > from) break;
    ranges.push(marker.range(line.from));
    if (line.to + 1 > doc.length) break;
    pos = line.to + 1;
  }
  return RangeSet.of(ranges);
}

export const currentChange = StateField.define<RangeSet<GutterMarker>>({
  create: mark,
  update: (value, tr) => (tr.selection || tr.docChanged ? mark(tr.state) : value),
  provide: (f) => gutterLineClass.from(f),
});
