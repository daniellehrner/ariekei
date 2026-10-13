# Phase 0 spike — results (2026-10-13)

> **Superseded for the agent:** the ACP chat UI below was replaced by the embedded Claude
> Code TUI; see [Agent integration v2](#agent-integration-v2-embedded-claude-code-tui).
> The ACP findings are kept for the record.

Stack under test: Tauri 2.12 (Rust core, WebKitGTK 2.52 on Linux) + Svelte 5 UI, Claude Code
via ACP (`@agentclientprotocol/claude-agent-acp` 0.84.0, Rust `agent-client-protocol` 2.2.0).
Measured on Linux (release build, driven headless under Xvfb via `tauri-driver`).

## Exit criteria

| # | Criterion | Result |
|---|-----------|--------|
| 1 | App starts on Linux; cold start and idle memory | ✅ first paint 0.3–0.4 s; idle 235–242 MB PSS (app + WebKit web/network processes) |
| 2 | ACP drives Claude Code in a worktree, permission round-trip through the UI | ✅ `Write hello.txt` shown with Yes / Allow all edits / No; answer reaches the agent; file written |
| 3 | Session survives app restart | ✅ `session/load` (~0.6 s) replays user/tool/agent history; agent remembers earlier turn. Agent cold start to *ready* ≈ 2.5–3 s |
| 4 | ~5k-line diff renders smoothly in WebKitGTK | ✅ 4,616-line Java file: 351 ms with exact diff (91 ms with CodeMirror's default crude diff, which is unusable — see below); scrolling fine |
| 5 | CI builds on Linux, macOS, Windows | ⏳ workflow written (`.github/workflows/ci.yml`), not yet run — needs a GitHub repo |

Quitting the app leaves no adapter processes behind (the ACP crate kills the process group).

## Findings that change the plan

1. **Agents inherit the user's global Claude config.** The adapter starts the SDK with
   `settingSources: ["user", "project", "local"]`, so `~/.claude/settings.json` applies, including
   its permission mode (`auto` approves writes without asking the client) and user-scope MCP servers.
   **Decided:** inherit the global config as is (settings, permission mode, CLAUDE.md, skills), but
   not MCP servers. Sessions pass `_meta.claudeCode.options.strictMcpConfig = true`, so only servers
   the IDE passes are used (none yet). This also skips a repo's `.mcp.json`; forwarding those would
   bypass Claude Code's approval prompt for project MCP servers, so that needs its own design.
   Verified: `claude` runs with `--setting-sources=user,project,local --strict-mcp-config
   --permission-mode auto`, and no MCP server processes start.
2. **Memory is dominated by agents.** Without MCP servers, ~345 MB each (Node adapter ~100 MB,
   bundled `claude` ~260 MB, plus a short-lived helper `claude`). App + two live agents: ~785 MB.
   Idle-session shutdown (already planned) is essential; consider a cap on live agents.
3. **Diff source of truth must be git.** `@codemirror/merge` defaults to `scanLimit: 500` and falls
   back to a crude diff on large files (whole file marked changed). Spike uses an exact diff with a
   1 s timeout; M1 should feed git's hunks via `diffConfig.override` (keeping the package diff for
   intra-chunk highlighting), so the review view matches commit drill-down exactly.
4. **Session transcripts outlive worktrees.** They are stored under
   `~/.claude/projects/<encoded-cwd>/`. Cleanup must call ACP `session/delete` (advertised).
5. **Node 22 is a runtime dependency** (adapter `engines: >=22`). **Decided:** detect, don't bundle.
   `src-tauri/src/node.rs` checks `ARIEKEI_NODE`, then `node` on PATH, then the newest install from
   fnm / nvm / mise / asdf / Volta / Homebrew / nvm-windows. If nothing qualifies, starting an agent
   reports what was found. Verified with Node 18 first on PATH (picks fnm's 22).
6. **Fast-moving dependencies.** The adapter ships many releases per day; the Rust ACP crate went
   2.x → 3.3 in one week. Pin exact versions and upgrade deliberately; 3.x upgrade is pending.
7. **E2E harness:** WebDriver via `tauri-driver` + `WebKitWebDriver` under Xvfb works for Linux
   (and CI). `tauri-driver` does not support macOS; macOS/Windows get build + unit tests only.
   Quirk: WebKitWebDriver's element text omits ellipsized spans — select by attributes.
8. **npm 10 resolver crash** (`edgesOut` null) with vitest 5's optional peers; vitest dropped for
   the spike. Revisit (npm 11 or pnpm) when adding UI tests in M1.

## Agent integration v2: embedded Claude Code TUI

Trying the ACP version on besu showed our own chat UI could not keep up with Claude Code
(slash-command autocomplete, skills such as `/code-review`, plugins and their views). Decided:
run the user's installed `claude` unmodified in an embedded terminal (xterm.js + PTY) and
integrate around it. No IDE bridge. The ACP adapter and the Node 22 dependency are gone.

How it works:
- One PTY per work item (`portable-pty`), `claude --strict-mcp-config --settings
  <claude-hooks.json> (--session-id <uuid> | --resume <uuid>)` in the worktree. Output is kept
  in a 2 MB scrollback buffer so views detach/reattach losslessly.
- Status comes from `http` hooks POSTing to a token-protected localhost endpoint, routed by
  session id: UserPromptSubmit/PreToolUse/PostToolUse → working, PermissionRequest or
  Notification `permission_prompt` → needs you, Stop or Notification `idle_prompt` → finished,
  StopFailure → error, SessionEnd/exit → stopped. `ready` = first terminal output.
- `agent_send` types a message as one bracketed paste followed by Enter.

Verified (release build, Xvfb, `e2e/terminal_scenario.py`):

| Check | Result |
|---|---|
| TUI in the worktree, slash commands | ✅ Claude Code v2.1.296 renders; `/code-rev` autocompletes `/code-review` |
| Status from hooks only | ✅ working → needs you (permission dialog) → working → finished |
| Permission prompt | ✅ answered in the TUI itself (Enter on "Yes") |
| `agent_send` multi-line | ✅ two lines arrive as one prompt (bracketed paste + `\r` works) |
| Switch items | ✅ earlier conversation still on screen after switching back |
| Restart | ✅ `--resume` shows the earlier conversation; no `claude` left after quit |
| MCP servers | ✅ none started (`--strict-mcp-config` alone is enough) |
| User/project hooks | ✅ merge with ours: the user's `PreToolUse` hook `block-dev-tcp.sh` ran alongside Ariekei's http hook |
| Memory | app 239 MB PSS idle; ~205 MB per agent (was ~345 MB with ACP); two agents 675 MB total |
| Startup | first paint 0.3–0.5 s; TUI ready ~1–2 s after opening an item |

Findings:
1. **SessionStart ignores `http` hooks** (only `command` hooks run for it; SessionEnd and the
   others accept `http`). Not needed: Ariekei chose the session id, and `ready` is derived from
   the terminal's first output.
2. **Parent-session env leaks in.** If Ariekei is started from inside a Claude Code session,
   markers such as `CLAUDECODE` and `CLAUDE_CODE_CHILD_SESSION` make our `claude` a nested child
   session ("Transcript saving is off"). Ariekei removes these markers from the PTY env.
3. **Claude Code saves a transcript only after the first prompt.** A session that was opened but
   never prompted cannot be resumed, so Ariekei records `started` (on UserPromptSubmit) and the
   transcript path, and only uses `--resume` when that transcript exists.
4. At quit, `claude`'s SessionEnd hook fails with ECONNREFUSED because the app's endpoint is
   already gone. Harmless; noted in case it shows up in debug logs.
5. The trust dialog did not appear for these worktrees; when it does, the user answers it in
   the terminal (Ariekei does not edit `~/.claude.json`).

## What exists after the spike

- `src-tauri/src/terminal.rs` — one PTY per worktree running `claude`, scrollback, attach/detach,
  resume logic, parent-session env stripping, `agent_send`.
- `src-tauri/src/hooks.rs` — hooks settings file, localhost hook endpoint, event → status mapping.
- `src-tauri/src/claude.rs` — locates the `claude` binary; unit tests.
- `src-tauri/src/git.rs` — changed files vs a base (incl. untracked) and file versions; unit tests.
- `src-tauri/src/sessions.rs` — spike-only JSON map worktree → session (id, started, transcript); SQLite in M1.
- `e2e/` — stdlib-only WebDriver client and the terminal scenario (run via `tauri-driver` + Xvfb).
- `ui/` — worktree list with hook-driven agent status, terminal panel (xterm.js, WebGL with DOM
  fallback), review view (file list + CodeMirror merge view, base ref).
