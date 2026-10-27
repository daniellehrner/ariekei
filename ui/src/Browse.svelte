<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!--
  The browser over the window: the repo's open PRs and issues, searched with GitHub's syntax,
  to start working on (added to the Inbox, the item opens and its agent starts in a worktree),
  only add to the Inbox (listed under Open, e.g. to review without a worktree) or take out
  again. It stays open after adding, so several can be added. Esc closes; ↑/↓ move through
  the list, Enter starts working on the item (or opens it, when it is in the Inbox).
-->
<script lang="ts">
  import { onMount, untrack } from "svelte";
  import Button from "./controls/Button.svelte";
  import Icon from "./controls/Icon.svelte";
  import IconButton from "./controls/IconButton.svelte";
  import Kbd from "./controls/Kbd.svelte";
  import Pill from "./controls/Pill.svelte";
  import Segmented from "./controls/Segmented.svelte";
  import Spinner from "./controls/Spinner.svelte";
  import { api, type SearchHit } from "./lib/api";
  import { browser } from "./lib/layout.svelte";
  import { modal } from "./lib/modal";

  let {
    onselect,
    onstart,
    onchanged,
  }: {
    onselect: (id: number) => void;
    /** Open item `id` and start working on it. */
    onstart: (id: number) => void;
    /** An item was added to or taken out of the Inbox. */
    onchanged: () => void;
  } = $props();

  const KINDS = [
    { value: "pr" as const, label: "Pull requests" },
    { value: "issue" as const, label: "Issues" },
  ];
  // GitHub's search returns at most this many.
  const PAGE = 50;

  let hits: SearchHit[] = $state([]);
  let loading = $state(true);
  let error: string | null = $state(null);
  // The hit the keys act on.
  let cursor = $state(0);
  // Hits being added or removed (by number).
  let busy: number[] = $state([]);
  let input: HTMLInputElement | undefined = $state();
  let list: HTMLElement | undefined = $state();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let asked = 0;

  onMount(() => {
    input?.focus();
    return () => clearTimeout(timer);
  });

  // Searches again when the kind or the text changes (typing settles first).
  $effect(() => {
    const kind = browser.kind;
    const text = browser.text.trim();
    const n = ++asked;
    // The first search (and one after an error) runs at once.
    const delay = untrack(() => (hits.length && !error ? 350 : 0));
    clearTimeout(timer);
    loading = true;
    timer = setTimeout(async () => {
      try {
        const found = await api.searchItems(kind, text);
        if (n !== asked) return;
        hits = found;
        error = null;
        cursor = 0;
      } catch (e) {
        if (n === asked) error = String(e);
      } finally {
        if (n === asked) loading = false;
      }
    }, delay);
  });

  async function add(hit: SearchHit) {
    busy = [...busy, hit.number];
    try {
      hit.itemId = await api.trackItem(hit.kind, hit.number);
      hit.removable = true;
      onchanged();
    } catch (e) {
      error = String(e);
    } finally {
      busy = busy.filter((n) => n !== hit.number);
    }
  }

  // Adds it and leaves for its agent: the item makes the worktree, as its own Start working.
  async function startWorking(hit: SearchHit) {
    await add(hit);
    if (hit.itemId == null) return;
    browser.open = false;
    onstart(hit.itemId);
  }

  async function remove(hit: SearchHit) {
    if (hit.itemId == null) return;
    busy = [...busy, hit.number];
    try {
      await api.untrack(hit.itemId);
      hit.itemId = null;
      onchanged();
    } catch (e) {
      error = String(e);
    } finally {
      busy = busy.filter((n) => n !== hit.number);
    }
  }

  function open(hit: SearchHit) {
    if (hit.itemId == null) return;
    onselect(hit.itemId);
    browser.open = false;
  }

  function move(step: number) {
    if (!hits.length) return;
    cursor = (cursor + step + hits.length) % hits.length;
    list?.querySelectorAll("[data-browse-hit]")[cursor]?.scrollIntoView({ block: "nearest" });
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      browser.open = false;
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      move(e.key === "ArrowDown" ? 1 : -1);
    } else if (e.key === "Enter" && e.target === input && hits[cursor] && !busy.includes(hits[cursor].number)) {
      e.preventDefault();
      const hit = hits[cursor];
      if (hit.itemId == null) startWorking(hit);
      else open(hit);
    }
  }

  const ago = (iso: string) => {
    if (!iso) return "";
    const days = Math.floor((Date.now() - new Date(iso).getTime()) / 86_400_000);
    if (days < 1) return "today";
    if (days === 1) return "yesterday";
    if (days < 30) return `${days} days ago`;
    return new Date(iso).toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
  };
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
<dialog class="browse" use:modal aria-label="Browse open pull requests and issues" onkeydown={onKey}
  oncancel={(e) => { e.preventDefault(); browser.open = false; }}
  onclick={(e) => e.target === e.currentTarget && (browser.open = false)} data-browse>
  <div class="box">
    <div class="head">
      <strong>Browse</strong>
      <Segmented options={KINDS} bind:value={browser.kind} label="What to browse" data-browse-kind />
      <span class="grow"></span>
      <IconButton icon="x" label="Close (Esc)" size={26} onclick={() => (browser.open = false)} />
    </div>
    <label class="search">
      <Icon name="search" size={14} />
      <input bind:this={input} bind:value={browser.text} placeholder="Search — words, #123, author:name, label:bug"
        aria-label="Search open {browser.kind === 'pr' ? 'pull requests' : 'issues'}" data-browse-search />
      {#if loading}<span class="loading" role="status" aria-label="Searching"><Spinner /></span>{/if}
    </label>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <ul bind:this={list} aria-label="Open {browser.kind === 'pr' ? 'pull requests' : 'issues'}" aria-busy={loading}>
      {#each hits as hit, i (`${hit.kind}${hit.number}`)}
        {@const pending = busy.includes(hit.number)}
        <li class:cursor={i === cursor} data-browse-hit={hit.number}>
          <span class="kind"><Icon name={hit.kind === "pr" ? "pull-request" : "issue"} size={14} label={hit.kind === "pr" ? "Pull request" : "Issue"} /></span>
          <span class="lines">
            <span class="line1"><span class="num">#{hit.number}</span><span class="title" title={hit.title}>{hit.title}</span></span>
            <span class="line2">
              <span>{hit.author}</span>
              {#if hit.isDraft}<Pill size="xs">draft</Pill>{/if}
              {#each hit.labels as label}<Pill size="xs">{label}</Pill>{/each}
              <span class="faint">updated {ago(hit.updatedAt)}</span>
            </span>
          </span>
          <!-- A fixed slot, so the column never shifts. In the Inbox: the pill, or on the row
               under the cursor or pointer, Remove (for what the user added) and Open. -->
          <span class="acts">
            {#if hit.itemId == null}
              <Button size="sm" variant="quiet" icon="plus" onclick={() => add(hit)} disabled={pending}
                title="Add it to the Inbox's Open section, without a worktree or agent" data-browse-add>Add</Button>
              <Button size="sm" icon="play" onclick={() => startWorking(hit)} loading={pending}
                title="Add it to the Inbox, make (or reuse) its worktree and open Claude Code in it" data-browse-start>Start working</Button>
            {:else}
              <span class="at-rest"><Pill size="sm" tone="tint" data-in-inbox>In Inbox</Pill></span>
              <span class="on-row">
                {#if hit.removable}
                  <Button size="sm" variant="quiet" onclick={() => remove(hit)} loading={pending} data-browse-remove>Remove</Button>
                {/if}
                <Button size="sm" variant="quiet" onclick={() => open(hit)} data-browse-open>Open</Button>
              </span>
            {/if}
          </span>
        </li>
      {:else}
        {#if !loading && !error}
          <li class="empty">{browser.text.trim() ? `No open ${browser.kind === "pr" ? "pull requests match" : "issues match"} “${browser.text.trim()}”.` : `No open ${browser.kind === "pr" ? "pull requests" : "issues"}.`}</li>
        {/if}
      {/each}
    </ul>
    <p class="foot">
      {#if hits.length >= PAGE}The {PAGE} most recently updated — search to narrow.{:else}Items you add or start working on are listed under Open in the Inbox.{/if}
      <span class="grow"></span>
      <span class="keys"><Kbd key="↑" /><Kbd key="↓" /> move</span>
      <span class="keys"><Kbd key="Enter" /> start working or open</span>
      <span class="keys"><Kbd key="Esc" /> close</span>
    </p>
  </div>
</dialog>

<style>
  /* The dialog covers the window and is the scrim (custom properties don't reach ::backdrop in
     every webview); the box sits near the top and grows with its results. */
  dialog {
    position: fixed; inset: 0; width: 100%; height: 100%; max-width: none; max-height: none; margin: 0; padding: 0; border: 0;
    background: var(--scrim); color: var(--text); overflow: hidden;
  }
  dialog::backdrop { background: transparent; }
  dialog:focus { outline: none; }
  .box {
    position: absolute; top: 64px; left: 0; right: 0; margin: 0 auto;
    width: min(var(--w-dialog), calc(100% - 48px)); max-height: calc(100% - 128px);
    display: flex; flex-direction: column; overflow: hidden;
    border-radius: var(--r-xl); background: var(--surface-raised); box-shadow: var(--shadow-pop);
  }
  .head { flex: none; display: flex; align-items: center; gap: 10px; height: 44px; padding: 0 var(--sp-4) 0 14px; }
  .head strong { font-size: var(--fs-13); line-height: var(--lh-13); font-weight: var(--fw-semibold); }
  .grow { flex: 1; }
  .search {
    flex: none; display: flex; align-items: center; gap: var(--sp-3); height: var(--h-control-lg); margin: 0 var(--sp-6) var(--sp-4);
    padding: 0 var(--sp-4); box-sizing: border-box;
    border: 1px solid var(--border-strong); border-radius: var(--r-md); background: var(--surface-0); color: var(--text-faint);
  }
  .search:focus-within { border-color: var(--you); outline: 2px solid color-mix(in srgb, var(--focus) 25%, transparent); }
  .search input { flex: 1; min-width: 0; height: 26px; border: 0; outline: none; padding: 0; background: transparent; font-size: var(--fs-13); }
  .search input:focus { outline: none; }
  .loading { display: flex; color: var(--text-muted); }
  .faint { color: var(--text-faint); }
  .error { margin: 0 var(--sp-6) var(--sp-4); font-size: var(--fs-12); color: var(--bad); }
  ul { flex: 1 1 auto; min-height: 0; overflow-y: auto; list-style: none; margin: 0; padding: 0 var(--sp-3) var(--sp-3); display: flex; flex-direction: column; gap: 1px; }
  li { display: flex; align-items: center; gap: 10px; min-height: 44px; padding: var(--sp-2) var(--sp-4); box-sizing: border-box; border-radius: var(--r-md); }
  li:hover { background: var(--surface-hover); }
  /* The keyboard's cursor is yours, like the selected Inbox row. */
  li.cursor { background: var(--surface-selected); }
  .kind { flex: none; display: flex; color: var(--text-muted); align-self: flex-start; padding-top: 3px; }
  .lines { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .line1 { display: flex; align-items: baseline; gap: var(--sp-3); min-width: 0; font-size: var(--fs-13); }
  .num { flex: none; font-family: var(--font-mono); font-size: var(--fs-code-inline); color: var(--text-muted); }
  .title { min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-weight: var(--fw-medium); }
  .line2 { display: flex; align-items: center; gap: var(--sp-3); min-width: 0; overflow: hidden; white-space: nowrap; font-size: var(--fs-11); line-height: var(--lh-11); color: var(--text-muted); }
  .acts { flex: none; width: 200px; display: flex; align-items: center; justify-content: flex-end; gap: var(--sp-3); }
  .on-row { display: none; gap: var(--sp-3); }
  li:hover .on-row, li.cursor .on-row, li:focus-within .on-row { display: flex; }
  li:hover .at-rest, li.cursor .at-rest, li:focus-within .at-rest { display: none; }
  .empty { padding: var(--sp-6) var(--sp-5); font-size: var(--fs-12); line-height: var(--lh-12); color: var(--text-muted); }
  .empty:hover { background: none; }
  .foot {
    flex: none; margin: 0; display: flex; align-items: center; gap: var(--sp-6); height: 34px; padding: 0 14px;
    border-top: 1px solid var(--border); font-size: var(--fs-12); line-height: var(--lh-12); color: var(--text-muted);
  }
  .keys { display: inline-flex; align-items: center; gap: var(--sp-2); }
</style>
