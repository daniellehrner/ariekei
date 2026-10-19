// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

// Diff for the review view: chunk boundaries come from git's hunks, so the view shows
// exactly what git considers changed; within each hunk CodeMirror's own diff adds
// character-level detail.

import { Change, diff } from "@codemirror/merge";

export interface Hunk {
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
}

function lineStarts(text: string): number[] {
  const starts = [0];
  for (let i = text.indexOf("\n"); i !== -1; i = text.indexOf("\n", i + 1)) starts.push(i + 1);
  return starts;
}

// Offset of the start of 1-based `line`; past the last line it is the end of the text.
function offset(starts: number[], text: string, line: number): number {
  return line - 1 < starts.length ? starts[line - 1] : text.length;
}

// A hunk's region in one document: git reports an empty side as the line *after* which
// the other side's lines go.
function region(starts: number[], text: string, start: number, lines: number): [number, number] {
  const from = offset(starts, text, lines ? start : start + 1);
  return [from, lines ? Math.min(offset(starts, text, start + lines), text.length) : from];
}

export function gitChanges(oldText: string, newText: string, hunks: Hunk[]): Change[] {
  const a = lineStarts(oldText);
  const b = lineStarts(newText);
  const changes: Change[] = [];
  for (const h of hunks) {
    const [fromA, toA] = region(a, oldText, h.oldStart, h.oldLines);
    const [fromB, toB] = region(b, newText, h.newStart, h.newLines);
    for (const c of diff(oldText.slice(fromA, toA), newText.slice(fromB, toB))) {
      changes.push(new Change(c.fromA + fromA, c.toA + fromA, c.fromB + fromB, c.toB + fromB));
    }
  }
  return changes;
}

// The line separator of a document for a file's text (CodeMirror's `lineSeparator`): "\r\n"
// when every line ends so, else "\n". Then the editor's lines are git's (a lone "\r" is part
// of its line, as for git; CodeMirror would otherwise split there too), and its text joined
// with the separator is the file again, line endings and all: Save keeps them.
export function lineSeparatorOf(text: string): "\r\n" | "\n" {
  return text.includes("\r\n") && !/(^|[^\r])\n/.test(text) ? "\r\n" : "\n";
}

// The text of such a document as CodeMirror's diff gets it: lines joined with "\n".
export function docText(text: string): string {
  const sep = lineSeparatorOf(text);
  return sep === "\n" ? text : text.split(sep).join("\n");
}

// A `diffConfig.override` that uses git's hunks for the two files it was made for (in
// documents split with `lineSeparatorOf`) and CodeMirror's diff for anything else (e.g.
// re-diffing after an edit).
export function gitOverride(oldText: string, newText: string, hunks: Hunk[]) {
  const [a0, b0] = [docText(oldText), docText(newText)];
  return (a: string, b: string): readonly Change[] => (a === a0 && b === b0 ? gitChanges(a0, b0, hunks) : diff(a, b));
}
