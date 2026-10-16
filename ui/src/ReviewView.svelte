<script lang="ts" module>
  import type { DraftComment } from "./lib/reviewComments";
  import type { Order } from "./lib/review";

  export interface ReviewViewState {
    // "all" (vs the base branch) or a commit sha; empty picks the default.
    scope: string;
    file: string | null;
    comments: DraftComment[];
    // Layout: the commit list expanded, the whole side panel (scopes + files) hidden.
    commitsOpen?: boolean;
    sideHidden?: boolean;
    // Diff: unified instead of side-by-side (unset: unified when the diff column is narrow),
    // whitespace changes ignored; file order.
    unified?: boolean;
    ignoreWs?: boolean;
    order?: Order;
    // Sections of the file list the user opened (generated files start folded) or folded.
    opened?: string[];
    folded?: string[];
  }
</script>

<script lang="ts">
  import { MergeView, getChunks, unifiedMergeView } from "@codemirror/merge";
  import { syntaxHighlighting } from "@codemirror/language";
  import { EditorSelection, EditorState, type Extension } from "@codemirror/state";
  import { EditorView } from "@codemirror/view";
  import { basicSetup } from "codemirror";
  import { onDestroy, tick } from "svelte";
  import { fade } from "svelte/transition";
  import AskAgent from "./AskAgent.svelte";
  import CodeMenu from "./CodeMenu.svelte";
  import Button from "./controls/Button.svelte";
  import Icon from "./controls/Icon.svelte";
  import IconButton from "./controls/IconButton.svelte";
  import Pill from "./controls/Pill.svelte";
  import Segmented from "./controls/Segmented.svelte";
  import Toggle from "./controls/Toggle.svelte";
  import { api, type ReviewState } from "./lib/api";
  import { ariekeiHighlight, ariekeiTheme } from "./lib/codemirrorTheme";
  import { currentChange } from "./lib/currentChange";
  import { gitOverride } from "./lib/gitDiff";
  import { externalLinks } from "./lib/html";
  import { isTyping } from "./lib/keys";
  import { shortcuts } from "./lib/layout.svelte";
  import { languageFor } from "./lib/languages";
  import { layout } from "./lib/layout.svelte";
  import { flatten, neighbourFile, nextChange, rangeOf, scopeOf, sections, type ReviewFile } from "./lib/review";
  import { askMessage, classify, feedbackMessage, refLines, reviewBody, type CodeRef, type LineComment, type Ranges } from "./lib/reviewComments";

  let {
    id,
    view = $bindable(),
    onchanged,
    canSendToAgent,
    agentStatus,
    onterminal,
  }: {
    id: number;
    view: ReviewViewState;
    onchanged: () => void;
    canSendToAgent: boolean;
    /// The status of the agent that gets what is sent (lib/agent.svelte.ts).
    agentStatus?: string;
    /// Shows the agent's terminal (where its answer appears); after asking it about code.
    onterminal?: () => void;
  } = $props();

  let review = $state<ReviewState | null>(null);
  let files: ReviewFile[] = $state([]);
  let error: string | null = $state(null);
  // n / p go from change to change, and on into the next (previous) file at the last (first)
  // change of one: entering it at its first (1) or last (-1) change. With no change further
  // on in the scope, `edge` says so at that end of the diff for a moment.
  let landing: 1 | -1 | 0 = 0;
  let edge = $state<1 | -1 | null>(null);
  let edgeTimer: ReturnType<typeof setTimeout> | undefined;
  const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
  let notice: string | null = $state(null);
  let noticeUrl: string | null = $state(null);
  // Sending to the agent failed: the error offers its terminal.
  let errorTerminal = $state(false);
  let host: HTMLElement | undefined = $state();
  let main: HTMLElement | undefined = $state();
  // The diff shown: a side-by-side MergeView, or one editor with the unified merge view.
  let merge: MergeView | null = null;
  let unified: EditorView | null = null;
  let selection: { fromLine: number; toLine: number; code: string } | null = $state(null);
  let draft = $state("");
  // Right-click on code: its menu, then the prompt box for the agent.
  // `below`: the bottom of the lines, where the prompt box goes so they stay in view.
  let codeMenu: { x: number; y: number; below: number; code: CodeRef } | null = $state(null);
  let asking: { x: number; y: number; code: CodeRef } | null = $state(null);
  let editedPath: string | null = $state(null);
  let editedFile: string | null = null;
  // The file shown has only whitespace changes, and they are hidden.
  let onlyWhitespace = $state(false);
  // Below 720 px the diff column is too narrow for two sides.
  let mainWidth = $state(1000);

  // Submitting to GitHub.
  let submitting = $state(false);
  let event = $state<"COMMENT" | "APPROVE" | "REQUEST_CHANGES">("COMMENT");
  let summary = $state("");
  let preview: { line: LineComment[]; body: DraftComment[] } | null = $state(null);
  let busy = $state(false);

  // (An order saved before the review guide was removed reads as review order.)
  const order = $derived(view.order === "tree" ? "tree" : "review");
  const list = $derived(sections(files, order));
  const ordered = $derived(flatten(list));
  const isUnified = $derived(view.unified ?? mainWidth < 720);

  // The Inbox steps aside on narrow windows while the side panel is open.
  $effect(() => {
    layout.reviewPanel = !view.sideHidden;
  });
  onDestroy(() => (layout.reviewPanel = false));

  const scope = $derived(scopeOf(review, view.scope));
  const range = $derived(review ? rangeOf(review, scope) : null);
  // The commit the right side shows; null when it includes uncommitted changes.
  const shownRev = $derived(range ? (range[1] ?? (review?.dirty ? null : review?.head ?? null)) : null);
  const editable = $derived(range?.[1] === null);

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
      files = await api.reviewFiles(id, range[0], range[1], !!view.ignoreWs);
      if (view.file && !files.some((f) => f.path === view.file)) view.file = null;
      view.file ??= flatten(sections(files, order))[0]?.path ?? null;
    } catch (e) {
      error = String(e);
      files = [];
    }
  }

  function destroyEditors() {
    merge?.destroy();
    unified?.destroy();
    merge = unified = null;
  }

  // The editor whose document is the new version (selection, change navigation).
  const newSide = () => merge?.b ?? unified;

  // Each show() is numbered: one overtaken by a later one (the file changed again while it
  // loaded) leaves the editors to it, and its n / p landing too.
  let showing = 0;
  async function show(path: string) {
    if (!range || !host) return;
    const [from, to] = range;
    const ignoreWs = !!view.ignoreWs;
    const n = ++showing;
    const [d, lang] = await Promise.all([api.reviewFile(id, from, to, path, ignoreWs), languageFor(path)]);
    if (n !== showing || !host) return;
    destroyEditors();
    selection = null;
    editedPath = null;
    onlyWhitespace = ignoreWs && !d.binary && d.hunks.length === 0 && !!files.find((f) => f.path === path)?.whitespaceOnly;
    if (d.binary) {
      host.textContent = "Binary file: not shown.";
      return land();
    }
    host.textContent = "";
    if (onlyWhitespace) return land();
    const common: Extension[] = [basicSetup, ...lang, ariekeiTheme, syntaxHighlighting(ariekeiHighlight)];
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
    const newDoc = [...common, EditorView.editable.of(to === null), EditorState.readOnly.of(to !== null), onUpdate, currentChange];
    const diffConfig = { override: gitOverride(d.old, d.new, d.hunks) };
    const collapseUnchanged = { margin: 3, minSize: 6 };
    if (isUnified) {
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
    land();
  }

  // The changes of the new side, as document ranges in order.
  function changesOf(v: EditorView): { from: number; to: number }[] {
    const info = getChunks(v.state);
    if (!info) return [];
    const a = info.side === "a";
    return info.chunks.map((c) => (a ? { from: c.fromA, to: c.toA } : { from: c.fromB, to: c.toB }));
  }

  function goToChange(v: EditorView, c: { from: number; to: number }) {
    v.dispatch({ selection: { anchor: c.from }, userEvent: "select.byChunk", effects: EditorView.scrollIntoView(EditorSelection.range(c.to, c.from)) });
  }

  // n (dir 1) / p (dir -1). Files of folded sections (generated ones, unless opened) are passed
  // over, as they are in the list.
  function stepChange(dir: 1 | -1) {
    const v = newSide();
    if (v) {
      const changes = changesOf(v);
      const i = nextChange(changes, v.state.selection.main.head, dir);
      if (i !== null) return goToChange(v, changes[i]);
    }
    const shown = list.filter(isOpen).flatMap((sec) => sec.files.map((f) => f.path));
    const paths = view.file && !shown.includes(view.file) ? ordered.map((f) => f.path) : shown;
    const next = neighbourFile(paths, view.file, dir);
    if (!next) {
      clearTimeout(edgeTimer);
      edge = dir;
      edgeTimer = setTimeout(() => (edge = null), 1800);
      return;
    }
    landing = dir;
    view.file = next;
  }

  // A file entered with n / p: on to its first (last) change, or past it when it shows none.
  function land() {
    const dir = landing;
    landing = 0;
    if (!dir) return;
    const v = newSide();
    const changes = v ? changesOf(v) : [];
    if (v && changes.length) goToChange(v, changes[dir > 0 ? 0 : changes.length - 1]);
    else stepChange(dir);
  }

  $effect(() => {
    void id;
    load();
  });
  $effect(() => {
    void range?.[0];
    void range?.[1];
    void view.ignoreWs;
    loadFiles();
  });
  $effect(() => {
    void isUnified;
    void view.ignoreWs;
    if (view.file && range) show(view.file);
    return destroyEditors;
  });

  $effect(() => {
    if (!main) return;
    const observer = new ResizeObserver(([entry]) => (mainWidth = entry.contentRect.width));
    observer.observe(main);
    return () => observer.disconnect();
  });

  function setScope(value: string) {
    view.scope = value;
  }

  const scopeLabel = $derived.by(() => {
    if (scope === "all") return `All changes vs ${review?.baseRef ?? "base"}`;
    const c = review?.commits.find((c) => c.sha === scope);
    return c ? `${short(c.sha)} ${c.subject}` : scope;
  });

  const fileIndex = $derived(ordered.findIndex((f) => f.path === view.file));
  function stepFile(delta: number) {
    if (!ordered.length) return;
    const next = (Math.max(fileIndex, 0) + delta + ordered.length) % ordered.length;
    view.file = ordered[next].path;
  }

  // Shortcuts work while the review is shown, wherever the focus is (e.g. still on the Review
  // tab or an Inbox row), except where the user types: fields, the editable diff, the terminal.
  function onKeydown(e: KeyboardEvent) {
    if (e.ctrlKey || e.metaKey || e.altKey || isTyping(e.target)) return;
    if (e.key === "Escape" && !shortcuts.open && selection) {
      clearSelection();
      return;
    }
    const actions: Record<string, () => void> = {
      a: () => askAboutSelection(),
      "]": () => stepFile(1),
      "[": () => stepFile(-1),
      n: () => stepChange(1),
      p: () => stepChange(-1),
      u: () => (view.unified = !isUnified),
      w: () => (view.ignoreWs = !view.ignoreWs),
    };
    const action = actions[e.key];
    if (action) {
      e.preventDefault();
      action();
    }
  }

  function clearSelection() {
    const v = newSide();
    if (v) v.dispatch({ selection: { anchor: v.state.selection.main.head } });
    selection = null;
  }

  function addComment(e?: SubmitEvent) {
    e?.preventDefault();
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

  // The editor an element is in, and its side of the diff.
  function editorAt(target: Node): { editor: EditorView; side: "new" | "old" } | null {
    if (merge?.a.dom.contains(target)) return { editor: merge.a, side: "old" };
    const v = newSide();
    return v?.dom.contains(target) ? { editor: v, side: "new" } : null;
  }

  // The lines from..to (document offsets) of one side, as the agent is told about them.
  function codeRef(editor: EditorView, side: "new" | "old", from: number, to: number): CodeRef | null {
    if (!view.file || !range) return null;
    const doc = editor.state.doc;
    // A selection that ends at the start of a line does not include that line.
    const end = to > from && doc.lineAt(to).from === to ? to - 1 : to;
    const first = doc.lineAt(from);
    return {
      path: view.file,
      fromLine: first.number,
      toLine: doc.lineAt(end).number,
      code: from === to ? first.text : doc.sliceString(from, to),
      side,
      rev: side === "old" ? range[0] : shownRev,
      unsaved: side === "new" && editedPath === view.file,
    };
  }

  // Right-click (or the ContextMenu key) on code: the selection when the click is in it,
  // else the clicked line, which gets selected so it shows what the menu is about.
  function onContextMenu(e: MouseEvent) {
    const target = e.target as HTMLElement;
    // Deleted lines in the unified diff are not part of a document: the browser's menu.
    if (target.closest?.(".cm-deletedChunk")) return;
    const at = editorAt(target);
    if (!at) return;
    const { editor, side } = at;
    const sel = editor.state.selection.main;
    let [from, to] = [sel.from, sel.to];
    let point = { x: e.clientX, y: e.clientY };
    const fromKeyboard = e.clientX === 0 && e.clientY === 0;
    const pos = fromKeyboard ? sel.head : editor.posAtCoords(point);
    if (pos === null) return;
    if (sel.empty || pos < sel.from || pos > sel.to) {
      const line = editor.state.doc.lineAt(pos);
      [from, to] = [line.from, line.to];
      editor.dispatch({ selection: { anchor: from, head: to } });
    }
    if (fromKeyboard) {
      const c = editor.coordsAtPos(pos);
      if (c) point = { x: c.left, y: c.bottom };
    }
    const code = codeRef(editor, side, from, to);
    if (!code) return;
    e.preventDefault();
    asking = null;
    codeMenu = { ...point, below: Math.max(point.y, editor.coordsAtPos(to)?.bottom ?? 0), code };
  }

  // `a`: ask about the selection on the new side.
  function askAboutSelection() {
    const v = newSide();
    const sel = v?.state.selection.main;
    if (!v || !sel || sel.empty || !canSendToAgent) return;
    const code = codeRef(v, "new", sel.from, sel.to);
    const c = v.coordsAtPos(sel.head);
    if (code && c) asking = { x: c.left, y: c.bottom, code };
  }

  async function commentOn() {
    await tick();
    document.getElementById(`composer-${id}`)?.focus();
  }

  async function copyCode(code: CodeRef) {
    try {
      await navigator.clipboard.writeText(code.code);
    } catch (e) {
      error = `Could not copy: ${e}`;
    }
  }

  async function ask(code: CodeRef, prompt: string) {
    error = null;
    try {
      await api.agentSend(id, askMessage(code, prompt));
      asking = null;
      // Its answer appears in the terminal.
      if (onterminal) onterminal();
      else say(`Sent ${code.path}:${refLines(code)} and your prompt to the agent.`);
    } catch (e) {
      error = `Could not send to the agent: ${e}`;
      errorTerminal = true;
    }
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

  async function submit(e?: SubmitEvent) {
    e?.preventDefault();
    if (!preview) return;
    busy = true;
    try {
      const body = reviewBody(summary, preview.body);
      if (event === "REQUEST_CHANGES" && !body && !preview.line.length) throw new Error("To request changes, add a summary or a line comment.");
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

  // Sections start open, except generated files; the user's choice is remembered either way.
  const isOpen = (s: { key: string; folded: boolean }) =>
    s.folded ? (view.opened ?? []).includes(s.key) : !(view.folded ?? []).includes(s.key);
  function toggleSection(s: { key: string; folded: boolean }) {
    const key = s.key;
    const flip = (list: string[] | undefined) => {
      const set = new Set(list ?? []);
      if (set.has(key)) set.delete(key);
      else set.add(key);
      return [...set];
    };
    if (s.folded) view.opened = flip(view.opened);
    else view.folded = flip(view.folded);
  }

  // Directory sub-headers in Review order and Tree: the directory of each run of files.
  const dirOf = (path: string) => (path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "");
  const nameOf = (path: string) => path.slice(path.lastIndexOf("/") + 1);
  const shortDir = (dir: string) => (dir.split("/").length > 3 ? `…/${dir.split("/").slice(-3).join("/")}` : dir);

  const short = (sha: string) => sha.slice(0, 8);
  const linesOf = (c: DraftComment) => (c.fromLine === c.toLine ? `${c.fromLine}` : `${c.fromLine}–${c.toLine}`);
  const statusOf = (f: ReviewFile) => (f.status === "?" ? "U" : f.status);
  const STATUS_TEXT: Record<string, string> = { A: "added", D: "deleted", U: "uncommitted", M: "modified" };
  const ORDERS = [
    { value: "review" as const, label: "Review order" },
    { value: "tree" as const, label: "Tree" },
  ];
  const EVENTS = [
    ["COMMENT", "Comment"],
    ["APPROVE", "Approve"],
    ["REQUEST_CHANGES", "Request changes"],
  ] as const;
</script>

<svelte:window onkeydown={onKeydown} />

<div class="review" use:externalLinks role="tabpanel" aria-label="Review">
  <header>
    {#if review}
      <div class="marker" data-marker title="{review.mode === 'ref' ? 'PR head' : 'Worktree'} {review.head}">
        <Icon name="branch" size={14} />
        <span class="text">
          {review.mode === "ref" ? "PR head" : "Worktree"} <code>{short(review.head)}</code> · {review.commits.length} commit{review.commits.length === 1 ? "" : "s"} vs {review.baseRef}{#if review.dirty}{" · "}<span class="warn">uncommitted changes</span>{/if}
        </span>
      </div>
      <IconButton icon="refresh" label="Reload" onclick={() => { load(); loadFiles(); }} data-reload />
    {:else}
      <span class="spacer"></span>
    {/if}
    <IconButton icon="panel" label={view.sideHidden ? "Show the side panel" : "Hide the side panel"} pressed={!view.sideHidden}
      onclick={() => (view.sideHidden = !view.sideHidden)} data-toggle="side" />
    <IconButton icon="keyboard" label="Keyboard shortcuts (?)" onclick={() => (shortcuts.open = !shortcuts.open)} data-keys />
  </header>

  {#if error}
    <div class="note bad" role="alert"><Icon name="error" size={14} /><span class="grow">{error}</span>
      {#if errorTerminal && onterminal}<Button size="sm" variant="quiet" icon="terminal" onclick={onterminal}>Open Terminal</Button>{/if}
      <IconButton icon="x" label="Dismiss" size={22} onclick={() => { error = null; errorTerminal = false; }} /></div>
  {/if}
  {#if notice}
    <div class="note" class:gh={!!noticeUrl} role="status" data-notice>
      <Icon name={noticeUrl ? "submit-github" : "info"} size={14} /><span class="grow">{notice}</span>
      {#if noticeUrl}<a href={noticeUrl}>Open</a>{/if}
      <IconButton icon="x" label="Dismiss" size={22} onclick={() => (notice = null)} />
    </div>
  {/if}
  <div class="body">
    {#if !view.sideHidden}
      <aside class="side" aria-label="Review side panel">
        <section class="scope">
          <button class="section-head" onclick={() => (view.commitsOpen = !view.commitsOpen)} aria-expanded={!!view.commitsOpen} data-toggle="commits">
            <span class="chev" class:folded={!view.commitsOpen}><Icon name="chevron-down" size={14} /></span>
            <span class="grow">Scope</span>
            <span class="count">{review ? `${review.commits.length} commit${review.commits.length === 1 ? "" : "s"}` : ""}</span>
          </button>
          {#if view.commitsOpen}
            <div class="scopes" role="listbox" aria-label="Scope">
              <button role="option" aria-selected={scope === "all"} class:active={scope === "all"} onclick={() => setScope("all")} data-scope="all">
                <span class="grow">All changes vs {review?.baseRef ?? "base"}</span>
              </button>
              {#each review?.commits ?? [] as c (c.sha)}
                <button role="option" aria-selected={scope === c.sha} class:active={scope === c.sha} onclick={() => setScope(c.sha)}
                  title="{c.subject}&#10;{c.author}, {c.date}" data-scope={c.sha}>
                  <code class="sha">{short(c.sha)}</code>
                  <span class="grow subject">{c.subject}</span>
                </button>
              {/each}
            </div>
          {:else}
            <!-- Folded: the active scope. -->
            <div class="scope-folded">
              <div class="active-scope" title={scopeLabel}>{scopeLabel}</div>
            </div>
          {/if}
        </section>

        <section class="files-section">
          <div class="files-head">
            <span class="grow label-caps">Files</span>
            <Segmented label="File order" options={ORDERS} bind:value={() => order, (v) => (view.order = v)} data-order />
          </div>
          {#each list as s (s.key)}
            {@const open = isOpen(s)}
            <div class="group" data-group={s.key}>
              <button class="group-title" onclick={() => toggleSection(s)} aria-expanded={open}>
                <span class="chev" class:folded={!open}><Icon name="chevron-down" size={14} /></span>
                <span class="grow title">{s.title}</span>
                <span class="count">{s.files.length}</span>
              </button>
              {#if open}
                <ul class="files">
                  {#each s.files as f, i (f.path)}
                    {@const dir = dirOf(f.path)}
                    {#if dir && (i === 0 || dirOf(s.files[i - 1].path) !== dir)}
                      <li class="dir" data-dir={dir} title={dir}>{shortDir(dir)}</li>
                    {/if}
                    <li class="file" class:current={f.path === view.file} class:deleted={statusOf(f) === "D"}>
                      <span class="st s{statusOf(f)}" title={STATUS_TEXT[statusOf(f)] ?? statusOf(f)}>{statusOf(f)}</span>
                      <button class:active={f.path === view.file} onclick={() => (view.file = f.path)} title={f.path} data-file={f.path}>{!dir ? f.path : nameOf(f.path)}</button>
                      {#if f.whitespaceOnly}<span class="ws"><Pill size="xs">whitespace</Pill></span>{/if}
                    </li>
                  {/each}
                </ul>
              {/if}
            </div>
          {:else}
            <p class="none">No changes in this scope.</p>
          {/each}
        </section>
      </aside>
    {/if}

    <div class="main" bind:this={main}>
      <div class="toolbar" role="toolbar" aria-label="Diff">
        {#if view.sideHidden && ordered.length}
          <IconButton icon="chevron-up" label="Previous file ([)" size={24} onclick={() => stepFile(-1)} />
          <IconButton icon="chevron-down" label="Next file (])" size={24} onclick={() => stepFile(1)} />
          <span class="current-file" title={view.file ?? ""}><strong>{view.file ? nameOf(view.file) : ""}</strong> <span class="muted">{fileIndex + 1}/{ordered.length} · {scopeLabel}</span></span>
        {/if}
        {#if editedPath}<Button variant="primary" onclick={save} data-save>Save {nameOf(editedPath)}</Button>{/if}
        {#if selection && view.file}
          <form class="composer" onsubmit={addComment} data-composer>
            <label for="composer-{id}">Lines {selection.fromLine}{selection.toLine !== selection.fromLine ? `–${selection.toLine}` : ""}:</label>
            <input id="composer-{id}" bind:value={draft} placeholder="Comment on these lines" />
            <Button type="submit" variant="primary" disabled={!draft.trim()} data-add-comment>Add comment</Button>
            <IconButton icon="x" label="Cancel comment" size={24} type="button" onclick={clearSelection} />
          </form>
        {:else}
          <span class="hint">Select code to comment on it{editable ? "; edit it to change the file" : ""}</span>
        {/if}
        <Toggle pressed={!!view.ignoreWs} icon="whitespace" kbd="w" title="Hide whitespace (w)" onclick={() => (view.ignoreWs = !view.ignoreWs)} data-opt="ws"><span class="tlabel">Whitespace</span></Toggle>
        <Toggle pressed={isUnified} icon="unified" kbd="u" title="Unified diff (u)" onclick={() => (view.unified = !isUnified)} data-opt="unified"><span class="tlabel">Unified</span></Toggle>
      </div>
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div class="diff" bind:this={host} tabindex="0" role="region" oncontextmenu={onContextMenu}
        aria-label="Diff of {view.file ?? 'no file'}. Shortcuts: ] and [ files, n and p changes (on into the next or previous file), u unified, w whitespace, a ask the agent about the selection, ? help. Right-click code to ask the agent about it."></div>
      {#if edge}
        <!-- At that end of the diff itself (the toolbar above it, the drafts bar below). -->
        <div class="edge" style:top={edge < 0 ? `${(host?.offsetTop ?? 0) + 12}px` : undefined}
          style:bottom={edge > 0 ? `${(main?.clientHeight ?? 0) - (host ? host.offsetTop + host.offsetHeight : 0) + 16}px` : undefined} role="status"
          transition:fade={{ duration: still ? 0 : 180 }} data-edge={edge > 0 ? "end" : "start"}>
          <Icon name={edge > 0 ? "chevron-down" : "chevron-up"} size={14} />
          {edge > 0 ? "That was the last change in this scope" : "That was the first change in this scope"}
        </div>
      {/if}
      {#if codeMenu}
        {@const m = codeMenu}
        <CodeMenu x={m.x} y={m.y} lines={m.code.fromLine === m.code.toLine ? `line ${m.code.fromLine}` : `lines ${refLines(m.code)}`}
          noAgent={canSendToAgent ? null : "No agent works on this item: start working on it first"}
          oncomment={m.code.side === "new" ? commentOn : undefined}
          onask={() => (asking = { x: m.x, y: m.below, code: m.code })} oncopy={() => copyCode(m.code)} onclose={() => (codeMenu = null)} />
      {/if}
      {#if asking}
        {@const a = asking}
        <AskAgent x={a.x} y={a.y} code={a.code} status={agentStatus} onsend={(prompt) => ask(a.code, prompt)} onclose={() => (asking = null)} />
      {/if}
      {#if onlyWhitespace}
        <div class="ws-only">
          <strong>Only whitespace changed in this file</strong>
          <span class="muted">Whitespace changes are hidden.</span>
          <Button onclick={() => (view.ignoreWs = false)} kbd="w">Show whitespace</Button>
        </div>
      {/if}

      <div class="dock" data-dock>
        {#if view.comments.length || submitting}
          <div class="drafts">
            {#if view.comments.length}
              <div class="label-caps">Draft comments · {view.comments.length}</div>
              <ul data-drafts>
                {#each view.comments as c, i}
                  <li>
                    <svg width="8" height="8" viewBox="0 0 8 8" aria-hidden="true"><rect width="8" height="8" rx="1.5" /></svg>
                    <code title={c.path}>{nameOf(c.path)}:{linesOf(c)}</code>
                    <span class="text" title={c.text}>{c.text}</span>
                    <IconButton icon="trash" label="Remove draft" size={22} onclick={() => (view.comments = view.comments.filter((_, j) => j !== i))} />
                  </li>
                {/each}
              </ul>
            {/if}
            {#if submitting}
              <form class="submit" onsubmit={submit} aria-label="Submit review to GitHub" data-submit>
                <label><span class="field">Summary</span>
                  <textarea bind:value={summary} rows="2" placeholder="Overall comment (optional for a comment review)"></textarea>
                </label>
                <div class="events" role="group" aria-label="Event">
                  <span class="field">Event</span>
                  {#each EVENTS as [value, label]}
                    <Button type="button" size="sm" pressed={event === value} class="event" disabled={value !== "COMMENT" && review?.own}
                      onclick={() => (event = value)} data-event={value}>{label}</Button>
                  {/each}
                  {#if review?.own}<span class="faint">No approve on your own PR</span>{/if}
                </div>
                {#if preview}
                  <p class="preview" data-preview>
                    {preview.line.length} as line comment{preview.line.length === 1 ? "" : "s"} on the PR diff{#if preview.body.length}; {preview.body.length} in the review text (not on lines of the PR diff at its head commit){/if}.
                  </p>
                {/if}
                <div class="actions">
                  <Button type="button" variant="quiet" onclick={() => (submitting = false)}>Cancel</Button>
                  <Button type="submit" variant="gh" icon="submit-github" loading={busy} disabled={!preview} data-send-review>Submit to GitHub</Button>
                </div>
              </form>
            {:else}
              <div class="actions">
                {#if canSendToAgent}<Button variant="agent" icon="send-to-agent" onclick={sendToAgent} data-send-agent>Send to the agent</Button>{/if}
                {#if review?.prHead}<Button icon="submit-github" onclick={openSubmit} data-open-submit>Submit to GitHub…</Button>{/if}
              </div>
            {/if}
          </div>
        {:else}
          <div class="empty-dock">
            <span class="grow">No draft comments</span>
            {#if review?.prHead}<Button variant="quiet" icon="submit-github" onclick={openSubmit} data-open-submit>Review on GitHub…</Button>{/if}
          </div>
        {/if}
      </div>
    </div>
  </div>

</div>

<style>
  .review { flex: 1; display: flex; flex-direction: column; height: 100%; min-height: 0; position: relative; }
  .grow { flex: 1; min-width: 0; }
  .muted { color: var(--text-muted); }
  .faint { color: var(--text-faint); }
  .spacer { flex: 1; }
  .chev { display: flex; transition: transform var(--dur-fast) var(--ease); }
  .chev.folded { transform: rotate(-90deg); }

  /* Header: what is shown, progress and the review actions. */
  header {
    flex: none; display: flex; align-items: center; gap: var(--sp-3); min-height: 38px;
    padding: 0 var(--sp-4) 0 var(--sp-7); border-bottom: 1px solid var(--border); box-sizing: border-box;
  }
  .marker { flex: 1; min-width: 0; display: flex; align-items: center; gap: var(--sp-3); font-size: var(--fs-12); color: var(--text-muted); white-space: nowrap; }
  .marker :global(svg) { flex: none; }
  .marker .text { overflow: hidden; text-overflow: ellipsis; }
  .marker code { font-size: 11.5px; color: var(--text); }
  .warn { color: var(--warn); }

  /* Notes under the header. */
  .note {
    flex: none; display: flex; align-items: center; gap: var(--sp-4); min-height: 28px; padding: var(--sp-1) var(--sp-3) var(--sp-1) var(--sp-7);
    border-bottom: 1px solid var(--border); background: var(--info-bg); font-size: var(--fs-12); box-sizing: border-box;
  }
  .note > :global(svg) { flex: none; color: var(--info); }
  .note.gh { background: var(--gh-bg); }
  .note.gh > :global(svg), .note.gh a { color: var(--gh-strong); }
  .note a { font-weight: var(--fw-medium); text-decoration: none; padding: 0 var(--sp-4); }
  .note.bad { background: var(--bad-bg); }
  .note.bad > :global(svg) { color: var(--bad); }

  .body { flex: 1; display: flex; min-height: 0; }

  /* Side panel: scope, files. */
  .side {
    flex: none; width: var(--w-panel); display: flex; flex-direction: column; min-height: 0; overflow-y: auto;
    border-right: 1px solid var(--border); background: var(--surface-2); font-size: var(--fs-12); line-height: var(--lh-12);
  }
  @media (max-width: 1100px) { .side { width: var(--w-panel-compact); } }
  .side > section { flex: none; display: flex; flex-direction: column; border-bottom: 1px solid var(--border); }
  .side > section.files-section { flex: 1 0 auto; border-bottom: 0; }
  .section-head {
    display: flex; align-items: center; gap: var(--sp-3); height: 30px; padding: 0 var(--sp-5); flex: none;
    border: 0; border-radius: 0; background: transparent; color: var(--text-muted); text-align: left;
    font-size: var(--fs-11); font-weight: var(--fw-semibold); letter-spacing: 0.04em; text-transform: uppercase;
  }
  .section-head:focus-visible, .group-title:focus-visible { outline-offset: -2px; }
  .section-head .count { letter-spacing: 0; text-transform: none; font-weight: var(--fw-regular); }

  .scopes { padding: 0 var(--sp-3) var(--sp-4); display: flex; flex-direction: column; gap: 1px; }
  .scopes button {
    display: flex; align-items: center; gap: var(--sp-4); min-height: 26px; padding: var(--sp-1) var(--sp-3);
    border: 0; border-radius: var(--r-md); background: transparent; color: var(--text); font-size: var(--fs-12); text-align: left;
  }
  .scopes button:hover { background: var(--surface-hover); }
  .scopes button.active { background: var(--surface-selected); font-weight: var(--fw-semibold); }
  .scopes button:focus-visible { outline-offset: -2px; }
  .sha { flex: none; font-size: var(--fs-11); color: var(--text-muted); }
  .subject { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .scope-folded { padding: 0 var(--sp-5) var(--sp-5); display: flex; flex-direction: column; gap: var(--sp-3); }
  .active-scope { font-weight: var(--fw-semibold); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }

  .files-head { display: flex; align-items: center; gap: var(--sp-3); height: 34px; padding: 0 var(--sp-4) 0 var(--sp-5); flex: none; }
  .group { display: flex; flex-direction: column; padding-bottom: var(--sp-2); }
  .group-title {
    display: flex; align-items: center; gap: var(--sp-3); min-height: 26px; padding: var(--sp-1) var(--sp-5);
    border: 0; border-radius: 0; background: transparent; color: var(--text); font-size: var(--fs-12); font-weight: var(--fw-semibold); text-align: left;
  }
  .group-title .title { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .group .count { flex: none; font-family: var(--font-mono); font-size: var(--fs-11); font-weight: var(--fw-medium); color: var(--text-muted); }
  .files { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 1px; }
  .dir { padding: var(--sp-2) var(--sp-5) 1px var(--sp-7); font-family: var(--font-mono); font-size: var(--fs-11); color: var(--text-faint); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .file {
    display: flex; align-items: center; gap: var(--sp-3); height: 24px; margin: 0 var(--sp-2); padding: 0 var(--sp-3) 0 14px;
    border-radius: var(--r-md);
  }
  .file:hover { background: var(--surface-hover); }
  .file.current { background: var(--surface-selected); box-shadow: inset 0 0 0 1px var(--you-line); }
  .st { flex: none; width: 12px; font-family: var(--font-mono); font-size: var(--fs-11); font-weight: var(--fw-semibold); text-align: center; color: var(--text-muted); }
  .st.sA { color: var(--ok); }
  .st.sD { color: var(--bad); }
  .st.sU { color: var(--warn); }
  .file button {
    flex: 1; min-width: 0; height: 22px; padding: 0; border: 0; border-radius: 0; background: transparent; color: var(--text);
    font-size: var(--fs-12); text-align: left; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .file button:focus-visible { outline-offset: 0; }
  .file.current button { font-weight: var(--fw-semibold); color: var(--text); }
  .file.deleted button { text-decoration: line-through; color: var(--text-muted); }
  .ws { flex: none; display: flex; }
  .none { padding: var(--sp-2) var(--sp-6); color: var(--text-muted); margin: 0; }

  /* The diff column. */
  .main { flex: 1; display: flex; flex-direction: column; min-width: 0; min-height: 0; position: relative; }
  .toolbar {
    flex: none; display: flex; align-items: center; gap: var(--sp-3); min-height: var(--h-toolbar); padding: 0 var(--sp-4);
    border-bottom: 1px solid var(--border); box-sizing: border-box;
  }
  .current-file { flex: 0 1 auto; display: inline-flex; align-items: baseline; gap: var(--sp-3); font-size: var(--fs-12); white-space: nowrap; max-width: 40%; overflow: hidden; text-overflow: ellipsis; }
  .current-file strong { font-family: var(--font-mono); font-weight: var(--fw-semibold); }
  .hint { flex: 1; min-width: 0; font-size: var(--fs-12); color: var(--text-faint); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .composer { flex: 1; min-width: 0; display: flex; align-items: center; gap: var(--sp-3); margin: 0; }
  .composer label { flex: none; font-size: var(--fs-12); font-weight: var(--fw-semibold); color: var(--you-strong); }
  .composer input { flex: 1; min-width: 120px; height: var(--h-control); }
  @media (max-width: 1200px) { .tlabel { display: none; } }

  /* n / p found no change further on: a raised note at that end of the diff. */
  .edge {
    position: absolute; left: 50%; z-index: 5; transform: translateX(-50%);
    display: flex; align-items: center; gap: var(--sp-3); padding: var(--sp-3) var(--sp-6); white-space: nowrap;
    border-radius: var(--r-lg); background: var(--surface-raised); box-shadow: var(--shadow-pop);
    font-size: var(--fs-12); color: var(--text);
  }
  .edge :global(svg) { color: var(--text-muted); }
  .diff { flex: 1; overflow: auto; min-height: 0; background: var(--surface-editor); }
  .diff:focus-visible { outline: 2px solid var(--focus); outline-offset: -2px; }
  .diff :global(.cm-editor) { font-size: var(--fs-code); }
  .diff :global(.cm-editor.cm-focused) { outline: none; }
  .ws-only { position: absolute; top: 120px; left: var(--sp-9); display: flex; flex-direction: column; align-items: flex-start; gap: var(--sp-3); }
  .ws-only strong { font-weight: var(--fw-semibold); }

  /* Drafts dock. */
  .dock { flex: none; border-top: 1px solid var(--border); background: var(--surface-0); display: flex; flex-direction: column; }
  .empty-dock { display: flex; align-items: center; gap: var(--sp-4); height: 32px; padding: 0 var(--sp-5); font-size: var(--fs-12); color: var(--text-faint); }
  .drafts { display: flex; flex-direction: column; gap: var(--sp-2); padding: var(--sp-3) var(--sp-5) var(--sp-4); }
  .drafts > .label-caps { height: 22px; display: flex; align-items: center; }
  .drafts ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: var(--sp-1); max-height: 96px; overflow: auto; }
  .drafts li {
    display: flex; align-items: center; gap: var(--sp-4); min-height: 24px; padding: 0 var(--sp-2) 0 var(--sp-4);
    border-radius: var(--r-md); background: var(--you-bg); font-size: var(--fs-12);
  }
  .drafts li svg { flex: none; fill: var(--you); }
  .drafts li code { flex: none; font-size: 11.5px; color: var(--you-strong); }
  .drafts li .text { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .actions { display: flex; justify-content: flex-end; gap: var(--sp-3); padding-top: var(--sp-2); }
  .submit {
    margin: var(--sp-2) 0 0; padding: var(--sp-5); border-radius: var(--r-lg); background: var(--surface-raised);
    box-shadow: inset 0 0 0 1px var(--gh-line); display: flex; flex-direction: column; gap: var(--sp-4); font-size: var(--fs-12);
  }
  .submit label { display: flex; flex-direction: column; gap: var(--sp-2); }
  .field { font-weight: var(--fw-semibold); }
  .submit textarea { width: 100%; background: var(--surface-editor); }
  .events { display: flex; align-items: center; gap: var(--sp-2); flex-wrap: wrap; }
  .events .field { margin-right: var(--sp-4); }
  .events :global(.event[aria-pressed="true"]) { border-color: var(--gh); background: var(--gh-bg); color: var(--gh-strong); }
  .events .faint { margin-left: var(--sp-3); }
  .preview { margin: 0; color: var(--text-muted); }
</style>
