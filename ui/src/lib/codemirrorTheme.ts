// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

// The CodeMirror 6 and @codemirror/merge theme. It reads the tokens.css variables, so one theme
// serves light and dark.
import { EditorView } from "@codemirror/view";
import { HighlightStyle } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";

export const ariekeiTheme = EditorView.theme({
  "&": { backgroundColor: "var(--surface-editor)", color: "var(--text)", fontSize: "var(--fs-code)" },
  ".cm-scroller": { fontFamily: "var(--font-mono)", lineHeight: "var(--lh-code)" },
  ".cm-content": { caretColor: "var(--you)" },
  ".cm-cursor, .cm-dropCursor": { borderLeft: "2px solid var(--you)" },
  // The selection is drawn above the text, as a tint: below it, the changed lines' backgrounds
  // would hide it. (CodeMirror sets the layer's z-index inline, hence !important.)
  ".cm-selectionLayer": { zIndex: "1 !important", pointerEvents: "none" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground":
    { backgroundColor: "var(--diff-selection)" },
  ".cm-activeLine": { backgroundColor: "transparent" },
  ".cm-gutters": { backgroundColor: "var(--surface-editor)",
    color: "var(--diff-gutter-text)", border: "none" },
  ".cm-lineNumbers .cm-gutterElement": { minWidth: "38px", padding: "0 8px 0 0" },
  ".cm-activeLineGutter": { backgroundColor: "transparent", color: "var(--text-muted)" },
  ".cm-searchMatch": { backgroundColor: "var(--diff-search)", borderRadius: "2px" },
  ".cm-searchMatch-selected": { backgroundColor: "var(--diff-search-active)" },

  // merge view (@codemirror/merge)
  "&.cm-merge-a .cm-changedLine, .cm-deletedChunk": { backgroundColor: "var(--diff-del-line)" },
  "&.cm-merge-b .cm-changedLine, .cm-insertedLine": { backgroundColor: "var(--diff-add-line)" },
  "&.cm-merge-a .cm-changedText, .cm-deletedChunk .cm-deletedText":
    { background: "var(--diff-del-word)", borderRadius: "2px" },
  "&.cm-merge-b .cm-changedText": { background: "var(--diff-add-word)", borderRadius: "2px" },
  // Only the changed lines' markers are coloured, not the whole change-gutter column.
  ".cm-changeGutter": { backgroundColor: "transparent" },
  "&.cm-merge-a .cm-changedLineGutter, .cm-deletedLineGutter": { backgroundColor: "var(--diff-del-gutter)" },
  "&.cm-merge-b .cm-changedLineGutter, .cm-changedLineGutter": { backgroundColor: "var(--diff-add-gutter)" },
  ".cm-mergeSpacer": { backgroundColor: "var(--diff-spacer)" },
  ".cm-collapsedLines": { background: "var(--diff-fold-bg)", color: "var(--diff-fold-text)",
    borderBlock: "1px solid var(--border)", fontFamily: "var(--font-ui)",
    fontSize: "var(--fs-code-inline)", padding: "3px 12px", cursor: "pointer" },
  ".cm-collapsedLines:hover": { color: "var(--text)" },
  ".cm-mergeView .cm-editor + .cm-editor": { borderLeft: "1px solid var(--border)" },
  // The change n / p moved to: a bar on the gutter (currentChange.ts).
  ".cm-lineNumbers .ak-current-change": { boxShadow: "inset 2px 0 0 var(--you)" },
  // Your draft comments' lines (draftMarks.ts).
  ".ak-drafts .cm-gutterElement": { display: "flex", alignItems: "center", justifyContent: "center", width: "12px", padding: "0 0 0 4px" },
  ".ak-draft-mark": { display: "block", width: "8px", height: "8px", borderRadius: "1.5px", backgroundColor: "var(--diff-draft-mark)" },
});

export const ariekeiHighlight = HighlightStyle.define([
  { tag: [t.keyword, t.modifier, t.controlKeyword, t.definitionKeyword], color: "var(--syn-keyword)" },
  { tag: [t.typeName, t.className, t.namespace], color: "var(--syn-type)" },
  { tag: [t.string, t.special(t.string), t.regexp], color: "var(--syn-string)" },
  { tag: [t.number, t.bool, t.null, t.atom], color: "var(--syn-number)" },
  { tag: [t.constant(t.variableName), t.standard(t.name)], color: "var(--syn-constant)" },
  { tag: [t.function(t.variableName), t.function(t.propertyName)], color: "var(--syn-function)" },
  { tag: [t.annotation, t.meta, t.processingInstruction], color: "var(--syn-annotation)" },
  { tag: [t.tagName, t.angleBracket], color: "var(--syn-tag)" },
  { tag: [t.comment, t.docComment], color: "var(--syn-comment)", fontStyle: "italic" },
  { tag: [t.punctuation, t.operator, t.bracket], color: "var(--syn-punct)" },
]);
