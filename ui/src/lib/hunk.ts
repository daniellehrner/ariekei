// The end of a review thread's diff hunk (the lines the comment is on), with line numbers.

export interface HunkLine {
  kind: "add" | "del" | "ctx";
  // The new file's line number (the old file's for deletions); null when unknown.
  number: number | null;
  text: string;
}

export function hunkTail(hunk: string, count: number): HunkLine[] {
  if (!hunk.trim()) return [];
  const lines = hunk.split("\n");
  const header = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/.exec(lines[0] ?? "");
  let old = header ? Number(header[1]) : null;
  let neu = header ? Number(header[2]) : null;
  const out: HunkLine[] = [];
  for (const line of header ? lines.slice(1) : lines) {
    if (line.startsWith("\\")) continue;
    const kind = line.startsWith("+") ? "add" : line.startsWith("-") ? "del" : "ctx";
    const number = kind === "del" ? old : neu;
    out.push({ kind, number, text: line.slice(1) });
    if (kind !== "add" && old !== null) old++;
    if (kind !== "del" && neu !== null) neu++;
  }
  return out.slice(-count);
}
