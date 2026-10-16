<!--
  The review's menu on code (right-click, or the ContextMenu key): ask the agent about the
  lines, comment on them, copy them. Placed at the pointer, inside the window.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "./controls/Icon.svelte";
  import { place } from "./lib/popover";

  let {
    x,
    y,
    lines,
    noAgent,
    oncomment,
    onask,
    oncopy,
    onclose,
  }: {
    x: number;
    y: number;
    // "line 12" or "lines 12–18", for the items' labels.
    lines: string;
    // Why the agent can't be told, if it can't.
    noAgent: string | null;
    // Only on the side comments go to (the new one).
    oncomment?: () => void;
    onask: () => void;
    oncopy: () => void;
    onclose: () => void;
  } = $props();

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
  <button class="action" role="menuitem" onclick={run(onask)} disabled={!!noAgent} title={noAgent ?? undefined} data-ask-agent>
    <span class="agent"><svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><circle cx="5" cy="5" r="4.5" /></svg></span>
    <span class="grow">Ask agent…</span><span class="muted">{lines}</span>
  </button>
  {#if oncomment}
    <button class="action" role="menuitem" onclick={run(oncomment)} data-comment-lines>
      <Icon name="comment" /><span class="grow">Comment on {lines}</span>
    </button>
  {/if}
  <button class="action" role="menuitem" onclick={run(oncopy)} data-copy-code>
    <span class="blank"></span><span class="grow">Copy</span>
  </button>
</div>

<style>
  .menu {
    position: fixed; z-index: 40; min-width: 220px; max-width: calc(100vw - 24px);
    padding: var(--sp-2); box-sizing: border-box; display: flex; flex-direction: column;
    border-radius: var(--r-lg); background: var(--surface-raised); box-shadow: var(--shadow-pop);
  }
  .menu:focus { outline: none; }
  .action {
    display: flex; align-items: center; gap: var(--sp-4); height: 30px; padding: 0 var(--sp-4);
    border: 0; border-radius: var(--r-md); background: transparent; color: var(--text); text-align: left;
    font: inherit; font-size: var(--fs-13);
  }
  .action:hover:not(:disabled), .action:focus-visible { background: var(--surface-hover); }
  .action:focus-visible { outline-offset: -2px; }
  .action:disabled { color: var(--text-faint); cursor: default; }
  .action :global(svg) { flex: none; color: var(--text-muted); }
  .agent { width: 16px; display: flex; justify-content: center; }
  .agent svg { fill: var(--agent); }
  .action:disabled .agent svg { fill: var(--text-faint); }
  .blank { width: 16px; flex: none; }
  .grow { flex: 1; white-space: nowrap; }
  .muted { color: var(--text-muted); font-size: var(--fs-12); white-space: nowrap; }
</style>
