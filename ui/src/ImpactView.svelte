<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!--
  The Impact tab: the agent's map of how the item's change affects the rest of the code
  (src-tauri/src/impact.rs). On the left what the change means for the project as a whole and
  the entries (affected code, similar code, paths), each with its places; on the right the code
  of the place picked, also of files the diff does not touch.
-->
<script lang="ts" module>
  import type { Picked } from "./lib/impact";

  export interface ImpactViewState {
    // The place whose code shows, and the entries opened.
    picked?: Picked | null;
    open?: number[];
    // The picked entry is to be scrolled to (picked elsewhere: the diff's link), then cleared.
    reveal?: boolean;
  }
</script>

<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { tick, untrack } from "svelte";
  import CodePeek from "./CodePeek.svelte";
  import Button from "./controls/Button.svelte";
  import Icon from "./controls/Icon.svelte";
  import IconButton from "./controls/IconButton.svelte";
  import Mark from "./controls/Mark.svelte";
  import Pill from "./controls/Pill.svelte";
  import StatusDot from "./controls/StatusDot.svelte";
  import { api } from "./lib/api";
  import { externalLinks, markdown } from "./lib/html";
  import { linesOf, pickedLocation, placesText, sectionsOf, splitPath, staleText, type ImpactEntry, type ImpactShown } from "./lib/impact";

  let {
    id,
    view = $bindable(),
    hasWorktree,
    agentAway = null,
    agentStatus,
    onterminal,
    onshow,
    polls = 0,
  }: {
    id: number;
    view: ImpactViewState;
    // The item has a worktree, and so an agent to make the map.
    hasWorktree: boolean;
    // Why the agent takes no work now (its worktree is on another branch), if it doesn't.
    agentAway?: string | null;
    agentStatus?: string;
    // Shows the agent's Terminal.
    onterminal?: () => void;
    // Shows lines of the diff in the Review tab.
    onshow?: (path: string, line: number) => void;
    // Counts the polls of the item's repo: the head may have moved.
    polls?: number;
  } = $props();

  let shown = $state.raw<ImpactShown | null>(null);
  let error: string | null = $state(null);
  let notice: string | null = $state(null);
  let asking = $state(false);
  // The user asked for a map in this view: while the agent works, it is at that.
  let asked = $state(false);

  async function load() {
    try {
      shown = await api.impactMap(id);
    } catch (e) {
      error = String(e);
    }
  }
  $effect(() => {
    void id;
    untrack(load);
    const unlisten = listen<number>("impact-changed", (e) => e.payload === id && load());
    return () => void unlisten.then((f) => f());
  });
  // The head moves with the agent's commits and the author's pushes.
  $effect(() => {
    void polls;
    void agentStatus;
    untrack(load);
  });
  $effect(() => {
    if (agentStatus === "finished" || agentStatus === "error") asked = false;
  });

  const map = $derived(shown?.map ?? null);
  const sections = $derived(map ? sectionsOf(map.entries) : []);
  const stale = $derived(shown ? staleText(shown) : null);
  const working = $derived(agentStatus === "working");
  const waits = $derived(agentStatus === "waitingForPermission");
  // Why the agent can't be asked now, if it can't.
  const noAgent = $derived(!hasWorktree ? "No agent on this item yet: start working first" : agentAway);
  const picked = $derived(map ? pickedLocation(map.entries, view.picked) : null);
  const pickedEntry = $derived(map?.entries.find((e) => e.id === view.picked?.entry) ?? null);
  const short = (sha: string) => sha.slice(0, 8);

  // With nothing picked (or what was picked is gone), the first place of the first entry.
  $effect(() => {
    if (!map || picked) return;
    const first = sections[0]?.entries[0];
    if (!first) return;
    untrack(() => {
      view.picked = { entry: first.id, place: 0 };
      if (!(view.open ?? []).includes(first.id)) view.open = [...(view.open ?? []), first.id];
    });
  });

  // An entry picked elsewhere (the diff's link) is scrolled to once the map shows.
  $effect(() => {
    const entry = view.picked?.entry;
    if (!view.reveal || entry == null || !map) return;
    untrack(async () => {
      view.reveal = false;
      await tick();
      document.querySelector(`[data-entry="${entry}"]`)?.scrollIntoView({ block: "nearest" });
    });
  });

  const isOpen = (e: ImpactEntry) => (view.open ?? []).includes(e.id);
  function toggle(e: ImpactEntry) {
    const open = new Set(view.open ?? []);
    if (open.has(e.id)) open.delete(e.id);
    else {
      open.add(e.id);
      // Opening an entry shows its first place.
      if (view.picked?.entry !== e.id && e.locations.length) view.picked = { entry: e.id, place: 0 };
    }
    view.open = [...open];
  }
  const pick = (e: ImpactEntry, place: number) => (view.picked = { entry: e.id, place });
  const isPicked = (e: ImpactEntry, place: number) => view.picked?.entry === e.id && view.picked.place === place;

  async function ask(update: boolean) {
    asking = true;
    error = null;
    try {
      await api.mapImpact(id, update);
      asked = true;
      notice = "The agent got the request; the map changes here as it works. Its Terminal shows what it does.";
    } catch (e) {
      error = `Could not ask the agent: ${e}`;
    } finally {
      asking = false;
    }
  }
