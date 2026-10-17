<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!--
  NEW (review 2026-10-17). The "who is speaking" mark as a small solid glyph:
  agent = circle (copper), you = square (blue), gh = diamond (violet).
  Use it wherever an action or a line belongs to one voice and an icon would only repeat it:
  code menu items, the Ask agent and submit headers, draft rows and their gutter marks,
  "Send to agent" / "Submit to GitHub…" / "Review on GitHub…" / "Send PR feedback to agent".
  It replaces the send-to-agent and submit-github arrow icons, which were indistinguishable at 16 px.
  `onAccent` draws it in --text-on-accent inside a filled button.
-->
<script lang="ts" module>
  export type Who = "agent" | "you" | "gh";
</script>

<script lang="ts">
  let { who, size = 8, onAccent = false }: { who: Who; size?: number; onAccent?: boolean } = $props();
</script>

<svg class="mark {who}" class:on-accent={onAccent} width={size} height={size} viewBox="0 0 8 8" aria-hidden="true">
  {#if who === "agent"}
    <circle cx="4" cy="4" r="4" />
  {:else if who === "you"}
    <rect x="0" y="0" width="8" height="8" rx="1.5" />
  {:else}
    <path d="M4 0.3L7.7 4 4 7.7 0.3 4z" />
  {/if}
</svg>

<style>
  .mark { flex: none; display: block; }
  .agent { fill: var(--agent); }
  .you { fill: var(--you); }
  .gh { fill: var(--gh); }
  .on-accent { fill: var(--text-on-accent); }
</style>
