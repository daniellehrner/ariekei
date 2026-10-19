// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

// The latest published release and its installers, read once at build time, so the download
// buttons link straight to the files. Without a release (or without network) the page links
// to the Releases page instead.

export const repo = 'https://github.com/daniellehrner/ariekei';
export const releasesPage = `${repo}/releases`;

export type Platform = 'linux' | 'macos' | 'windows';

// How a file is shown: a name ("AppImage", "Installer"), its extension, and a note.
interface Kind {
  platform: Platform;
  name?: string;
  ext?: string;
  note?: string;
}

export interface Download extends Kind {
  url: string;
}

export interface Release {
  version: string;
  url: string;
  downloads: Download[];
}

// File-name endings from tauri-action, most useful first per platform.
const kinds: (Kind & { test: RegExp })[] = [
  { platform: 'linux', name: 'AppImage', test: /_amd64\.AppImage$/ },
  { platform: 'linux', ext: '.deb', test: /_amd64\.deb$/ },
  { platform: 'linux', ext: '.rpm', test: /\.x86_64\.rpm$/ },
  { platform: 'macos', ext: '.dmg', note: 'Apple silicon and Intel', test: /_universal\.dmg$/ },
  { platform: 'windows', name: 'Installer', ext: '.exe', test: /_x64-setup\.exe$/ },
  { platform: 'windows', ext: '.msi', test: /_x64_[\w-]+\.msi$/ },
];

export const platformNames: Record<Platform, string> = { linux: 'Linux', macos: 'macOS', windows: 'Windows' };

export async function latestRelease(): Promise<Release | null> {
  const headers: Record<string, string> = { Accept: 'application/vnd.github+json' };
  if (process.env.GITHUB_TOKEN) headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
  try {
    const res = await fetch('https://api.github.com/repos/daniellehrner/ariekei/releases/latest', { headers });
    if (!res.ok) {
      if (res.status !== 404) console.warn(`release: GitHub answered ${res.status}; linking to the Releases page`);
      return null;
    }
    const data = (await res.json()) as {
      tag_name: string;
      html_url: string;
      assets: { name: string; browser_download_url: string }[];
    };
    const downloads = kinds.flatMap(({ test, ...kind }) => {
      const asset = data.assets.find((a) => test.test(a.name));
      return asset ? [{ ...kind, url: asset.browser_download_url }] : [];
    });
    return { version: data.tag_name.replace(/^v/, ''), url: data.html_url, downloads };
  } catch (e) {
    console.warn(`release: ${e}; linking to the Releases page`);
    return null;
  }
}
