// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

// Syntax highlighting for any file type: CodeMirror's language list, picked by file name
// and loaded only when a file of that type is shown.

import { LanguageDescription } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import type { Extension } from "@codemirror/state";

export async function languageFor(path: string): Promise<Extension[]> {
  const name = path.split("/").pop() ?? path;
  const desc = LanguageDescription.matchFilename(languages, name);
  if (!desc) return [];
  try {
    return [await desc.load()];
  } catch {
    // A language that fails to load just means no highlighting.
    return [];
  }
}
