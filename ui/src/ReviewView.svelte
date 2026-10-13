<script lang="ts" module>
  import type { DraftComment } from "./lib/reviewComments";

  export interface ReviewViewState {
    // "all" (vs the base branch), "review" (since the user's review) or a commit sha;
    // empty picks the default.
    scope: string;
    file: string | null;
    comments: DraftComment[];
  }
</script>

<script lang="ts">
  import { MergeView } from "@codemirror/merge";
  import { java } from "@codemirror/lang-java";
  import { EditorState, type Extension } from "@codemirror/state";
  import { EditorView } from "@codemirror/view";
  import { oneDark } from "@codemirror/theme-one-dark";
  import { basicSetup } from "codemirror";
  import { api, type ChangedFile, type ReviewState } from "./lib/api";
  import { gitOverride } from "./lib/gitDiff";
  import { externalLinks } from "./lib/html";
  import { classify, feedbackMessage, reviewBody, type LineComment, type Ranges } from "./lib/reviewComments";

  let {
    id,
    view = $bindable(),
    onchanged,
    canSendToAgent,
  }: { id: number; view: ReviewViewState; onchanged: () => void; canSendToAgent: boolean } = $props();

  let review = $state<ReviewState | null>(null);
  let files: ChangedFile[] = $state([]);
  let error: string | null = $state(null);
  let notice: string | null = $state(null);
  let noticeUrl: string | null = $state(null);
  let confirmMark = $state(false);
  let host: HTMLElement | undefined = $state();
  let merge: MergeView | null = null;
  let selection: { fromLine: number; toLine: number; code: string } | null = $state(null);
  let draft = $state("");
  let editedPath: string | null = $state(null);
  let editedFile: string | null = null;

  // Submitting to GitHub.
  let submitting = $state(false);
  let event = $state<"COMMENT" | "APPROVE" | "REQUEST_CHANGES">("COMMENT");
  let summary = $state("");
  let preview: { line: LineComment[]; body: DraftComment[] } | null = $state(null);
  let busy = $state(false);

  const scope = $derived.by(() => {
    if (!review) return "all";
    if (view.scope === "review" && review.reviewed) return "review";
    if (view.scope && view.scope !== "all" && view.scope !== "review" && review.commits.some((c) => c.sha === view.scope)) return view.scope;
    if (!view.scope && review.reviewed) return "review";
    return "all";
  });

  // [from, to] of the diff; to = null is the working tree (worktree mode only).
  const range = $derived.by((): [string, string | null] | null => {
    if (!review) return null;
    const head = review.mode === "worktree" ? null : review.head;
    if (scope === "all") return [review.base, head];
    if (scope === "review" && review.reviewed) return [review.reviewed, head];
    return [`${scope}^`, scope];
  });
  // The commit the right side shows; null when it includes uncommitted changes.
  const shownRev = $derived(range ? (range[1] ?? (review?.dirty ? null : review?.head ?? null)) : null);
  const editable = $derived(range?.[1] === null);
  const newCommits = $derived(review?.commits.filter((c) => c.afterReview).length ?? 0);

  async function load() {
    error = null;
    try {
      review = await api.reviewState(id);
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

  function setScope(value: string) {
    view.scope = value;
  }

  async function markReviewed() {
    if (review?.dirty && !confirmMark) {
      confirmMark = true;
      return;
    }
    confirmMark = false;
    const sha = await api.markReviewed(id);
    say(`Marked ${sha.slice(0, 8)} as reviewed.`);
    view.scope = "review";
    await load();
    onchanged();
  }

  function addComment() {
    if (!selection || !view.file || !draft.trim()) return;
    // Edits not saved yet are not part of any version on disk.
    const rev = editedPath === view.file ? null : shownRev;
    view.comments = [...view.comments, { path: view.file, ...selection, text: draft, rev }];
    draft = "";
  }

  function say(message: string, url: string | null = null) {
    notice = message;
    noticeUrl = url;
  }

  async function sendToAgent() {
    if (!view.comments.length) return;
    try {
      await api.agentSend(id, feedbackMessage(view.comments));
      say(`Sent ${view.comments.length} comment${view.comments.length > 1 ? "s" : ""} to the agent.`);
      view.comments = [];
    } catch (e) {
      error = `Could not send to the agent: ${e}`;
    }
  }

  async function openSubmit() {
    if (!review) return;
    submitting = true;
    preview = null;
    if (review.own) event = "COMMENT";
    try {
      const paths = [...new Set(view.comments.map((c) => c.path))];
      const ranges: Ranges = Object.fromEntries(await api.commentRanges(id, paths));
      preview = classify(view.comments, review.prHead, ranges);
    } catch (e) {
      error = String(e);
    }
  }

  async function submit() {
    if (!preview) return;
    busy = true;
    try {
      const body = reviewBody(summary, preview.body);
      if (event === "REQUEST_CHANGES" && !body && !preview.line.length) throw new Error("Requesting changes needs a comment.");
      const url = await api.submitReview(id, event, body, preview.line);
      const what = { COMMENT: "Review submitted", APPROVE: "Approved", REQUEST_CHANGES: "Changes requested" }[event];
      say(`${what} on GitHub with ${preview.line.length} line comment${preview.line.length === 1 ? "" : "s"}.`, url || null);
      view.comments = [];
      summary = "";
      submitting = false;
      onchanged();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function save() {
    if (!editedPath || editedFile === null) return;
    try {
      await api.saveFile(id, editedPath, editedFile);
      say(`Saved ${editedPath}.`);
      editedPath = null;
      await loadFiles();
      onchanged();
    } catch (e) {
      error = String(e);
    }
  }

  const short = (sha: string) => sha.slice(0, 8);
  const linesOf = (c: DraftComment) => (c.fromLine === c.toLine ? `${c.fromLine}` : `${c.fromLine}-${c.toLine}`);
</script>

<section class="review" use:externalLinks>
  <header>
    {#if review}
      <span class="marker">
        {review.mode === "ref" ? "PR head" : "Worktree"} {short(review.head)} · {review.commits.length} commit{review.commits.length === 1 ? "" : "s"} vs {review.baseRef}
        {#if review.reviewed} · reviewed {short(review.reviewed)}{newCommits ? `, ${newCommits} new since` : ""}{/if}
        {review.dirty ? " · uncommitted changes" : ""}
      </span>
      <button onclick={load} title="Reload">↻</button>
      {#if confirmMark}
        <span class="warn">Uncommitted changes are not part of HEAD and will still show as changed.</span>
        <button onclick={markReviewed}>Mark anyway</button>
        <button onclick={() => (confirmMark = false)}>Cancel</button>
      {:else}
        <button class="primary" onclick={markReviewed} disabled={review.reviewed === review.head && !review.dirty}>Mark reviewed</button>
      {/if}
    {/if}
    {#if notice}<span class="notice">{notice}{#if noticeUrl} <a href={noticeUrl}>open</a>{/if}</span>{/if}
  </header>
  {#if error}<p class="error">{error}</p>{/if}

  <div class="body">
    <nav class="side">
      <h3>Scope</h3>
      <ul class="scopes">
        <li><button class:active={scope === "all"} onclick={() => setScope("all")} data-scope="all">All changes vs {review?.baseRef ?? "base"}</button></li>
        {#if review?.reviewed}
          <li><button class:active={scope === "review"} onclick={() => setScope("review")} data-scope="review">Since your review</button></li>
        {/if}
        {#each review?.commits ?? [] as c (c.sha)}
          <li>
            <button class:active={scope === c.sha} onclick={() => setScope(c.sha)} title="{c.subject}&#10;{c.author}, {c.date}">
              <span class="sha" class:new={c.afterReview}>{short(c.sha)}</span> {c.subject}
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
          <input bind:value={draft} placeholder="Review comment" onkeydown={(e) => e.key === "Enter" && addComment()} />
          <button onclick={addComment} disabled={!draft.trim()}>Add comment</button>
        {:else}
          <span class="hint">Select code on the right to comment on it{editable ? "; edit it there to change the file" : ""}.</span>
        {/if}
      </div>
      <div class="diff" bind:this={host}></div>

      {#if view.comments.length || submitting}
        <div class="comments">
          <ol>
            {#each view.comments as c, i}
              <li>
                <span class="where">{c.path}:{linesOf(c)}</span> {c.text}
                <button class="remove" onclick={() => (view.comments = view.comments.filter((_, j) => j !== i))} aria-label="Remove comment">×</button>
              </li>
            {/each}
          </ol>
          {#if submitting}
            <div class="submit">
              <textarea bind:value={summary} rows="3" placeholder="Review summary (optional for a comment review)"></textarea>
              <div class="events">
                <label><input type="radio" bind:group={event} value="COMMENT" /> Comment</label>
                {#if !review?.own}
                  <label><input type="radio" bind:group={event} value="APPROVE" /> Approve</label>
                  <label><input type="radio" bind:group={event} value="REQUEST_CHANGES" /> Request changes</label>
                {/if}
              </div>
              {#if preview}
                <p class="hint">
                  {preview.line.length} as line comment{preview.line.length === 1 ? "" : "s"} on the PR diff{#if preview.body.length}; {preview.body.length} in the review text (not on lines of the PR diff at its head commit){/if}.
                </p>
              {/if}
              <div class="actions">
                <button onclick={() => (submitting = false)}>Cancel</button>
                <button class="primary" onclick={submit} disabled={busy || !preview}>Submit to GitHub</button>
              </div>
            </div>
          {:else}
            <div class="actions">
              {#if canSendToAgent}<button onclick={sendToAgent}>Send to the agent</button>{/if}
              {#if review?.prHead}<button class="primary" onclick={openSubmit}>Submit to GitHub…</button>{/if}
            </div>
          {/if}
        </div>
      {:else}
        {#if review?.prHead}
          <div class="comments quiet">
            <button onclick={openSubmit}>Review on GitHub…</button>
          </div>
        {/if}
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
  .notice a { color: var(--accent); }
  .primary { border-color: var(--accent); }
  .body { flex: 1; display: flex; min-height: 0; }
  .side { width: 300px; flex-shrink: 0; overflow-y: auto; border-right: 1px solid var(--border); padding-bottom: 12px; }
  h3 { font-size: 0.75em; text-transform: uppercase; letter-spacing: 0.05em; color: var(--muted); margin: 12px 12px 4px; }
  .side ul { list-style: none; margin: 0; padding: 0; }
  .side button { width: 100%; text-align: left; border: 0; border-radius: 0; background: none; padding: 3px 12px; font-size: 0.85em; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .side button.active { background: var(--panel); }
  .files button { font-family: var(--mono); }
  .sha { font-family: var(--mono); color: var(--muted); }
  .sha.new { color: var(--accent); }
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
  .comments { border-top: 1px solid var(--border); padding: 8px 12px; max-height: 40%; overflow-y: auto; }
  .comments.quiet { display: flex; justify-content: flex-end; padding: 6px 12px; }
  .comments ol { margin: 0 0 8px; padding-left: 20px; font-size: 0.9em; }
  .where { font-family: var(--mono); color: var(--muted); }
  .remove { border: 0; background: none; color: var(--muted); padding: 0 6px; }
  .submit textarea { width: 100%; box-sizing: border-box; font: inherit; }
  .events { display: flex; gap: 14px; margin: 6px 0; font-size: 0.9em; }
  .actions { display: flex; gap: 8px; justify-content: flex-end; }
  .error { color: var(--bad); margin: 6px 12px; }
</style>
