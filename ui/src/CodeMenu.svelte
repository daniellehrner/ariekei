<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!--
  The review's menu on code (right-click, or the ContextMenu key): ask the agent about the
  lines, comment on them, copy them. Placed at the pointer, inside the window.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "./controls/Icon.svelte";
  import Kbd from "./controls/Kbd.svelte";
  import Mark from "./controls/Mark.svelte";
  import { place } from "./lib/popover";

  let {
    x,
    y,
    lines,
    side,
    noAgent,
    oncomment,
    onask,
    oncopy,
    onclose,
  }: {
    x: number;
    y: number;
    // "Line 12" or "Lines 12–18", for the header.
    lines: string;
    // Which version the lines are from.
    side: "new" | "old";
    // Why the agent can't be told, if it can't.
    noAgent: string | null;
    // Only on the side comments go to (the new one).
    oncomment?: () => void;
    onask: () => void;
    oncopy: () => void;
    onclose: () => void;
  } = $props();

  const COPY_KEY = navigator.platform.startsWith("Mac") ? "⌘ C" : "Ctrl C";

  let menu: HTMLElement | undefined = $state();
  let pos = $state({ left: 0, top: 0 });

  onMount(() => {
    if (!menu) return;
    pos = place(menu, x, y);
    menu.querySelector<HTMLElement>("[role='menuitem']:not(:disabled)")?.focus();
  });

  const items = () => [...(menu?.querySelectorAll<HTMLElement>("[role='menuitem']:not(:disabled)") ?? [])];

  function onKey(e: KeyboardEvent) {
    const list = items();
    const i = list.indexOf(document.activeElement as HTMLElement);
    const step = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
    if (step) {
      e.preventDefault();
      list[(i + step + list.length) % list.length]?.focus();
    } else if (e.key === "Escape" || e.key === "Tab") {
      e.preventDefault();
      e.stopPropagation();
      onclose();
    }
  }

  function onWindowDown(e: MouseEvent) {
    if (menu && !menu.contains(e.target as Node)) onclose();
  }

  // The action first: closing drops what the menu is about.
  const run = (f: () => void) => () => {
    f();
    onclose();
  };
</script>

<svelte:window onmousedown={onWindowDown} onblur={onclose} onresize={onclose} />

<div class="menu" role="menu" aria-label="Code" tabindex="-1" bind:this={menu} onkeydown={onKey}
  style:left="{pos.left}px" style:top="{pos.top}px" data-code-menu>
  <div class="head">{lines} · {side} version</div>
  <button class="action" role="menuitem" onclick={run(onask)} disabled={!!noAgent} data-ask-agent>
    <span class="lead"><Mark who="agent" /></span>
    <span class="label">
      <span>Ask agent…</span>
      {#if noAgent}<span class="why">{noAgent}</span>{/if}
    </span>
    {#if !noAgent}<Kbd key="a" />{/if}
  </button>
  {#if oncomment}
    <button class="action" role="menuitem" onclick={run(oncomment)} data-comment-lines>
      <span class="lead"><Mark who="you" /></span>
      <span class="label">Comment on lines</span>
      <Kbd key="c" />
    </button>
  {/if}
  <div class="sep" role="separator"></div>
  <button class="action" role="menuitem" onclick={run(oncopy)} data-copy-code>
    <span class="lead"><Icon name="copy" size={14} /></span>
    <span class="label">Copy</span>
    <Kbd key={COPY_KEY} />
  </button>
</div>

<style>
  .menu {
    position: fixed; z-index: 40; min-width: 232px; max-width: calc(100vw - 24px);
    padding: var(--sp-2); box-sizing: border-box; display: flex; flex-direction: column;
    border-radius: var(--r-lg); background: var(--surface-raised); box-shadow: var(--shadow-pop);
  }
  .menu:focus { outline: none; }
  .head {
    height: var(--h-section); display: flex; align-items: center; padding: 0 var(--sp-4);
    font-size: var(--fs-11); line-height: var(--lh-11); color: var(--text-faint); white-space: nowrap;
  }
  .action {
    display: flex; align-items: center; gap: var(--sp-4); min-height: 28px; padding: 0 var(--sp-4);
    border: 0; border-radius: var(--r-md); background: transparent; color: var(--text); text-align: left;
    font: inherit; font-size: var(--fs-13);
  }
  .action:hover:not(:disabled), .action:focus-visible { background: var(--surface-hover); }
  .action:focus-visible { outline-offset: -2px; }
  .action:disabled { color: var(--text-faint); cursor: default; opacity: 1; padding-block: var(--sp-2); }
  .action:disabled :global(.mark) { opacity: 0.45; }
  .lead { width: 16px; flex: none; display: flex; justify-content: center; color: var(--text-muted); }
  .label { flex: 1; display: flex; flex-direction: column; white-space: nowrap; }
  .why { font-size: var(--fs-11); line-height: var(--lh-11); color: var(--text-faint); }
  .sep { height: 1px; margin: var(--sp-2) var(--sp-2); background: var(--border); }
</style>
