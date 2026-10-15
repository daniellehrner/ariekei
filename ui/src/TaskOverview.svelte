<!--
  A task's Overview: what to look into, and the PRs and issues its agent opened (linked by
  Ariekei from `gh pr create` / `gh issue create`).
-->
<script lang="ts">
  import Icon from "./controls/Icon.svelte";
  import Pill from "./controls/Pill.svelte";
  import { shortName, type Row } from "./lib/api";

  let { task, links, onselect }: { task: Row; links: Row[]; onselect: (id: number) => void } = $props();

  const CI: Record<string, ["ok" | "bad" | "warn", string]> = {
    SUCCESS: ["ok", "CI passing"],
    FAILURE: ["bad", "CI failing"],
    ERROR: ["bad", "CI failing"],
  };
</script>

<div class="overview" role="tabpanel" aria-label="Overview">
  <article>
    <h2>What to look into</h2>
    {#if task.notes?.trim()}
      <p class="notes">{task.notes}</p>
    {:else}
      <p class="muted">No notes. Tell the agent in the Terminal what to look into.</p>
    {/if}
    <h2>PRs and issues</h2>
    {#if links.length}
      <ul data-links>
        {#each links as link (link.id)}
          {@const ci = link.ci ? (CI[link.ci] ?? ["warn", "CI running"]) : null}
          <li>
            <button onclick={() => onselect(link.id)} data-link={link.number}>
              <Icon name={link.kind === "pr" ? "pull-request" : "issue"} size={14} label={link.kind === "pr" ? "Pull request" : "Issue"} />
              <span class="num">{shortName(link)}</span>
              <span class="title">{link.title}</span>
              {#if ci}<span class={ci[0]}>{ci[1]}</span>{/if}
              <Pill size="sm" tone={link.open ? (link.isDraft ? "neutral" : "ok") : "neutral"}>
                {link.open ? (link.isDraft ? "Draft" : "Open") : "Closed"}
              </Pill>
            </button>
          </li>
        {/each}
      </ul>
    {:else}
      <p class="muted">
        None yet. When the agent opens a pull request (<code>gh pr create</code>) or an issue
        (<code>gh issue create</code>), it is linked here. Ask it to, when the task is ready.
      </p>
    {/if}
  </article>
</div>

<style>
  .overview { flex: 1; min-height: 0; overflow-y: auto; padding: var(--sp-7) var(--sp-9); }
  article { max-width: 760px; display: flex; flex-direction: column; gap: var(--sp-4); }
  h2 {
    margin: var(--sp-5) 0 0; font-size: var(--fs-11); font-weight: var(--fw-semibold); letter-spacing: 0.04em;
    text-transform: uppercase; color: var(--text-muted);
  }
  h2:first-child { margin-top: 0; }
  p { margin: 0; font-size: var(--fs-13); line-height: 1.5; }
  .notes { white-space: pre-wrap; }
  .muted { color: var(--text-muted); font-size: var(--fs-12); }
  code { font-size: 11.5px; }
  ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: var(--sp-1); }
  li button {
    width: 100%; display: flex; align-items: center; gap: var(--sp-4); padding: var(--sp-3) var(--sp-4);
    border: 0; border-radius: var(--r-md); background: transparent; color: var(--text); text-align: left; font-size: var(--fs-13);
  }
  li button:hover { background: var(--surface-hover); }
  li :global(svg) { flex: none; color: var(--text-muted); }
  .num { flex: none; font-family: var(--font-mono); font-size: 11.5px; color: var(--text-muted); }
  .title { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ok, .bad, .warn { flex: none; font-size: var(--fs-12); }
  .ok { color: var(--ok); }
  .bad { color: var(--bad); }
  .warn { color: var(--warn); }
</style>
