<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!-- Banners and notices: bad (work at risk, errors), warn, info, gh (GitHub results), agent. -->
<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";
  import Mark from "./Mark.svelte";
  import type { IconName } from "./icons";

  type Tone = "bad" | "warn" | "info" | "gh" | "agent" | "ok";
  // GitHub's results carry its mark (the diamond).
  const ICON: Record<Exclude<Tone, "gh">, IconName> = { bad: "error", warn: "warning", info: "info", agent: "terminal", ok: "check" };
  let {
    tone,
    icon,
    compact = true,
    children,
    action,
    ...rest
  }: { tone: Tone; icon?: IconName; compact?: boolean; children: Snippet; action?: Snippet; [key: `data-${string}`]: unknown } = $props();
</script>

<div class="banner {tone}" class:compact role={tone === "bad" || tone === "warn" ? "alert" : "status"} {...rest}>
  <span class="icon">{#if icon}<Icon name={icon} size={14} />{:else if tone === "gh"}<span class="mark"><Mark who="gh" /></span>{:else}<Icon name={ICON[tone]} size={14} />{/if}</span>
  <div class="text">{@render children()}</div>
  {#if action}<div class="action">{@render action()}</div>{/if}
</div>

<style>
  .banner {
    display: flex; gap: var(--sp-5); align-items: flex-start; padding: var(--sp-5) var(--sp-6);
    border-radius: var(--r-lg); font-size: var(--fs-12); line-height: var(--lh-12);
  }
  .compact { align-items: center; gap: var(--sp-4); padding: var(--sp-3) var(--sp-6); }
  .compact:has(.action) { padding-right: var(--sp-3); }
  .icon { flex: none; display: flex; padding-top: 2px; }
  .compact .icon { padding-top: 0; }
  .text { flex: 1; min-width: 0; }
  .text :global(strong) { font-weight: var(--fw-semibold); }
  .text :global(code) { font-size: var(--fs-code-inline); }
  .text :global(ul) { margin: 2px 0 0; padding-left: 16px; }
  .action { flex: none; display: flex; gap: var(--sp-2); }
  .bad { background: var(--bad-bg); box-shadow: inset 0 0 0 1px var(--bad-line); }
  .bad .icon { color: var(--bad); }
  .warn { background: var(--warn-bg); }
  .warn .icon { color: var(--warn); }
  .info { background: var(--info-bg); box-shadow: inset 0 0 0 1px var(--info-line); }
  .mark { width: 14px; height: 14px; display: grid; place-items: center; }
  .info .icon { color: var(--info); }
  .gh { background: var(--gh-bg); }
  .gh .icon { color: var(--gh); }
  .agent { background: var(--agent-bg); box-shadow: inset 0 0 0 1px var(--agent-line); }
  .agent .icon { color: var(--agent); }
  .ok { background: var(--ok-bg); }
  .ok .icon { color: var(--ok); }
</style>
