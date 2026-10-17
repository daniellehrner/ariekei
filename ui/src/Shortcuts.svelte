<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<!--
  The keyboard shortcuts (?): the app's, which work anywhere outside text fields and the
  terminal, and the review's, which work while the Review tab is shown.
-->
<script lang="ts">
  import IconButton from "./controls/IconButton.svelte";
  import Kbd from "./controls/Kbd.svelte";
  import { APP_SHORTCUTS, SHORTCUTS } from "./lib/keys";
  import { shortcuts } from "./lib/layout.svelte";
  import { modal } from "./lib/modal";

  const SECTIONS: [string, [string[], string][]][] = [
    ["Anywhere", APP_SHORTCUTS],
    ["In the review", SHORTCUTS],
  ];
</script>

{#if shortcuts.open}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
  <dialog use:modal aria-label="Keyboard shortcuts" aria-modal="true" data-shortcuts
    oncancel={(e) => { e.preventDefault(); shortcuts.open = false; }}
    onclick={(e) => e.target === e.currentTarget && (shortcuts.open = false)}>
    <div class="box">
      <div class="dialog-head">
        <strong>Keyboard shortcuts</strong>
        <!-- svelte-ignore a11y_autofocus -->
        <IconButton icon="x" label="Close (Esc)" size={26} onclick={() => (shortcuts.open = false)} autofocus />
      </div>
      {#each SECTIONS as [title, list]}
        <section>
          <h3 class="label-caps">{title}</h3>
          <dl>
            {#each list as [keys, what]}
              <dt>{#each keys as k}<Kbd key={k} />{/each}</dt>
              <dd>{what}</dd>
            {/each}
          </dl>
        </section>
      {/each}
      <p class="note">Review keys work anywhere in the review except in fields, the editable diff and the terminal.</p>
    </div>
  </dialog>
{/if}

<style>
  /* The dialog covers the window and is the scrim; the box is centred. */
  dialog {
    position: fixed; inset: 0; width: 100%; height: 100%; max-width: none; max-height: none; margin: 0; padding: 0; border: 0;
    display: grid; place-items: center; background: var(--scrim); color: var(--text);
  }
  dialog::backdrop { background: transparent; }
  .box {
    width: 440px; max-width: calc(100% - 32px); max-height: calc(100% - 32px); overflow-y: auto; padding: var(--sp-7) 18px;
    box-sizing: border-box; border-radius: var(--r-xl); background: var(--surface-raised); box-shadow: var(--shadow-pop);
    display: flex; flex-direction: column; gap: var(--sp-5);
  }
  .dialog-head { display: flex; align-items: center; }
  .dialog-head strong { flex: 1; font-size: var(--fs-13); line-height: var(--lh-13); font-weight: var(--fw-semibold); }
  section { display: flex; flex-direction: column; gap: var(--sp-3); }
  h3 { margin: 0; }
  dl { margin: 0; display: grid; grid-template-columns: 96px minmax(0, 1fr); row-gap: var(--sp-3); font-size: var(--fs-12); line-height: var(--lh-12); }
  dt { display: flex; align-items: center; gap: var(--sp-2); }
  dd { margin: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .note { margin: 0; font-size: var(--fs-12); line-height: var(--lh-12); color: var(--text-muted); }
</style>
