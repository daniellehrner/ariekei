<script lang="ts">
  // The work item's `claude` TUI. The process lives in the backend; this view attaches to
  // it (replaying the scrollback) and detaches on unmount, so switching items is lossless.
  import { Channel } from "@tauri-apps/api/core";
  import { Terminal, type ITheme } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { Unicode11Addon } from "@xterm/addon-unicode11";
  import { WebglAddon } from "@xterm/addon-webgl";
  import "@xterm/xterm/css/xterm.css";
  import { onMount } from "svelte";
  import { api } from "./lib/api";

  let { id, visible = true }: { id: number; visible?: boolean } = $props();

  let host: HTMLElement;
  let error: string | null = $state(null);
  let fit: FitAddon | null = null;

  function theme(): ITheme {
    const css = getComputedStyle(document.documentElement);
    const v = (name: string) => css.getPropertyValue(name).trim();
    return { background: v("--bg"), foreground: v("--text"), cursor: v("--text"), selectionBackground: v("--user-bg") };
  }

  // Fit only when the panel has a real size (it is display:none while another tab shows).
  function refit() {
    if (fit && host?.clientWidth > 0 && host.clientHeight > 0) fit.fit();
  }

  $effect(() => {
    if (visible) requestAnimationFrame(refit);
  });

  onMount(() => {
    const term = new Terminal({
      allowProposedApi: true,
      fontFamily: getComputedStyle(document.documentElement).getPropertyValue("--mono"),
      fontSize: 13,
      scrollback: 10000,
      theme: theme(),
    });
    fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new Unicode11Addon());
    term.unicode.activeVersion = "11";
    term.open(host);
    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => webgl.dispose());
      term.loadAddon(webgl);
    } catch {
      // No WebGL (e.g. some WebKitGTK setups): xterm keeps its DOM renderer.
    }
    refit();

    let opened = false;
    let disposed = false;
    const output = new Channel<ArrayBuffer>();
    output.onmessage = (data) => term.write(new Uint8Array(data));

    const send = (data: string) => opened && api.termWrite(id, data);
    term.onData(send);
    term.onBinary(send);
    term.onResize(({ cols, rows }) => opened && api.termResize(id, cols, rows));

    const resizeObserver = new ResizeObserver(() => refit());
    resizeObserver.observe(host);

    api
      .termOpen(id, term.cols, term.rows, output)
      .then(() => {
        opened = true;
        if (disposed) api.termDetach(id, output.id);
        else {
          // Test handle: lets the e2e scenario read the screen buffer.
          ((window as any).__ariekeiTerminals ??= {})[String(id)] = term;
          term.focus();
        }
      })
      .catch((e) => (error = String(e)));

    const prefersDark = matchMedia("(prefers-color-scheme: dark)");
    const retheme = () => (term.options.theme = theme());
    prefersDark.addEventListener("change", retheme);

    return () => {
      disposed = true;
      prefersDark.removeEventListener("change", retheme);
      resizeObserver.disconnect();
      if (opened) {
        // Detach only this view: a newer view of the same item may already be attached.
        api.termDetach(id, output.id);
        const handles = (window as any).__ariekeiTerminals;
        if (handles?.[String(id)] === term) delete handles[String(id)];
      }
      term.dispose();
    };
  });
</script>

<section class="terminal" class:hidden={!visible}>
  {#if error}<p class="error">{error}</p>{/if}
  <div class="host" bind:this={host}></div>
</section>

<style>
  .terminal { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .terminal.hidden { display: none; }
  .host { flex: 1; min-height: 0; padding: 6px 0 0 8px; }
  .error { color: var(--bad); margin: 8px 12px; }
</style>
