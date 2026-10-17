<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!--
  Pills and tags. GitHub facts are outlined (neutral, ok, warn, bad, gh); local state is a
  tinted fill (tint when it wants nothing, you, warn-fill, agent).
-->
<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";
  import type { IconName } from "./icons";

  type Tone = "neutral" | "ok" | "warn" | "bad" | "gh" | "you" | "warn-fill" | "agent" | "tint";
  let {
    tone = "neutral",
    size = "md",
    icon,
    title,
    children,
    ...rest
  }: { tone?: Tone; size?: "xs" | "sm" | "md" | "lg"; icon?: IconName; title?: string; children: Snippet; [key: `data-${string}`]: unknown } = $props();
</script>

<span class="pill {tone} {size}" {title} {...rest}>
  {#if icon}<Icon name={icon} size={12} />{/if}
  {@render children()}
</span>

<style>
  .pill {
    display: inline-flex; align-items: center; gap: var(--sp-2); flex: none;
    height: 20px; padding: 0 var(--sp-4); border-radius: var(--r-pill); box-sizing: border-box;
    font-size: var(--fs-code-inline); line-height: 1; font-weight: var(--fw-medium); white-space: nowrap;
    color: var(--text-muted); box-shadow: inset 0 0 0 1px var(--border-strong);
  }
  .xs { height: 16px; padding: 0 5px; font-size: var(--fs-11); font-weight: var(--fw-medium); }
  .sm { height: 18px; padding: 0 var(--sp-3); font-size: var(--fs-11); }
  .lg { height: 22px; font-weight: var(--fw-semibold); }
  .ok { color: var(--ok); box-shadow: inset 0 0 0 1px var(--ok-line); }
  .warn { color: var(--warn); box-shadow: inset 0 0 0 1px var(--warn-line); }
  .bad { color: var(--bad); box-shadow: inset 0 0 0 1px var(--bad-line); }
  .gh { color: var(--gh-strong); box-shadow: inset 0 0 0 1px var(--gh-line); }
  .you { background: var(--you-bg); color: var(--you-strong); box-shadow: none; }
  .warn-fill { background: var(--warn-bg); color: var(--warn); box-shadow: none; }
  .agent { background: var(--agent-bg); color: var(--agent-strong); box-shadow: none; }
  .tint { background: var(--surface-active); color: var(--text-muted); box-shadow: none; }
</style>
