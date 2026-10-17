<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!--
  Stars or unstars an item: starred items head the Inbox. The star is the user's own mark, so
  a set star is filled in the user's hue; an unset one is a grey outline.
-->
<script lang="ts">
  import Icon from "./Icon.svelte";

  let {
    starred,
    name,
    size = 22,
    onclick,
    ...rest
  }: {
    starred: boolean;
    // The item, for the label: "#12", "T1".
    name: string;
    size?: number;
    onclick: () => void;
    [key: `data-${string}`]: unknown;
  } = $props();
</script>

<button {...rest} class="star" class:on={starred} style:width="{size}px" style:height="{size}px" aria-pressed={starred}
  aria-label={starred ? `Unstar ${name}` : `Star ${name}`} title={starred ? "Unstar" : "Star: keep it at the top of the Inbox"}
  onclick={(e) => { e.stopPropagation(); onclick(); }}>
  <Icon name="star" size={14} filled={starred} />
</button>

<style>
  .star {
    flex: none; display: inline-grid; place-items: center; padding: 0;
    border: 0; border-radius: var(--r-md); background: transparent; color: var(--text-faint); cursor: pointer;
    transition: background-color var(--dur-fast) var(--ease);
  }
  .star:hover { background: var(--surface-hover); color: var(--text-muted); }
  .star.on, .star.on:hover { color: var(--you); }
</style>
