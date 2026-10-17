<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!--
  NEW (review 2026-10-17). CI state as one glyph, used in Inbox rows, the item meta line,
  Overview checks and Task Overview links.
    pass     ✓ in --ok
    fail     ✗ in --bad
    pending  a hollow diamond in --warn  (was a hollow circle: the agent's shape, right under the agent dot)
    skipped  a short dash in --text-faint (Overview checks only)
  CI is a GitHub fact, so "pending" takes GitHub's shape; pass and fail keep the conventional marks.
  Callers hold GitHub's words; map them with ciState() (Inbox.svelte:175, WorkItem.svelte:238–243,
  TaskOverview.svelte:15–19, Overview.svelte:13).
-->
<script lang="ts" module>
  export type Ci = "pass" | "fail" | "pending" | "skipped" | null | undefined;
  export const CI_LABEL = { pass: "CI passing", fail: "CI failing", pending: "CI running", skipped: "Skipped" } as const;

  /** GitHub's rollup state or check conclusion/status → a CiMark state. Unknown → null (no mark). */
  export function ciState(github: string | null | undefined): Ci {
    switch ((github ?? "").toUpperCase()) {
      case "SUCCESS": case "PASSED": case "PASSING":
        return "pass";
      case "FAILURE": case "FAILED": case "FAILING": case "ERROR": case "TIMED_OUT": case "CANCELLED": case "ACTION_REQUIRED": case "STARTUP_FAILURE":
        return "fail";
      case "PENDING": case "QUEUED": case "IN_PROGRESS": case "EXPECTED": case "WAITING": case "REQUESTED": case "RUNNING":
        return "pending";
      case "SKIPPED": case "NEUTRAL": case "STALE":
        return "skipped";
      default:
        return null;
    }
  }
</script>

<script lang="ts">
  import Icon from "./Icon.svelte";
  let { state, size = 12, label }: { state: Ci; size?: number; label?: string } = $props();
</script>

{#if state === "pass" || state === "fail"}
  <span class="ci {state}" role="img" aria-label={label ?? CI_LABEL[state]}>
    <Icon name={state === "pass" ? "check" : "x"} {size} />
  </span>
{:else if state === "pending" || state === "skipped"}
  <svg class="ci {state}" width={size} height={size} viewBox="0 0 16 16" role="img" aria-label={label ?? CI_LABEL[state]}>
    {#if state === "pending"}<path d="M8 3.2L12.8 8 8 12.8 3.2 8z" />{:else}<path d="M4.5 8h7" />{/if}
  </svg>
{/if}

<style>
  .ci { flex: none; display: inline-flex; }
  .pass { color: var(--ok); }
  .fail { color: var(--bad); }
  .pending path { fill: none; stroke: var(--warn); stroke-width: 2; stroke-linejoin: round; }
  .skipped path { fill: none; stroke: var(--text-faint); stroke-width: 2; stroke-linecap: round; }
</style>
