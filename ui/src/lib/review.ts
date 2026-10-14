// File lists of the review view: grouping, order, progress and the next file to look at.

export type Group = "source" | "tests" | "config" | "generated";

export interface ReviewFile {
  path: string;
  status: string;
  group: Group;
  reviewed: boolean;
  whitespaceOnly: boolean;
}

export interface GuideGroup {
  title: string;
  intent: string;
  risk: "high" | "medium" | "low";
  files: string[];
}

export interface Guide {
  focus: string;
  groups: GuideGroup[];
  claims: { claim: string; status: "implemented" | "partial" | "missing"; files: string[] }[];
  unexplained: string[];
}

export type Order = "review" | "tree" | "guide";

/** What the guide says about one file: its group, the claims it backs, and whether no claim explains it. */
export interface FileGuide {
  group: GuideGroup | null;
  claims: Guide["claims"];
  unexplained: boolean;
}

export function fileGuide(guide: Guide | null, path: string | null): FileGuide | null {
  if (!guide || !path) return null;
  return {
    group: guide.groups.find((g) => g.files.includes(path)) ?? null,
    claims: guide.claims.filter((c) => c.files.includes(path)),
    unexplained: guide.unexplained.includes(path),
  };
}

export interface Section {
  key: string;
  title: string;
  // Shown folded until opened (generated, lock and vendored files).
  folded: boolean;
  files: ReviewFile[];
  intent?: string;
  risk?: GuideGroup["risk"];
}

const GROUP_TITLES: Record<Group, string> = {
  source: "Source",
  tests: "Tests",
  config: "Config & build",
  generated: "Generated, lock & vendored",
};
const RISK_RANK = { high: 0, medium: 1, low: 2 } as const;

// Files arrive in review order from the backend (source by directory, tests, config,
// generated); "tree" is plain path order; "guide" follows the agent guide's groups,
// riskiest first, with files the guide does not mention at the end.
export function sections(files: ReviewFile[], order: Order, guide: Guide | null): Section[] {
  if (order === "tree") {
    return [{ key: "tree", title: "Files", folded: false, files: [...files].sort((a, b) => a.path.localeCompare(b.path)) }];
  }
  if (order === "guide" && guide) {
    const byPath = new Map(files.map((f) => [f.path, f]));
    const used = new Set<string>();
    const groups = [...guide.groups].sort((a, b) => RISK_RANK[a.risk] - RISK_RANK[b.risk]);
    const out: Section[] = groups.map((g, i) => {
      const list = g.files.map((p) => byPath.get(p)).filter((f): f is ReviewFile => !!f && !used.has(f.path));
      list.forEach((f) => used.add(f.path));
      return { key: `guide-${i}`, title: g.title, folded: false, files: list, intent: g.intent, risk: g.risk };
    });
    const rest = files.filter((f) => !used.has(f.path));
    if (rest.length) out.push({ key: "guide-rest", title: "Not in the guide", folded: false, files: rest });
    return out.filter((s) => s.files.length);
  }
  return (Object.keys(GROUP_TITLES) as Group[])
    .map((g) => ({ key: g, title: GROUP_TITLES[g], folded: g === "generated", files: files.filter((f) => f.group === g) }))
    .filter((s) => s.files.length);
}

// The files in the order they are shown.
export function flatten(list: Section[]): ReviewFile[] {
  return list.flatMap((s) => s.files);
}

export function progress(files: ReviewFile[]) {
  return { reviewed: files.filter((f) => f.reviewed).length, total: files.length };
}

// The next unreviewed file after `current` (wrapping around), or null when all are reviewed.
export function nextUnreviewed(ordered: ReviewFile[], current: string | null): string | null {
  const start = Math.max(ordered.findIndex((f) => f.path === current), -1);
  for (let i = 1; i <= ordered.length; i++) {
    const f = ordered[(start + i) % ordered.length];
    if (!f.reviewed && f.path !== current) return f.path;
  }
  return null;
}
