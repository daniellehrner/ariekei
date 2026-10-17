<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!--
  A task's Overview: what to look into, and the PRs and issues its agent opened (linked by
  Ariekei from `gh pr create` / `gh issue create`, or opened for its branch).
-->
<script lang="ts">
  import CiMark, { ciState } from "./controls/CiMark.svelte";
  import Icon from "./controls/Icon.svelte";
  import Pill from "./controls/Pill.svelte";
  import { shortName, type Row } from "./lib/api";

  let { task, links, onselect }: { task: Row; links: Row[]; onselect: (id: number) => void } = $props();

  const CI_TEXT = { pass: ["ok", "CI passing"], fail: ["bad", "CI failing"], pending: ["warn", "CI running"] } as const;
</script>

<div class="overview" role="tabpanel" aria-label="Overview">
  <article>
    <h2 class="label-caps">What to look into</h2>
    {#if task.notes?.trim()}
      <p class="notes">{task.notes}</p>
    {:else}
      <p class="muted">No notes. Tell the agent in the Terminal what to look into.</p>
    {/if}
    <h2 class="label-caps">PRs and issues</h2>
    {#if links.length}
      <ul data-links>
        {#each links as link (link.id)}
          {@const ci = ciState(link.ci)}
          <li>
            <button onclick={() => onselect(link.id)} data-link={link.number}>
              <Icon name={link.kind === "pr" ? "pull-request" : "issue"} size={14} label={link.kind === "pr" ? "Pull request" : "Issue"} />
              <span class="num">{shortName(link)}</span>
              <span class="title">{link.title}</span>
              {#if ci === "pass" || ci === "fail" || ci === "pending"}
                <span class="ci {CI_TEXT[ci][0]}"><CiMark state={ci} label="" />{CI_TEXT[ci][1]}</span>
              {/if}
              <Pill size="sm" tone={link.open && !link.isDraft ? "gh" : "neutral"}>
                {link.open ? (link.isDraft ? "Draft" : "Open") : "Closed"}
              </Pill>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
    <p class="muted">
      PRs and issues the agent opens{#if task.branch}{" "}from <code>{task.branch}</code>{/if} are listed here
      (<code>gh pr create</code>, <code>gh issue create</code>, or a PR opened for the branch).
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
  ul {
    list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column;
    border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-raised); overflow: hidden;
  }
  li + li { border-top: 1px solid var(--border); }
  li button {
    width: 100%; height: 36px; display: flex; align-items: center; gap: var(--sp-4); padding: 0 10px;
    border: 0; border-radius: 0; background: transparent; color: var(--text); text-align: left; font-size: var(--fs-13);
  }
  li button:hover { background: var(--surface-hover); }
  li button:focus-visible { outline-offset: -2px; }
  li :global(svg) { flex: none; color: var(--text-muted); }
  .num { flex: none; font-family: var(--font-mono); font-size: var(--fs-code-inline); color: var(--text-muted); }
  .title { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ci { flex: none; display: inline-flex; align-items: center; gap: var(--sp-2); font-size: var(--fs-12); }
  .ci :global(svg) { color: inherit; }
  .ok { color: var(--ok); }
  .bad { color: var(--bad); }
  .warn { color: var(--warn); }
</style>
