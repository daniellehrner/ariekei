// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

// The impact map: how an item's change affects the rest of the code, as its agent mapped it
// with Ariekei's tools (src-tauri/src/impact.rs). The Impact tab shows it, and the diff links
// to it at the changed lines its entries follow from.

export type ImpactKind = "affected" | "similar" | "path";

/** Lines of a file at the map's commit; `inDiff`: the change touches them. */
export interface ImpactLocation {
  path: string;
  fromLine: number;
  toLine: number;
  code: string;
  note: string;
  inDiff: boolean;
}

export interface ImpactEntry {
  id: number;
  kind: ImpactKind;
  title: string;
  body: string;
  // Worth the reviewer's attention: behaviour may break, or the change may miss a place.
  attention: boolean;
  // The changed lines it follows from.
  changed: ImpactLocation | null;
  locations: ImpactLocation[];
  // The commit its places are of.
  rev: string;
}

export interface ImpactMap {
  rev: string;
  base: string;
  overview: string;
  entries: ImpactEntry[];
}

/** The map, and how it stands to the head the review shows now. */
export interface ImpactShown {
  map: ImpactMap | null;
  head: string;
  // Commits the head has on top of the map's; `rewritten`: the map's is not in its history.
  newer: number;
  rewritten: boolean;
}

/** A place picked in the map: entry `entry`'s location `place` (-1: the lines it follows from). */
export interface Picked {
  entry: number;
  place: number;
}

export const SECTIONS: { kind: ImpactKind; title: string; hint: string }[] = [
  { kind: "affected", title: "Affected", hint: "Code outside the diff that the change reaches" },
  { kind: "similar", title: "Similar", hint: "Code elsewhere that does the same thing" },
  { kind: "path", title: "Paths", hint: "From an entry point to the changed code" },
];

/** The map's sections that have entries, the ones worth attention first in each. */
export function sectionsOf(entries: ImpactEntry[]): { kind: ImpactKind; title: string; hint: string; entries: ImpactEntry[] }[] {
  return SECTIONS.map((s) => ({
    ...s,
    entries: entries.filter((e) => e.kind === s.kind).sort((a, b) => Number(b.attention) - Number(a.attention)),
  })).filter((s) => s.entries.length);
}

/** The location `p` picks, if the map still has it. */
export function pickedLocation(entries: ImpactEntry[], p: Picked | null | undefined): ImpactLocation | null {
  const e = p ? entries.find((e) => e.id === p.entry) : undefined;
  if (!e || !p) return null;
  return p.place < 0 ? e.changed : (e.locations[p.place] ?? null);
}

export const linesOf = (l: { fromLine: number; toLine: number }) => (l.fromLine === l.toLine ? `${l.fromLine}` : `${l.fromLine}–${l.toLine}`);

export const placesText = (n: number) => `${n} place${n === 1 ? "" : "s"}`;

/** How far behind the head the map is, in words; null when it is of the head. */
export function staleText(s: ImpactShown): string | null {
  if (!s.map || s.map.rev === s.head) return null;
  if (s.rewritten) return "the branch was rewritten since";
  return `${s.newer || "some"} newer commit${s.newer === 1 ? "" : "s"}`;
}

/** The entries the diff links to on file `path` shown at commit `rev`: by the first line of
 *  the changed lines each follows from. Only when the diff shows the map's version of it. */
export function linksIn(map: ImpactMap | null, path: string | null, rev: string | null): { line: number; entries: number[] }[] {
  if (!map || !path || !rev) return [];
  const at = new Map<number, number[]>();
  for (const e of map.entries) {
    const c = e.changed;
    if (!c || !c.inDiff || c.path !== path || e.rev !== rev) continue;
    at.set(c.fromLine, [...(at.get(c.fromLine) ?? []), e.id]);
  }
  return [...at].sort(([a], [b]) => a - b).map(([line, entries]) => ({ line, entries }));
}

/** `dir/` and `name` of a path, the directory shortened to its last three parts. */
export function splitPath(path: string): { dir: string; name: string } {
  const i = path.lastIndexOf("/");
  if (i < 0) return { dir: "", name: path };
  const parts = path.slice(0, i).split("/");
  const dir = parts.length > 3 ? `…/${parts.slice(-3).join("/")}` : parts.join("/");
  return { dir: `${dir}/`, name: path.slice(i + 1) };
}
