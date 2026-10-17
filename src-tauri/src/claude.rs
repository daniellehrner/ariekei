// Copyright contributors to Ariekei.
// SPDX-License-Identifier: Apache-2.0

//! Finds the user's installed `claude` CLI, which Ariekei runs unmodified in a terminal.
//!
//! Order: `ARIEKEI_CLAUDE`, then PATH, then the native installer's and Homebrew's
//! locations. GUI apps often start with a minimal PATH (macOS in particular), so the
//! fixed locations matter in practice.

use std::path::{Path, PathBuf};

const EXE: &str = if cfg!(windows) {
    "claude.exe"
} else {
    "claude"
};

/// Install locations used when `claude` is not on PATH.
fn known_locations(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".local/bin").join(EXE),
        home.join(".claude/local").join(EXE),
        "/opt/homebrew/bin/claude".into(),
        "/usr/local/bin/claude".into(),
    ]
}

fn first_file(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|p| p.is_file())
}

/// Locates `claude`, or explains where it looked.
pub fn find() -> Result<PathBuf, String> {
    if let Some(explicit) = std::env::var_os("ARIEKEI_CLAUDE").map(PathBuf::from) {
        return if explicit.is_file() {
            Ok(explicit)
        } else {
            Err(format!(
                "ARIEKEI_CLAUDE={} does not exist",
                explicit.display()
            ))
        };
    }
    let on_path: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).map(|dir| dir.join(EXE)).collect())
        .unwrap_or_default();
    let known = known_locations(&std::env::home_dir().unwrap_or_default());
    first_file(on_path.into_iter().chain(known.iter().cloned())).ok_or_else(|| {
        let searched: Vec<String> = known.iter().map(|p| p.display().to_string()).collect();
        format!(
            "Claude Code (`claude`) was not found on PATH or in {}. Install it or set ARIEKEI_CLAUDE.",
            searched.join(", ")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_locations_cover_native_installer_layout() {
        let home = tempfile::tempdir().unwrap();
        let bin = home.path().join(".local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join(EXE), "").unwrap();

        let found = first_file(known_locations(home.path()));
        assert_eq!(found, Some(bin.join(EXE)));
    }

    #[test]
    fn first_file_skips_missing_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("claude");
        std::fs::write(&real, "").unwrap();

        let found = first_file([dir.path().join("missing"), real.clone()]);
        assert_eq!(found, Some(real));
        assert_eq!(first_file([dir.path().join("missing")]), None);
    }
}
