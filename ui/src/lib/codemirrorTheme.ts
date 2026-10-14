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
  "&.cm-merge-a .cm-changeGutter": { backgroundColor: "var(--diff-del-gutter)" },
  "&.cm-merge-b .cm-changeGutter": { backgroundColor: "var(--diff-add-gutter)" },
  ".cm-mergeSpacer": { backgroundColor: "var(--diff-spacer)" },
  ".cm-collapsedLines": { background: "var(--diff-fold-bg)", color: "var(--diff-fold-text)",
    borderBlock: "1px solid var(--border)", fontFamily: "var(--font-ui)",
    fontSize: "11.5px", padding: "3px 12px", cursor: "pointer" },
  ".cm-collapsedLines:hover": { color: "var(--text)" },
  ".cm-mergeView .cm-editor + .cm-editor": { borderLeft: "1px solid var(--border)" },
  // The change n / p moved to: a bar on the gutter (currentChange.ts).
  ".cm-lineNumbers .ak-current-change": { boxShadow: "inset 2px 0 0 var(--you)" },
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
