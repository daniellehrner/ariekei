<!-- Copyright contributors to Ariekei.
     SPDX-License-Identifier: Apache-2.0 -->

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
  import { EditorSelection, EditorState, Text, type Extension } from "@codemirror/state";
  import { EditorView } from "@codemirror/view";
  import { basicSetup } from "codemirror";
  import { onDestroy, tick, untrack } from "svelte";
  import { fade } from "svelte/transition";
  import AskAgent from "./AskAgent.svelte";
  import CodeMenu from "./CodeMenu.svelte";
  import Button from "./controls/Button.svelte";
  import Icon from "./controls/Icon.svelte";
  import IconButton from "./controls/IconButton.svelte";
  import Mark from "./controls/Mark.svelte";
  import Pill from "./controls/Pill.svelte";
  import Segmented from "./controls/Segmented.svelte";
  import Spinner from "./controls/Spinner.svelte";
  import Toggle from "./controls/Toggle.svelte";
  import { api, type ReviewState } from "./lib/api";
  import { ariekeiHighlight, ariekeiTheme } from "./lib/codemirrorTheme";
  import { currentChange } from "./lib/currentChange";
  import { draftLines, draftMarks, setDraftLines } from "./lib/draftMarks";
  import { gitOverride, lineSeparatorOf } from "./lib/gitDiff";
  import { hscroll } from "./lib/hscroll";
  import { externalLinks } from "./lib/html";
  import { isTyping } from "./lib/keys";
  import { shortcuts } from "./lib/layout.svelte";
  import { durationToken } from "./lib/tokens";
  import { languageFor } from "./lib/languages";
  import { layout } from "./lib/layout.svelte";
  import type { Area } from "./lib/popover";
  import { flatten, neighbourFile, nextChange, rangeOf, scopeOf, sections, whatMoved, type ReviewFile } from "./lib/review";
  import { askMessage, classify, feedbackMessage, refLines, reviewBody, type CodeRef, type LineComment, type Ranges } from "./lib/reviewComments";

  let {
    id,
    view = $bindable(),
    onchanged,
    canSendToAgent,
    agentAway = null,
    agentItem,
    agentStatus,
    onterminal,
    polls = 0,
  }: {
    id: number;
    view: ReviewViewState;
    onchanged: () => void;
    canSendToAgent: boolean;
    /// Why the agent takes no work now (its worktree is on another branch), if it doesn't.
    agentAway?: string | null;
    /// The item whose agent gets what is sent (this one, or its task).
    agentItem?: number;
    /// The status of the agent that gets what is sent (lib/agent.svelte.ts).
    agentStatus?: string;
    /// Shows the agent's terminal (where its answer appears); after asking it about code.
    onterminal?: () => void;
    /// Counts the polls of the item's repo: after each, GitHub may have newer (see `check`).
    polls?: number;
  } = $props();

  let review = $state<ReviewState | null>(null);
  // GitHub has newer than what shows (`check` found it): its new commits, or a moved base.
  // The diff stays as it is until the user reloads.
  let newer = $state<"head" | "base" | null>(null);
  // Nothing shows yet, and the fetch it waits for runs.
  let fetching = $state(false);
  let files: ReviewFile[] = $state([]);
  let error: string | null = $state(null);
  // n / p go from change to change, and on into the next (previous) file at the last (first)
  // change of one: entering it at its first (1) or last (-1) change. With no change further
  // on in the scope, `edge` says so at that end of the diff for a moment.
  let landing: 1 | -1 | 0 = 0;
  let edge = $state<1 | -1 | null>(null);
  // Where the note goes: by the change (below the last one, above the first), centred in the
  // new pane; null when that line is off screen (then at that end of the diff).
  let edgeAt = $state<{ top: number; left: number } | null>(null);
  let edgeTimer: ReturnType<typeof setTimeout> | undefined;
  const NOTE_MS = durationToken("--dur-note", 1800);
  const DUR = durationToken("--dur", 120);

  function edgeAnchor(dir: 1 | -1): { top: number; left: number } | null {
    const v = newSide();
    if (!v || !main || !host) return null;
    const changes = changesOf(v);
    const c = changes[dir > 0 ? changes.length - 1 : 0];
    if (!c) return null;
    const line = v.lineBlockAt(dir > 0 ? Math.max(c.from, c.to - 1) : c.from);
    const box = main.getBoundingClientRect();
    const view_ = host.getBoundingClientRect();
    const pane = v.dom.getBoundingClientRect();
    const y = v.documentTop + (dir > 0 ? line.bottom : line.top);
    const left = pane.left + pane.width / 2 - box.left;
    const top = dir > 0 ? y - box.top + 6 : y - box.top - 32;
    // Off screen: at that end of the diff instead.
    return top < view_.top - box.top || top > view_.bottom - box.top - 26 ? null : { top, left };
  }
  const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
  let notice: string | null = $state(null);
  let noticeUrl: string | null = $state(null);
  // Sending to the agent failed: the error offers its terminal.
  let errorTerminal = $state(false);
  let host: HTMLElement | undefined = $state();
  // The horizontal scrollbars, right below the diff (lib/hscroll.ts).
  let bars: HTMLElement | undefined = $state();
  let main: HTMLElement | undefined = $state();
  // The diff shown: a side-by-side MergeView, or one editor with the unified merge view.
  let merge: MergeView | null = null;
  let unified: EditorView | null = null;
  let selection: { fromLine: number; toLine: number; code: string } | null = $state(null);
  let draft = $state("");
  // The dock's comment composer opens with a selection; asking the agent or submitting closes
  // it (the selection stays) until the selection changes.
  let composerOff = $state(false);
  // Right-click on code: its menu, then the prompt box for the agent, placed by the lines it
  // is about (`lines`: their top and bottom) within the pane they are in (`area`).
  type Asking = { x: number; lines: { top: number; bottom: number }; area: Area; code: CodeRef };
  let codeMenu: { x: number; y: number; ask: Asking } | null = $state(null);
  let asking: Asking | null = $state(null);
  let scopeMenu = $state(false);
  let dock: HTMLElement | undefined = $state();
  let editedPath: string | null = $state(null);
  let editedFile: string | null = null;
  // The new side's file as it was loaded (or last saved): Save is refused if the disk has
  // something else by then (the agent edited it), and the error offers a reload.
  let loadedFile = "";
  let errorReload = $state(false);
  // The file shown has only whitespace changes, and they are hidden.
  let onlyWhitespace = $state(false);
  // The file shown is not valid UTF-8: it cannot be edited (saving would garble it).
  let lossy = $state(false);
  // Below 880 px the diff column is too narrow for two sides (at 1024 wide it is about 730).
  let mainWidth = $state(1200);

  // Submitting to GitHub.
  let submitting = $state(false);
  let event = $state<"COMMENT" | "APPROVE" | "REQUEST_CHANGES">("COMMENT");
  let summary = $state("");
  // What goes where, and the commit it goes to: the PR head the drafts were made on.
  let preview: { line: LineComment[]; body: DraftComment[]; commit: string } | null = $state(null);
  let busy = $state(false);

  // (An order saved before the review guide was removed reads as review order.)
  const order = $derived(view.order === "tree" ? "tree" : "review");
  const list = $derived(sections(files, order));
  const ordered = $derived(flatten(list));
  const isUnified = $derived(view.unified ?? mainWidth < 880);
  // The agent waits for an answer: anything sent now would answer its prompt.
  const agentWaits = $derived(agentStatus === "waitingForPermission");

  // The Inbox steps aside on narrow windows while the side panel is open.
  $effect(() => {
    layout.reviewPanel = !view.sideHidden;
  });
  onDestroy(() => (layout.reviewPanel = false));

  // Opening the review sees what the agent did: no longer "ready for review" in the Inbox
  // (also when it finishes while the review is shown).
  $effect(() => {
    if (agentItem == null || (agentStatus && agentStatus !== "finished")) return;
    api.reviewSeen(agentItem).then(onchanged, () => {});
  });

  // The notices sit above the dock (Notices.svelte reads --dock-h).
  $effect(() => {
    if (!dock) return;
    const root = document.documentElement.style;
    const observer = new ResizeObserver(() => root.setProperty("--dock-h", `${dock?.offsetHeight ?? 0}px`));
    observer.observe(dock);
    return () => {
      observer.disconnect();
      root.removeProperty("--dock-h");
    };
  });

  const scope = $derived(scopeOf(review, view.scope));
  const range = $derived(review ? rangeOf(review, scope) : null);
  // The commit the right side shows; null when it includes uncommitted changes.
  const shownRev = $derived(range ? (range[1] ?? (review?.dirty ? null : review?.head ?? null)) : null);
  const editable = $derived(range?.[1] === null);

  // What the clone has, at once; then `check` asks GitHub in the background.
  async function load(refresh = false) {
    error = null;
    if (refresh) newer = null;
    try {
      review = await api.reviewState(id, refresh);
    } catch (e) {
      if (refresh) {
        error = String(e);
        return;
      }
      // Nothing to show yet (e.g. the PR head is not fetched): fetched first.
      fetching = true;
      try {
        review = await api.reviewState(id, true);
      } catch (e) {
        error = String(e);
      } finally {
        fetching = false;
      }
      return;
    }
    if (!refresh) check();
  }

  // Whether GitHub has newer than what shows: fetched in the background (the backend goes to
  // GitHub only when a poll saw a new head, or the base was not fetched lately), then what the
  // clone has is compared with what shows, so newer fetched by an earlier check (or by Reload
  // meanwhile) counts too. Asked again while it runs (a poll came in), it runs once more after.
  // Offline, the review as it was fetched will do.
  let checking = false;
  let again = false;
  async function check() {
    if (!review) return;
    if (checking) {
      again = true;
      return;
    }
    checking = true;
    try {
      do {
        again = false;
        await api.reviewFetch(id);
        const next = await api.reviewState(id);
        if (review) newer = whatMoved(review, next) ?? newer;
      } while (again);
    } catch (e) {
      console.warn(`review: could not check GitHub for newer: ${e}`);
    } finally {
      checking = false;
    }
  }
  $effect(() => {
    void polls;
    untrack(check);
  });

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
    // An added or deleted file has one side only: one pane.
    // (Read untracked: the diff is not rebuilt whenever the file list reloads.)
    const status = untrack(() => files.find((f) => f.path === path)?.status);
    const onePane = isUnified || status === "A" || status === "D";
    const [d, lang] = await Promise.all([api.reviewFile(id, from, to, path, ignoreWs), languageFor(path)]);
    if (n !== showing || !host) return;
    destroyEditors();
    selection = null;
    composerOff = false;
    editedPath = null;
    loadedFile = d.new;
    lossy = d.lossy;
    onlyWhitespace = ignoreWs && !d.binary && d.hunks.length === 0 && !!files.find((f) => f.path === path)?.whitespaceOnly;
    if (d.binary) {
      host.textContent = "Binary file: not shown.";
      return land();
    }
    host.textContent = "";
    if (onlyWhitespace) return land();
    const common: Extension[] = [basicSetup, ...lang, ariekeiTheme, syntaxHighlighting(ariekeiHighlight), hscroll(bars!)];
    const onUpdate = EditorView.updateListener.of((u) => {
      if (u.docChanged) {
        editedPath = path;
        // With the file's own line endings (doc.toString() would join with "\n").
        editedFile = u.state.sliceDoc();
      }
      if (u.selectionSet || u.docChanged) {
        const sel = u.state.selection.main;
        if (sel.empty) selection = null;
        else {
          const doc = u.state.doc;
          const next = { fromLine: doc.lineAt(sel.from).number, toLine: doc.lineAt(sel.to).number, code: doc.sliceString(sel.from, sel.to) };
          if (next.fromLine !== selection?.fromLine || next.toLine !== selection?.toLine) composerOff = false;
          selection = next;
        }
      }
    });
    // Each side split into git's lines, keeping its line endings (lib/gitDiff.ts).
    const [oldSep, newSep] = [EditorState.lineSeparator.of(lineSeparatorOf(d.old)), EditorState.lineSeparator.of(lineSeparatorOf(d.new))];
    const canEdit = to === null && !d.lossy;
    const newDoc = [...draftMarks, ...common, newSep, EditorView.editable.of(canEdit), EditorState.readOnly.of(!canEdit), onUpdate, currentChange];
    const diffConfig = { override: gitOverride(d.old, d.new, d.hunks) };
    const collapseUnchanged = { margin: 3, minSize: 6 };
    if (onePane) {
      unified = new EditorView({
        parent: host,
        doc: d.new,
        extensions: [
          ...newDoc,
          unifiedMergeView({ original: Text.of(d.old.split(lineSeparatorOf(d.old))), gutter: true, mergeControls: false, highlightChanges: true, syntaxHighlightDeletions: true, diffConfig, collapseUnchanged }),
        ],
      });
    } else {
      merge = new MergeView({
        parent: host,
        a: { doc: d.old, extensions: [...common, oldSep, EditorState.readOnly.of(true)] },
        b: { doc: d.new, extensions: newDoc },
        collapseUnchanged,
        gutter: true,
        diffConfig,
      });
    }
    markDrafts();
    land();
  }

  // The new side's lines the drafts on this file cover.
  function markDrafts() {
    const v = newSide();
    if (!v) return;
    const mine = view.comments.filter((c) => c.path === view.file);
    v.dispatch({ effects: setDraftLines.of(draftLines(mine)) });
  }
  $effect(() => {
    void view.comments.length;
    void view.file;
    markDrafts();
  });

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
      edgeAt = edgeAnchor(dir);
      edgeTimer = setTimeout(() => (edge = null), NOTE_MS);
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
      c: () => commentOn(),
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

  // Adding closes the composer and drops the selection: the draft's mark in the gutter shows
  // where it is.
  function addComment(e?: SubmitEvent) {
    e?.preventDefault();
    if (!selection || !view.file || !draft.trim()) return;
    // Edits not saved yet are not part of any version on disk.
    const rev = editedPath === view.file ? null : shownRev;
    view.comments = [...view.comments, { path: view.file, ...selection, text: draft, rev }];
    draft = "";
    clearSelection();
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
    codeMenu = { ...point, ask: askingAt(editor, from, to, code) };
  }

  // The Ask agent box for code from..to of `editor`: by those lines, within its pane.
  function askingAt(editor: EditorView, from: number, to: number, code: CodeRef): Asking {
    const first = editor.coordsAtPos(from);
    const last = editor.coordsAtPos(Math.max(from, to > from && editor.state.doc.lineAt(to).from === to ? to - 1 : to));
    const pane = editor.dom.getBoundingClientRect();
    const view_ = host?.getBoundingClientRect() ?? pane;
    const floor = dock?.getBoundingClientRect().top ?? view_.bottom;
    return {
      x: first?.left ?? pane.left,
      lines: { top: first?.top ?? view_.top, bottom: last?.bottom ?? view_.top },
      area: { left: pane.left, right: pane.right, top: view_.top, bottom: floor },
      code,
    };
  }

  // `a`: ask about the selection on the new side.
  function askAboutSelection() {
    const v = newSide();
    const sel = v?.state.selection.main;
    if (!v || !sel || sel.empty || !canSendToAgent || agentAway) return;
    const code = codeRef(v, "new", sel.from, sel.to);
    if (code) openAsking(askingAt(v, sel.from, sel.to, code));
  }

  function openAsking(a: Asking) {
    composerOff = true;
    asking = a;
  }

  // The dock's composer, for the selection (`c`, or Comment on lines).
  async function commentOn() {
    if (!selection) return;
    composerOff = false;
    asking = null;
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

  // The drafts go once the agent took them (an open dialog can take the Enter instead); that
  // can take a few seconds, and drafts added meanwhile stay.
  let sendingToAgent = $state(false);
  async function sendToAgent() {
    if (!view.comments.length || agentWaits || agentAway || sendingToAgent) return;
    const sent = view.comments;
    sendingToAgent = true;
    try {
      await api.agentSend(id, feedbackMessage(sent));
      say(`Sent ${sent.length} comment${sent.length > 1 ? "s" : ""} to the agent.`);
      view.comments = view.comments.filter((c) => !sent.includes(c));
    } catch (e) {
      error = `Could not send to the agent: ${e}`;
    } finally {
      sendingToAgent = false;
    }
  }

  async function openSubmit() {
    if (!review) return;
    composerOff = true;
    submitting = true;
    preview = null;
    if (review.own) event = "COMMENT";
    try {
      const paths = [...new Set(view.comments.map((c) => c.path))];
      const commit = review.prHead;
      const ranges: Ranges = Object.fromEntries(await api.commentRanges(id, paths, commit));
      preview = { ...classify(view.comments, commit, ranges), commit };
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
      const url = await api.submitReview(id, event, body, preview.line, preview.commit);
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
    const content = editedFile;
    try {
      await api.saveFile(id, editedPath, content, loadedFile);
      say(`Saved ${editedPath}.`);
      loadedFile = content;
      editedPath = null;
      await loadFiles();
      onchanged();
    } catch (e) {
      error = String(e);
      errorReload = true;
    }
  }

  // Shows the review as it is on disk now (the edits not saved are dropped).
  function reload() {
    error = null;
    errorReload = false;
    load(true);
    loadFiles();
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
  const withDrafts = $derived(new Set(view.comments.map((c) => c.path)));
  const showComposer = $derived(!!selection && !!view.file && !composerOff && !asking && !submitting);

  // The scope menu: ↑↓ move, Enter picks, Esc closes.
  function onScopeKey(e: KeyboardEvent) {
    const items = [...(e.currentTarget as HTMLElement).querySelectorAll<HTMLElement>("[role='menuitemradio']")];
    const i = items.indexOf(document.activeElement as HTMLElement);
    const step = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
    if (step) {
      e.preventDefault();
      items[(i + step + items.length) % items.length]?.focus();
    } else if (e.key === "Escape" || e.key === "Tab") {
      e.preventDefault();
      e.stopPropagation();
      scopeMenu = false;
    }
  }
  function focusActive(node: HTMLElement) {
    (node.querySelector<HTMLElement>("[aria-checked='true']") ?? node.querySelector<HTMLElement>("[role='menuitemradio']"))?.focus();
  }
  function onWindowDown(e: MouseEvent) {
    if (scopeMenu && !(e.target as Element).closest?.("[data-scope-wrap]")) scopeMenu = false;
  }
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

<svelte:window onkeydown={onKeydown} onmousedown={onWindowDown} />

<div class="review" use:externalLinks role="tabpanel" aria-label="Review">
  <!-- The review bar: the scope, what is shown, file navigation, the diff options. -->
  <div class="bar" role="toolbar" aria-label="Review">
    <Icon name="branch" size={14} />
    {#if review}
      <span class="scope-wrap" data-scope-wrap>
        <button class="scope-button" onclick={() => (scopeMenu = !scopeMenu)} aria-haspopup="menu" aria-expanded={scopeMenu}
          title={scopeLabel} data-scope-menu>
          <span class="scope-label">{scopeLabel}</span><Icon name="chevron-down" size={12} />
        </button>
        {#if scopeMenu}
          <div class="scope-menu" role="menu" aria-label="Scope" tabindex="-1" use:focusActive onkeydown={onScopeKey}>
            <button role="menuitemradio" aria-checked={scope === "all"} onclick={() => { setScope("all"); scopeMenu = false; }} data-scope="all">
              <span class="check">{#if scope === "all"}<Icon name="check" size={14} />{/if}</span>
              <span class="grow">All changes vs {review.baseRef}</span>
              {#if scope === "all"}<span class="n">{files.length}</span>{/if}
            </button>
            {#each review.commits as c (c.sha)}
              <button role="menuitemradio" aria-checked={scope === c.sha} onclick={() => { setScope(c.sha); scopeMenu = false; }}
                title="{c.subject}&#10;{c.author}, {c.date}" data-scope={c.sha}>
                <span class="check">{#if scope === c.sha}<Icon name="check" size={14} />{/if}</span>
                <code class="sha">{short(c.sha)}</code>
                <span class="grow subject">{c.subject}</span>
                <span class="n">{c.files}</span>
              </button>
            {/each}
          </div>
        {/if}
      </span>
      <span class="marker" data-marker title="{review.mode === 'ref' ? 'PR head' : 'Worktree'} {review.head}">
        <code>{short(review.head)}</code> · {review.commits.length} commit{review.commits.length === 1 ? "" : "s"}{#if review.dirty}{" · "}<span class="warn">uncommitted changes</span>{/if}
      </span>
    {/if}
    {#if view.sideHidden && ordered.length}
      <span class="divider"></span>
      <IconButton icon="chevron-up" label="Previous file ([)" size={22} onclick={() => stepFile(-1)} />
      <IconButton icon="chevron-down" label="Next file (])" size={22} onclick={() => stepFile(1)} />
      <span class="current-file" title={view.file ?? ""}><strong>{view.file ? nameOf(view.file) : ""}</strong> <span class="muted">{fileIndex + 1}/{ordered.length}</span></span>
    {/if}
    <span class="spacer"></span>
    {#if editedPath}<Button size="sm" variant="primary" onclick={save} title="Save {editedPath}" data-save>Save file</Button>{/if}
    <Toggle pressed={!!view.ignoreWs} icon="whitespace" kbd="w" aria-label="Hide whitespace (w)" title="Hide whitespace (w)"
      onclick={() => (view.ignoreWs = !view.ignoreWs)} data-opt="ws"><span class="tlabel">Whitespace</span></Toggle>
    <Toggle pressed={isUnified} icon="unified" kbd="u" aria-label="Unified diff (u)" title="Unified diff (u)"
      onclick={() => (view.unified = !isUnified)} data-opt="unified"><span class="tlabel">Unified</span></Toggle>
    <span class="divider"></span>
    {#if review}<IconButton icon="refresh" label="Reload" onclick={reload} data-reload />{/if}
    <IconButton icon="panel" label={view.sideHidden ? "Show the files" : "Hide the files"} pressed={!view.sideHidden}
      onclick={() => (view.sideHidden = !view.sideHidden)} data-toggle="side" />
    <IconButton icon="keyboard" label="Keyboard shortcuts (?)" onclick={() => (shortcuts.open = !shortcuts.open)} data-keys />
  </div>

  {#if error}
    <div class="note bad" role="alert"><Icon name="error" size={14} /><span class="grow">{error}</span>
      {#if errorTerminal && onterminal}<Button size="sm" variant="quiet" icon="terminal" onclick={onterminal}>Open Terminal</Button>{/if}
      {#if errorReload}<Button size="sm" variant="quiet" icon="refresh" onclick={reload} data-error-reload>Reload</Button>{/if}
      <IconButton icon="x" label="Dismiss" size={22} onclick={() => { error = null; errorTerminal = false; errorReload = false; }} /></div>
  {/if}
  {#if fetching}
    <div class="note" role="status" data-fetching><Spinner /><span class="grow">Fetching from GitHub…</span></div>
  {/if}
  {#if newer && review}
    <div class="note gh" role="status" data-newer={newer}>
      <span class="lead"><Mark who="gh" /></span>
      <span class="grow">{newer === "head" ? "The PR has new commits on GitHub." : `${review.baseRef} moved on GitHub.`}</span>
      <Button size="sm" variant="quiet" icon="refresh" onclick={reload} data-newer-reload>Reload</Button>
    </div>
  {/if}
  {#if notice}
    <div class="note" class:gh={!!noticeUrl} role="status" data-notice>
      {#if noticeUrl}<span class="lead"><Mark who="gh" /></span>{:else}<Icon name="info" size={14} />{/if}<span class="grow">{notice}</span>
      {#if noticeUrl}<a href={noticeUrl}>Open</a>{/if}
      <IconButton icon="x" label="Dismiss" size={22} onclick={() => (notice = null)} />
    </div>
  {/if}
  <div class="body">
    {#if !view.sideHidden}
      <aside class="side" aria-label="Files">
        <section class="files-section">
          <div class="files-head">
            <span class="label-caps">Files</span>
            <span class="count">{files.length}</span>
            <span class="grow"></span>
            <Segmented label="File order" options={ORDERS} bind:value={() => order, (v) => (view.order = v)} data-order />
          </div>
          {#each list as s (s.key)}
            {@const open = isOpen(s)}
            <div class="group" data-group={s.key}>
              <button class="group-title" onclick={() => toggleSection(s)} aria-expanded={open}>
                <span class="chev" class:folded={!open}><Icon name="chevron-down" size={12} /></span>
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
                      {#if withDrafts.has(f.path)}<span class="has-drafts" title="Has draft comments"><Mark who="you" /></span>{/if}
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
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div class="diff" bind:this={host} tabindex="0" role="region" oncontextmenu={onContextMenu}
        aria-label="Diff of {view.file ?? 'no file'}. Shortcuts: ] and [ files, n and p changes (on into the next or previous file), u unified, w whitespace, a ask the agent about the selection, c comment on it, ? help. Right-click code to ask the agent about it."></div>
      <div class="hbars" bind:this={bars}></div>
      {#if edge}
        <!-- By the change itself (or at that end of the diff when it is off screen). -->
        <div class="edge" style:top={edgeAt ? `${edgeAt.top}px` : edge < 0 ? `${(host?.offsetTop ?? 0) + 12}px` : undefined}
          style:left={edgeAt ? `${edgeAt.left}px` : undefined}
          style:bottom={!edgeAt && edge > 0 ? `${(main?.clientHeight ?? 0) - (host ? host.offsetTop + host.offsetHeight : 0) + 16}px` : undefined} role="status"
          transition:fade={{ duration: still ? 0 : DUR }} data-edge={edge > 0 ? "end" : "start"}>
          <span class="end" class:first={edge < 0}><Icon name="end" size={14} /></span>
          {edge > 0 ? "Last change in this scope" : "First change in this scope"}
        </div>
      {/if}
      {#if codeMenu}
        {@const m = codeMenu}
        <CodeMenu x={m.x} y={m.y} lines={m.ask.code.fromLine === m.ask.code.toLine ? `Line ${m.ask.code.fromLine}` : `Lines ${refLines(m.ask.code)}`}
          side={m.ask.code.side}
          noAgent={canSendToAgent ? agentAway : "No agent on this item yet"}
          oncomment={m.ask.code.side === "new" ? commentOn : undefined}
          onask={() => openAsking(m.ask)} oncopy={() => copyCode(m.ask.code)} onclose={() => (codeMenu = null)} />
      {/if}
      {#if asking}
        {@const a = asking}
        <AskAgent x={a.x} lines={a.lines} area={a.area} code={a.code} status={agentStatus} {onterminal}
          onsend={(prompt) => ask(a.code, prompt)} onclose={() => (asking = null)} />
      {/if}
      {#if onlyWhitespace}
        <div class="ws-only">
          <strong>Only whitespace changed in this file</strong>
          <span class="muted">Whitespace changes are hidden.</span>
          <Button onclick={() => (view.ignoreWs = false)} kbd="w">Show whitespace</Button>
        </div>
      {/if}

      <!-- The dock: your review — the comment composer, your drafts, and where they go. -->
      <div class="dock" bind:this={dock} data-dock>
        {#if showComposer && selection}
          <form class="composer" onsubmit={addComment} data-composer>
            <Mark who="you" />
            <label for="composer-{id}">Lines {selection.fromLine}{selection.toLine !== selection.fromLine ? `–${selection.toLine}` : ""}</label>
            <!-- Enter adds; Esc drops the selection, also while typing here. -->
            <input id="composer-{id}" bind:value={draft} placeholder="Comment on these lines"
              onkeydown={(e) => e.key === "Escape" && (e.preventDefault(), clearSelection())} />
            <Button type="submit" size="sm" variant="primary" disabled={!draft.trim()} data-add-comment>Add comment</Button>
            <IconButton icon="x" label="Cancel comment (Esc)" size={22} type="button" onclick={clearSelection} />
          </form>
        {/if}
        {#if view.comments.length}
          <div class="drafts">
            <div class="drafts-head"><span class="label-caps">Draft comments</span><span class="count">{view.comments.length}</span></div>
            <ul data-drafts>
              {#each view.comments as c, i}
                <li>
                  <Mark who="you" />
                  <code title={c.path}>{nameOf(c.path)}:{linesOf(c)}</code>
                  <span class="text" title={c.text}>{c.text}</span>
                  <IconButton icon="trash" label="Remove draft" size={22} onclick={() => (view.comments = view.comments.filter((_, j) => j !== i))} />
                </li>
              {/each}
            </ul>
          </div>
          <div class="actions">
            {#if canSendToAgent && agentAway}
              <span class="waits" title={agentAway}>{agentAway}</span>
            {:else if canSendToAgent && agentWaits}
              <span class="waits">The agent is waiting for your permission — answer it before sending.</span>
            {/if}
            {#if canSendToAgent}<Button size="sm" variant="agent" mark="agent" onclick={sendToAgent} disabled={agentWaits || !!agentAway} loading={sendingToAgent} data-send-agent>Send to agent</Button>{/if}
            {#if review?.prHead}
              <span class="submit-anchor">
                <Button size="sm" mark="gh" onclick={openSubmit} aria-expanded={submitting} data-open-submit>Submit to GitHub…</Button>
                {#if submitting}{@render submitPanel()}{/if}
              </span>
            {/if}
          </div>
        {:else if !showComposer}
          <div class="empty-dock">
            <span class="grow hint">Select code to comment on it{editable ? (lossy ? "; this file is not UTF-8 text, so it cannot be edited here" : "; edit it to change the file") : ""}.</span>
            {#if review?.prHead}
              <span class="submit-anchor">
                <Button size="sm" variant="quiet" mark="gh" onclick={openSubmit} aria-expanded={submitting} data-open-submit>Review on GitHub…</Button>
                {#if submitting}{@render submitPanel()}{/if}
              </span>
            {/if}
          </div>
        {/if}
      </div>
    </div>
  </div>
</div>

{#snippet submitPanel()}
  <form class="submit" onsubmit={submit} aria-label="Submit review to GitHub" data-submit>
    <div class="submit-head">
      <Mark who="gh" />
      <strong class="grow">Submit review to GitHub</strong>
      <IconButton icon="x" label="Close" size={22} type="button" onclick={() => (submitting = false)} />
    </div>
    <label><span class="field">Summary</span>
      <textarea bind:value={summary} rows="2" placeholder="Overall comment (optional for a comment review)"></textarea>
    </label>
    <div class="events">
      <span class="field">Event</span>
      <div class="seg" role="radiogroup" aria-label="Event">
        {#each EVENTS as [value, label]}
          <button type="button" role="radio" aria-checked={event === value} class:on={event === value} disabled={value !== "COMMENT" && review?.own}
            onclick={() => (event = value)} data-event={value}>{label}</button>
        {/each}
      </div>
    </div>
    {#if review?.own}<p class="own">Your own PR: you can only comment.</p>{/if}
    {#if preview}
      <p class="preview" data-preview>
        {preview.line.length} as line comment{preview.line.length === 1 ? "" : "s"} on the PR diff{#if preview.body.length}; {preview.body.length} in the review text (not on lines of the PR diff at its head commit){/if}.
      </p>
    {/if}
    <div class="submit-actions">
      <Button type="button" size="sm" variant="quiet" onclick={() => (submitting = false)}>Cancel</Button>
      <Button type="submit" size="sm" variant="gh" mark="gh" loading={busy} disabled={!preview} data-send-review>Submit to GitHub</Button>
    </div>
  </form>
{/snippet}

<style>
  .review { flex: 1; display: flex; flex-direction: column; height: 100%; min-height: 0; position: relative; }
  .grow { flex: 1; min-width: 0; }
  .muted { color: var(--text-muted); }
  .spacer { flex: 1; }
  .chev { display: flex; color: var(--text-faint); transition: transform var(--dur-fast) var(--ease); }
  .chev.folded { transform: rotate(-90deg); }
  .count { flex: none; font-family: var(--font-mono); font-size: var(--fs-11); font-weight: var(--fw-regular); color: var(--text-faint); }

  /* The review bar: one row. */
  .bar {
    flex: none; display: flex; align-items: center; gap: var(--sp-3); height: var(--h-review-bar);
    padding: 0 var(--sp-4) 0 var(--sp-5); border-bottom: 1px solid var(--border); box-sizing: border-box;
    color: var(--text-muted); white-space: nowrap;
  }
  .bar > :global(svg) { flex: none; }
  .divider { flex: none; width: 1px; height: 16px; margin: 0 var(--sp-1); background: var(--border); }
  .scope-wrap { position: relative; flex: 0 1 auto; min-width: 0; display: flex; }
  .scope-button {
    display: flex; align-items: center; gap: var(--sp-2); min-width: 0; height: var(--h-control); padding: 0 var(--sp-3) 0 var(--sp-4);
    border: 0; border-radius: var(--r-md); background: transparent; color: var(--text);
    font-size: var(--fs-12); font-weight: var(--fw-semibold);
  }
  .scope-button:hover, .scope-button[aria-expanded="true"] { background: var(--surface-hover); }
  .scope-label { max-width: 280px; overflow: hidden; text-overflow: ellipsis; }
  .scope-menu {
    position: absolute; top: calc(100% + 4px); left: 0; z-index: 30; width: 360px; max-width: calc(100vw - 32px);
    max-height: 60vh; overflow-y: auto; padding: var(--sp-2); box-sizing: border-box; display: flex; flex-direction: column; gap: 1px;
    border-radius: var(--r-lg); background: var(--surface-raised); box-shadow: var(--shadow-pop);
  }
  .scope-menu:focus { outline: none; }
  .scope-menu button {
    display: flex; align-items: center; gap: var(--sp-4); height: 28px; padding: 0 var(--sp-4); flex: none;
    border: 0; border-radius: var(--r-md); background: transparent; color: var(--text); font-size: var(--fs-12); text-align: left;
  }
  .scope-menu button:hover { background: var(--surface-hover); }
  .scope-menu button[aria-checked="true"] { background: var(--surface-selected); font-weight: var(--fw-semibold); }
  .scope-menu button:focus-visible { outline-offset: -2px; }
  .check { flex: none; width: 14px; display: flex; color: var(--text-muted); }
  .sha { flex: none; font-size: var(--fs-code-inline); color: var(--text-muted); font-weight: var(--fw-regular); }
  .subject { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .n { flex: none; font-family: var(--font-mono); font-size: var(--fs-11); color: var(--text-faint); font-weight: var(--fw-regular); }
  .marker { flex: 0 100 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; font-size: var(--fs-12); }
  .marker code { font-size: var(--fs-code-inline); }
  .warn { color: var(--warn); }
  .current-file { flex: 0 1 auto; min-width: 0; display: inline-flex; align-items: baseline; gap: var(--sp-3); font-size: var(--fs-12); overflow: hidden; }
  .current-file strong { font-family: var(--font-mono); font-weight: var(--fw-semibold); color: var(--text); overflow: hidden; text-overflow: ellipsis; }
  @media (max-width: 1200px) { .tlabel { display: none; } }

  /* Notes under the bar. */
  .note {
    flex: none; display: flex; align-items: center; gap: var(--sp-4); min-height: 28px; padding: var(--sp-1) var(--sp-3) var(--sp-1) var(--sp-7);
    border-bottom: 1px solid var(--border); background: var(--info-bg); font-size: var(--fs-12); box-sizing: border-box;
  }
  .note > :global(svg) { flex: none; color: var(--info); }
  .note .lead { width: 14px; display: flex; justify-content: center; }
  .note.gh { background: var(--gh-bg); }
  .note.gh a { color: var(--gh-strong); }
  .note a { font-weight: var(--fw-medium); text-decoration: none; padding: 0 var(--sp-4); }
  .note.bad { background: var(--bad-bg); }
  .note.bad > :global(svg) { color: var(--bad); }

  .body { flex: 1; display: flex; min-height: 0; }

  /* Side panel: the files. */
  .side {
    flex: none; width: var(--w-panel); display: flex; flex-direction: column; min-height: 0; overflow-y: auto;
    border-right: 1px solid var(--border); background: var(--surface-2); font-size: var(--fs-12); line-height: var(--lh-12);
  }
  @media (max-width: 1100px) { .side { width: var(--w-panel-compact); } }
  .side > section { flex: 1 0 auto; display: flex; flex-direction: column; }
  .files-head { display: flex; align-items: center; gap: var(--sp-3); height: 34px; padding: 0 var(--sp-4) 0 var(--sp-5); flex: none; }
  .group { display: flex; flex-direction: column; padding-bottom: var(--sp-2); }
  .group-title {
    display: flex; align-items: center; gap: var(--sp-3); height: var(--h-section); padding: 0 var(--sp-5);
    border: 0; border-radius: 0; background: transparent; color: var(--text); font-size: var(--fs-12); font-weight: var(--fw-semibold); text-align: left;
  }
  .group-title:focus-visible { outline-offset: -2px; }
  .group-title .title { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .files { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 1px; }
  .dir { padding: var(--sp-2) var(--sp-5) 1px var(--sp-7); font-family: var(--font-mono); font-size: var(--fs-11); color: var(--text-faint); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .file {
    display: flex; align-items: center; gap: var(--sp-3); height: 24px; margin: 0 var(--sp-2); padding: 0 var(--sp-3) 0 14px;
    border-radius: var(--r-md);
  }
  .file:hover { background: var(--surface-hover); }
  .file.current { background: var(--surface-selected); box-shadow: inset 0 0 0 1px var(--you-line); }
  /* The focus ring goes on the whole row. */
  .file:has(button:focus-visible) { outline: 2px solid var(--focus); outline-offset: -2px; }
  .st { flex: none; width: 12px; font-family: var(--font-mono); font-size: var(--fs-11); font-weight: var(--fw-semibold); text-align: center; color: var(--text-muted); }
  .st.sA { color: var(--ok); }
  .st.sD { color: var(--bad); }
  .st.sU { color: var(--warn); }
  .file button {
    flex: 1; min-width: 0; height: 22px; padding: 0; border: 0; border-radius: 0; background: transparent; color: var(--text);
    font-size: var(--fs-12); text-align: left; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .file button:focus-visible { outline: none; }
  .file.current button { font-weight: var(--fw-semibold); color: var(--text); }
  .file.deleted button { text-decoration: line-through; color: var(--text-muted); }
  .ws, .has-drafts { flex: none; display: flex; }
  .none { padding: var(--sp-2) var(--sp-6); color: var(--text-muted); margin: 0; }

  /* The diff column. */
  .main { flex: 1; display: flex; flex-direction: column; min-width: 0; min-height: 0; position: relative; }

  /* n / p found no change further on: a note by that change. */
  .edge {
    position: absolute; left: 50%; z-index: 5; transform: translateX(-50%);
    display: flex; align-items: center; gap: var(--sp-3); height: 26px; padding: 0 var(--sp-6); box-sizing: border-box; white-space: nowrap;
    border-radius: var(--r-pill); background: var(--surface-raised); box-shadow: var(--shadow-pop);
    font-size: var(--fs-12); color: var(--text-muted);
  }
  .end { display: flex; }
  .end.first { transform: rotate(180deg); }
  .diff { flex: 1; overflow: auto; min-height: 0; background: var(--surface-editor); }
  .diff:focus-visible { outline: 2px solid var(--focus); outline-offset: -2px; }
  /* CodeMirror draws no focus ring of its own: the diff shows that it has the keys. */
  .diff:focus-within { box-shadow: inset 0 0 0 1px var(--focus-soft); }
  .diff :global(.cm-editor) { font-size: var(--fs-code); }
  .diff :global(.cm-editor.cm-focused) { outline: none; }
  /* One bar per editor (hscroll.ts), each under its pane: all in one grid cell, placed by
     their margin. No bar shown, no height. */
  .hbars { flex: none; display: grid; background: var(--surface-editor); }
  .hbars :global(.ak-hscroll) { grid-area: 1 / 1; min-height: 12px; overflow-x: auto; overflow-y: hidden; }
  .hbars :global(.ak-hscroll > div) { height: 1px; }
  .ws-only { position: absolute; top: 120px; left: var(--sp-9); display: flex; flex-direction: column; align-items: flex-start; gap: var(--sp-3); }
  .ws-only strong { font-weight: var(--fw-semibold); }

  /* The dock: your review. */
  .dock { flex: none; border-top: 1px solid var(--border); background: var(--surface-0); display: flex; flex-direction: column; font-size: var(--fs-12); line-height: var(--lh-12); }
  .composer { display: flex; align-items: center; gap: var(--sp-3); margin: 0; padding: var(--sp-3) var(--sp-4) var(--sp-3) var(--sp-6); }
  .composer label { flex: none; font-weight: var(--fw-semibold); color: var(--you-strong); }
  .composer input { flex: 1; min-width: 120px; height: var(--h-control); }
  .empty-dock { display: flex; align-items: center; gap: var(--sp-4); height: 32px; padding: 0 var(--sp-4) 0 var(--sp-6); }
  .hint { color: var(--text-faint); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .drafts { display: flex; flex-direction: column; gap: var(--sp-1); padding: 0 var(--sp-4) 0 var(--sp-6); }
  .drafts-head { height: 26px; display: flex; align-items: center; gap: var(--sp-3); }
  .drafts ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: var(--sp-1); max-height: 96px; overflow: auto; }
  .drafts li {
    display: flex; align-items: center; gap: var(--sp-4); min-height: 24px; padding: 0 var(--sp-2) 0 var(--sp-4);
    border-radius: var(--r-md); background: var(--you-bg);
  }
  .drafts li code { flex: none; font-size: var(--fs-code-inline); color: var(--you-strong); }
  .drafts li .text { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .actions { display: flex; align-items: center; justify-content: flex-end; gap: var(--sp-3); padding: var(--sp-3) var(--sp-4) var(--sp-4) var(--sp-6); }
  .waits { flex: 1; min-width: 0; color: var(--text-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .submit-anchor { position: relative; display: inline-flex; }

  /* The submit panel: a popover above its button. */
  .submit {
    position: absolute; right: 0; bottom: calc(100% + 8px); z-index: 30; width: var(--w-popover); max-width: calc(100vw - 32px);
    box-sizing: border-box; padding: var(--sp-6); display: flex; flex-direction: column; gap: 10px;
    border-radius: var(--r-lg); background: var(--surface-raised); box-shadow: var(--shadow-pop), inset 0 0 0 1px var(--gh-line);
    font-size: var(--fs-12); line-height: var(--lh-12); white-space: normal;
  }
  .submit-head { display: flex; align-items: center; gap: var(--sp-4); }
  .submit-head strong { font-weight: var(--fw-semibold); }
  .submit label { display: flex; flex-direction: column; gap: var(--sp-2); }
  .field { font-weight: var(--fw-semibold); }
  .submit textarea { width: 100%; background: var(--surface-editor); }
  .events { display: flex; align-items: center; gap: var(--sp-4); }
  .seg { display: inline-flex; padding: 2px; border-radius: var(--r-md); background: var(--surface-0); box-shadow: inset 0 0 0 1px var(--border); }
  .seg button {
    height: 22px; padding: 0 var(--sp-4); border: 0; border-radius: var(--r-sm); background: transparent;
    color: var(--text-muted); font-size: var(--fs-12); font-weight: var(--fw-medium);
  }
  .seg button.on { background: var(--surface-raised); color: var(--text); box-shadow: var(--shadow-1), inset 0 0 0 1px var(--border); }
  .own, .preview { margin: 0; color: var(--text-muted); }
  .submit-actions { display: flex; justify-content: flex-end; gap: var(--sp-3); }
</style>
