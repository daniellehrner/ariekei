// Draft review comments and where they can go: GitHub (as line comments when possible)
// or the agent.

export interface DraftComment {
  path: string;
  fromLine: number;
  toLine: number;
  code: string;
  text: string;
  // The commit whose version of the file the lines refer to; null for uncommitted changes.
  rev: string | null;
}

export interface LineComment {
  path: string;
  line: number;
  start_line?: number;
  side: "RIGHT";
  body: string;
}

export type Ranges = Record<string, [number, number][]>;

const lines = (c: DraftComment) => (c.fromLine === c.toLine ? `${c.fromLine}` : `${c.fromLine}-${c.toLine}`);

// GitHub takes a line comment only on lines of the PR diff at its head commit. A draft
// made on another version of the file, or outside the diff's hunks, goes into the body.
export function classify(comments: DraftComment[], prHead: string, ranges: Ranges) {
  const line: LineComment[] = [];
  const body: DraftComment[] = [];
  for (const c of comments) {
    const inHunk = (ranges[c.path] ?? []).some(([a, b]) => c.fromLine >= a && c.toLine <= b);
    if (c.rev === prHead && inHunk) {
      line.push({
        path: c.path,
        line: c.toLine,
        ...(c.fromLine < c.toLine ? { start_line: c.fromLine } : {}),
        side: "RIGHT",
        body: c.text.trim(),
      });
    } else {
      body.push(c);
    }
  }
  return { line, body };
}

export function reviewBody(summary: string, folded: DraftComment[]): string {
  const parts = [summary.trim()];
  if (folded.length) {
    const items = folded.map((c) => `- \`${c.path}:${lines(c)}\` — ${c.text.trim().replaceAll("\n", "\n  ")}`);
    parts.push(items.join("\n"));
  }
  return parts.filter(Boolean).join("\n\n");
}

export function feedbackMessage(comments: DraftComment[]): string {
  const items = comments.map((c, i) => {
    const quote = c.code.split("\n").slice(0, 6).map((l) => `   > ${l}`).join("\n");
    return `${i + 1}. ${c.path}:${lines(c)}\n${quote}\n   ${c.text.trim().replaceAll("\n", "\n   ")}`;
  });
  return `Review feedback on your changes:\n\n${items.join("\n\n")}\n\nPlease address each point.`;
}
