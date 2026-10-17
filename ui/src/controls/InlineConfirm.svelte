<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!-- An inline confirmation in place of the button that asked (Done → clean up, Discard). -->
<script lang="ts">
  import Button from "./Button.svelte";

  let {
    text,
    confirm,
    onconfirm,
    oncancel,
    ...rest
  }: {
    text: string;
    confirm: string;
    onconfirm: () => void;
    oncancel: () => void;
    [key: `data-${string}`]: unknown;
  } = $props();
</script>

<div class="confirm" role="group" aria-label={text} {...rest}>
  <span class="text">{text}</span>
  <Button size="sm" variant="danger" onclick={onconfirm}>{confirm}</Button>
  <Button size="sm" variant="quiet" onclick={oncancel}>Cancel</Button>
</div>

<style>
  .confirm {
    display: inline-flex; align-items: center; gap: var(--sp-3); height: var(--h-control); min-width: 0;
    padding: 0 var(--sp-2) 0 var(--sp-5); border-radius: var(--r-md); font-size: var(--fs-12); font-weight: var(--fw-regular);
    background: var(--bad-bg);
  }
  .text { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text); }
</style>
