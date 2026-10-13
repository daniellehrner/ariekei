# Ariekei

A desktop IDE where the unit of work is a GitHub pull request: each one gets its own worktree,
its own Claude Code session and its own review progress, and an Inbox orders them by what needs
you. Tauri 2 (Rust) + Svelte 5.

- What works today: [docs/m1.md](docs/m1.md) (Inbox, PRs, review) and [docs/m2.md](docs/m2.md) (issues, cleanup)
- Spike results and the agent integration: [docs/spike-results.md](docs/spike-results.md)

## Requirements

- `git`, the GitHub CLI `gh` (logged in), and Claude Code (`claude`).
- To build: Rust 1.88+, Node 22+, and on Linux the WebKitGTK 4.1 / GTK 3 development packages
  (see `.github/workflows/ci.yml`).

## Develop

```
npm ci
npm run tauri dev        # run the app
npm run check && npm test
(cd src-tauri && cargo test)
npx tauri build --no-bundle
```
