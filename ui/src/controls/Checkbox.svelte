<!-- The reviewed checkbox: a square (= you), blue when checked. -->
<script lang="ts">
  import type { HTMLButtonAttributes } from "svelte/elements";

  let {
    checked,
    label,
    onchange,
    ...rest
  }: Omit<HTMLButtonAttributes, "onchange"> & { checked: boolean; label: string; onchange: (checked: boolean) => void } = $props();
</script>

<button {...rest} class="cb" class:on={checked} role="checkbox" aria-checked={checked} aria-label={label} title={label}
  onclick={(e) => { e.stopPropagation(); onchange(!checked); }}>
  {#if checked}
    <svg width="10" height="10" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M3.5 8.5l3 3 6-7" />
    </svg>
  {/if}
</button>

<style>
  .cb {
    flex: none; width: 14px; height: 14px; padding: 0; display: inline-grid; place-items: center; box-sizing: border-box;
    border: 1.5px solid var(--border-strong); border-radius: var(--r-xs); background: transparent; color: var(--text-on-accent); cursor: pointer;
  }
  .cb:hover:not(:disabled) { border-color: var(--you); background: transparent; }
  .cb.on, .cb.on:hover:not(:disabled) { background: var(--you); border-color: var(--you); }
</style>
