// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

// Copies the user docs from ../docs into Starlight's content folder, so docs/*.md stays the one
// source. The leading "# Title" becomes frontmatter, links between published docs become site
// links, and links to the other docs (design notes, spike results) go to GitHub.

import { mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const source = join(here, '../../docs');
const target = join(here, '../src/content/docs/docs');
const github = 'https://github.com/daniellehrner/ariekei/blob/main/docs/';
const edit = 'https://github.com/daniellehrner/ariekei/edit/main/docs/';

// Sidebar order.
const published = [
  { file: 'manual.md', description: 'Everything in Ariekei, from connecting a repository to cleanup.' },
  { file: 'pull-requests.md', description: 'The Inbox, working on a pull request with the agent, and reviewing it.' },
  { file: 'review.md', description: 'How the review view works, and the research behind it.' },
  { file: 'lifecycle.md', description: 'From an issue or a task to a merged pull request, and back to a clean clone.' },
];
const slugs = new Set(published.map((d) => d.file));

function convert(file, markdown, description, order) {
  const lines = markdown.split('\n');
  const heading = lines.findIndex((l) => l.startsWith('# '));
  if (heading < 0) throw new Error('no "# Title" line');
  const title = lines[heading].slice(2).trim();
  let body = lines.slice(heading + 1).join('\n');
  // Starlight shows its own table of contents; drop the manual's.
  body = body.replace(/\nContents:\n[\s\S]*?\n---\n/, '\n');
  body = body.replace(/\]\(([\w-]+\.md)(#[^)]*)?\)/g, (_, linked, hash = '') =>
    slugs.has(linked) ? `](/docs/${linked.slice(0, -3)}/${hash})` : `](${github}${linked}${hash})`,
  );
  const front = ['---', `title: ${JSON.stringify(title)}`, `description: ${JSON.stringify(description)}`,
    `editUrl: ${edit}${file}`, 'sidebar:', `  order: ${order}`, '---', ''];
  return front.join('\n') + body.replace(/^\n+/, '\n');
}

await rm(target, { recursive: true, force: true });
await mkdir(target, { recursive: true });
for (const [i, doc] of published.entries()) {
  const markdown = await readFile(join(source, doc.file), 'utf8');
  await writeFile(join(target, doc.file), convert(doc.file, markdown, doc.description, i + 1));
}
console.log(`sync-docs: ${published.length} docs → src/content/docs/docs/`);
