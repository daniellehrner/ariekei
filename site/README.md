# The Ariekei website

The homepage and the user docs, as a static site: Astro with Starlight for the docs. It has
its own `package.json` (Node 22+) and has no effect on the app's build.

```
cd site
npm ci
npm run dev       # http://localhost:4321
npm run build     # into site/dist/, then checks the docs' internal links
npm run check
```

- **Homepage:** `src/pages/index.astro`, styled by `src/styles/home.css` with the app's own
  tokens (`../ui/src/tokens.css`), so it follows the OS theme like the app. The theme switch in
  the header overrides that and is remembered in the browser, shared with the docs' own picker.
- **Docs:** `docs/manual.md`, `pull-requests.md`, `review.md` and `lifecycle.md` stay the source.
  `scripts/sync-docs.mjs` copies them into `src/content/docs/docs/` (git-ignored) before every
  `dev`, `build` and `check`: the `# Title` becomes frontmatter and links between them become
  site links; links to the other docs go to GitHub. Add a doc there to publish it.
- **Download buttons:** at build time `src/lib/release.ts` reads the latest published GitHub
  Release and links its installers. Without one, the page says there is no release yet. So
  redeploy after publishing a release (`GITHUB_TOKEN` in the environment avoids GitHub's rate
  limit).

## Screenshots

`src/assets/shots/{light,dark}/*.png` come from `e2e/gallery.py` at 1440x900, cropped by
`scripts/shots.mjs`. To take them again, build the e2e app (`npx tauri build --no-bundle
--features e2e` in the repo root), run the gallery once per theme as its docstring describes,
with the screens `drafts,inbox,task,ask-agent`, then:

```
node scripts/shots.mjs <light work>/shots/light-1440 <dark work>/shots/dark-1440
```

The gallery's fixture repository (`owner/proj`, made-up PRs and people) is safe to publish.

## Deploy

The site, https://ariekei.com, is served by Caddy from `/var/www/ariekei`. The server's Caddyfile
imports `/etc/caddy/sites/*.caddy`; this site's block is `deploy/ariekei.caddy`.

```
site/deploy.sh            # build, then rsync dist/ to /var/www/ariekei
site/deploy.sh --caddy    # also install the Caddy block and reload Caddy
```

`deploy.sh` connects to the SSH host `ariekei.com` (`ARIEKEI_SERVER` overrides it), so the port
and user come from your `~/.ssh/config`. The user needs passwordless `sudo` for `--caddy`.
