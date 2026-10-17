<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!--
  In-app notices (the backend's `notice` event, see notify.rs): shown instead of an OS
  notification while this window is focused. Clicking one opens its item; "waiting for your
  permission" stays until the agent moves on, the rest fade after a while. Each is a neutral
  raised card with its kind's pip (NoticeCard); only a permission request is tinted. The newest
  three show; older ones fold into "+N more" (docs/design.md, Notices).
-->
<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { fly } from "svelte/transition";
  import NoticeCard from "./controls/NoticeCard.svelte";
  import { agents } from "./lib/agent.svelte";
  import { api } from "./lib/api";
  import { openOn } from "./lib/layout.svelte";
  import { durationToken } from "./lib/tokens";
  import { FADE_MS, fades, shown, stillCurrent, withNotice, type Notice, type NoticeKind } from "./lib/notices";

  // The item this window shows: notices about it are done once it is on screen.
  let { selected }: { selected: number | null } = $props();
  let notices: Notice[] = $state([]);
  let next = 0;
  // The older notices, folded into "+N more", are shown too.
  let unfolded = $state(false);
  const stack = $derived(shown(notices, unfolded));
  $effect(() => {
    if (notices.length <= 1) unfolded = false;
  });
  // The exit: a fade and a short slide over --dur (nothing with reduced motion).
  const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const DUR = durationToken("--dur", 120);

  function dismiss(id: number) {
    notices = notices.filter((n) => n.id !== id);
  }

  // Notices from the agent (waiting, finished, stopped with an error) open its Terminal: what
  // it said or asks is there, whatever else the item wants.
  const FROM_AGENT: NoticeKind[] = ["needs-you", "finished", "error"];

  function open(n: Notice) {
    dismiss(n.id);
    if (n.item == null) return;
    if (FROM_AGENT.includes(n.kind)) Object.assign(openOn, { item: n.item, tab: "terminal" });
    api.openItem(n.item);
  }

  const current = (n: Notice) => stillCurrent(n, selected, (item) => agents[String(item)]?.status);

  // Assigns only when something is done, so the rerun it causes changes nothing.
  $effect(() => {
    if (!notices.every(current)) notices = notices.filter(current);
  });

  onMount(() => {
    const unlisten = listen<Omit<Notice, "id">>("notice", ({ payload }) => {
      const notice = { ...payload, id: next++ };
      if (!current(notice)) return;
      notices = withNotice(notices, notice);
      if (fades(notice)) setTimeout(() => dismiss(notice.id), FADE_MS);
    });
    return () => unlisten.then((f) => f());
  });
</script>

{#if notices.length}
  <div class="notices" role="status" aria-live="polite">
    {#if stack.folded}
      <button class="more" onclick={() => (unfolded = true)} data-more-notices>+{stack.folded} more</button>
    {/if}
    {#each stack.visible as n (n.id)}
      <div out:fly={{ x: 8, duration: still ? 0 : DUR }}>
        <NoticeCard notice={n} onopen={() => open(n)} ondismiss={() => dismiss(n.id)} />
      </div>
    {/each}
  </div>
{/if}

<style>
  /* Above the review's drafts dock when it is shown (ReviewView sets --dock-h). */
  .notices {
    position: fixed; right: var(--sp-7); bottom: max(var(--sp-7), calc(var(--dock-h, 0px) + var(--sp-6))); z-index: 50;
    display: flex; flex-direction: column; align-items: stretch; gap: var(--sp-4);
    width: min(var(--w-notice), calc(100vw - 2 * var(--sp-7)));
  }
  @media (max-width: 1199px) {
    .notices { width: min(320px, calc(100vw - 2 * var(--sp-7))); }
  }
  .more {
    align-self: flex-end; height: 22px; padding: 0 var(--sp-5); border: 0; border-radius: var(--r-pill);
    background: var(--surface-raised); box-shadow: var(--shadow-pop); color: var(--text-muted);
    font-size: var(--fs-12); line-height: var(--lh-12); font-weight: var(--fw-medium);
  }
  .more:hover { color: var(--text); }
</style>
