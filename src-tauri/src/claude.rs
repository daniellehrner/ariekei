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

/// Claude Code's own directory: `CLAUDE_CONFIG_DIR`, else `~/.claude`.
pub fn config_dir() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::home_dir().map(|h| h.join(".claude")))
}

/// The transcript Claude Code saved for `session_id`, if any: `projects/<project>/<id>.jsonl`
/// in its directory `dir` (every project is searched: how it names one from the working
/// directory is its own business).
pub fn find_transcript(dir: &Path, session_id: &str) -> Option<PathBuf> {
    let name = format!("{session_id}.jsonl");
    std::fs::read_dir(dir.join("projects"))
        .ok()?
        .flatten()
        .map(|project| project.path().join(&name))
        .find(|t| t.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sessions_transcript_is_found_in_any_project() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(find_transcript(dir.path(), "s1"), None, "no projects yet");
        let project = dir.path().join("projects/-home-me-proj");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(dir.path().join("projects/-other")).unwrap();
        assert_eq!(find_transcript(dir.path(), "s1"), None);
        std::fs::write(project.join("s1.jsonl"), "{}\n").unwrap();
        assert_eq!(
            find_transcript(dir.path(), "s1"),
            Some(project.join("s1.jsonl"))
        );
        assert_eq!(find_transcript(dir.path(), "s2"), None);
    }

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
