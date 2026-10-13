<script lang="ts" module>
  export interface ReviewComment {
    path: string;
    fromLine: number;
    toLine: number;
    code: string;
    text: string;
  }

  export interface ReviewViewState {
    // "review": everything since the marker (incl. uncommitted); otherwise a commit sha.
    scope: string;
    file: string | null;
    comments: ReviewComment[];
  }

  export function feedbackMessage(comments: ReviewComment[]): string {
    const items = comments.map((c, i) => {
      const lines = c.fromLine === c.toLine ? `${c.fromLine}` : `${c.fromLine}-${c.toLine}`;
      const quote = c.code.split("\n").slice(0, 6).map((l) => `   > ${l}`).join("\n");
      return `${i + 1}. ${c.path}:${lines}\n${quote}\n   ${c.text.trim().replaceAll("\n", "\n   ")}`;
    });
    return `Review feedback on your changes:\n\n${items.join("\n\n")}\n\nPlease address each point.`;
  }
</script>

<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { MergeView } from "@codemirror/merge";
  import { java } from "@codemirror/lang-java";
  import { EditorState, type Extension } from "@codemirror/state";
  import { EditorView } from "@codemirror/view";
  import { oneDark } from "@codemirror/theme-one-dark";
  import { basicSetup } from "codemirror";
  import { api, type ChangedFile, type ReviewState } from "./lib/api";
  import { gitOverride } from "./lib/gitDiff";

  let { id, view = $bindable(), onchanged }: { id: number; view: ReviewViewState; onchanged: () => void } = $props();

  let review: ReviewState | null = $state(null);
  let files: ChangedFile[] = $state([]);
  let error: string | null = $state(null);
  let notice: string | null = $state(null);
  let confirmMark = $state(false);
  let host: HTMLElement | undefined = $state();
  let merge: MergeView | null = null;
  let selection: { fromLine: number; toLine: number; code: string } | null = $state(null);
  let draft = $state("");
  let editedPath: string | null = $state(null);
  let editedFile: string | null = null;

  // The diff range for the current scope: [from, to], to = null meaning the worktree.
  const range = $derived.by((): [string, string | null] | null => {
    if (!review) return null;
    return view.scope === "review" ? [review.marker, null] : [`${view.scope}^`, view.scope];
  });
  const editable = $derived(range?.[1] === null);

  async function load() {
    error = null;
    try {
      review = await api.reviewState(id);
      if (view.scope !== "review" && !review.commits.some((c) => c.sha === view.scope)) view.scope = "review";
    } catch (e) {
      error = String(e);
    }
  }

  async function loadFiles() {
    if (!range) return;
    try {
      files = await api.reviewFiles(id, range[0], range[1]);
      if (view.file && !files.some((f) => f.path === view.file)) view.file = null;
      view.file ??= files[0]?.path ?? null;
    } catch (e) {
      error = String(e);
      files = [];
    }
  }

  const prefersDark = () => matchMedia("(prefers-color-scheme: dark)").matches;
  const language = (path: string): Extension[] => (path.endsWith(".java") ? [java()] : []);

  async function show(path: string) {
    if (!range || !host) return;
    const [from, to] = range;
    const d = await api.reviewFile(id, from, to, path);
    merge?.destroy();
    merge = null;
    selection = null;
    editedPath = null;
    if (d.binary) {
      host.textContent = "Binary file";
      return;
    }
    host.textContent = "";
    const common = [basicSetup, ...language(path), ...(prefersDark() ? [oneDark] : [])];
    merge = new MergeView({
      parent: host,
      a: { doc: d.old, extensions: [...common, EditorState.readOnly.of(true)] },
      b: {
        doc: d.new,
        extensions: [
          ...common,
          EditorView.editable.of(to === null),
          EditorState.readOnly.of(to !== null),
          EditorView.updateListener.of((u) => {
            if (u.docChanged) {
              editedPath = path;
              editedFile = u.state.doc.toString();
            }
            if (u.selectionSet || u.docChanged) {
              const sel = u.state.selection.main;
              if (sel.empty) selection = null;
              else {
                const doc = u.state.doc;
                selection = { fromLine: doc.lineAt(sel.from).number, toLine: doc.lineAt(sel.to).number, code: doc.sliceString(sel.from, sel.to) };
              }
            }
          }),
        ],
      },
      collapseUnchanged: { margin: 3, minSize: 6 },
      gutter: true,
      diffConfig: { override: gitOverride(d.old, d.new, d.hunks) },
    });
  }

  $effect(() => {
    void id;
    load();
  });
  $effect(() => {
    void range?.[0];
    void range?.[1];
    loadFiles();
  });
  $effect(() => {
    if (view.file && range) show(view.file);
    return () => {
      merge?.destroy();
      merge = null;
    };
  });

  async function markReviewed() {
    if (review?.dirty && !confirmMark) {
      confirmMark = true;
      return;
    }
    confirmMark = false;
    const sha = await api.markReviewed(id);
    notice = `Marked ${sha.slice(0, 8)} as reviewed.`;
    view.scope = "review";
    await load();
    onchanged();
  }

  function addComment() {
    if (!selection || !view.file || !draft.trim()) return;
    view.comments = [...view.comments, { path: view.file, ...selection, text: draft }];
    draft = "";
  }

  async function sendFeedback() {
    if (!view.comments.length) return;
    try {
      await api.agentSend(id, feedbackMessage(view.comments));
      notice = `Sent ${view.comments.length} comment${view.comments.length > 1 ? "s" : ""} to the agent.`;
      view.comments = [];
    } catch (e) {
      error = `Could not send to the agent: ${e}`;
    }
  }

  async function save() {
    if (!editedPath || editedFile === null) return;
    try {
      await invoke("save_file", { id, path: editedPath, content: editedFile });
      notice = `Saved ${editedPath}.`;
      editedPath = null;
      await loadFiles();
      onchanged();
    } catch (e) {
      error = String(e);
    }
  }

  const short = (sha: string) => sha.slice(0, 8);
