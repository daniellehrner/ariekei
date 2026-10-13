<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { MergeView } from "@codemirror/merge";
  import { EditorState, type Extension } from "@codemirror/state";
  import { EditorView } from "@codemirror/view";
  import { basicSetup } from "codemirror";
  import { java } from "@codemirror/lang-java";

  interface ChangedFile { path: string; status: string }
  interface FileVersions { old: string; new: string; binary: boolean }

  let {
    cwd,
    base = $bindable("HEAD"),
    selected = $bindable<string | null>(null),
  }: { cwd: string; base: string; selected: string | null } = $props();

  let files: ChangedFile[] = $state([]);
  let error: string | null = $state(null);
  let renderMs: number | null = $state(null);
  let lines = $state(0);
  let host: HTMLElement | undefined = $state();
  let view: MergeView | null = null;

  async function refresh() {
    error = null;
    try {
      files = await invoke<ChangedFile[]>("diff_files", { cwd, base });
      if (selected && !files.some((f) => f.path === selected)) selected = null;
      selected ??= files[0]?.path ?? null;
    } catch (e) {
      error = String(e);
      files = [];
    }
  }

  function language(path: string): Extension[] {
    return path.endsWith(".java") ? [java()] : [];
  }

  async function show(path: string) {
    const v = await invoke<FileVersions>("diff_file", { cwd, base, path });
    view?.destroy();
    view = null;
    if (!host || v.binary) return;
    const t0 = performance.now();
    view = new MergeView({
      parent: host,
      a: { doc: v.old, extensions: [basicSetup, ...language(path), EditorState.readOnly.of(true)] },
      // The right side is the live worktree file; editing it is a later milestone.
      b: { doc: v.new, extensions: [basicSetup, ...language(path), EditorView.editable.of(false)] },
      collapseUnchanged: { margin: 3, minSize: 6 },
      // The default scanLimit (500) degrades to a crude diff on large files; prefer an
      // exact diff with a time cap. M1: feed git's hunks in via diffConfig.override.
      diffConfig: { timeout: 1000 },
      gutter: true,
    });
    // Measure until the browser has painted the result.
    requestAnimationFrame(() => requestAnimationFrame(() => (renderMs = Math.round(performance.now() - t0))));
    lines = v.new.split("\n").length;
  }

  $effect(() => {
    void cwd;
    void base;
    refresh();
  });

  $effect(() => {
    if (selected) show(selected);
    return () => {
      view?.destroy();
      view = null;
    };
  });
</script>

<section class="review">
  <header>
    <label>Changes since <input bind:value={base} size="14" onchange={refresh} /></label>
    <button onclick={refresh}>Refresh</button>
    {#if renderMs !== null}<span class="metric">{lines} lines rendered in {renderMs} ms</span>{/if}
  </header>
  {#if error}<p class="error">{error}</p>{/if}
  <div class="body">
    <ul class="files">
      {#each files as f (f.path)}
        <li>
          <button class:active={f.path === selected} onclick={() => (selected = f.path)}>
            <span class="status s{f.status === '?' ? 'U' : f.status}">{f.status === "?" ? "U" : f.status}</span>
            {f.path}
          </button>
        </li>
      {:else}
        <li class="none">No changes.</li>
      {/each}
    </ul>
    <div class="diff" bind:this={host}></div>
  </div>
</section>

<style>
  .review { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  header { display: flex; gap: 12px; align-items: center; padding: 8px 12px; border-bottom: 1px solid var(--border); flex-wrap: wrap; }
  .metric { color: var(--muted); font-size: 0.85em; margin-left: auto; }
  .body { flex: 1; display: flex; min-height: 0; }
  .files { list-style: none; margin: 0; padding: 4px 0; width: 280px; flex-shrink: 0; overflow-y: auto; border-right: 1px solid var(--border); }
  .files button { width: 100%; text-align: left; border: 0; background: none; padding: 3px 10px; font-family: var(--mono); font-size: 0.85em; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .files button.active { background: var(--panel); }
  .status { display: inline-block; width: 1.2em; font-weight: 700; }
  .sA, .sU { color: var(--ok); }
  .sD { color: var(--bad); }
  .sM { color: var(--warn); }
  .none { padding: 8px 12px; color: var(--muted); }
  .diff { flex: 1; overflow: auto; min-width: 0; }
  .diff :global(.cm-mergeView) { height: 100%; }
  .diff :global(.cm-editor) { font-size: 13px; }
  .error { color: var(--bad); padding: 0 12px; }
</style>
