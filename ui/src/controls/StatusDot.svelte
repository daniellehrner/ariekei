<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!--
  Agent status dot. Always a circle (= the agent), copper, red only for error.
  none: dotted grey ring · starting: dashed ring · ready: ring · working: ring + centre dot (ring spins)
  waiting: solid + halo · finished: donut · error: solid red · stopped: grey ring + stop square
-->
<script lang="ts" module>
  import type { AgentStatus } from "../lib/agent.svelte";

  export const AGENT_LABEL: Record<AgentStatus | "none", string> = {
    none: "No agent",
    starting: "Agent starting",
    ready: "Agent ready",
    working: "Agent working",
    waitingForPermission: "Agent waiting for permission",
    finished: "Agent finished",
    error: "Agent error",
    stopped: "Agent stopped",
  };
</script>

<script lang="ts">
  let { status, size = 14 }: { status: AgentStatus | null | undefined; size?: number } = $props();
  const s = $derived(status ?? "none");
</script>

<svg class="dot {s}" width={size} height={size} viewBox="0 0 14 14" role="img" aria-label={AGENT_LABEL[s]} data-agent={s}>
  {#if s === "waitingForPermission"}<circle class="halo" cx="7" cy="7" r="6.25" />{/if}
  <circle class="ring" cx="7" cy="7" r="3.75" />
  {#if s === "working" || s === "finished"}<circle class="inner" cx="7" cy="7" r="1.6" />{/if}
  {#if s === "stopped"}<rect class="stop" x="5.4" y="5.4" width="3.2" height="3.2" rx="0.6" />{/if}
</svg>

<style>
  .dot { flex: none; display: block; }
  .ring { fill: none; stroke: var(--agent); stroke-width: 1.5; }
  .halo { fill: none; stroke: var(--agent); stroke-opacity: 0.4; stroke-width: 1.5; }
  .inner { fill: var(--agent); }

  .none .ring { stroke: var(--border-strong); stroke-dasharray: 2 2; }
  .starting .ring { stroke-dasharray: 2 2; }
  .waitingForPermission .ring, .finished .ring { fill: var(--agent); }
  .finished .inner { fill: var(--surface-0); }
  .error .ring { fill: var(--bad); stroke: var(--bad); }
  .stopped .ring { stroke: var(--text-faint); }
  .stop { fill: var(--text-faint); }

  .working .ring { stroke-dasharray: 17 6.6; transform-origin: 7px 7px; animation: spin 1.2s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) {
    .working .ring { animation: none; stroke-dasharray: none; }
  }
</style>
