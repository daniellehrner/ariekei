<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!--
  Attention pip for Inbox rows, the collapsed strip and notices.
  Shape = who has to act (circle agent, square you, diamond GitHub); hue = the same,
  except red whenever something is broken (red changes the hue, never the shape).
  Most urgent first:
    needs-you   copper circle + halo     the agent asks you
    blocked     red square               your work would be lost (cleanup blocked)
    ci-failing  red diamond              GitHub: CI is broken
    error       red circle               the agent stopped with an error        ← NEW (was a red square)
    activity    violet diamond           GitHub: new reviews or comments
    review      blue square              your turn: the agent finished, review its changes
    working     hollow copper circle
    none        no pip
-->
<script lang="ts" module>
  export type Attention =
    | "needs-you" | "blocked" | "ci-failing" | "error" | "activity" | "review" | "info" | "working" | "none";
  export const URGENCY: Record<Attention, number> = {
    "needs-you": 0, blocked: 1, "ci-failing": 1, error: 1, activity: 2, review: 3, info: 4, working: 4, none: 9,
  };
</script>

<script lang="ts">
  // label: the accessible name. Pass "" when text next to the pip already says it (notices,
  // Inbox rows with the attention label): the pip is then hidden from assistive tech.
  let { kind, size = 14, label }: { kind: Attention; size?: number; label: string } = $props();
</script>

{#if kind !== "none"}
  <svg class="pip {kind}" width={size} height={size} viewBox="0 0 14 14"
    role={label ? "img" : undefined} aria-label={label || undefined} aria-hidden={label ? undefined : "true"}>
    {#if kind === "needs-you"}
      <circle class="halo" cx="7" cy="7" r="6.25" />
      <circle class="fill" cx="7" cy="7" r="3.9" />
    {:else if kind === "working"}
      <circle class="ring" cx="7" cy="7" r="3.9" />
    {:else if kind === "error"}
      <circle class="fill" cx="7" cy="7" r="4.4" />
    {:else if kind === "blocked" || kind === "review"}
      <rect class="fill" x="2.75" y="2.75" width="8.5" height="8.5" rx="1.75" />
    {:else if kind === "info"}
      <rect class="bar" x="2.5" y="5.75" width="9" height="2.5" rx="1.25" />
    {:else}
      <path class="fill" d="M7 1.5L12.5 7 7 12.5 1.5 7z" />
    {/if}
  </svg>
{/if}

<style>
  .pip { flex: none; display: block; }
  .halo { fill: none; stroke: var(--agent); stroke-opacity: 0.4; stroke-width: 1.5; }
  .ring { fill: none; stroke: var(--agent); stroke-width: 1.6; }
  .needs-you .fill { fill: var(--agent); }
  .blocked .fill, .ci-failing .fill, .error .fill { fill: var(--bad); }
  .activity .fill { fill: var(--gh); }
  .review .fill { fill: var(--you); }
  .bar { fill: var(--info); }
</style>
