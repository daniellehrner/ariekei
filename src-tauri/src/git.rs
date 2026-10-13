//! Git access via the `git` CLI (respects the user's config, credentials and worktrees).

use serde::Serialize;
use std::path::Path;
use std::process::Command;

fn git(cwd: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out.stdout)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFile {
    pub path: String,
    /// Git status letter: A, M, D, R, … ("?" for untracked).
    pub status: String,
}

/// Files that differ between `base` and the working tree, including untracked files.
pub fn changed_files(cwd: &Path, base: &str) -> Result<Vec<ChangedFile>, String> {
    let out = git(
        cwd,
        &["diff", "--name-status", "--no-renames", "-z", base, "--"],
    )?;
    let fields: Vec<&[u8]> = out.split(|b| *b == 0).filter(|f| !f.is_empty()).collect();
    let mut files: Vec<ChangedFile> = fields
        .chunks(2)
        .filter_map(|c| match c {
            [status, path] => Some(ChangedFile {
                status: String::from_utf8_lossy(status).into_owned(),
                path: String::from_utf8_lossy(path).into_owned(),
            }),
            _ => None,
        })
        .collect();

    let untracked = git(cwd, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    files.extend(
        untracked
            .split(|b| *b == 0)
            .filter(|p| !p.is_empty())
            .map(|p| ChangedFile {
                path: String::from_utf8_lossy(p).into_owned(),
                status: "?".into(),
            }),
    );
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileVersions {
    pub old: String,
    pub new: String,
    pub binary: bool,
}

/// Content of `path` at `base` and in the working tree (empty when absent).
pub fn file_versions(cwd: &Path, base: &str, path: &str) -> Result<FileVersions, String> {
    let old = git(cwd, &["show", &format!("{base}:{path}")]).unwrap_or_default();
    let new = std::fs::read(cwd.join(path)).unwrap_or_default();
    let binary = old.contains(&0) || new.contains(&0);
    if binary {
        return Ok(FileVersions {
            old: String::new(),
            new: String::new(),
            binary,
        });
    }
    Ok(FileVersions {
        old: String::from_utf8_lossy(&old).into_owned(),
        new: String::from_utf8_lossy(&new).into_owned(),
        binary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        git(p, &["init", "-q", "-b", "main"]).unwrap();
        std::fs::write(p.join("a.txt"), "one\n").unwrap();
        std::fs::write(p.join("gone.txt"), "bye\n").unwrap();
        git(p, &["add", "."]).unwrap();
        git(
            p,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-qm",
                "init",
            ],
        )
        .unwrap();
        dir
    }

    #[test]
    fn changed_files_include_modified_deleted_and_untracked() {
        let dir = repo();
        let p = dir.path();
        std::fs::write(p.join("a.txt"), "two\n").unwrap();
        std::fs::remove_file(p.join("gone.txt")).unwrap();
        std::fs::write(p.join("new.txt"), "hi\n").unwrap();

        let files = changed_files(p, "HEAD").unwrap();
        let summary: Vec<_> = files
            .iter()
            .map(|f| (f.path.as_str(), f.status.as_str()))
            .collect();
        assert_eq!(
            summary,
            [("a.txt", "M"), ("gone.txt", "D"), ("new.txt", "?")]
        );
    }

    #[test]
    fn file_versions_reads_base_and_working_tree() {
        let dir = repo();
        let p = dir.path();
        std::fs::write(p.join("a.txt"), "two\n").unwrap();

        let v = file_versions(p, "HEAD", "a.txt").unwrap();
        assert_eq!(
            (v.old.as_str(), v.new.as_str(), v.binary),
            ("one\n", "two\n", false)
        );

        let added = file_versions(p, "HEAD", "missing.txt").unwrap();
        assert_eq!((added.old.as_str(), added.new.as_str()), ("", ""));
    }
}
