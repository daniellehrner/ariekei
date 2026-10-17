<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!-- An icon-only button; `label` is its accessible name and tooltip. -->
<script lang="ts">
  import type { HTMLButtonAttributes } from "svelte/elements";
  import Icon from "./Icon.svelte";
  import Spinner from "./Spinner.svelte";
  import type { IconName } from "./icons";

  let {
    icon,
    label,
    title,
    pressed,
    loading = false,
    size = 26,
    class: cls = "",
    ...rest
  }: HTMLButtonAttributes & { icon: IconName; label: string; pressed?: boolean; loading?: boolean; size?: number } = $props();
</script>

<button {...rest} class="ib {cls}" style:width="{size}px" style:height="{size}px" aria-label={label} title={title ?? label}
  aria-pressed={pressed} aria-busy={loading || undefined}>
  {#if loading}<Spinner />{:else}<Icon name={icon} />{/if}
</button>

<style>
  .ib {
    flex: none; display: inline-grid; place-items: center; padding: 0;
    border: 0; border-radius: var(--r-md); background: transparent; color: var(--text-muted); cursor: pointer;
    transition: background-color var(--dur-fast) var(--ease);
  }
  .ib:hover:not(:disabled), .ib[aria-pressed="true"] { background: var(--surface-hover); color: var(--text); }
  .ib:disabled { opacity: 0.45; cursor: not-allowed; }
</style>
