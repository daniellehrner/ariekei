<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!--
  An inline confirmation in place of the button that asked: "danger" where work would be lost
  (Discard), "neutral" where nothing would (Done only cleans up what is safe). Focus moves to
  Cancel, so Enter never confirms by accident.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import Button from "./Button.svelte";

  let {
    text,
    confirm,
    tone = "danger",
    onconfirm,
    oncancel,
    ...rest
  }: {
    text: string;
    confirm: string;
    tone?: "danger" | "neutral";
    onconfirm: () => void;
    oncancel: () => void;
    [key: `data-${string}`]: unknown;
  } = $props();

  let box: HTMLDivElement;
  onMount(() => box.querySelector<HTMLButtonElement>("[data-cancel]")?.focus());
</script>

<div class="confirm {tone}" role="group" aria-label={text} bind:this={box} {...rest}>
  <span class="text">{text}</span>
  <Button size="sm" variant={tone === "danger" ? "danger" : "secondary"} onclick={onconfirm}>{confirm}</Button>
  <Button size="sm" variant="quiet" onclick={oncancel} data-cancel>Cancel</Button>
</div>

<style>
  .confirm {
    display: inline-flex; align-items: center; gap: var(--sp-3); height: var(--h-control); min-width: 0;
    padding: 0 var(--sp-2) 0 var(--sp-5); border-radius: var(--r-md); font-size: var(--fs-12); font-weight: var(--fw-regular);
  }
  .danger { background: var(--bad-bg); }
  .neutral { background: var(--warn-bg); }
  .text { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text); }
</style>
