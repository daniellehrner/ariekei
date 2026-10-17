<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!-- Busy indicator for buttons that are working (aria-busy). -->
<script lang="ts">
  let { size = 14 }: { size?: number } = $props();
</script>

<svg class="spinner" width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" aria-hidden="true">
  <circle cx="8" cy="8" r="5.5" stroke-opacity="0.25" />
  <path d="M13.5 8A5.5 5.5 0 0 0 8 2.5" />
</svg>

<style>
  .spinner { flex: none; display: block; animation: spin 0.9s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .spinner { animation: none; } }
</style>
