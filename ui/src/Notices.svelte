<!--
  In-app notices (the backend's `notice` event, see notify.rs): shown instead of an OS
  notification while this window is focused. Clicking one opens its item; "waiting for your
  permission" stays until the agent moves on, the rest fade after a while. Each is tinted in
  its kind's colour and slides in with a pulse, so it catches the eye in a corner; a
  permission request keeps pulsing until it is answered.
-->
<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { fly } from "svelte/transition";
  import AttentionPip, { type Attention } from "./controls/AttentionPip.svelte";
  import IconButton from "./controls/IconButton.svelte";
  import { agents } from "./lib/agent.svelte";
  import { api } from "./lib/api";
  import { FADE_MS, fades, stillCurrent, withNotice, type Notice, type NoticeKind } from "./lib/notices";

  const PIP: Record<NoticeKind, Attention> = {
    "needs-you": "needs-you", finished: "review", error: "blocked", "ci-failing": "ci-failing", activity: "activity",
    lifecycle: "none",
  };

  // The item this window shows: notices about it are done once it is on screen.
  let { selected }: { selected: number | null } = $props();
  let notices: Notice[] = $state([]);
  let next = 0;
  const still = matchMedia("(prefers-reduced-motion: reduce)").matches;

  function dismiss(id: number) {
    notices = notices.filter((n) => n.id !== id);
  }

  function open(n: Notice) {
    dismiss(n.id);
    if (n.item != null) api.openItem(n.item);
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
    {#each notices as n (n.id)}
      <div class="notice" data-notice={n.kind} out:fly={{ x: 48, duration: still ? 0 : 180 }}>
        <button class="body" onclick={() => open(n)} disabled={n.item == null}
          title={n.item == null ? undefined : "Open"}>
          <span class="pip"><AttentionPip kind={PIP[n.kind]} size={16} label="" /></span>
          <span class="text">
            <strong>{n.title}</strong>
            <span>{n.body}</span>
          </span>
        </button>
        <IconButton icon="x" label="Dismiss" size={22} onclick={() => dismiss(n.id)} />
      </div>
    {/each}
  </div>
{/if}

<style>
  .notices {
    position: fixed; right: var(--sp-7); bottom: var(--sp-7); z-index: 50;
    display: flex; flex-direction: column; gap: var(--sp-4);
    width: min(380px, calc(100vw - 2 * var(--sp-7)));
  }
  /* --tone: the kind's colour, as in the Inbox's attention pips. */
  .notice {
    --tone: var(--info);
    --lift: inset 4px 0 0 var(--tone), var(--shadow-pop), 0 0 22px color-mix(in srgb, var(--tone) 30%, transparent);
    display: flex; align-items: flex-start; gap: var(--sp-2); padding: var(--sp-2) var(--sp-2) var(--sp-2) var(--sp-4);
    background: color-mix(in srgb, var(--tone) 22%, var(--surface-raised));
    border: 1px solid color-mix(in srgb, var(--tone) 75%, transparent); border-radius: var(--r-lg);
    box-shadow: var(--lift);
    animation: arrive 420ms cubic-bezier(0.3, 1.4, 0.5, 1) both, pulse 1.2s 420ms ease-out 2;
  }
  .notice[data-notice="needs-you"] { --tone: var(--agent); animation-iteration-count: 1, infinite; animation-duration: 420ms, 1.6s; }
  .notice[data-notice="finished"] { --tone: var(--you); }
  .notice[data-notice="error"], .notice[data-notice="ci-failing"] { --tone: var(--bad); }
  .notice[data-notice="activity"] { --tone: var(--gh); }
  @keyframes arrive {
    from { opacity: 0; transform: translateX(calc(100% + var(--sp-7))); }
    to { opacity: 1; transform: none; }
  }
  @keyframes pulse {
    from { box-shadow: var(--lift), 0 0 0 0 color-mix(in srgb, var(--tone) 70%, transparent); }
    to { box-shadow: var(--lift), 0 0 0 12px color-mix(in srgb, var(--tone) 0%, transparent); }
  }
  @media (prefers-reduced-motion: reduce) {
    .notice { animation: none; }
  }
  .body {
    flex: 1; min-width: 0; display: flex; gap: var(--sp-4); align-items: flex-start; text-align: left;
    padding: var(--sp-3) var(--sp-4); border: 0; border-radius: var(--r-md); background: none; color: var(--text);
    font: inherit; font-size: var(--fs-12); line-height: var(--lh-12); cursor: pointer;
  }
  .body:hover:not(:disabled) { background: var(--surface-hover); }
  .body:disabled { cursor: default; color: var(--text); opacity: 1; }
  .body:focus-visible { outline: none; box-shadow: var(--focus-ring); }
  .pip { flex: none; display: flex; padding-top: 2px; }
  .pip:empty { display: none; }
  .text { display: flex; flex-direction: column; min-width: 0; }
  .text strong { font-weight: var(--fw-semibold); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .text span { color: color-mix(in srgb, var(--text) 82%, transparent); overflow-wrap: anywhere; }
</style>
