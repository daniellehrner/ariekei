<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!-- A key hint, as in toolbar buttons and tooltips. -->
<script lang="ts">
  let { key }: { key: string } = $props();
</script>

<kbd>{key}</kbd>

<style>
  kbd {
    font-family: var(--font-mono); font-size: 10.5px; line-height: 14px; padding: 0 3px;
    border: 1px solid var(--border); border-radius: var(--r-xs); color: var(--text-faint);
  }
  :global(.on) > kbd, :global([aria-pressed="true"]) > kbd { color: inherit; border-color: var(--you-line); }
</style>