</script>

<section class="review">
  <header>
    {#if review}
      <span class="marker" title={review.marker}>
        {review.markerKind === "reviewed" ? "Since your review" : "Since the PR base"} ({short(review.marker)})
        · {review.commits.length} commit{review.commits.length === 1 ? "" : "s"}{review.dirty ? " · uncommitted changes" : ""}
      </span>
      <button onclick={load} title="Reload">↻</button>
      {#if confirmMark}
        <span class="warn">Uncommitted changes are not part of HEAD and will still show as changed.</span>
        <button onclick={markReviewed}>Mark anyway</button>
        <button onclick={() => (confirmMark = false)}>Cancel</button>
      {:else}
        <button class="primary" onclick={markReviewed} disabled={review.marker === review.head && !review.dirty}>Mark reviewed</button>
      {/if}
    {/if}
    {#if notice}<span class="notice">{notice}</span>{/if}
  </header>
  {#if error}<p class="error">{error}</p>{/if}

  <div class="body">
    <nav class="side">
      <h3>Scope</h3>
      <ul class="scopes">
        <li><button class:active={view.scope === "review"} onclick={() => (view.scope = "review")}>All changes since {review?.markerKind === "reviewed" ? "review" : "base"}</button></li>
        {#each review?.commits ?? [] as c (c.sha)}
          <li>
            <button class:active={view.scope === c.sha} onclick={() => (view.scope = c.sha)} title="{c.subject}&#10;{c.author}, {c.date}">
              <span class="sha">{short(c.sha)}</span> {c.subject}
            </button>
          </li>
        {/each}
      </ul>
      <h3>Files</h3>
      <ul class="files">
        {#each files as f (f.path)}
          <li>
            <button class:active={f.path === view.file} onclick={() => (view.file = f.path)} title={f.path}>
              <span class="status s{f.status === '?' ? 'U' : f.status}">{f.status === "?" ? "U" : f.status}</span>{f.path}
            </button>
          </li>
        {:else}
          <li class="none">No changes.</li>
        {/each}
      </ul>
    </nav>

    <div class="main">
      <div class="toolbar">
        {#if editedPath}<button class="primary" onclick={save}>Save {editedPath}</button>{/if}
        {#if selection && view.file}
          <span class="sel">Lines {selection.fromLine}{selection.toLine !== selection.fromLine ? `–${selection.toLine}` : ""}:</span>
          <input bind:value={draft} placeholder="Comment for the agent" onkeydown={(e) => e.key === "Enter" && addComment()} />
          <button onclick={addComment} disabled={!draft.trim()}>Add comment</button>
        {:else}
          <span class="hint">Select code on the right to comment on it{editable ? "; edit it there to change the file" : ""}.</span>
        {/if}
      </div>
      <div class="diff" bind:this={host}></div>
      {#if view.comments.length}
        <div class="comments">
          <ol>
            {#each view.comments as c, i}
              <li>
                <span class="where">{c.path}:{c.fromLine}{c.toLine !== c.fromLine ? `-${c.toLine}` : ""}</span> {c.text}
                <button class="remove" onclick={() => (view.comments = view.comments.filter((_, j) => j !== i))} aria-label="Remove comment">×</button>
              </li>
            {/each}
          </ol>
          <button class="primary" onclick={sendFeedback}>Send {view.comments.length} comment{view.comments.length > 1 ? "s" : ""} to the agent</button>
        </div>
      {/if}
    </div>
  </div>
</section>

<style>
  .review { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  header { display: flex; gap: 10px; align-items: center; padding: 8px 12px; border-bottom: 1px solid var(--border); flex-wrap: wrap; }
  .marker { font-size: 0.9em; }
  .warn { color: var(--warn); font-size: 0.85em; }
  .notice { color: var(--ok); font-size: 0.85em; margin-left: auto; }
  .primary { border-color: var(--accent); }
  .body { flex: 1; display: flex; min-height: 0; }
  .side { width: 300px; flex-shrink: 0; overflow-y: auto; border-right: 1px solid var(--border); padding-bottom: 12px; }
  h3 { font-size: 0.75em; text-transform: uppercase; letter-spacing: 0.05em; color: var(--muted); margin: 12px 12px 4px; }
  .side ul { list-style: none; margin: 0; padding: 0; }
  .side button { width: 100%; text-align: left; border: 0; border-radius: 0; background: none; padding: 3px 12px; font-size: 0.85em; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .side button.active { background: var(--panel); }
  .files button { font-family: var(--mono); }
  .sha { font-family: var(--mono); color: var(--muted); }
  .status { display: inline-block; width: 1.4em; font-weight: 700; }
  .sA, .sU { color: var(--ok); }
  .sD { color: var(--bad); }
  .sM { color: var(--warn); }
  .none { padding: 4px 12px; color: var(--muted); font-size: 0.85em; }
  .main { flex: 1; display: flex; flex-direction: column; min-width: 0; }
  .toolbar { display: flex; gap: 8px; align-items: center; padding: 6px 12px; border-bottom: 1px solid var(--border); min-height: 30px; }
  .toolbar input { flex: 1; min-width: 0; }
  .sel, .hint { color: var(--muted); font-size: 0.85em; }
  .diff { flex: 1; overflow: auto; min-height: 0; }
  .diff :global(.cm-editor) { font-size: 13px; }
  .comments { border-top: 1px solid var(--border); padding: 8px 12px; max-height: 30%; overflow-y: auto; }
  .comments ol { margin: 0 0 8px; padding-left: 20px; font-size: 0.9em; }
  .where { font-family: var(--mono); color: var(--muted); }
  .remove { border: 0; background: none; color: var(--muted); padding: 0 6px; }
  .error { color: var(--bad); margin: 6px 12px; }
</style>
