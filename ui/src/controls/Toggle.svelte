<!-- A pressed/unpressed toolbar toggle with its key hint (Whitespace w, Unified u). -->
<script lang="ts">
  import type { Snippet } from "svelte";
  import type { HTMLButtonAttributes } from "svelte/elements";
  import Icon from "./Icon.svelte";
  import Kbd from "./Kbd.svelte";
  import type { IconName } from "./icons";

  let {
    pressed,
    icon,
    kbd,
    children,
    ...rest
  }: HTMLButtonAttributes & { pressed: boolean; icon?: IconName; kbd?: string; children: Snippet } = $props();
</script>

<button {...rest} class="tg" aria-pressed={pressed}>
  {#if icon}<Icon name={icon} size={14} />{/if}
  {@render children()}
  {#if kbd}<Kbd key={kbd} />{/if}
</button>

<style>
  .tg {
    display: inline-flex; align-items: center; gap: 5px; height: 24px; padding: 0 var(--sp-3);
    border: 0; border-radius: var(--r-md); background: transparent; color: var(--text-muted);
    font: inherit; font-size: var(--fs-12); white-space: nowrap; cursor: pointer;
  }
  .tg:hover { background: var(--surface-hover); }
  .tg[aria-pressed="true"] { background: var(--you-bg); color: var(--you-strong); }
</style>
