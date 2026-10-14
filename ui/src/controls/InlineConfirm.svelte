<!-- An inline confirmation in place of the button that asked (Done → clean up, coverage guard). -->
<script lang="ts">
  import Button from "./Button.svelte";

  let {
    tone = "bad",
    text,
    confirm,
    onconfirm,
    oncancel,
    ...rest
  }: {
    tone?: "bad" | "warn";
    text: string;
    confirm: string;
    onconfirm: () => void;
    oncancel: () => void;
    [key: `data-${string}`]: unknown;
  } = $props();
</script>

<div class="confirm {tone}" role={tone === "warn" ? "alertdialog" : "group"} aria-label={text} {...rest}>
  <span class="text">{text}</span>
  <Button size="sm" variant={tone === "bad" ? "danger" : "primary"} onclick={onconfirm}>{confirm}</Button>
  <Button size="sm" variant="quiet" onclick={oncancel}>Cancel</Button>
</div>

<style>
  .confirm {
    display: inline-flex; align-items: center; gap: var(--sp-3); height: var(--h-control); min-width: 0;
    padding: 0 var(--sp-2) 0 var(--sp-5); border-radius: var(--r-md); font-size: var(--fs-12); font-weight: var(--fw-regular);
  }
  .text { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text); }
  .bad { background: var(--bad-bg); }
  .warn { background: var(--warn-bg); }
</style>
