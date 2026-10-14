<script lang="ts" module>
  import type { DraftComment } from "./lib/reviewComments";
  import type { Order } from "./lib/review";

  export interface ReviewViewState {
    // "all" (vs the base branch), "review" (since the user's review) or a commit sha;
    // empty picks the default.
    scope: string;
    file: string | null;
    comments: DraftComment[];
    // Layout: the commit list expanded, the whole side panel (scopes + files) hidden.
    commitsOpen?: boolean;
    sideHidden?: boolean;
    // Diff: unified instead of side-by-side, whitespace changes ignored; file order.
    unified?: boolean;
    ignoreWs?: boolean;
    order?: Order;
    // Sections of the file list the user opened (generated files start folded).
    opened?: string[];
  }
</script>

<script lang="ts">
  import { MergeView, goToNextChunk, goToPreviousChunk, unifiedMergeView } from "@codemirror/merge";
  import { EditorState, type Extension } from "@codemirror/state";
  import { EditorView } from "@codemirror/view";
  import { oneDark } from "@codemirror/theme-one-dark";
  import { basicSetup } from "codemirror";
  import { api, type ReviewState } from "./lib/api";
  import { gitOverride } from "./lib/gitDiff";
  import { externalLinks } from "./lib/html";
  import { SHORTCUTS, isTyping } from "./lib/keys";
  import { languageFor } from "./lib/languages";
  import { fileGuide, flatten, nextUnreviewed, progress, sections, type Guide, type ReviewFile } from "./lib/review";
  import { classify, feedbackMessage, reviewBody, type LineComment, type Ranges } from "./lib/reviewComments";

  let {
    id,
    view = $bindable(),
    onchanged,
    canSendToAgent,
  }: { id: number; view: ReviewViewState; onchanged: () => void; canSendToAgent: boolean } = $props();

  let review = $state<ReviewState | null>(null);
  let files: ReviewFile[] = $state([]);
  let error: string | null = $state(null);
  let notice: string | null = $state(null);
  let noticeUrl: string | null = $state(null);
  let confirmMark = $state<string | null>(null);
  let host: HTMLElement | undefined = $state();
  // The diff shown: a side-by-side MergeView, or one editor with the unified merge view.
  let merge: MergeView | null = null;
  let unified: EditorView | null = null;
  let selection: { fromLine: number; toLine: number; code: string } | null = $state(null);
  let draft = $state("");
  let editedPath: string | null = $state(null);
  let editedFile: string | null = null;
  let showKeys = $state(false);

  // The agent's review guide.
  let guide = $state<Guide | null>(null);
  let guideStale = $state(false);
  let guideBusy = $state(false);
  let guideOpen = $state(true);
  // The guide's line above the diff: one line, or wrapped with the whole change's focus.
  let fileGuideOpen = $state(false);

  // Submitting to GitHub.
  let submitting = $state(false);
  let event = $state<"COMMENT" | "APPROVE" | "REQUEST_CHANGES">("COMMENT");
  let summary = $state("");
  let preview: { line: LineComment[]; body: DraftComment[] } | null = $state(null);
  let busy = $state(false);

  const order = $derived(view.order === "guide" && !guide ? "review" : (view.order ?? "review"));
  const list = $derived(sections(files, order, guide));
  const here = $derived(fileGuide(guide, view.file));
  const ordered = $derived(flatten(list));
  const done = $derived(progress(files));

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
    if (scope === "review" && review.reviewed) return [review.sinceReviewFrom ?? review.reviewed, head];
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

  async function loadGuide() {
    try {
      const g = await api.reviewGuide(id);
      guide = g?.guide ?? null;
      guideStale = g?.stale ?? false;
    } catch {
      guide = null;
    }
  }

  async function loadFiles() {
    if (!range) return;
    try {
      files = await api.reviewFiles(id, range[0], range[1], !!view.ignoreWs);
      if (view.file && !files.some((f) => f.path === view.file)) view.file = null;
      view.file ??= flatten(sections(files, order, guide))[0]?.path ?? null;
    } catch (e) {
      error = String(e);
      files = [];
    }
  }

  const prefersDark = () => matchMedia("(prefers-color-scheme: dark)").matches;

  function destroyEditors() {
    merge?.destroy();
    unified?.destroy();
    merge = unified = null;
  }

  // The editor whose document is the new version (selection, change navigation).
  const newSide = () => merge?.b ?? unified;

  async function show(path: string) {
    if (!range || !host) return;
    const [from, to] = range;
    const ignoreWs = !!view.ignoreWs;
    const [d, lang] = await Promise.all([api.reviewFile(id, from, to, path, ignoreWs), languageFor(path)]);
    destroyEditors();
    selection = null;
    editedPath = null;
    if (d.binary) {
      host.textContent = "Binary file";
      return;
    }
    host.textContent = "";
    const common: Extension[] = [basicSetup, ...lang, ...(prefersDark() ? [oneDark] : [])];
    const onUpdate = EditorView.updateListener.of((u) => {
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
    });
    const newDoc = [...common, EditorView.editable.of(to === null), EditorState.readOnly.of(to !== null), onUpdate];
    const diffConfig = { override: gitOverride(d.old, d.new, d.hunks) };
    const collapseUnchanged = { margin: 3, minSize: 6 };
    if (view.unified) {
      unified = new EditorView({
        parent: host,
        doc: d.new,
        extensions: [
          ...newDoc,
          unifiedMergeView({ original: d.old, gutter: true, mergeControls: false, highlightChanges: true, syntaxHighlightDeletions: true, diffConfig, collapseUnchanged }),
        ],
      });
    } else {
      merge = new MergeView({
        parent: host,
        a: { doc: d.old, extensions: [...common, EditorState.readOnly.of(true)] },
        b: { doc: d.new, extensions: newDoc },
        collapseUnchanged,
        gutter: true,
        diffConfig,
      });
    }
  }

  $effect(() => {
    void id;
    load();
    loadGuide();
  });
  $effect(() => {
    void range?.[0];
    void range?.[1];
    void view.ignoreWs;
    loadFiles();
  });
  $effect(() => {
    void view.unified;
    void view.ignoreWs;
    if (view.file && range) show(view.file);
    return destroyEditors;
  });

  function setScope(value: string) {
    view.scope = value;
  }

  const scopeLabel = $derived.by(() => {
    if (scope === "all") return `All changes vs ${review?.baseRef ?? "base"}`;
    if (scope === "review") return "Since your review";
    const c = review?.commits.find((c) => c.sha === scope);
    return c ? `${short(c.sha)} ${c.subject}` : scope;
  });

  const fileIndex = $derived(ordered.findIndex((f) => f.path === view.file));
  function stepFile(delta: number) {
    if (!ordered.length) return;
    const next = (Math.max(fileIndex, 0) + delta + ordered.length) % ordered.length;
    view.file = ordered[next].path;
  }

  async function setReviewed(path: string, reviewed: boolean) {
    await api.markFileReviewed(id, path, reviewed);
    files = files.map((f) => (f.path === path ? { ...f, reviewed } : f));
  }

  // Mark the current file reviewed and move on to the next one that is not.
  async function reviewedAndNext() {
    if (!view.file) return;
    const current = view.file;
    await setReviewed(current, true);
    const next = nextUnreviewed(ordered.map((f) => (f.path === current ? { ...f, reviewed: true } : f)), current);
    if (next) view.file = next;
    else say("All files of this scope are reviewed.");
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.ctrlKey || e.metaKey || e.altKey || isTyping(e.target)) return;
    const actions: Record<string, () => void> = {
      "]": () => stepFile(1),
      "[": () => stepFile(-1),
      n: () => { const v = newSide(); if (v) goToNextChunk(v); },
      p: () => { const v = newSide(); if (v) goToPreviousChunk(v); },
      r: () => reviewedAndNext(),
      u: () => (view.unified = !view.unified),
      w: () => (view.ignoreWs = !view.ignoreWs),
      "?": () => (showKeys = !showKeys),
      Escape: () => (showKeys = false),
    };
    const action = actions[e.key];
    if (action) {
      e.preventDefault();
      action();
    }
  }

  async function markReviewed() {
    const unreviewed = files.filter((f) => !f.reviewed).length;
    const reason = review?.dirty
      ? "Uncommitted changes are not part of HEAD and will still show as changed."
      : unreviewed
        ? `${unreviewed} file${unreviewed === 1 ? " is" : "s are"} not marked reviewed in this scope.`
        : null;
    if (reason && !confirmMark) {
      confirmMark = reason;
      return;
    }
    confirmMark = null;
    const sha = await api.markReviewed(id);
    say(`Marked ${sha.slice(0, 8)} as reviewed.`);
    view.scope = "review";
    await load();
    await loadFiles();
    onchanged();
  }

  async function prepareGuide() {
    guideBusy = true;
    error = null;
    try {
      const g = await api.prepareGuide(id);
      guide = g.guide;
      guideStale = g.stale;
      guideOpen = true;
      view.order = "guide";
    } catch (e) {
      error = `Could not prepare the review guide: ${e}`;
    } finally {
      guideBusy = false;
    }
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

  function toggleSection(key: string) {
    const opened = new Set(view.opened ?? []);
    if (opened.has(key)) opened.delete(key);
    else opened.add(key);
    view.opened = [...opened];
  }
  const isOpen = (s: { key: string; folded: boolean }) => !s.folded || (view.opened ?? []).includes(s.key);

  const short = (sha: string) => sha.slice(0, 8);
  const linesOf = (c: DraftComment) => (c.fromLine === c.toLine ? `${c.fromLine}` : `${c.fromLine}-${c.toLine}`);
  const CLAIM: Record<string, string> = { implemented: "✓", partial: "◐", missing: "✗" };
  const reviewedOf = (path: string) => files.find((f) => f.path === path)?.reviewed ?? false;
</script>

<svelte:window onkeydown={onKeydown} />

<section class="review" use:externalLinks>
  <header>
    <button class="toggle" onclick={() => (view.sideHidden = !view.sideHidden)}
      title={view.sideHidden ? "Show scopes and files" : "Hide scopes and files (more room for the diff)"}
      aria-label={view.sideHidden ? "Show scopes and files" : "Hide scopes and files"} data-toggle="side">{view.sideHidden ? "»" : "«"}</button>
    {#if review}
      <span class="marker">
        {review.mode === "ref" ? "PR head" : "Worktree"} {short(review.head)} · {review.commits.length} commit{review.commits.length === 1 ? "" : "s"} vs {review.baseRef}
        {#if review.reviewed} · reviewed {short(review.reviewed)}{newCommits ? `, ${newCommits} new since` : ""}{/if}
        {review.dirty ? " · uncommitted changes" : ""}
      </span>
      <span class="progress" class:complete={done.total > 0 && done.reviewed === done.total} title="Files of this scope marked reviewed" data-progress>
        {done.reviewed}/{done.total} reviewed
      </span>
      <button onclick={load} title="Reload">↻</button>
      {#if confirmMark}
        <span class="warn">{confirmMark}</span>
        <button onclick={markReviewed}>Mark anyway</button>
        <button onclick={() => (confirmMark = null)}>Cancel</button>
      {:else}
        <button class="primary" onclick={markReviewed} disabled={review.reviewed === review.head && !review.dirty}>Mark reviewed</button>
      {/if}
      <button onclick={prepareGuide} disabled={guideBusy} title="Claude reads the change and suggests where to start, what each part does, and checks the description's claims">
        {guideBusy ? "Preparing the guide…" : guide ? "Regenerate guide" : "Prepare review guide"}
      </button>
    {/if}
    <button class="keys" onclick={() => (showKeys = !showKeys)} title="Keyboard shortcuts (?)" aria-label="Keyboard shortcuts">?</button>
    {#if notice}<span class="notice">{notice}{#if noticeUrl} <a href={noticeUrl}>open</a>{/if}</span>{/if}
  </header>
  {#if error}<p class="error">{error}</p>{/if}
  {#if scope === "review" && review?.rebaseNote}<p class="rebased" data-rebase-note>{review.rebaseNote}</p>{/if}
  {#if showKeys}
    <div class="shortcuts" role="dialog" aria-label="Keyboard shortcuts">
      {#each SHORTCUTS as [key, what]}<div><kbd>{key}</kbd> {what}</div>{/each}
    </div>
  {/if}

  <div class="body">
    {#if !view.sideHidden}
    <nav class="side">
      {#if guide}
        <div class="guide" data-guide>
          <h3>
            <button class="section" onclick={() => (guideOpen = !guideOpen)} aria-expanded={guideOpen}>{guideOpen ? "▾" : "▸"} Review guide</button>
            {#if guideStale}<span class="stale" title="The change moved on since the guide was made">outdated</span>{/if}
          </h3>
          {#if guideOpen}
            <p class="focus">{guide.focus}</p>
            {#if guide.claims.length}
              <ul class="claims">
                {#each guide.claims as c}
                  <li class={c.status} title={c.files.join("\n")}><span class="mark">{CLAIM[c.status] ?? "?"}</span> {c.claim}</li>
                {/each}
              </ul>
            {/if}
            {#if guide.unexplained.length}
              <p class="unexplained">Not explained by the description: {guide.unexplained.join(", ")}</p>
            {/if}
          {/if}
        </div>
      {/if}

      <h3>
        <button class="section" onclick={() => (view.commitsOpen = !view.commitsOpen)} aria-expanded={!!view.commitsOpen} data-toggle="commits">
          {view.commitsOpen ? "▾" : "▸"} Scope <span class="count">{review ? `${review.commits.length} commit${review.commits.length === 1 ? "" : "s"}` : ""}</span>
        </button>
      </h3>
      <ul class="scopes">
        {#if view.commitsOpen}
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
        {:else}
          <!-- Collapsed: only the active scope, plus quick switches between the two main ones. -->
          <li><button class="active" onclick={() => (view.commitsOpen = true)} title="Show all scopes and commits">{scopeLabel}</button></li>
          {#if scope !== "all"}<li><button onclick={() => setScope("all")} data-scope="all">All changes vs {review?.baseRef ?? "base"}</button></li>{/if}
          {#if review?.reviewed && scope !== "review"}<li><button onclick={() => setScope("review")} data-scope="review">Since your review</button></li>{/if}
        {/if}
      </ul>

      <div class="files-head">
        <h3>Files</h3>
        <select bind:value={view.order} aria-label="File order" data-order>
          <option value="review">Review order</option>
          <option value="tree">Tree</option>
          {#if guide}<option value="guide">Guide</option>{/if}
        </select>
      </div>
      {#each list as s (s.key)}
        <div class="group" data-group={s.key}>
          <button class="group-title" onclick={() => toggleSection(s.key)} aria-expanded={isOpen(s)}>
            {isOpen(s) ? "▾" : "▸"} {s.title}
            {#if s.risk}<span class="risk {s.risk}">{s.risk}</span>{/if}
            <span class="count">{s.files.filter((f) => f.reviewed).length}/{s.files.length}</span>
          </button>
          {#if s.intent && isOpen(s)}<p class="intent">{s.intent}</p>{/if}
          {#if isOpen(s)}
            <ul class="files">
              {#each s.files as f (f.path)}
                <li class:reviewed={f.reviewed}>
                  <input type="checkbox" checked={f.reviewed} onchange={(e) => setReviewed(f.path, (e.currentTarget as HTMLInputElement).checked)}
                    aria-label="Reviewed {f.path}" data-reviewed={f.path} />
                  <button class:active={f.path === view.file} onclick={() => (view.file = f.path)} title={f.path} data-file={f.path}>
                    <span class="status s{f.status === '?' ? 'U' : f.status}">{f.status === "?" ? "U" : f.status}</span>{f.path}
                    {#if f.whitespaceOnly}<span class="ws">whitespace</span>{/if}
                  </button>
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      {:else}
        <p class="none">No changes.</p>
      {/each}
    </nav>
    {/if}

    <div class="main">
      <div class="toolbar">
        {#if view.sideHidden && ordered.length}
          <button onclick={() => stepFile(-1)} title="Previous file ([)" aria-label="Previous file">‹</button>
          <span class="current" title={view.file ?? ""}>{view.file ?? ""} <span class="muted">{fileIndex + 1}/{ordered.length} · {scopeLabel}</span></span>
          <button onclick={() => stepFile(1)} title="Next file (])" aria-label="Next file">›</button>
        {/if}
        {#if editedPath}<button class="primary" onclick={save}>Save {editedPath}</button>{/if}
        {#if selection && view.file}
          <span class="sel">Lines {selection.fromLine}{selection.toLine !== selection.fromLine ? `–${selection.toLine}` : ""}:</span>
          <input bind:value={draft} placeholder="Review comment" onkeydown={(e) => e.key === "Enter" && addComment()} />
          <button onclick={addComment} disabled={!draft.trim()}>Add comment</button>
        {:else}
          <span class="hint">Select code to comment on it{editable ? "; edit it to change the file" : ""}.</span>
        {/if}
        <span class="spacer"></span>
        <label class="opt" title="Ignore whitespace (w)"><input type="checkbox" bind:checked={view.ignoreWs} data-opt="ws" /> whitespace</label>
        <label class="opt" title="Unified diff (u)"><input type="checkbox" bind:checked={view.unified} data-opt="unified" /> unified</label>
        {#if view.file}
          <button class="primary" onclick={reviewedAndNext} disabled={reviewedOf(view.file) && !ordered.some((f) => !f.reviewed)}
            title="Mark this file reviewed and go to the next unreviewed one (r)" data-reviewed-next>
            {reviewedOf(view.file) ? "Next unreviewed (r)" : "Reviewed, next (r)"}
          </button>
        {/if}
      </div>
      {#if here && guide}
        <!-- The guide for the file shown, where it is seen with the side panel closed too. -->
        <div class="file-guide" class:open={fileGuideOpen} data-file-guide>
          <button class="expand" onclick={() => (fileGuideOpen = !fileGuideOpen)} aria-expanded={fileGuideOpen}
            title={fileGuideOpen ? "Show one line" : "Show all of it, and the guide's focus for the whole change"}>{fileGuideOpen ? "▾" : "▸"}</button>
          <div class="what">
            {#if here.group}
              <span class="risk {here.group.risk}">{here.group.risk}</span> <strong>{here.group.title}</strong> — {here.group.intent}
            {:else}
              <span class="muted">Not in the review guide.</span>
            {/if}
            {#each here.claims as c}<span class="claim {c.status}"> · <span class="mark">{CLAIM[c.status] ?? "?"}</span> {c.claim}</span>{/each}
            {#if here.unexplained}<span class="warn"> · not explained by the description</span>{/if}
            {#if guideStale}<span class="stale"> · guide outdated</span>{/if}
            {#if fileGuideOpen}<p class="focus-all"><span class="muted">Focus for the whole change:</span> {guide.focus}</p>{/if}
          </div>
        </div>
      {/if}
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
  .review { display: flex; flex-direction: column; height: 100%; min-height: 0; position: relative; }
  header { display: flex; gap: 10px; align-items: center; padding: 8px 12px; border-bottom: 1px solid var(--border); flex-wrap: wrap; }
  .marker { font-size: 0.9em; }
  .progress { font-size: 0.85em; color: var(--muted); border: 1px solid var(--border); border-radius: 10px; padding: 0 8px; }
  .progress.complete { color: var(--ok); border-color: var(--ok); }
  .toggle, .keys { padding: 2px 8px; }
  .side h3 .section { width: auto; border: 0; background: none; padding: 0; font: inherit; text-transform: inherit; letter-spacing: inherit; color: inherit; }
  .count { text-transform: none; letter-spacing: 0; font-weight: 400; color: var(--muted); margin-left: auto; }
  .current { font-family: var(--mono); font-size: 0.85em; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 40%; }
  .muted { color: var(--muted); }
  .warn { color: var(--warn); font-size: 0.85em; }
  .notice { color: var(--ok); font-size: 0.85em; margin-left: auto; }
  .notice a { color: var(--accent); }
  .rebased { margin: 0; padding: 4px 12px; font-size: 0.85em; color: var(--warn); border-bottom: 1px solid var(--border); }
  .shortcuts { position: absolute; right: 12px; top: 44px; z-index: 5; background: var(--bg); border: 1px solid var(--border); border-radius: 6px; padding: 8px 12px; font-size: 0.85em; box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3); }
  .shortcuts kbd { font-family: var(--mono); display: inline-block; min-width: 3.5em; color: var(--accent); }
  .primary { border-color: var(--accent); }
  .body { flex: 1; display: flex; min-height: 0; }
  .side { width: 320px; flex-shrink: 0; overflow-y: auto; border-right: 1px solid var(--border); padding-bottom: 12px; }
  h3 { font-size: 0.75em; text-transform: uppercase; letter-spacing: 0.05em; color: var(--muted); margin: 12px 12px 4px; display: flex; gap: 6px; align-items: center; }
  .side ul { list-style: none; margin: 0; padding: 0; }
  .side button { width: 100%; text-align: left; border: 0; border-radius: 0; background: none; padding: 3px 12px; font-size: 0.85em; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .side button.active { background: var(--panel); }
  .guide { border-bottom: 1px solid var(--border); padding-bottom: 6px; }
  .guide .stale { color: var(--warn); text-transform: none; letter-spacing: 0; }
  .focus { margin: 0 12px 6px; font-size: 0.88em; }
  .claims { margin: 0 12px 6px !important; font-size: 0.82em; }
  .claims li { padding: 1px 0; }
  .claims .mark { display: inline-block; width: 1.2em; }
  .claims .implemented .mark { color: var(--ok); }
  .claims .partial .mark { color: var(--warn); }
  .claims .missing { color: var(--bad); }
  .unexplained { margin: 0 12px 6px; font-size: 0.82em; color: var(--warn); }
  .files-head { display: flex; align-items: center; justify-content: space-between; padding-right: 12px; }
  .files-head select { font-size: 0.8em; }
  .group-title { display: flex !important; gap: 6px; align-items: center; color: var(--muted); font-size: 0.8em !important; }
  .risk { border-radius: 3px; padding: 0 4px; font-size: 0.9em; }
  .risk.high { color: var(--bad); border: 1px solid var(--bad); }
  .risk.medium { color: var(--warn); border: 1px solid var(--warn); }
  .risk.low { color: var(--muted); border: 1px solid var(--border); }
  .intent { margin: 0 12px 2px 26px; font-size: 0.8em; color: var(--muted); }
  .files li { display: flex; align-items: center; padding-left: 10px; }
  .files li input { margin: 0 2px 0 0; }
  .files li button { padding-left: 4px; }
  .files li.reviewed button { color: var(--muted); }
  .files button { font-family: var(--mono); }
  .ws { margin-left: 6px; font-family: system-ui, sans-serif; font-size: 0.85em; color: var(--muted); }
  .sha { font-family: var(--mono); color: var(--muted); }
  .sha.new { color: var(--accent); }
  .status { display: inline-block; width: 1.4em; font-weight: 700; }
  .sA, .sU { color: var(--ok); }
  .sD { color: var(--bad); }
  .sM { color: var(--warn); }
  .none { padding: 4px 12px; color: var(--muted); font-size: 0.85em; margin: 0; }
  .main { flex: 1; display: flex; flex-direction: column; min-width: 0; }
  .file-guide { display: flex; gap: 6px; align-items: flex-start; padding: 4px 12px; border-bottom: 1px solid var(--border); font-size: 0.85em; background: var(--sidebar); }
  .file-guide .expand { border: 0; background: none; padding: 0 2px; color: var(--muted); }
  .file-guide .what { flex: 1; min-width: 0; }
  .file-guide:not(.open) .what { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .file-guide .claim.implemented .mark { color: var(--ok); }
  .file-guide .claim.partial .mark { color: var(--warn); }
  .file-guide .claim.missing { color: var(--bad); }
  .file-guide .stale { color: var(--warn); }
  .focus-all { margin: 4px 0 0; }
  .toolbar { display: flex; gap: 8px; align-items: center; padding: 6px 12px; border-bottom: 1px solid var(--border); min-height: 30px; flex-wrap: wrap; }
  .toolbar input:not([type]) { flex: 1; min-width: 120px; }
  .spacer { flex: 1; }
  .opt { font-size: 0.85em; color: var(--muted); display: flex; gap: 4px; align-items: center; }
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
