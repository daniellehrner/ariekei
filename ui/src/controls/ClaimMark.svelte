<!-- A guide claim's status: implemented (check, ok), partial (half-filled circle, warn), missing (x, bad). -->
<script lang="ts">
  let { status, size = 12 }: { status: "implemented" | "partial" | "missing"; size?: number } = $props();
  const LABEL = { implemented: "Implemented", partial: "Partly implemented", missing: "Missing" };
</script>

<svg class="mark {status}" width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.75"
  stroke-linecap="round" stroke-linejoin="round" role="img" aria-label={LABEL[status]}>
  {#if status === "implemented"}
    <path d="M3.5 8.5l3 3 6-7" />
  {:else if status === "partial"}
    <circle cx="8" cy="8" r="5.25" />
    <path d="M8 2.75a5.25 5.25 0 0 1 0 10.5z" fill="currentColor" stroke="none" />
  {:else}
    <path d="M4 4l8 8M12 4l-8 8" />
  {/if}
</svg>

<style>
  .mark { flex: none; display: inline-block; vertical-align: -1px; }
  .implemented { color: var(--ok); }
  .partial { color: var(--warn); }
  .missing { color: var(--bad); }
</style>
