<!-- The mark: a circle (the agent) and a square (you) that overlap where the work is decided. -->
<script lang="ts">
  let { size = 28 }: { size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 16 16" aria-hidden="true">
  <rect width="16" height="16" rx="3.5" fill="var(--logo-ink)" />
  <rect x="0.25" y="0.25" width="15.5" height="15.5" rx="3.3" fill="none" stroke="white" stroke-opacity="0.18" stroke-width="0.5" />
  <circle cx="5.9" cy="8" r="3.9" fill="var(--logo-agent)" />
  <rect x="6.4" y="4.4" width="7.2" height="7.2" rx="1.4" fill="var(--logo-you)" />
  <path d="M6.4 5.1A3.9 3.9 0 0 1 9.8 8 3.9 3.9 0 0 1 6.4 10.9z" fill="var(--logo-overlap)" />
</svg>

<style>
  svg { flex: none; display: block; }
</style>
