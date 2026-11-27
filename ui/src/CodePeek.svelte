<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!--
  The code at a place of the impact map: the whole file as committed at the map's commit, read
  only, with the place's lines marked and scrolled into the middle. Also files the diff does not
  touch. Right-click asks the agent about lines or copies them, as in the review.
-->
<script lang="ts" module>
  // Files as committed: a commit's file never changes.
  const files = new Map<string, Promise<string>>();
</script>

<script lang="ts">
  import { syntaxHighlighting } from "@codemirror/language";
  import { EditorState, StateEffect, StateField } from "@codemirror/state";
  import { Decoration, EditorView, type DecorationSet } from "@codemirror/view";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { basicSetup } from "codemirror";
  import { untrack } from "svelte";
  import AskAgent from "./AskAgent.svelte";
  import CodeMenu from "./CodeMenu.svelte";
  import Button from "./controls/Button.svelte";
  import IconButton from "./controls/IconButton.svelte";
  import Pill from "./controls/Pill.svelte";
  import { api } from "./lib/api";
  import { ariekeiHighlight, ariekeiTheme } from "./lib/codemirrorTheme";
  import { hscroll } from "./lib/hscroll";
  import { linesOf, splitPath, type ImpactLocation } from "./lib/impact";
  import { languageFor } from "./lib/languages";
  import type { Area } from "./lib/popover";
  import { askMessage, refLines, type CodeRef } from "./lib/reviewComments";

  let {
    id,
    rev,
    place,
    noAgent,
    agentStatus,
    onterminal,
    onshow,
    onerror,
  }: {
    id: number;
    // The commit the place is of.
    rev: string;
    place: ImpactLocation;
    // Why the agent can't be asked, if it can't.
    noAgent: string | null;
    agentStatus?: string;
    // Shows the agent's Terminal, where its answer appears.
    onterminal?: () => void;
    // Shows the lines in the Review tab (they are in the diff).
    onshow?: (path: string, line: number) => void;
    onerror: (message: string) => void;
  } = $props();

  let host: HTMLElement | undefined = $state();
  let bars: HTMLElement | undefined = $state();
  let editor: EditorView | null = null;
  // The file in the editor, as `rev:path`.
  let shown = "";
  let loading = $state(false);
  let failed: string | null = $state(null);

  // The place's lines: tinted, with the agent's bar at their left.
  const setPlace = StateEffect.define<{ from: number; to: number }>();
  const marked = StateField.define<DecorationSet>({
    create: () => Decoration.none,
    update(value, tr) {
      value = value.map(tr.changes);
      for (const e of tr.effects) {
        if (!e.is(setPlace)) continue;
        const doc = tr.state.doc;
        const to = Math.min(e.value.to, doc.lines);
        const lines = [];
        for (let n = Math.max(1, e.value.from); n <= to; n++) lines.push(Decoration.line({ class: "ak-place-line" }).range(doc.line(n).from));
        value = Decoration.set(lines);
      }
      return value;
    },
    provide: (f) => EditorView.decorations.from(f),
  });

  function mark() {
    if (!editor) return;
    const doc = editor.state.doc;
    const line = doc.line(Math.min(Math.max(1, place.fromLine), doc.lines));
    editor.dispatch({
      effects: [setPlace.of({ from: place.fromLine, to: place.toLine }), EditorView.scrollIntoView(line.from, { y: "center" })],
    });
  }

  async function open(key: string, path: string) {
    if (!host) return;
    loading = true;
    failed = null;
    let text: string;
    let lang;
    try {
      if (!files.has(key)) files.set(key, api.fileAt(id, rev, path));
      [text, lang] = await Promise.all([files.get(key)!, languageFor(path)]);
    } catch (e) {
      files.delete(key);
      if (key === `${rev}:${place.path}`) failed = String(e);
      return;
    } finally {
      loading = false;
    }
    // Another place was picked meanwhile.
    if (key !== `${rev}:${place.path}` || !host) return;
    editor?.destroy();
    host.textContent = "";
    editor = new EditorView({
      parent: host,
      doc: text,
      extensions: [
        basicSetup, ...lang, ariekeiTheme, syntaxHighlighting(ariekeiHighlight), hscroll(bars!),
        EditorState.readOnly.of(true), EditorView.editable.of(false), marked,
      ],
    });
    shown = key;
    mark();
  }

  // Another file: loaded; other lines of the same file: marked and scrolled to.
  $effect(() => {
    const key = `${rev}:${place.path}`;
    void place.fromLine;
    void place.toLine;
    untrack(() => (key === shown ? mark() : open(key, place.path)));
  });
  $effect(() => () => {
    editor?.destroy();
    editor = null;
  });

  // Right-click: the selection when the click is in it, else the clicked line (selected, so it
  // shows what the menu is about); then the menu, and the prompt box by the lines.
  type Asking = { x: number; lines: { top: number; bottom: number }; area: Area; code: CodeRef };
  let menu: { x: number; y: number; ask: Asking } | null = $state(null);
  let asking: Asking | null = $state(null);

  function onContextMenu(e: MouseEvent) {
    const v = editor;
    if (!v || !host) return;
    const sel = v.state.selection.main;
    let [from, to] = [sel.from, sel.to];
    let point = { x: e.clientX, y: e.clientY };
    const fromKeyboard = e.clientX === 0 && e.clientY === 0;
    const pos = fromKeyboard ? sel.head : v.posAtCoords(point);
    if (pos === null) return;
    if (sel.empty || pos < sel.from || pos > sel.to) {
      const line = v.state.doc.lineAt(pos);
      [from, to] = [line.from, line.to];
      v.dispatch({ selection: { anchor: from, head: to } });
    }
    if (fromKeyboard) {
      const c = v.coordsAtPos(pos);
      if (c) point = { x: c.left, y: c.bottom };
    }
    e.preventDefault();
    const doc = v.state.doc;
    const end = to > from && doc.lineAt(to).from === to ? to - 1 : to;
    const code: CodeRef = {
      path: place.path,
      fromLine: doc.lineAt(from).number,
      toLine: doc.lineAt(end).number,
      code: from === to ? doc.lineAt(from).text : doc.sliceString(from, to),
      side: "new",
      rev,
    };
    const first = v.coordsAtPos(from);
    const last = v.coordsAtPos(end);
    const pane = host.getBoundingClientRect();
    asking = null;
    menu = {
      ...point,
      ask: {
        x: first?.left ?? pane.left,
        lines: { top: first?.top ?? pane.top, bottom: last?.bottom ?? pane.top },
        area: { left: pane.left, right: pane.right, top: pane.top, bottom: pane.bottom },
        code,
      },
    };
  }

  async function ask(code: CodeRef, prompt: string) {
    try {
      await api.agentSend(id, askMessage(code, prompt));
      asking = null;
      onterminal?.();
    } catch (e) {
      onerror(`Could not send to the agent: ${e}`);
    }
  }

  async function copy(text: string) {
    try {
      await writeText(text);
    } catch (e) {
      onerror(`Could not copy: ${e}`);
    }
  }

  const where = $derived(splitPath(place.path));
