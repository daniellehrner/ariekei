// Paths as shown: the home directory as ~.

let home: string | null = null;

/** Set once at startup (from Tauri's homeDir). */
export function setHome(dir: string) {
  home = dir.replace(/[\\/]+$/, "");
}

export function shortPath(path: string): string {
  if (home && (path === home || path.startsWith(home + "/") || path.startsWith(home + "\\"))) return "~" + path.slice(home.length);
  return path;
}
