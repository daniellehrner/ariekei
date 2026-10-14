// The xterm.js theme. xterm needs concrete colours, so they are read from tokens.css at runtime
// (and again when the system switches between light and dark).

import type { ITheme } from "@xterm/xterm";

export function xtermTheme(): ITheme {
  const css = getComputedStyle(document.documentElement);
  const v = (name: string) => css.getPropertyValue(`--${name}`).trim();
  return {
    background: v("term-bg"),
    foreground: v("term-fg"),
    cursor: v("term-cursor"),
    cursorAccent: v("term-bg"),
    selectionBackground: v("term-selection"),
    black: v("ansi-black"),
    red: v("ansi-red"),
    green: v("ansi-green"),
    yellow: v("ansi-yellow"),
    blue: v("ansi-blue"),
    magenta: v("ansi-magenta"),
    cyan: v("ansi-cyan"),
    white: v("ansi-white"),
    brightBlack: v("ansi-bright-black"),
    brightRed: v("ansi-bright-red"),
    brightGreen: v("ansi-bright-green"),
    brightYellow: v("ansi-bright-yellow"),
    brightBlue: v("ansi-bright-blue"),
    brightMagenta: v("ansi-bright-magenta"),
    brightCyan: v("ansi-bright-cyan"),
    brightWhite: v("ansi-bright-white"),
  };
}

export const monoFont = () => getComputedStyle(document.documentElement).getPropertyValue("--font-mono").trim();
