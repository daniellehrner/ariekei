// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";

// Tauri expects a fixed dev port and serves the built UI from ../dist.
export default defineConfig({
  root: "ui",
  // The Vite root is ui/, so point the plugin at the shared config explicitly.
  plugins: [svelte({ configFile: fileURLToPath(new URL("./svelte.config.js", import.meta.url)) })],
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
  // A desktop app loads its bundle from disk, so chunk size is not a concern.
  build: { outDir: "../dist", emptyOutDir: true, target: "es2022", chunkSizeWarningLimit: 2000 },
});
