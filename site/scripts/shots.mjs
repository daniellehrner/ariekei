// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

// Copies the homepage's screenshots out of e2e/gallery.py's output (1440x900, one folder per
// theme) into src/assets/shots, cropped to the part each section is about, so the text stays
// readable at the size the page shows them. See site/README.md.
//
//   node scripts/shots.mjs <light shots dir> <dark shots dir>

import { existsSync } from 'node:fs';
import { mkdir } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import sharp from 'sharp';

// Screen → region of the 1440x900 window (null: all of it).
const shots = {
  drafts: null, // hero: Inbox, review with a draft comment under its lines
  inbox: { left: 0, top: 0, width: 1080, height: 660 }, // Inbox and a PR's overview
  task: { left: 0, top: 0, width: 1080, height: 640 }, // a task, and the PR its agent opened in My PRs
  'ask-agent': { left: 546, top: 174, width: 894, height: 726 }, // diff, selection, Ask the agent
};

const [light, dark] = process.argv.slice(2);
if (!light || !dark) {
  console.error('usage: node scripts/shots.mjs <light shots dir> <dark shots dir>');
  process.exit(2);
}
const out = join(dirname(fileURLToPath(import.meta.url)), '../src/assets/shots');
for (const [theme, dir] of [['light', light], ['dark', dark]]) {
  await mkdir(join(out, theme), { recursive: true });
  for (const [name, region] of Object.entries(shots)) {
    // A screen the gallery did not take this time keeps its shot.
    if (!existsSync(join(dir, `${name}.png`))) {
      console.log(`shots: no ${theme} ${name}.png, kept the old one`);
      continue;
    }
    let image = sharp(join(dir, `${name}.png`));
    if (region) image = image.extract(region);
    await image.png({ compressionLevel: 9 }).toFile(join(out, theme, `${name}.png`));
  }
}
console.log('shots: → src/assets/shots/');
