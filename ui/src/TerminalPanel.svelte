<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

<script lang="ts">
  // The work item's `claude` TUI. The process lives in the backend; this view attaches to
  // it (replaying the scrollback) and detaches on unmount, so switching items is lossless.
  import { Channel } from "@tauri-apps/api/core";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { Unicode11Addon } from "@xterm/addon-unicode11";
  import { WebglAddon } from "@xterm/addon-webgl";
  import "@xterm/xterm/css/xterm.css";
  import { onMount } from "svelte";
  import StatusDot, { AGENT_LABEL } from "./controls/StatusDot.svelte";
  import type { AgentStatus } from "./lib/agent.svelte";
  import { api } from "./lib/api";
  import { shortPath } from "./lib/paths";
  import { monoFont, token, xtermTheme } from "./lib/xtermTheme";

  let {
    id,
    visible = true,
    status = null,
    path = "",
  }: { id: number; visible?: boolean; status?: AgentStatus | null; path?: string } = $props();

  let host: HTMLElement;
  let error: string | null = $state(null);
  let fit: FitAddon | null = null;
  let term: Terminal | null = null;

  // Fit only when the panel has a real size (it is display:none while another tab shows).
  function refit() {
    if (fit && host?.clientWidth > 0 && host.clientHeight > 0) fit.fit();
  }

  // Showing the terminal (also from "Answer in Terminal") puts the keyboard there.
  $effect(() => {
    if (visible)
      requestAnimationFrame(() => {
        refit();
        term?.focus();
      });
  });

  onMount(() => {
    const t = new Terminal({
      allowProposedApi: true,
      fontFamily: monoFont(),
      fontSize: parseFloat(token("--fs-term")) || 12.5,
      lineHeight: parseFloat(token("--lh-term")) || 1.35,
      scrollback: 10000,
      theme: xtermTheme(),
    });
    term = t;
    fit = new FitAddon();
    t.loadAddon(fit);
    t.loadAddon(new Unicode11Addon());
    t.unicode.activeVersion = "11";
    t.open(host);
    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => webgl.dispose());
      t.loadAddon(webgl);
    } catch {
      // No WebGL (e.g. some WebKitGTK setups): xterm keeps its DOM renderer.
    }
    refit();

    let opened = false;
    let disposed = false;
    const output = new Channel<ArrayBuffer>();
    output.onmessage = (data) => t.write(new Uint8Array(data));

    // What xterm sends before the terminal is open waits for it, in order. Not dropped: on
    // Windows ConPTY asks for the cursor position (ESC[6n) as the process starts and holds
    // the process until xterm's answer arrives; a lost answer left the agent hanging.
    let pending: string[] = [];
    const send = (data: string) => {
      if (opened) api.termWrite(id, data);
      else pending.push(data);
    };
    t.onData(send);
    t.onBinary(send);
    t.onResize(({ cols, rows }) => opened && api.termResize(id, cols, rows));

    const resizeObserver = new ResizeObserver(() => refit());
    resizeObserver.observe(host);

    api
      .termOpen(id, t.cols, t.rows, output)
      .then(() => {
        opened = true;
        if (pending.length) api.termWrite(id, pending.join(""));
        pending = [];
        if (disposed) api.termDetach(id, output.id);
        else {
          // Test handle: lets the e2e scenario read the screen buffer.
          ((window as any).__ariekeiTerminals ??= {})[String(id)] = t;
          t.focus();
        }
      })
      .catch((e) => (error = String(e)));

    const prefersDark = matchMedia("(prefers-color-scheme: dark)");
    const retheme = () => (t.options.theme = xtermTheme());
    prefersDark.addEventListener("change", retheme);

    return () => {
      disposed = true;
      prefersDark.removeEventListener("change", retheme);
      resizeObserver.disconnect();
      if (opened) {
        // Detach only this view: a newer view of the same item may already be attached.
        api.termDetach(id, output.id);
        const handles = (window as any).__ariekeiTerminals;
        if (handles?.[String(id)] === t) delete handles[String(id)];
      }
      t.dispose();
      term = null;
    };
  });
</script>

<div class="terminal" class:hidden={!visible} role="tabpanel" aria-label="Terminal">
  <div class="frame">
    <div class="bar">
      <StatusDot {status} size={12} />
      <span class="status">{AGENT_LABEL[status ?? "none"]}</span>
      <span>·</span>
      <code title={path}>claude — {shortPath(path)}</code>
    </div>
    {#if error}<p class="error">{error}</p>{/if}
    <div class="host" bind:this={host}></div>
  </div>
</div>

<style>
  .terminal { flex: 1; display: flex; flex-direction: column; height: 100%; min-height: 0; padding: var(--sp-4); box-sizing: border-box; }
  .terminal.hidden { display: none; }
  .frame {
    flex: 1; min-height: 0; display: flex; flex-direction: column; overflow: hidden;
    border-radius: var(--r-lg); background: var(--term-bg); box-shadow: inset 0 0 0 1px var(--border);
  }
  .bar {
    flex: none; display: flex; align-items: center; gap: var(--sp-4); height: 28px; padding: 0 var(--sp-5);
    border-bottom: 1px solid var(--border); font-size: var(--fs-12); color: var(--text-muted); white-space: nowrap; overflow: hidden;
  }
  .status { font-weight: var(--fw-medium); color: var(--text); }
  .bar code { font-size: var(--fs-11); overflow: hidden; text-overflow: ellipsis; }
  .host { flex: 1; min-height: 0; padding: 10px 14px; }
  .host :global(.xterm-viewport) { background-color: var(--term-bg) !important; }
  .error { color: var(--bad); margin: var(--sp-4) var(--sp-6); font-size: var(--fs-12); }
</style>
