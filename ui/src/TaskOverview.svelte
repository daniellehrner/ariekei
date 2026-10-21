<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!--
  A task's Overview: what to look into. The PRs and issues its agent opens are items of their
  own in the Inbox.
-->
<script lang="ts">
  import type { Row } from "./lib/api";

  let { task }: { task: Row } = $props();
</script>

<div class="overview" role="tabpanel" aria-label="Overview">
  <article>
    <h2 class="label-caps">What to look into</h2>
    {#if task.notes?.trim()}
      <p class="notes">{task.notes}</p>
    {:else}
      <p class="muted">No notes. Tell the agent in the Terminal what to look into.</p>
    {/if}
    <p class="muted">
      PRs and issues the agent opens{#if task.branch}{" "}from <code>{task.branch}</code>{/if} (<code>gh pr create</code>,
      <code>gh issue create</code>, or a PR opened for the branch) are added to the Inbox as items of their own. Start working
      on one, and its agent is told how it came about.
    </p>
  </article>
</div>

<style>
  .overview { flex: 1; min-height: 0; overflow-y: auto; padding: var(--sp-8) var(--sp-9); }
  article { max-width: 760px; display: flex; flex-direction: column; gap: var(--sp-4); }
  h2 { margin: var(--sp-5) 0 0; }
  h2:first-child { margin-top: 0; }
  p { margin: 0; font-size: var(--fs-13); line-height: var(--lh-13); }
  .notes { white-space: pre-wrap; }
  .muted { color: var(--text-muted); font-size: var(--fs-12); line-height: var(--lh-12); }
  code { font-size: var(--fs-code-inline); }
</style>
