<!--
  Buttons. Filled agent / GitHub variants only where an action leaves Ariekei for that party;
  "you" is the reviewer's own action (Reviewed, next).
-->
<script lang="ts">
  import type { Snippet } from "svelte";
  import type { HTMLButtonAttributes } from "svelte/elements";
  import Icon from "./Icon.svelte";
  import Kbd from "./Kbd.svelte";
  import Spinner from "./Spinner.svelte";
  import type { IconName } from "./icons";

  type Variant = "primary" | "secondary" | "quiet" | "danger" | "agent" | "gh" | "you";
  let {
    variant = "secondary",
    size = "md",
    icon,
    kbd,
    loading = false,
    pressed,
    class: cls = "",
    children,
    disabled,
    ...rest
  }: HTMLButtonAttributes & {
    variant?: Variant;
    size?: "sm" | "md" | "lg";
    icon?: IconName;
    kbd?: string;
    loading?: boolean;
    pressed?: boolean;
    children?: Snippet;
  } = $props();
</script>

<button {...rest} class="btn {variant} {size} {cls}" disabled={disabled || loading} aria-busy={loading || undefined} aria-pressed={pressed}>
  {#if loading}<Spinner />{:else if icon}<Icon name={icon} />{/if}
  {@render children?.()}
  {#if kbd}<Kbd key={kbd} />{/if}
</button>

<style>
  .btn {
    display: inline-flex; align-items: center; justify-content: center; gap: var(--sp-3);
    height: var(--h-control); padding: 0 var(--sp-5); box-sizing: border-box;
    border: 1px solid transparent; border-radius: var(--r-md);
    font: inherit; font-size: var(--fs-12); font-weight: var(--fw-medium); white-space: nowrap; cursor: pointer;
    transition: background-color var(--dur-fast) var(--ease);
  }
  .sm { height: var(--h-control-sm); padding: 0 var(--sp-4); border-radius: var(--r-sm); }
  .lg { height: var(--h-control-lg); padding: 0 14px; font-size: var(--fs-13); }
  .btn:disabled { opacity: 0.45; cursor: not-allowed; }
  .btn[aria-busy="true"] { opacity: 1; cursor: progress; }

  .primary { background: var(--you); border-color: var(--you); color: var(--text-on-accent); }
  .primary:hover:not(:disabled) { background: var(--you-strong); border-color: var(--you-strong); }
  .primary:active:not(:disabled), .danger:active:not(:disabled) { box-shadow: inset 0 1px 2px rgb(0 0 0 / 0.25); }

  .secondary { background: var(--surface-raised); border-color: var(--border-strong); color: var(--text); }
  .secondary:hover:not(:disabled) { background: var(--surface-2); }
  .secondary:active:not(:disabled) { background: var(--surface-active); }
  .secondary[aria-pressed="true"] { background: var(--you-bg); border-color: var(--you); color: var(--you-strong); }

  .quiet { background: transparent; color: var(--text-muted); }
  .quiet:hover:not(:disabled) { background: var(--surface-hover); color: var(--text); }
  .quiet:active:not(:disabled) { background: var(--surface-active); color: var(--text); }

  .danger { background: var(--bad); border-color: var(--bad); color: var(--text-on-accent); }
  .danger:hover:not(:disabled) { filter: brightness(0.92); }

  .agent { background: var(--agent); border-color: var(--agent); color: var(--text-on-accent); }
  .gh { background: var(--gh); border-color: var(--gh); color: var(--text-on-accent); }
  .agent:hover:not(:disabled), .gh:hover:not(:disabled) { filter: brightness(0.92); }

  .you { background: var(--you-bg); border-color: var(--you); color: var(--you-strong); font-weight: var(--fw-semibold); }
  .you:hover:not(:disabled) { background: color-mix(in srgb, var(--you-bg) 80%, var(--you)); }
</style>