</script>

<div class="impact" use:externalLinks role="tabpanel" aria-label="Impact">
  {#if error}
    <div class="note bad" role="alert"><Icon name="error" size={14} /><span class="grow">{error}</span>
      <IconButton icon="x" label="Dismiss" size={22} onclick={() => (error = null)} /></div>
  {/if}
  {#if notice}
    <div class="note" role="status" data-notice><Icon name="info" size={14} /><span class="grow">{notice}</span>
      {#if onterminal}<Button size="sm" variant="quiet" icon="terminal" onclick={onterminal}>Open Terminal</Button>{/if}
      <IconButton icon="x" label="Dismiss" size={22} onclick={() => (notice = null)} /></div>
  {/if}
  {#if map && stale}
    <div class="note agent" role="status" data-stale-map>
      <span class="lead"><Mark who="agent" /></span>
      <span class="grow">The map is of <code>{short(map.rev)}</code>: {stale}.</span>
      {#if noAgent || waits}<span class="why">{noAgent ?? "The agent is waiting for your permission"}</span>{/if}
      <Button size="sm" variant="agent" mark="agent" onclick={() => ask(true)} loading={asking} disabled={!!noAgent || waits}
        title="Ask the agent to map the change again, at its head" data-update-map>Update the map</Button>
    </div>
  {/if}

  {#if !shown}
    <p class="hint">Loading…</p>
  {:else if !map}
    <div class="empty" data-impact-empty>
      <h2>How the change affects the rest of the code</h2>
      <p>The agent maps the change: what it means for the project as a whole, the code outside the diff that it
        reaches, code elsewhere that does the same, and the way from an entry point to the changed code. Every place
        it names opens here with its code, also in files the diff does not touch.</p>
      {#if !hasWorktree}
        <p class="muted">It needs the item's worktree, where the agent has the whole repository: Start working, above.
          On a pull request someone else wrote, the agent maps it as soon as it starts.</p>
      {:else if working || asked}
        <p class="working" role="status" data-mapping><StatusDot status="working" /> The agent is working; the map shows here as it adds to it.</p>
      {:else}
        {#if noAgent || waits}<p class="muted">{noAgent ?? "The agent is waiting for your permission: answer it in its Terminal first."}</p>{/if}
        <div><Button variant="agent" mark="agent" onclick={() => ask(false)} loading={asking} disabled={!!noAgent || waits} data-map>Ask agent to map the change</Button></div>
      {/if}
    </div>
  {:else}
    <div class="body">
      <div class="map" data-map-list>
        <div class="map-head">
          <Mark who="agent" />
          <span class="grow">Mapped by the agent at <code title={map.rev}>{short(map.rev)}</code></span>
          {#if working && asked}<span class="working"><StatusDot status="working" size={10} /> mapping…</span>{/if}
          <Button size="sm" variant="quiet" mark="agent" onclick={() => ask(!!stale)} disabled={!!noAgent || waits || asking}
            title={noAgent ?? "Ask the agent to map the change again"} data-map-again>Map again</Button>
        </div>
        {#if map.overview}
          <section class="overview" data-overview>
            <h3 class="label-caps">What changes</h3>
            <div class="markdown">{@html markdown(map.overview)}</div>
          </section>
        {/if}
        {#each sections as s (s.kind)}
          <section class="section" data-section={s.kind}>
            <h3 class="sec-head" title={s.hint}><span class="label-caps">{s.title}</span><span class="count">{s.entries.length}</span></h3>
            <ul class="entries">
              {#each s.entries as e (e.id)}
                {@const open = isOpen(e)}
                <li class="entry" class:open data-entry={e.id}>
                  <button class="entry-head" aria-expanded={open} onclick={() => toggle(e)}>
                    <span class="chev" class:folded={!open}><Icon name="chevron-down" size={12} /></span>
                    <span class="att">{#if e.attention}<span title="Worth checking: behaviour may break here, or the change may miss it"><Icon name="warning" size={14} label="Worth checking" /></span>{/if}</span>
                    <span class="title">{e.title}</span>
                    <span class="count">{placesText(e.locations.length)}</span>
                  </button>
                  {#if open}
                    <div class="entry-body">
                      {#if e.body}<div class="markdown">{@html markdown(e.body)}</div>{/if}
                      {#if e.changed}
                        {@const c = e.changed}
                        <button class="place from" class:picked={isPicked(e, -1)} aria-pressed={isPicked(e, -1)} onclick={() => pick(e, -1)} data-place="from">
                          <span class="label">Follows from</span>
                          <code>{splitPath(c.path).name}:{linesOf(c)}</code>
                          {#if c.inDiff}<Pill size="xs">in the diff</Pill>{/if}
                        </button>
                      {/if}
                      <ol class="places" class:steps={e.kind === "path"}>
                        {#each e.locations as l, i}
                          {@const p = splitPath(l.path)}
                          <li>
                            <button class="place" class:picked={isPicked(e, i)} aria-pressed={isPicked(e, i)} onclick={() => pick(e, i)}
                              title="{l.path}:{linesOf(l)}" data-place={i}>
                              {#if e.kind === "path"}<span class="step">{i + 1}</span>{/if}
                              <code><span class="dir">{p.dir}</span>{p.name}:{linesOf(l)}</code>
                              {#if l.inDiff}<Pill size="xs">in the diff</Pill>{/if}
                              {#if l.note}<span class="pnote">{l.note}</span>{/if}
                            </button>
                          </li>
                        {/each}
                      </ol>
                    </div>
                  {/if}
                </li>
              {/each}
            </ul>
          </section>
        {:else}
          {#if !map.overview}<p class="hint">The map has no entries yet.</p>{/if}
        {/each}
      </div>
      <div class="code">
        {#if picked && pickedEntry}
          <CodePeek {id} rev={pickedEntry.rev} place={picked} {noAgent} {agentStatus} {onterminal} {onshow}
            onerror={(m) => (error = m)} />
        {:else}
          <p class="hint">Pick a place to see its code.</p>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .impact { flex: 1; display: flex; flex-direction: column; height: 100%; min-height: 0; container-type: inline-size; }
  .grow { flex: 1; min-width: 0; }
  .muted { color: var(--text-muted); }
  .hint { margin: var(--sp-9); font-size: var(--fs-12); color: var(--text-muted); }
  .count { flex: none; font-family: var(--font-mono); font-size: var(--fs-11); font-weight: var(--fw-regular); color: var(--text-faint); }

  /* Notes under the tab bar, as in the review. */
  .note {
    flex: none; display: flex; align-items: center; gap: var(--sp-4); min-height: 28px; padding: var(--sp-1) var(--sp-3) var(--sp-1) var(--sp-7);
    border-bottom: 1px solid var(--border); background: var(--info-bg); font-size: var(--fs-12); box-sizing: border-box;
  }
  .note > :global(svg) { flex: none; color: var(--info); }
  .note.bad { background: var(--bad-bg); }
  .note.bad > :global(svg) { color: var(--bad); }
  .note.agent { background: var(--agent-bg); }
  .note .lead { width: 14px; display: flex; justify-content: center; }
  .note code { font-size: var(--fs-code-inline); }
  .note .why { flex: 0 1 auto; min-width: 0; color: var(--text-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

  .empty { max-width: 620px; margin: var(--sp-9); display: flex; flex-direction: column; gap: var(--sp-5); font-size: var(--fs-13); line-height: var(--lh-13); }
  .empty h2 { margin: 0; font-size: var(--fs-15); line-height: var(--lh-15); font-weight: var(--fw-semibold); }
  .empty p { margin: 0; }
  .working { display: inline-flex; align-items: center; gap: var(--sp-3); color: var(--agent-strong); font-size: var(--fs-12); }

  .body { flex: 1; min-height: 0; display: flex; }
  .map {
    flex: 0 0 44%; min-width: 340px; max-width: 640px; overflow-y: auto; box-sizing: border-box;
    padding: var(--sp-5) var(--sp-6) var(--sp-9); border-right: 1px solid var(--border);
    display: flex; flex-direction: column; gap: var(--sp-7); font-size: var(--fs-12); line-height: var(--lh-12);
  }
  .code { flex: 1; min-width: 0; min-height: 0; display: flex; background: var(--surface-editor); }
  /* Narrow: the code goes under the map. */
  @container (max-width: 860px) {
    .body { flex-direction: column; }
    .map { flex: 0 1 auto; max-height: 50%; min-width: 0; max-width: none; border-right: 0; border-bottom: 1px solid var(--border); }
    .code { min-height: 240px; }
  }
  .map-head { display: flex; align-items: center; gap: var(--sp-4); min-height: var(--h-control); color: var(--text-muted); }
  .map-head code { font-size: var(--fs-code-inline); color: var(--text); }
  h3 { margin: 0; }
  .overview { display: flex; flex-direction: column; gap: var(--sp-3); }
  .section { display: flex; flex-direction: column; gap: var(--sp-2); }
  .sec-head { display: flex; align-items: center; gap: var(--sp-3); height: var(--h-section); font-size: inherit; font-weight: inherit; }
  .entries { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: var(--sp-1); }
  .entry { border-radius: var(--r-md); }
  .entry.open { background: var(--surface-1); box-shadow: inset 0 0 0 1px var(--border); }
  .entry-head {
    width: 100%; display: flex; align-items: flex-start; gap: var(--sp-3); padding: var(--sp-3) var(--sp-4); box-sizing: border-box;
    border: 0; border-radius: var(--r-md); background: transparent; color: var(--text); font: inherit; text-align: left;
  }
  .entry-head:hover { background: var(--surface-hover); }
  .entry-head:focus-visible { outline-offset: -2px; }
  .chev { flex: none; display: flex; padding-top: 2px; color: var(--text-faint); transition: transform var(--dur-fast) var(--ease); }
  .chev.folded { transform: rotate(-90deg); }
  .att { flex: none; width: 14px; display: flex; padding-top: 1px; color: var(--warn); }
  .entry-head .title { flex: 1; min-width: 0; font-weight: var(--fw-semibold); }
  .entry-head .count { padding-top: 1px; }
  .entry-body { display: flex; flex-direction: column; gap: var(--sp-3); padding: 0 var(--sp-4) var(--sp-4) calc(var(--sp-4) + 12px + 14px + 2 * var(--sp-3)); }
  .places { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 1px; }
  .place {
    width: 100%; display: flex; flex-wrap: wrap; align-items: baseline; column-gap: var(--sp-3); row-gap: 0; padding: var(--sp-2) var(--sp-3);
    box-sizing: border-box; border: 0; border-radius: var(--r-sm); background: transparent; color: var(--text); font: inherit; text-align: left;
  }
  .place:hover { background: var(--surface-hover); }
  .place.picked { background: var(--surface-selected); box-shadow: inset 2px 0 0 var(--agent); }
  .place:focus-visible { outline-offset: -2px; }
  .place code { font-size: var(--fs-code-inline); overflow-wrap: anywhere; }
  .place .dir { color: var(--text-faint); }
  .place .pnote { flex-basis: 100%; color: var(--text-muted); }
  .from .label { color: var(--text-muted); }
  .step {
    flex: none; min-width: 16px; height: 16px; border-radius: var(--r-pill); box-shadow: inset 0 0 0 1px var(--border-strong);
    font-family: var(--font-mono); font-size: var(--fs-11); line-height: 16px; text-align: center; color: var(--text-muted);
  }
  .markdown { font-size: var(--fs-13); line-height: 20px; overflow-wrap: anywhere; }
  .markdown :global(p) { margin: 0 0 var(--sp-4); text-wrap: pretty; }
  .markdown :global(:last-child) { margin-bottom: 0; }
  .markdown :global(ul), .markdown :global(ol) { margin: 0 0 var(--sp-4); padding-left: 20px; }
  .markdown :global(h1), .markdown :global(h2), .markdown :global(h3), .markdown :global(h4) {
    margin: var(--sp-3) 0 var(--sp-3); font-size: var(--fs-13); font-weight: var(--fw-semibold);
  }
  .markdown :global(pre) { background: var(--surface-2); padding: var(--sp-4) var(--sp-5); overflow-x: auto; border-radius: var(--r-md); font-size: 12px; line-height: 18px; }
  .markdown :global(code) { font-family: var(--font-mono); font-size: var(--fs-code-inline); padding: 1px 5px; border-radius: var(--r-sm); background: var(--surface-2); }
  .markdown :global(pre code) { padding: 0; background: none; }
  /* Blue is "yours", not "clickable": links are underlined text. */
  .markdown :global(a) { color: var(--text); text-decoration: underline; text-decoration-color: var(--border-strong); }
  .markdown :global(table) { border-collapse: collapse; margin-bottom: var(--sp-4); }
  .markdown :global(td), .markdown :global(th) { border: 1px solid var(--border); padding: 3px 6px; }
  .markdown :global(blockquote) { border-left: 3px solid var(--border); margin: 0 0 var(--sp-4); padding-left: 10px; color: var(--text-muted); }
</style>
