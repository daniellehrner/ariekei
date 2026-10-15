<!--
  The keyboard shortcuts (?): the app's, which work anywhere outside text fields and the
  terminal, and the review's, which work while the Review tab is shown.
-->
<script lang="ts">
  import IconButton from "./controls/IconButton.svelte";
  import Kbd from "./controls/Kbd.svelte";
  import { APP_SHORTCUTS, SHORTCUTS, shortcuts } from "./lib/keys.svelte";

  const SECTIONS: [string, [string[], string][]][] = [
    ["Anywhere", APP_SHORTCUTS],
    ["In the review", SHORTCUTS],
  ];
</script>

{#if shortcuts.open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && (shortcuts.open = false)}>
    <div class="shortcuts" role="dialog" aria-label="Keyboard shortcuts" data-shortcuts>
      <div class="dialog-head"><strong>Keyboard shortcuts</strong><IconButton icon="x" label="Close" size={24} onclick={() => (shortcuts.open = false)} /></div>
      {#each SECTIONS as [title, list]}
        <section>
          <h3>{title}</h3>
          <dl>
            {#each list as [keys, what]}
              <dt>{#each keys as k}<kbd>{k}</kbd>{/each}</dt>
              <dd>{what}</dd>
            {/each}
          </dl>
        </section>
      {/each}
      <p class="faint">Not while typing in a field, the editable diff or the agent terminal (there the keys are Claude Code's). <Kbd key="Esc" /> closes.</p>
    </div>
  </div>
{/if}

<style>
  .scrim { position: fixed; inset: 0; z-index: 40; display: grid; place-items: center; background: var(--scrim); }
  .shortcuts {
    width: 400px; max-width: calc(100% - 32px); max-height: calc(100% - 32px); overflow-y: auto; padding: var(--sp-7) 18px;
    box-sizing: border-box; border-radius: var(--r-xl); background: var(--surface-raised); box-shadow: var(--shadow-pop);
    display: flex; flex-direction: column; gap: var(--sp-5);
  }
  .dialog-head { display: flex; align-items: center; }
  .dialog-head strong { flex: 1; font-size: 14px; font-weight: var(--fw-semibold); }
  section { display: flex; flex-direction: column; gap: var(--sp-3); }
  h3 {
    margin: 0; font-size: var(--fs-11); font-weight: var(--fw-semibold); letter-spacing: 0.04em; text-transform: uppercase;
    color: var(--text-muted);
  }
  dl { margin: 0; display: grid; grid-template-columns: 112px minmax(0, 1fr); row-gap: var(--sp-3); font-size: 12.5px; }
  dt { display: flex; gap: var(--sp-2); }
  dd { margin: 0; }
  dl kbd {
    min-width: 18px; height: 20px; padding: 0 5px; box-sizing: border-box; border: 1px solid var(--border-strong); border-bottom-width: 2px;
    border-radius: var(--r-sm); background: var(--surface-0); font-family: var(--font-mono); font-size: var(--fs-11); line-height: 17px; text-align: center;
  }
  .faint { margin: 0; font-size: 11.5px; color: var(--text-faint); }
</style>
