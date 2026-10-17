<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!--
  The browser over the window: the repo's open PRs and issues, searched with GitHub's syntax,
  to add to the Inbox (they are listed under Open) or take out again. It stays open after
  adding, so several can be added. Esc closes; ↑/↓ move through the list, Enter adds the item
  (or opens it, when it is in the Inbox).
-->
<script lang="ts">
  import { onMount, untrack } from "svelte";
  import Button from "./controls/Button.svelte";
  import Icon from "./controls/Icon.svelte";
  import IconButton from "./controls/IconButton.svelte";
  import Pill from "./controls/Pill.svelte";
  import Segmented from "./controls/Segmented.svelte";
  import { api, type SearchHit } from "./lib/api";
  import { browser } from "./lib/layout.svelte";

  let {
    onselect,
    onchanged,
  }: {
    onselect: (id: number) => void;
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
      if (hit.itemId == null) add(hit);
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

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && (browser.open = false)}>
  <div class="browse" role="dialog" aria-modal="true" aria-label="Browse open pull requests and issues" tabindex="-1" onkeydown={onKey} data-browse>
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
      {#if loading}<span class="muted">Searching…</span>{/if}
    </label>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <ul bind:this={list} aria-label="Open {browser.kind === 'pr' ? 'pull requests' : 'issues'}" aria-busy={loading}>
      {#each hits as hit, i (`${hit.kind}${hit.number}`)}
        {@const pending = busy.includes(hit.number)}
        <li class:cursor={i === cursor} data-browse-hit={hit.number} onmouseenter={() => (cursor = i)}>
          <span class="kind"><Icon name={hit.kind === "pr" ? "pull-request" : "issue"} size={14} label={hit.kind === "pr" ? "Pull request" : "Issue"} /></span>
          <span class="lines">
            <span class="line1"><span class="num">#{hit.number}</span><span class="title" title={hit.title}>{hit.title}</span></span>
            <span class="line2">
              <span>{hit.author}</span>
              {#if hit.isDraft}<Pill size="sm">draft</Pill>{/if}
              {#each hit.labels as label}<Pill size="sm">{label}</Pill>{/each}
              <span class="faint">updated {ago(hit.updatedAt)}</span>
            </span>
          </span>
          <span class="acts">
            {#if hit.itemId == null}
              <Button size="sm" icon="plus" onclick={() => add(hit)} loading={pending} data-browse-add>Add to Open</Button>
            {:else}
              <Pill size="sm" tone="you">in Inbox</Pill>
              {#if hit.removable}
                <Button size="sm" variant="quiet" onclick={() => remove(hit)} loading={pending} data-browse-remove>Remove</Button>
              {/if}
              <Button size="sm" onclick={() => open(hit)} data-browse-open>Open</Button>
            {/if}
          </span>
        </li>
      {:else}
        {#if !loading && !error}
          <li class="empty">{browser.text.trim() ? `No open ${browser.kind === "pr" ? "pull request" : "issue"} matches “${browser.text.trim()}”.` : `No open ${browser.kind === "pr" ? "pull requests" : "issues"}.`}</li>
        {/if}
      {/each}
    </ul>
    <p class="foot">
      {#if hits.length >= PAGE}The {PAGE} most recently updated — search to narrow.{:else}Added items are listed under Open in the Inbox.{/if}
      <span class="grow"></span><span class="faint">↑↓ Enter · Esc closes</span>
    </p>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; z-index: 40; display: grid; place-items: center; background: var(--scrim); }
  .browse {
    width: min(880px, calc(100vw - 48px)); height: min(80vh, calc(100vh - 48px)); box-sizing: border-box;
    display: flex; flex-direction: column; gap: var(--sp-4); padding: var(--sp-6) var(--sp-6) var(--sp-4);
    border-radius: var(--r-xl); background: var(--surface-raised); box-shadow: var(--shadow-pop);
  }
  .browse:focus { outline: none; }
  .head { display: flex; align-items: center; gap: var(--sp-5); }
  .head strong { font-size: 14px; font-weight: var(--fw-semibold); }
  .grow { flex: 1; }
  .search {
    display: flex; align-items: center; gap: var(--sp-3); height: 32px; padding: 0 var(--sp-4); box-sizing: border-box; flex: none;
    border: 1px solid var(--border-strong); border-radius: var(--r-md); background: var(--surface-0); color: var(--text-faint);
  }
  .search:focus-within { border-color: var(--focus); box-shadow: 0 0 0 3px var(--focus-soft); }
  .search input { flex: 1; min-width: 0; height: 28px; border: 0; outline: none; padding: 0; background: transparent; font-size: var(--fs-13); }
  .search input:focus { outline: none; }
  .muted { color: var(--text-muted); font-size: var(--fs-12); }
  .faint { color: var(--text-faint); }
  .error { margin: 0; font-size: var(--fs-12); color: var(--bad); }
  ul { flex: 1; min-height: 0; overflow-y: auto; list-style: none; margin: 0 calc(-1 * var(--sp-3)); padding: 0; display: flex; flex-direction: column; gap: 1px; }
  li { display: flex; align-items: center; gap: var(--sp-4); padding: var(--sp-3) var(--sp-4); border-radius: var(--r-md); }
  li.cursor { background: var(--surface-hover); }
  .kind { flex: none; display: flex; color: var(--text-faint); align-self: flex-start; padding-top: 2px; }
  .lines { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .line1 { display: flex; align-items: baseline; gap: var(--sp-3); min-width: 0; font-size: var(--fs-13); }
  .num { flex: none; font-family: var(--font-mono); font-size: 11.5px; color: var(--text-muted); }
  .title { min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-weight: var(--fw-medium); }
  .line2 { display: flex; align-items: center; gap: var(--sp-3); min-width: 0; overflow: hidden; white-space: nowrap; font-size: 11.5px; color: var(--text-muted); }
  .acts { flex: none; display: flex; align-items: center; gap: var(--sp-3); }
  .empty { padding: var(--sp-6) var(--sp-4); color: var(--text-faint); font-size: var(--fs-12); }
  .foot { margin: 0; display: flex; gap: var(--sp-4); font-size: var(--fs-11); color: var(--text-muted); }
</style>
