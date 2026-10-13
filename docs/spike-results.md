# Phase 0 spike — results (2026-10-13)

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

## What exists after the spike

- `src-tauri/src/agent.rs` — ACP session manager: one adapter process per worktree, permission
  requests parked until the UI answers (off the connection's event loop), prompt/cancel, status
  events (`starting/ready/working/waitingForPermission/finished/error/stopped`).
- `src-tauri/src/git.rs` — changed files vs a base (incl. untracked) and file versions; unit tests.
- `src-tauri/src/node.rs` — Node 22+ detection; unit tests.
- `src-tauri/src/sessions.rs` — spike-only JSON map worktree → session id (SQLite in M1).
- `src-tauri/examples/acp_probe.rs` — headless ACP probe (spawn, permission, kill, `session/load`).
- `e2e/` — stdlib-only WebDriver client and the spike scenario (run via `tauri-driver` + Xvfb).
- `ui/` — worktree list with agent status, agent panel (transcript, tool calls, plan, permission
  prompt, cancel), review view (file list + CodeMirror merge view, base ref).
