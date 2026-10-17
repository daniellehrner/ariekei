<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!-- A segmented switch (radio group), e.g. the file order. -->
<script lang="ts" generics="T extends string">
  let {
    options,
    value = $bindable(),
    label,
    ...rest
  }: { options: { value: T; label: string }[]; value: T; label: string; [key: `data-${string}`]: unknown } = $props();

  function onkeydown(e: KeyboardEvent) {
    const i = options.findIndex((o) => o.value === value);
    const step = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    if (!step) return;
    e.preventDefault();
    value = options[(i + step + options.length) % options.length].value;
  }
</script>

<div class="seg" role="radiogroup" aria-label={label} {...rest}>
  {#each options as o (o.value)}
    <button role="radio" aria-checked={o.value === value} class:on={o.value === value} tabindex={o.value === value ? 0 : -1}
      onclick={() => (value = o.value)} {onkeydown} data-value={o.value}>{o.label}</button>
  {/each}
</div>

<style>
  .seg { display: inline-flex; padding: 2px; border-radius: var(--r-md); background: var(--surface-0); box-shadow: inset 0 0 0 1px var(--border); }
  button {
    height: 20px; padding: 0 var(--sp-4); border: 0; border-radius: var(--r-sm); background: transparent;
    color: var(--text-muted); font: inherit; font-size: var(--fs-code-inline); font-weight: var(--fw-medium); cursor: pointer;
  }
  button.on { background: var(--surface-raised); color: var(--text); box-shadow: var(--shadow-1), inset 0 0 0 1px var(--border); }
</style>
