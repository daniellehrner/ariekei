// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import starlightLinksValidator from 'starlight-links-validator';

// The app's tokens (ui/src/tokens.css) and the homepage switch to dark with the OS, through
// `@media (prefers-color-scheme: dark) { :root { … } }`. The site also has a theme switch, which
// sets data-theme on <html> (Starlight's, shared with the homepage): this rewrites each such
// block so that data-theme="light" keeps it off and data-theme="dark" applies it on any OS.
const themeAttribute = {
  postcssPlugin: 'theme-attribute',
  Once(root, { Rule }) {
    const file = root.source?.input.file ?? '';
    if (!/[\\/](ui|site)[\\/]src[\\/]/.test(file)) return;
    let found = false;
    root.walkAtRules('media', (media) => {
      if (!/^\(prefers-color-scheme:\s*dark\)$/.test(media.params)) return;
      media.walkRules(':root', (rule) => {
        const forced = rule.clone({ selector: ':root[data-theme="dark"]' });
        forced.append({ prop: 'color-scheme', value: 'dark' });
        media.after(forced);
        rule.selector = ':root:not([data-theme="light"])';
        found = true;
      });
    });
    if (found) root.append(new Rule({ selector: ':root[data-theme="light"]' }).append({ prop: 'color-scheme', value: 'light' }));
  },
};

export default defineConfig({
  site: 'https://ariekei.com',
  // The site's own tsconfig for every file. Searched for, a file outside src/ (e.g. Vite's
  // entries) would get the app's ../tsconfig.json, whose base (@tsconfig/svelte) is not
  // installed with the site's packages (CI installs only those).
  vite: { tsconfig: 'tsconfig.json', css: { postcss: { plugins: [themeAttribute] } } },
  integrations: [
    starlight({
      title: 'Ariekei',
      description: 'A review workbench for coding agents.',
      logo: { src: './src/assets/logo.svg' },
      favicon: '/favicon.svg',
      social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/daniellehrner/ariekei' }],
      sidebar: [{ label: 'Docs', items: [{ autogenerate: { directory: 'docs' } }] }],
      customCss: ['@fontsource/jetbrains-mono/400.css', '../ui/src/tokens.css', './src/styles/docs.css'],
      plugins: [starlightLinksValidator()],
    }),
  ],
});
