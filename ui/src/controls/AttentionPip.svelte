<!--
  Attention pip for Inbox rows and the collapsed strip.
  Shape = who wants something (circle agent, square you, diamond GitHub); hue = the same,
  except red whenever something is broken. Most urgent first:
  needs-you > blocked / ci-failing > activity > review / all-closed > working > none (no pip).
-->
<script lang="ts" module>
  export type Attention = "needs-you" | "blocked" | "ci-failing" | "activity" | "review" | "all-closed" | "working" | "none";
  export const URGENCY: Record<Attention, number> = {
    "needs-you": 0, blocked: 1, "ci-failing": 1, activity: 2, review: 3, "all-closed": 3, working: 4, none: 9
  };
</script>

<script lang="ts">
  let { kind, size = 14, label }: { kind: Attention; size?: number; label: string } = $props();
</script>

{#if kind !== "none"}
  <svg class="pip {kind}" width={size} height={size} viewBox="0 0 14 14" role="img" aria-label={label}>
    {#if kind === "needs-you"}
      <circle class="halo" cx="7" cy="7" r="6.25" />
      <circle class="fill" cx="7" cy="7" r="3.9" />
    {:else if kind === "working"}
      <circle class="ring" cx="7" cy="7" r="3.9" />
    {:else if kind === "blocked" || kind === "review"}
      <rect class="fill" x="2.75" y="2.75" width="8.5" height="8.5" rx="1.75" />
    {:else if kind === "all-closed"}
      <rect class="outline" x="3.25" y="3.25" width="7.5" height="7.5" rx="1.5" />
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
  .blocked .fill, .ci-failing .fill { fill: var(--bad); }
  .activity .fill { fill: var(--gh); }
  .review .fill { fill: var(--you); }
  .all-closed .outline { fill: none; stroke: var(--you); stroke-width: 1.5; }
</style>
