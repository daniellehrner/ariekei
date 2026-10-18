// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import starlightLinksValidator from 'starlight-links-validator';

export default defineConfig({
  site: 'https://ariekei.com',
  // The site's own tsconfig for every file. Searched for, a file outside src/ (e.g. Vite's
  // entries) would get the app's ../tsconfig.json, whose base (@tsconfig/svelte) is not
  // installed with the site's packages (CI installs only those).
  vite: { tsconfig: 'tsconfig.json' },
  integrations: [
    starlight({
      title: 'Ariekei',
      description: 'A review workbench for coding agents.',
      logo: { src: './src/assets/logo.svg' },
      favicon: '/favicon.svg',
      social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/daniellehrner/ariekei' }],
      sidebar: [{ label: 'Docs', items: [{ autogenerate: { directory: 'docs' } }] }],
      customCss: ['@fontsource/jetbrains-mono/400.css', '../ui/src/tokens.css', './src/styles/docs.css'],
      // The theme follows the OS, as in the app: no theme picker.
      components: { ThemeSelect: './src/components/Empty.astro' },
      plugins: [starlightLinksValidator()],
    }),
  ],
});
