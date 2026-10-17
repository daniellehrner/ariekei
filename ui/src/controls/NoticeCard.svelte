<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!--
  NEW (review 2026-10-17). One in-app notice, as Notices.svelte should draw it. Notices.svelte keeps
  its logic (listen, withNotice, fades, open, dismiss). Inside {#each}, keep a wrapper element for
  the exit transition (a transition can't sit on a component) and put the card in it:
    <div out:fly={{ x: 8, duration: still ? 0 : DUR }}><NoticeCard notice={n} onopen={() => open(n)} ondismiss={() => dismiss(n.id)} /></div>
  (DUR = the --dur token read once, as `still` is.) Replace the card CSS with the stack rules in HANDOFF.md (D5).

  Look: a raised neutral card, like an Inbox row lifted out of the list — the pip says who has to
  act, the reason line says it in words (same colours as the Inbox attention label). Only a
  permission request is tinted (agent-bg + agent-line, the waiting strip's look), because only it
  blocks work until you answer. No glow, no accent bar, no ring: the pulse is the pip growing and
  settling (its own shape), not a halo around the card.
-->
<script lang="ts" module>
  import type { Attention } from "./AttentionPip.svelte";
  import type { NoticeKind } from "../lib/notices";

  const PIP: Record<NoticeKind, Attention> = {
    "needs-you": "needs-you", finished: "review", error: "error", "ci-failing": "ci-failing", activity: "activity", lifecycle: "info",
  };
  // The reason, in the words of the Inbox attention label.
  const REASON: Record<NoticeKind, string> = {
    "needs-you": "Agent needs you", finished: "Ready for review", error: "Agent error", "ci-failing": "CI failing",
    activity: "New activity", lifecycle: "", // the body names who and what ("carol commented")
  };
</script>

<script lang="ts">
  import AttentionPip from "./AttentionPip.svelte";
  import IconButton from "./IconButton.svelte";
  import type { Notice } from "../lib/notices";

  let { notice, onopen, ondismiss }: { notice: Notice; onopen: () => void; ondismiss: () => void } = $props();

  // "#8012 Fix snap sync stall…" / "T1 Peers drop…": the number is set in mono like everywhere else.
  const parts = $derived(/^((?:#|T)\d+)\s+(.*)$/.exec(notice.title));
  const num = $derived(parts?.[1] ?? "");
  const title = $derived(parts?.[2] ?? notice.title);
  const pulses = $derived(notice.kind === "needs-you" ? 3 : 1);
</script>

<div class="notice {notice.kind}" data-notice={notice.kind}>
  <button class="body" onclick={onopen} disabled={notice.item == null} title={notice.item == null ? undefined : "Open"}>
    <span class="pip" style:--pulses={pulses}><AttentionPip kind={PIP[notice.kind]} size={14} label="" /></span>
    <span class="text">
      <span class="t1">{#if num}<code>{num}</code>{/if}<strong>{title}</strong></span>
      <span class="t2">
        {#if REASON[notice.kind]}<span class="why {notice.kind}">{REASON[notice.kind]}</span>{/if}
        {#if notice.body}<span class="detail">{REASON[notice.kind] ? " · " : ""}{notice.body}</span>{/if}
      </span>
    </span>
  </button>
  <IconButton icon="x" label="Dismiss" size={22} onclick={ondismiss} />
</div>

<style>
  .notice {
    display: flex; align-items: flex-start; gap: var(--sp-1);
    padding: var(--sp-1) var(--sp-1) var(--sp-1) 0;
    border-radius: var(--r-lg); background: var(--surface-raised); box-shadow: var(--shadow-pop);
    font-size: var(--fs-12); line-height: var(--lh-12);
  }
  .notice.needs-you { background: var(--agent-bg); box-shadow: var(--shadow-pop), inset 0 0 0 1px var(--agent-line); }
  .body {
    flex: 1; min-width: 0; display: flex; align-items: flex-start; gap: var(--sp-5);
    padding: 7px var(--sp-2) 7px var(--sp-6); border: 0; border-radius: var(--r-md);
    background: transparent; color: var(--text); font: inherit; text-align: left; cursor: pointer;
  }
  .body:hover { background: var(--surface-hover); }
  .body:focus-visible { outline: 2px solid var(--focus); outline-offset: -2px; }
  .body:disabled { cursor: default; opacity: 1; } /* app.css dims disabled buttons; a notice without an item is not disabled-looking */
  .pip { flex: none; margin-top: 2px; display: block; }
  .text { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .t1 { display: flex; gap: var(--sp-3); min-width: 0; }
  .t1 code { flex: none; font-family: var(--font-mono); font-size: var(--fs-code-inline); color: var(--text-muted); }
  .t1 strong { font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .t2 { color: var(--text-muted); overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; }
  .why { font-weight: 500; }
  .why.needs-you { color: var(--agent-strong); }
  .why.finished { color: var(--you-strong); }
  .why.error, .why.ci-failing { color: var(--bad); }
  .why.activity { color: var(--gh-strong); }

  /* Arrival: fade + 8px slide from the edge (the card), then the pip grows and settles
     once (three times for a permission request) — its own shape, so a square stays a square.
     Nothing moves after that. */
  .notice { animation: arrive var(--dur-slow) var(--ease) both; }
  .pip { transform-origin: 50% 50%; animation: pulse var(--dur-pulse) var(--ease) var(--dur-slow) var(--pulses, 1); }
  @keyframes arrive { from { opacity: 0; transform: translateX(8px); } }
  @keyframes pulse { 0%, 100% { transform: scale(1); } 35% { transform: scale(1.35); } }
  @media (prefers-reduced-motion: reduce) {
    .notice, .pip { animation: none; }
  }
</style>