</script>

<section class="peek" aria-label="Code of {place.path}" data-peek={place.path}>
  <div class="head">
    <span class="where" title="{place.path}:{linesOf(place)} at {rev.slice(0, 8)}">
      <span class="dir">{where.dir}</span><strong>{where.name}</strong><span class="lines">:{linesOf(place)}</span>
    </span>
    {#if place.inDiff}<Pill size="xs" title="The change touches these lines" data-in-diff>in the diff</Pill>{/if}
    <span class="grow"></span>
    {#if place.inDiff && onshow}
      <Button size="sm" variant="quiet" icon="branch" onclick={() => onshow?.(place.path, place.fromLine)} data-show-in-review>Show in Review</Button>
    {/if}
    <IconButton icon="copy" label="Copy the path" size={22} onclick={() => copy(place.path)} />
  </div>
  {#if place.note}<p class="note">{place.note}</p>{/if}
  {#if failed}
    <p class="failed" role="alert">{failed}</p>
  {:else if loading}
    <p class="loading">Loading {where.name}…</p>
  {/if}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="code" bind:this={host} hidden={!!failed} tabindex="0" role="region" oncontextmenu={onContextMenu}
    aria-label="{place.path}, read only. Right-click code to ask the agent about it."></div>
  <div class="hbars" bind:this={bars}></div>
  {#if menu}
    {@const m = menu}
    <CodeMenu x={m.x} y={m.y} lines={m.ask.code.fromLine === m.ask.code.toLine ? `Line ${m.ask.code.fromLine}` : `Lines ${refLines(m.ask.code)}`}
      side="new" {noAgent} onask={() => (asking = m.ask)} oncopy={() => copy(m.ask.code.code)} onclose={() => (menu = null)} />
  {/if}
  {#if asking}
    {@const a = asking}
    <AskAgent x={a.x} lines={a.lines} area={a.area} code={a.code} status={agentStatus} {onterminal}
      onsend={(prompt) => ask(a.code, prompt)} onclose={() => (asking = null)} />
  {/if}
</section>

<style>
  .peek { flex: 1; min-width: 0; min-height: 0; display: flex; flex-direction: column; background: var(--surface-editor); }
  .head {
    flex: none; display: flex; align-items: center; gap: var(--sp-3); height: var(--h-review-bar); box-sizing: border-box;
    padding: 0 var(--sp-4) 0 var(--sp-6); border-bottom: 1px solid var(--border); background: var(--surface-0);
    font-size: var(--fs-12); white-space: nowrap;
  }
  .where { min-width: 0; overflow: hidden; text-overflow: ellipsis; font-family: var(--font-mono); font-size: var(--fs-code-inline); }
  .dir { color: var(--text-faint); }
  .where strong { font-weight: var(--fw-semibold); }
  .lines { color: var(--text-muted); }
  .grow { flex: 1; }
  .note {
    flex: none; margin: 0; padding: var(--sp-3) var(--sp-6); border-bottom: 1px solid var(--border);
    background: var(--surface-0); font-size: var(--fs-12); line-height: var(--lh-12); color: var(--text-muted);
  }
  .loading, .failed { margin: var(--sp-6); font-size: var(--fs-12); color: var(--text-muted); }
  .failed { color: var(--bad); }
  .code { flex: 1; min-height: 0; overflow: auto; }
  .code:focus-visible { outline: 2px solid var(--focus); outline-offset: -2px; }
  .code :global(.cm-editor) { font-size: var(--fs-code); }
  .code :global(.cm-editor.cm-focused) { outline: none; }
  .hbars { flex: none; display: grid; }
  .hbars :global(.ak-hscroll) { grid-area: 1 / 1; min-height: 12px; overflow-x: auto; overflow-y: hidden; }
  .hbars :global(.ak-hscroll > div) { height: 1px; }
</style>
